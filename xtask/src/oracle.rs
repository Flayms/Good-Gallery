//! `cargo xtask oracle [--reference <dir>] [--keep <dir>]`: generates a fixture library, indexes it with the reference
//! (`xtask/oracle/reference-index.ts`, bundled with the reference's vite and run with Node) and with this indexer, and
//! diffs the two indexes, ignoring ids and scan times. Documented deviations are listed but don't fail the run. Then
//! compares searches (`oracle_search`) and thumbnails rendered by both (`oracle_thumbs`).

use anyhow::{Context as _, bail};
use db::libraries::{self, LibraryStatus};
use db::rusqlite::Connection;
use db::writer::IndexWriter;
use indexer::ScanScope;
use indexer::scan::{ScanDeps, ScanOptions, ScanOutcome, scan_root};
use media::exiftool::{ExifTool, find_exiftool};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

/// Rows of the compared tables: table → row key → column → value (as SQLite's `quote()` renders it).
pub type Snapshot = BTreeMap<&'static str, BTreeMap<String, BTreeMap<String, String>>>;

/// What is compared, per table: a query whose first column is the row key. Ids are replaced by what they stand for
/// (paths, tag names); scan times (`last_scan_at`) are left out.
const TABLES: [(&str, &str); 5] = [
    (
        "library_roots",
        "SELECT 'library', quote(status) AS status FROM library_roots",
    ),
    (
        "media",
        "SELECT rel_path, quote(file_name) AS file_name, quote(kind) AS kind, quote(size) AS size,
                quote(mtime) AS mtime, quote(width) AS width, quote(height) AS height, quote(duration) AS duration,
                quote(taken_at) AS taken_at, quote(rating) AS rating, quote(meta_version) AS meta_version,
                quote(sidecar_mtime) AS sidecar_mtime, quote(thumb_status) AS thumb_status,
                quote(thumbhash) AS thumbhash
         FROM media",
    ),
    (
        "tags",
        "SELECT t.name_norm, quote(t.name) AS name, quote(p.name_norm) AS parent, quote(t.media_count) AS media_count
         FROM tags t LEFT JOIN tags p ON p.id = t.parent_id",
    ),
    (
        "media_tags",
        "SELECT m.rel_path || ' → ' || t.name_norm FROM media_tags mt
         JOIN media m ON m.id = mt.media_id JOIN tags t ON t.id = mt.tag_id",
    ),
    ("folders", "SELECT rel_dir, quote(mtime) AS mtime FROM folders"),
];

pub fn snapshot(conn: &Connection) -> db::Result<Snapshot> {
    let mut snapshot = Snapshot::new();
    for (table, sql) in TABLES {
        let mut stmt = conn.prepare(sql)?;
        let columns: Vec<String> = stmt.column_names().skip(1).map(str::to_string).collect();
        let rows = snapshot.entry(table).or_default();
        let mut query = stmt.query([])?;
        while let Some(row) = query.next()? {
            let key: String = row.get(0)?;
            let values = columns
                .iter()
                .enumerate()
                .map(|(i, column)| Ok((column.clone(), row.get::<_, String>(i + 1)?)))
                .collect::<db::Result<_>>()?;
            rows.insert(key, values);
        }
    }
    Ok(snapshot)
}

/// One difference between the reference's index and this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    pub table: &'static str,
    pub key: String,
    /// `None`: the whole row exists in only one index.
    pub column: Option<String>,
    pub reference: Option<String>,
    pub ours: Option<String>,
}

impl std::fmt::Display for Difference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let show = |value: &Option<String>| value.clone().unwrap_or_else(|| "(missing)".into());
        let column = self.column.as_deref().map(|c| format!(".{c}")).unwrap_or_default();
        write!(
            f,
            "{} [{}]{column}: reference {}, ours {}",
            self.table,
            self.key,
            show(&self.reference),
            show(&self.ours)
        )
    }
}

pub fn diff(reference: &Snapshot, ours: &Snapshot) -> Vec<Difference> {
    let mut differences = Vec::new();
    let empty = BTreeMap::new();
    for (table, _) in TABLES {
        let (a, b) = (
            reference.get(table).unwrap_or(&empty),
            ours.get(table).unwrap_or(&empty),
        );
        let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
        keys.sort();
        keys.dedup();
        for key in keys {
            match (a.get(key), b.get(key)) {
                (Some(ra), Some(rb)) => {
                    for (column, va) in ra {
                        let vb = rb.get(column);
                        if vb != Some(va) {
                            differences.push(Difference {
                                table,
                                key: key.clone(),
                                column: Some(column.clone()),
                                reference: Some(va.clone()),
                                ours: vb.cloned(),
                            });
                        }
                    }
                }
                (ra, rb) => differences.push(Difference {
                    table,
                    key: key.clone(),
                    column: None,
                    reference: ra.map(|_| "row".into()),
                    ours: rb.map(|_| "row".into()),
                }),
            }
        }
    }
    differences
}

/// A difference that is expected, with the reason (`docs/rewrite/milestones/M04-indexing.md`).
pub struct Deviation {
    pub table: &'static str,
    pub key: &'static str,
    pub column: Option<&'static str>,
    pub reason: &'static str,
}

const FACES: &str = "face regions: the reference reads only flattened region fields and misses the structs \
                     exiftool returns with struct=1 (REQ-004-09 wins)";
const GPS_ZONE: &str = "no GPS-based time zones: the reference takes the zone from the GPS position (geo-tz), \
                        this indexer the computer's zone (docs/architecture.md)";

pub const DEVIATIONS: &[Deviation] = &[
    Deviation {
        table: "media_tags",
        key: "people/faces.jpg → bob",
        column: None,
        reason: FACES,
    },
    Deviation {
        table: "media_tags",
        key: "people/faces.jpg → carol",
        column: None,
        reason: FACES,
    },
    Deviation {
        table: "tags",
        key: "bob",
        column: None,
        reason: FACES,
    },
    Deviation {
        table: "tags",
        key: "carol",
        column: None,
        reason: FACES,
    },
    Deviation {
        table: "media",
        key: "travel/tokyo-gps.jpg",
        column: Some("taken_at"),
        reason: GPS_ZONE,
    },
];

pub fn deviation_of(difference: &Difference) -> Option<&'static Deviation> {
    DEVIATIONS
        .iter()
        .find(|d| d.table == difference.table && d.key == difference.key && d.column == difference.column.as_deref())
}

const SIDECAR_XMP: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:lr="http://ns.adobe.com/lightroom/1.0/"
  xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmlns:exif="http://ns.adobe.com/exif/1.0/">
 <xmp:Rating>RATING</xmp:Rating>
 <exif:DateTimeOriginal>DATE</exif:DateTimeOriginal>
 <dc:subject><rdf:Bag>SUBJECTS</rdf:Bag></dc:subject>
 <lr:hierarchicalSubject><rdf:Bag><rdf:li>Places|Sidecar|Lake</rdf:li></rdf:Bag></lr:hierarchicalSubject>
</rdf:Description></rdf:RDF></x:xmpmeta>"#;

fn sidecar(rating: u8, date: &str, subjects: &[&str]) -> String {
    let subjects: String = subjects.iter().map(|s| format!("<rdf:li>{s}</rdf:li>")).collect();
    SIDECAR_XMP
        .replace("RATING", &rating.to_string())
        .replace("DATE", date)
        .replace("SUBJECTS", &subjects)
}

enum Content {
    Jpeg(u32, u32),
    Png(u32, u32),
    Bytes(&'static [u8]),
    Video,
}

/// A fixture file: where it goes, what it is, the exiftool arguments that tag it.
struct Fixture {
    path: &'static str,
    content: Content,
    tags: &'static [&'static str],
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        path: "2021/beach.jpg",
        content: Content::Jpeg(64, 48),
        tags: &[
            "-EXIF:DateTimeOriginal=2021:07:14 10:11:12",
            "-EXIF:OffsetTimeOriginal=+02:00",
            "-IPTC:Keywords=Beach",
            "-IPTC:Keywords=Summer",
            "-IPTC:Keywords=2021",
            "-XMP:Rating=4",
        ],
    },
    Fixture {
        path: "2021/subsec.jpg",
        content: Content::Jpeg(48, 64),
        tags: &[
            "-EXIF:DateTimeOriginal=2021:07:15 08:00:00",
            "-EXIF:SubSecTimeOriginal=123",
            "-EXIF:OffsetTimeOriginal=-05:00",
        ],
    },
    Fixture {
        path: "2021/local.jpg",
        content: Content::Jpeg(32, 32),
        tags: &["-EXIF:DateTimeOriginal=2021:12:24 18:30:00"],
    },
    Fixture {
        path: "2021/create-date.jpg",
        content: Content::Jpeg(32, 32),
        tags: &["-EXIF:CreateDate=2021:01:02 03:04:05", "-EXIF:OffsetTime=+01:00"],
    },
    Fixture {
        path: "people/hierarchy.jpg",
        content: Content::Jpeg(64, 48),
        tags: &[
            "-XMP-lr:HierarchicalSubject=Places|Italy|Rome",
            "-XMP-lr:HierarchicalSubject=People|Anna",
            "-XMP-dc:Subject=Rome",
            "-XMP-dc:Subject=Anna",
            "-XMP-dc:Subject=  New   York ",
            "-XMP-dc:Subject=Cafe\u{301}",
            "-XMP:Rating=5",
        ],
    },
    Fixture {
        path: "people/faces.jpg",
        content: Content::Jpeg(64, 48),
        tags: &[
            "-XMP-mwg-rs:RegionInfo={AppliedToDimensions={W=64,H=48,Unit=pixel},RegionList=[{Area={X=0.5,Y=0.5,W=0.2,H=0.2,Unit=normalized},Name=Bob,Type=Face}]}",
            "-XMP-MP:RegionInfoMP={Regions=[{PersonDisplayName=Carol,Rectangle=0.1|, 0.1|, 0.2|, 0.2}]}",
            "-XMP-dc:Subject=Café",
        ],
    },
    Fixture {
        path: "rotated/portrait.jpg",
        content: Content::Jpeg(64, 48),
        tags: &[
            "-EXIF:Orientation#=6",
            "-EXIF:DateTimeOriginal=2020:02:02 12:00:00",
            "-EXIF:OffsetTimeOriginal=+00:00",
        ],
    },
    Fixture {
        path: "rotated/mirrored.jpg",
        content: Content::Jpeg(64, 48),
        tags: &["-EXIF:Orientation#=2"],
    },
    Fixture {
        path: "plain/no-metadata.png",
        content: Content::Png(20, 10),
        tags: &[],
    },
    Fixture {
        path: "plain/tagged.png",
        content: Content::Png(20, 10),
        tags: &["-XMP-dc:Subject=Screenshot", "-XMP:Rating=1"],
    },
    Fixture {
        path: "plain/broken.jpg",
        content: Content::Bytes(b"not a jpeg at all"),
        tags: &[],
    },
    Fixture {
        path: "travel/tokyo-gps.jpg",
        content: Content::Jpeg(64, 48),
        tags: &[
            "-EXIF:DateTimeOriginal=2020:03:01 09:00:00",
            "-GPSLatitude=35.6895",
            "-GPSLatitudeRef=N",
            "-GPSLongitude=139.6917",
            "-GPSLongitudeRef=E",
        ],
    },
    Fixture {
        path: "travel/sidecar-base.jpg",
        content: Content::Jpeg(32, 32),
        tags: &["-IPTC:Keywords=FromFile"],
    },
    Fixture {
        path: "travel/sidecar-full.jpg",
        content: Content::Jpeg(32, 32),
        tags: &[
            "-EXIF:DateTimeOriginal=2019:06:01 10:00:00",
            "-EXIF:OffsetTimeOriginal=+03:00",
        ],
    },
    Fixture {
        path: "travel/deep/er/still/deep.jpg",
        content: Content::Jpeg(16, 16),
        tags: &["-XMP-lr:HierarchicalSubject=Places|Italy|Venice"],
    },
    Fixture {
        path: "video/clip.mp4",
        content: Content::Video,
        tags: &[],
    },
    // Housekeeping: none of these are indexed.
    Fixture {
        path: ".hidden.jpg",
        content: Content::Jpeg(8, 8),
        tags: &[],
    },
    Fixture {
        path: "@eaDir/beach.jpg/SYNOFILE_THUMB_M.jpg",
        content: Content::Jpeg(8, 8),
        tags: &[],
    },
    Fixture {
        path: ".thumbnails/x.jpg",
        content: Content::Jpeg(8, 8),
        tags: &[],
    },
    Fixture {
        path: "$RECYCLE.BIN/old.jpg",
        content: Content::Jpeg(8, 8),
        tags: &[],
    },
];

/// Other files of the fixture library: sidecars of both namings and housekeeping files.
fn extra_files() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (
            "travel/sidecar-base.xmp",
            sidecar(2, "2018-04-05T06:07:08", &["FromSidecar"]).into_bytes(),
        ),
        (
            "travel/sidecar-full.jpg.xmp",
            sidecar(3, "2017-01-01T00:00:00+00:00", &["Lake", "FromFullSidecar"]).into_bytes(),
        ),
        ("Thumbs.db", b"thumbs".to_vec()),
        ("plain/desktop.ini", b"[.ShellClassInfo]".to_vec()),
        ("plain/notes.txt", b"not media".to_vec()),
    ]
}

pub fn run_command(command: &mut Command) -> anyhow::Result<()> {
    let output = command.output().with_context(|| format!("can't run {command:?}"))?;
    if !output.status.success() {
        bail!(
            "{command:?} failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Writes the fixture library into `library`. Tags are written to an ASCII-named copy, which is then moved into place.
pub fn generate(library: &Path, exiftool: &Path) -> anyhow::Result<()> {
    for (index, fixture) in FIXTURES.iter().enumerate() {
        let target = library.join(fixture.path);
        std::fs::create_dir_all(target.parent().context("fixture without folder")?)?;
        let ext = Path::new(fixture.path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("bin");
        let staging = library.join(format!("staging-{index}.{ext}"));
        match fixture.content {
            Content::Jpeg(w, h) => gradient(w, h).save_with_format(&staging, image::ImageFormat::Jpeg)?,
            Content::Png(w, h) => gradient(w, h).save_with_format(&staging, image::ImageFormat::Png)?,
            Content::Bytes(bytes) => std::fs::write(&staging, bytes)?,
            Content::Video => run_command(
                Command::new("ffmpeg")
                    .args([
                        "-v",
                        "error",
                        "-f",
                        "lavfi",
                        "-i",
                        "testsrc=size=64x48:rate=10",
                        "-t",
                        "1",
                        "-pix_fmt",
                        "yuv420p",
                        "-metadata",
                        "creation_time=2020-05-01T10:00:00Z",
                    ])
                    .arg(&staging),
            )
            .context("ffmpeg is needed for the video fixture")?,
        }
        if !fixture.tags.is_empty() {
            // An argument file keeps non-ASCII tag values intact (the Windows command line doesn't).
            let args = library.join(format!("staging-{index}.args"));
            std::fs::write(&args, fixture.tags.join("\n"))?;
            run_command(
                Command::new(exiftool)
                    .args(["-q", "-overwrite_original", "-charset", "utf8", "-@"])
                    .arg(&args)
                    .arg(&staging),
            )?;
            std::fs::remove_file(args)?;
        }
        std::fs::rename(&staging, &target)?;
    }
    for (path, bytes) in extra_files() {
        let target = library.join(path);
        std::fs::create_dir_all(target.parent().context("file without folder")?)?;
        std::fs::write(target, bytes)?;
    }
    Ok(())
}

fn gradient(w: u32, h: u32) -> image::DynamicImage {
    image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x * 255 / w) as u8, (y * 255 / h) as u8, 128])
    }))
}

/// Indexes `library` into `index` with the reference.
fn index_with_reference(reference: &Path, work: &Path, library: &Path, index: &Path) -> anyhow::Result<()> {
    let oracle = Path::new(env!("CARGO_MANIFEST_DIR")).join("oracle");
    let bundle = work.join("reference-index.cjs");
    run_command(
        Command::new("node")
            .arg(oracle.join("build-reference.mjs"))
            .arg(reference)
            .arg(&bundle),
    )
    .context("bundling the reference (needs Node and the reference's node_modules)")?;
    run_command(
        Command::new("node")
            .env("NODE_PATH", reference.join("node_modules"))
            .arg(&bundle)
            .arg(library)
            .arg(index)
            .arg(reference.join("drizzle")),
    )
}

/// Indexes `library` into `index` with this indexer, as a library labelled `label`.
pub fn index_with_ours(exiftool: PathBuf, library: &Path, index: &Path, label: &str) -> anyhow::Result<()> {
    let conn = db::open(index)?;
    let path = library.to_str().context("library path isn't UTF-8")?;
    let id = libraries::insert(&conn, path, label, LibraryStatus::Unknown)?.id;
    let mut writer = IndexWriter::new(conn);
    let metadata = ExifTool::new(exiftool, 2);
    let cancel = AtomicBool::new(false);
    let mut on_progress = |_| {};
    let outcome = scan_root(
        ScanDeps {
            writer: &mut writer,
            metadata: &metadata,
            is_reachable: &|_: &Path| true,
        },
        id,
        &ScanScope::Full,
        ScanOptions::new(2, &cancel, &mut on_progress),
    )?;
    if outcome != ScanOutcome::Completed {
        bail!("our scan ended {outcome:?}");
    }
    Ok(())
}

/// Runs the oracle; `Ok(false)` when there are undocumented differences.
pub fn run(args: &[String]) -> anyhow::Result<bool> {
    const USAGE: &str = "usage: cargo xtask oracle [--reference <dir>] [--keep <dir>]";
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut reference = root.join("..").join("good-gallery");
    let mut keep = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--reference" => reference = args.next().context(USAGE)?.into(),
            "--keep" => keep = Some(PathBuf::from(args.next().context(USAGE)?)),
            other => bail!("unknown argument {other} ({USAGE})"),
        }
    }
    let reference = std::path::absolute(&reference)?;
    let temp = tempfile::tempdir()?;
    let work = match &keep {
        Some(dir) => {
            if dir.exists() {
                bail!("{} exists already", dir.display());
            }
            std::fs::create_dir_all(dir)?;
            std::path::absolute(dir)?
        }
        None => temp.path().to_path_buf(),
    };
    let exiftool = find_exiftool().context("exiftool not found (GOOD_GALLERY_EXIFTOOL or PATH)")?;
    let library = work.join("library");
    std::fs::create_dir_all(&library)?;
    generate(&library, &exiftool)?;
    println!("fixture library: {} files", FIXTURES.len() + extra_files().len());

    let (reference_index, our_index) = (work.join("reference.db"), work.join("ours.db"));
    index_with_reference(&reference, &work, &library, &reference_index)?;
    index_with_ours(exiftool.clone(), &library, &our_index, "Fixtures")?;

    let reference_rows = snapshot(&db::open_reader(&reference_index)?)?;
    let our_rows = snapshot(&db::open_reader(&our_index)?)?;
    let media = our_rows.get("media").map_or(0, BTreeMap::len);
    let tags = our_rows.get("tags").map_or(0, BTreeMap::len);
    println!("compared: {media} media, {tags} tags");

    let differences = diff(&reference_rows, &our_rows);
    let (documented, unexpected): (Vec<_>, Vec<_>) = differences.iter().partition(|d| deviation_of(d).is_some());
    for difference in &documented {
        let reason = deviation_of(difference).map_or("", |d| d.reason);
        println!("documented: {difference}\n            ({reason})");
    }
    for difference in &unexpected {
        println!("DIFFERENT:  {difference}");
    }
    let unused: Vec<_> = DEVIATIONS
        .iter()
        .filter(|d| {
            !documented
                .iter()
                .any(|diff| deviation_of(diff).is_some_and(|found| std::ptr::eq(found, *d)))
        })
        .collect();
    // Not a failure: some deviations depend on the computer (the GPS zone matches when it is Tokyo's).
    for deviation in &unused {
        println!(
            "not seen:   documented deviation {} [{}]",
            deviation.table, deviation.key
        );
    }
    println!(
        "{} documented, {} unexpected differences",
        documented.len(),
        unexpected.len()
    );
    let searches_match = crate::oracle_search::run(&reference, &work, &reference_index)?;
    let thumbnails_match = crate::oracle_thumbs::run(&reference, &work, &exiftool)?;
    if let Some(dir) = &keep {
        println!("kept in {}", dir.display());
    }
    Ok(unexpected.is_empty() && searches_match && thumbnails_match)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open(&dir.path().join("gallery.db")).unwrap();
        (dir, conn)
    }

    fn fill(conn: &Connection, width: i64, scan_time: i64) {
        conn.execute_batch(&format!(
            "INSERT INTO library_roots (path, label, status, last_scan_at) VALUES ('D:\\x', 'x', 'online', {scan_time});
             INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime, width)
               VALUES (1, 'a/b.jpg', 'b.jpg', 'image', 1, 2, {width});
             INSERT INTO tags (name, name_norm) VALUES ('Beach', 'beach');
             INSERT INTO media_tags (media_id, tag_id)
               SELECT (SELECT id FROM media), (SELECT id FROM tags WHERE name_norm = 'beach');
             INSERT INTO folders (root_id, rel_dir, mtime) VALUES (1, 'a', 5);"
        ))
        .unwrap();
    }

    #[test]
    fn equal_indexes_have_no_differences_even_with_other_ids_and_scan_times() {
        let (_a, a) = index();
        let (_b, b) = index();
        fill(&a, 64, 1);
        // Other ids for the same rows.
        b.execute_batch("INSERT INTO tags (name, name_norm) VALUES ('Gone', 'gone'); DELETE FROM tags;")
            .unwrap();
        fill(&b, 64, 2);
        let tag_ids: (i64, i64) = (
            a.query_row("SELECT id FROM tags", [], |r| r.get(0)).unwrap(),
            b.query_row("SELECT id FROM tags", [], |r| r.get(0)).unwrap(),
        );
        assert_ne!(tag_ids.0, tag_ids.1);

        assert_eq!(diff(&snapshot(&a).unwrap(), &snapshot(&b).unwrap()), []);
    }

    #[test]
    fn reports_changed_values_and_rows_in_only_one_index() {
        let (_a, a) = index();
        let (_b, b) = index();
        fill(&a, 64, 1);
        fill(&b, 32, 1);
        b.execute_batch(
            "INSERT INTO tags (name, name_norm) VALUES ('Bob', 'bob');
             INSERT INTO media_tags (media_id, tag_id) SELECT 1, id FROM tags WHERE name_norm = 'bob';",
        )
        .unwrap();

        let differences = diff(&snapshot(&a).unwrap(), &snapshot(&b).unwrap());

        let shown: Vec<String> = differences.iter().map(ToString::to_string).collect();
        assert_eq!(
            shown,
            [
                "media [a/b.jpg].width: reference 64, ours 32",
                "tags [bob]: reference (missing), ours row",
                "media_tags [a/b.jpg → bob]: reference (missing), ours row",
            ]
        );
    }

    #[test]
    fn documented_deviations_match_by_table_key_and_column() {
        let gps = Difference {
            table: "media",
            key: "travel/tokyo-gps.jpg".into(),
            column: Some("taken_at".into()),
            reference: Some("1".into()),
            ours: Some("2".into()),
        };
        assert!(deviation_of(&gps).is_some());
        let other_column = Difference {
            column: Some("width".into()),
            ..gps
        };
        assert!(deviation_of(&other_column).is_none());
    }
}
