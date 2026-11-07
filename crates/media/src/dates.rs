//! Fixing dates taken in files (spec 038): reading the date fields of many photos through the exiftool pool, writing
//! a new date taken, and a journal that gives _Undo_, also after a restart (REQ-038-08, REQ-038-09).

use crate::exiftool::{ExifTool, ExifToolError};
use crate::rating::{WriteError, library_path};
use domain::date_fix::{Read, Stamp, parse_exif};
use domain::edit::MtimeOnSave;
use serde_json::{Map, Value, json};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The EXIF date tags a fix writes, by the names exiftool reads them with: what _Undo_ puts back.
pub const WRITTEN: [&str; 6] = [
    "DateTimeOriginal",
    "SubSecTimeOriginal",
    "OffsetTimeOriginal",
    "CreateDate",
    "SubSecTimeDigitized",
    "OffsetTimeDigitized",
];

/// The values of [`WRITTEN`] a file had, `None` for a tag it didn't have.
pub type Previous = Vec<(String, Option<String>)>;

/// What reading a photo gave: its dates, and the values a fix would replace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dates {
    pub read: Read,
    pub previous: Previous,
}

/// The dates of `files`, read in one exiftool run (REQ-038-02). A file exiftool can't read has no dates.
pub fn read(exiftool: &ExifTool, files: &[PathBuf]) -> Result<Vec<Dates>, ExifToolError> {
    let mut args: Vec<String> = ["-charset", "filename=utf8", "-json", "-fast"]
        .map(String::from)
        .to_vec();
    args.extend(WRITTEN.iter().map(|tag| format!("-EXIF:{tag}")));
    args.extend(["-XMP-photoshop:DateCreated".into(), "-Composite:GPSDateTime".into()]);
    Ok(exiftool.read_many(&args, files)?.iter().map(dates_of).collect())
}

fn text(tags: &Map<String, Value>, tag: &str) -> Option<String> {
    match tags.get(tag)? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// A date with the offset of its own tag (`OffsetTimeOriginal` for `DateTimeOriginal`).
fn stamp(tags: &Map<String, Value>, date: &str, offset: &str) -> Option<Stamp> {
    let date = text(tags, date)?;
    let offset = text(tags, offset).unwrap_or_default();
    parse_exif(&format!("{date}{offset}")).or_else(|| parse_exif(&date))
}

fn dates_of(tags: &Map<String, Value>) -> Dates {
    Dates {
        read: Read {
            taken: stamp(tags, "DateTimeOriginal", "OffsetTimeOriginal"),
            created: stamp(tags, "CreateDate", "OffsetTimeDigitized"),
            xmp_created: text(tags, "DateCreated").and_then(|date| parse_exif(&date)),
            gps: text(tags, "GPSDateTime")
                .and_then(|date| parse_exif(&date))
                .map(|stamp| match stamp.offset {
                    Some(offset) => stamp.at - chrono::Duration::seconds(i64::from(offset.local_minus_utc())),
                    None => stamp.at,
                }),
        },
        previous: WRITTEN.iter().map(|tag| (tag.to_string(), text(tags, tag))).collect(),
    }
}

/// Writes `new` as the date taken of `rel_path` in `library`: `DateTimeOriginal` and `CreateDate`, without the old
/// sub-seconds, with its offset if it has one (REQ-038-07, REQ-038-08). The date modified follows `mtime`
/// (REQ-021-15). A file that can't be written stays as it was.
pub fn write(
    exiftool: &Path,
    library: &Path,
    rel_path: &str,
    new: &Stamp,
    mtime: MtimeOnSave,
) -> Result<(), WriteError> {
    let date = new.exif_text();
    let mut values = vec![
        ("DateTimeOriginal", Some(date.clone())),
        ("CreateDate", Some(date)),
        ("SubSecTimeOriginal", None),
        ("SubSecTimeDigitized", None),
    ];
    // Without an offset of its own, the photo's stays (REQ-038-07).
    if let Some(offset) = new.offset_text() {
        values.extend([
            ("OffsetTimeOriginal", Some(offset.clone())),
            ("OffsetTimeDigitized", Some(offset)),
        ]);
    }
    let file = write_tags(exiftool, library, rel_path, &values)?;
    if mtime == MtimeOnSave::Update {
        set_mtime(&file, SystemTime::now())?;
    }
    Ok(())
}

/// Writes EXIF `values` (`None` removes the tag) into the file `rel_path` of `library`; its date modified stays.
fn write_tags(
    exiftool: &Path,
    library: &Path,
    rel_path: &str,
    values: &[(&str, Option<String>)],
) -> Result<PathBuf, WriteError> {
    let file = library_path(library, rel_path).ok_or(WriteError::UnsafePath)?;
    crate::replace::recover(&file)?;
    if !file.is_file() {
        return Err(io::Error::from(io::ErrorKind::NotFound).into());
    }
    if std::fs::metadata(&file)?.permissions().readonly() {
        return Err(WriteError::ReadOnly);
    }
    let mut args: Vec<String> = vec!["-charset".into(), "filename=utf8".into()];
    args.extend(
        values
            .iter()
            .map(|(tag, value)| format!("-EXIF:{tag}={}", value.as_deref().unwrap_or_default())),
    );
    crate::replace::write_metadata(exiftool, None, &args, &file)?;
    Ok(file)
}

fn set_mtime(file: &Path, at: SystemTime) -> io::Result<()> {
    std::fs::File::options()
        .write(true)
        .open(file)?
        .set_times(std::fs::FileTimes::new().set_modified(at))
}

/// Puts `previous` back into `rel_path`, with the date modified it had (REQ-038-09).
pub fn restore(
    exiftool: &Path,
    library: &Path,
    rel_path: &str,
    previous: &Previous,
    mtime: i64,
) -> Result<(), WriteError> {
    let values: Vec<(&str, Option<String>)> = previous
        .iter()
        .filter(|(tag, _)| WRITTEN.contains(&tag.as_str()))
        .map(|(tag, value)| (tag.as_str(), value.clone()))
        .collect();
    let file = write_tags(exiftool, library, rel_path, &values)?;
    let at = UNIX_EPOCH + Duration::from_millis(u64::try_from(mtime).unwrap_or(0));
    set_mtime(&file, at)?;
    Ok(())
}

/// A photo whose date a fix changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub root_id: i64,
    pub library: PathBuf,
    pub rel_path: String,
    pub new: Stamp,
    pub previous: Previous,
    /// Date modified before the fix, ms since the epoch.
    pub mtime: i64,
}

/// A row of the journal that was written: what _Undo_ needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub root_id: i64,
    pub library: PathBuf,
    pub rel_path: String,
    pub previous: Previous,
    pub mtime_before: i64,
    /// Size and date modified after writing: a file that has others has been changed since, and is skipped.
    pub size: u64,
    pub mtime_after: i64,
}

/// The file recording a date fix, a line per event, kept until the next fix (REQ-038-09).
pub struct Journal {
    file: std::fs::File,
}

impl Journal {
    /// Starts a journal for `changes` at `path`, replacing the last one.
    pub fn start(path: &Path, changes: &[Change]) -> io::Result<Self> {
        let rows: Vec<Value> = changes
            .iter()
            .map(|change| {
                // Pairs, not an object: the order stays.
                let previous: Vec<Value> = change.previous.iter().map(|(tag, value)| json!([tag, value])).collect();
                json!({
                    "root_id": change.root_id,
                    "library": change.library.to_string_lossy(),
                    "rel_path": change.rel_path,
                    "previous": previous,
                    "mtime": change.mtime,
                })
            })
            .collect();
        let mut file = std::fs::File::create(path)?;
        writeln!(file, "{}", json!({ "rows": rows }))?;
        file.sync_data()?;
        Ok(Self { file })
    }

    /// Change `index` was written; the file now has `size` and `mtime`.
    pub fn done(&mut self, index: usize, size: u64, mtime: i64) -> io::Result<()> {
        writeln!(self.file, "{}", json!({ "done": index, "size": size, "mtime": mtime }))
    }

    pub fn finish(&mut self) -> io::Result<()> {
        writeln!(self.file, "{}", json!({ "finished": true }))?;
        self.file.sync_data()
    }

    /// The rows written by the last fix if it finished; `None` without a finished one.
    pub fn read(path: &Path) -> io::Result<Option<Vec<Written>>> {
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut lines = io::BufReader::new(file).lines();
        let Some(first) = lines.next().transpose()? else {
            return Ok(None);
        };
        let header: Value = serde_json::from_str(&first).map_err(io::Error::other)?;
        let rows = header["rows"].as_array().cloned().unwrap_or_default();
        let mut written = Vec::new();
        let mut finished = false;
        for line in lines {
            let Ok(event) = serde_json::from_str::<Value>(&line?) else {
                continue;
            };
            if event["finished"].as_bool() == Some(true) {
                finished = true;
            }
            let Some(index) = event["done"].as_u64() else { continue };
            let Some(row) = rows.get(index as usize) else { continue };
            let previous = row["previous"]
                .as_array()
                .map(|pairs| {
                    pairs
                        .iter()
                        .filter_map(|pair| Some((pair[0].as_str()?.to_owned(), pair[1].as_str().map(str::to_owned))))
                        .collect()
                })
                .unwrap_or_default();
            written.push(Written {
                root_id: row["root_id"].as_i64().unwrap_or_default(),
                library: PathBuf::from(row["library"].as_str().unwrap_or_default()),
                rel_path: row["rel_path"].as_str().unwrap_or_default().to_owned(),
                previous,
                mtime_before: row["mtime"].as_i64().unwrap_or_default(),
                size: event["size"].as_u64().unwrap_or_default(),
                mtime_after: event["mtime"].as_i64().unwrap_or_default(),
            });
        }
        Ok(finished.then_some(written))
    }
}

/// A file's size and date modified (ms since the epoch).
pub fn size_and_mtime(file: &Path) -> io::Result<(u64, i64)> {
    let metadata = std::fs::metadata(file)?;
    let mtime = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64);
    Ok((metadata.len(), mtime))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rating::path_arg;
    use chrono::{FixedOffset, NaiveDateTime};

    fn exiftool() -> Option<(ExifTool, PathBuf)> {
        let pool = crate::exiftool::tests::exiftool()?;
        Some((pool, crate::exiftool::find_exiftool()?))
    }

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn photo(dir: &Path, name: &str, tags: &[&str], exiftool: &Path) -> PathBuf {
        let path = dir.join(name);
        image::RgbImage::from_pixel(4, 4, image::Rgb([10, 200, 30]))
            .save(&path)
            .unwrap();
        if !tags.is_empty() {
            let mut args: Vec<String> = tags.iter().map(|tag| tag.to_string()).collect();
            args.extend(["-overwrite_original".into(), path_arg(&path)]);
            crate::exiftool::run_once(exiftool, None, &args).unwrap();
        }
        path
    }

    /// REQ-038-02, REQ-038-07: the date fields of several photos at once, with their offsets; a photo without them
    /// has none.
    #[test]
    fn req_038_02_reads_the_dates_of_several_photos() {
        let Some((pool, exiftool)) = exiftool() else { return };
        let dir = tempfile::tempdir().unwrap();
        let a = photo(
            dir.path(),
            "a.jpg",
            &[
                "-DateTimeOriginal=2020:01:02 03:04:05",
                "-SubSecTimeOriginal=250",
                "-OffsetTimeOriginal=+02:00",
                "-CreateDate=2021:02:03 04:05:06",
                "-XMP-photoshop:DateCreated=2019:05:06 07:08:09",
                "-GPSDateStamp=2021:02:03",
                "-GPSTimeStamp=02:05:06",
            ],
            &exiftool,
        );
        let b = photo(dir.path(), "b é.jpg", &[], &exiftool);
        let dates = read(&pool, &[a, b, dir.path().join("gone.jpg")]).unwrap();
        assert_eq!(dates.len(), 3);
        let two = FixedOffset::east_opt(7200).unwrap();
        assert_eq!(
            dates[0].read.taken,
            Some(Stamp {
                at: at("2020-01-02 03:04:05"),
                offset: Some(two)
            })
        );
        assert_eq!(dates[0].read.created, Some(Stamp::zoneless(at("2021-02-03 04:05:06"))));
        assert_eq!(
            dates[0].read.xmp_created.map(|stamp| stamp.at),
            Some(at("2019-05-06 07:08:09"))
        );
        assert_eq!(dates[0].read.gps, Some(at("2021-02-03 02:05:06")));
        let previous: Vec<_> = dates[0]
            .previous
            .iter()
            .filter(|(_, value)| value.is_some())
            .map(|(tag, _)| tag.as_str())
            .collect();
        assert_eq!(
            previous,
            [
                "DateTimeOriginal",
                "SubSecTimeOriginal",
                "OffsetTimeOriginal",
                "CreateDate"
            ]
        );
        assert_eq!(dates[1].read, Read::default());
        assert!(dates[1].previous.iter().all(|(_, value)| value.is_none()));
        assert_eq!(dates[2].read, Read::default());
    }

    /// REQ-038-07, REQ-038-08, REQ-038-09: a new date replaces both dates and the sub-seconds, keeps the offset when it
    /// has none, and the old values come back with the old date modified.
    #[test]
    fn req_038_08_req_038_09_writes_and_restores_the_date_taken() {
        let Some((pool, exiftool)) = exiftool() else { return };
        let dir = tempfile::tempdir().unwrap();
        let file = photo(
            dir.path(),
            "a.jpg",
            &[
                "-DateTimeOriginal=2020:01:02 03:04:05",
                "-SubSecTimeOriginal=250",
                "-OffsetTimeOriginal=+02:00",
            ],
            &exiftool,
        );
        let before = read(&pool, std::slice::from_ref(&file)).unwrap().remove(0);
        let (_, mtime) = size_and_mtime(&file).unwrap();

        write(
            &exiftool,
            dir.path(),
            "a.jpg",
            &Stamp::zoneless(at("2023-05-12 10:15:00")),
            MtimeOnSave::Keep,
        )
        .unwrap();
        let after = read(&pool, std::slice::from_ref(&file)).unwrap().remove(0);
        let two = FixedOffset::east_opt(7200).unwrap();
        assert_eq!(
            after.read.taken,
            Some(Stamp {
                at: at("2023-05-12 10:15:00"),
                offset: Some(two)
            })
        );
        assert_eq!(
            after.read.created.map(|stamp| stamp.at),
            Some(at("2023-05-12 10:15:00"))
        );
        assert!(
            after
                .previous
                .iter()
                .any(|(tag, value)| tag == "SubSecTimeOriginal" && value.is_none())
        );
        assert_eq!(size_and_mtime(&file).unwrap().1, mtime, "date modified kept");

        let minus = FixedOffset::east_opt(-5 * 3600).unwrap();
        let stamp = Stamp {
            at: at("2022-01-01 00:00:00"),
            offset: Some(minus),
        };
        write(&exiftool, dir.path(), "a.jpg", &stamp, MtimeOnSave::Update).unwrap();
        assert_eq!(
            read(&pool, std::slice::from_ref(&file)).unwrap()[0].read.taken,
            Some(stamp)
        );
        assert!(size_and_mtime(&file).unwrap().1 > mtime, "date modified updated");

        restore(&exiftool, dir.path(), "a.jpg", &before.previous, mtime).unwrap();
        assert_eq!(read(&pool, std::slice::from_ref(&file)).unwrap().remove(0), before);
        assert_eq!(size_and_mtime(&file).unwrap().1, mtime);
        assert!(
            write(&exiftool, dir.path(), "../a.jpg", &stamp, MtimeOnSave::Keep).is_err(),
            "never outside"
        );
    }

    /// REQ-038-09: the journal gives the written rows of a finished fix, and nothing for an unfinished one.
    #[test]
    fn req_038_09_the_journal_keeps_what_undo_needs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("date-journal.json");
        let change = |rel: &str| Change {
            root_id: 1,
            library: dir.path().to_path_buf(),
            rel_path: rel.into(),
            new: Stamp::zoneless(at("2023-05-12 10:15:00")),
            previous: vec![
                ("DateTimeOriginal".into(), Some("2020:01:02 03:04:05".into())),
                ("CreateDate".into(), None),
            ],
            mtime: 1_000,
        };
        let mut journal = Journal::start(&path, &[change("a.jpg"), change("b.jpg")]).unwrap();
        journal.done(1, 42, 2_000).unwrap();
        assert_eq!(Journal::read(&path).unwrap(), None, "not finished");
        journal.finish().unwrap();
        let rows = Journal::read(&path).unwrap().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (
                rows[0].rel_path.as_str(),
                rows[0].size,
                rows[0].mtime_after,
                rows[0].mtime_before
            ),
            ("b.jpg", 42, 2_000, 1_000)
        );
        assert_eq!(rows[0].previous, change("b.jpg").previous);
        assert_eq!(Journal::read(&dir.path().join("none.json")).unwrap(), None);
    }
}
