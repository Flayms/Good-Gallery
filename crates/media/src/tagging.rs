//! Writes a tag added or removed by hand into a file and its sidecar with exiftool (spec 026).

use crate::rating::{WriteError, existing_sidecar, library_path, path_arg};
use domain::media::MediaKind;
use domain::rating::{Target, sidecar_name};
use domain::tagging::{Change, Keywords};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The keyword fields as exiftool names them, in the order of [`Keywords`]'s list fields.
const HIERARCHICAL: &str = "XMP-lr:HierarchicalSubject";
const SUBJECT: &str = "XMP-dc:Subject";
const IPTC: &str = "IPTC:Keywords";
const PDF: &str = "XMP-pdf:Keywords";
const XP: &str = "IFD0:XPKeywords";
const PEOPLE: &str = "XMP-iptcExt:PersonInImage";

/// Applies `change` to the media item `rel_path` of the library in `library` (REQ-026-03): an added tag goes into the
/// file, or a video's sidecar unless the video has keywords itself; a removed one leaves the file and its sidecar.
/// The date modified stays. A file that can't be written stays as it was (REQ-026-04, REQ-026-07).
pub fn write(
    exiftool: &Path,
    library: &Path,
    rel_path: &str,
    kind: MediaKind,
    change: &Change,
) -> Result<(), WriteError> {
    let file = library_path(library, rel_path).ok_or(WriteError::UnsafePath)?;
    crate::replace::recover(&file)?;
    if !file.is_file() {
        return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
    }
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let folder = file.parent().ok_or(WriteError::UnsafePath)?;
    let sidecar = existing_sidecar(folder, name)?;
    let file_keywords = read(exiftool, &file)?;
    // (where, what it has, what it gets)
    let mut writes: Vec<(PathBuf, Keywords, Keywords)> = Vec::new();
    match change {
        Change::Add(path) => {
            let target = match Target::of(kind, !file_keywords.is_empty()) {
                Target::File => file.clone(),
                Target::Sidecar => sidecar.clone().unwrap_or_else(|| folder.join(sidecar_name(name))),
            };
            let before = if target == file {
                file_keywords
            } else if target.exists() {
                read(exiftool, &target)?
            } else {
                Keywords::default()
            };
            let after = domain::tagging::add(&before, path);
            writes.push((target, before, after));
        }
        Change::Remove(tag) => {
            let after = domain::tagging::remove(&file_keywords, tag);
            writes.push((file.clone(), file_keywords, after));
            if let Some(sidecar) = &sidecar {
                let before = read(exiftool, sidecar)?;
                let after = domain::tagging::remove(&before, tag);
                writes.push((sidecar.clone(), before, after));
            }
        }
    }
    writes.retain(|(_, before, after)| before != after);
    // Nothing is written unless every target can be.
    for (target, _, _) in &writes {
        if target.exists() && std::fs::metadata(target)?.permissions().readonly() {
            return Err(WriteError::ReadOnly);
        }
    }
    for (target, before, after) in writes {
        let mut args = vec!["-charset".to_owned(), "filename=utf8".to_owned()];
        let lists = [
            (HIERARCHICAL, &before.hierarchical, &after.hierarchical),
            (SUBJECT, &before.subject, &after.subject),
            (IPTC, &before.iptc, &after.iptc),
            (PDF, &before.pdf, &after.pdf),
            (PEOPLE, &before.people, &after.people),
        ];
        for (tag, before, after) in lists {
            if before != after {
                // `-Tag=` empties the list; the values after it make the new one.
                args.push(format!("-{tag}="));
                args.extend(after.iter().map(|value| format!("-{tag}={value}")));
            }
        }
        if before.xp != after.xp {
            args.push(format!("-{XP}={}", after.xp.as_deref().unwrap_or_default()));
        }
        if target.exists() {
            crate::replace::write_metadata(exiftool, None, &args, &target)?;
        } else {
            crate::replace::create(exiftool, None, &args, &target)?;
        }
    }
    Ok(())
}

/// The keyword fields of `file`; a file exiftool can't read has none.
fn read(exiftool: &Path, file: &Path) -> Result<Keywords, WriteError> {
    let mut args: Vec<String> = ["-charset", "filename=utf8", "-j", "-G1", "-struct"]
        .map(String::from)
        .to_vec();
    args.extend([HIERARCHICAL, SUBJECT, IPTC, PDF, XP, PEOPLE].map(|tag| format!("-{tag}")));
    args.push(path_arg(file));
    let output = crate::exiftool::run_once(exiftool, None, &args).map_err(WriteError::Exiftool)?;
    let read: Value = serde_json::from_str(&output).unwrap_or_default();
    let tags = &read[0];
    let list = |tag: &str| -> Vec<String> {
        match &tags[tag] {
            Value::Array(items) => items.iter().filter_map(text).collect(),
            Value::Null => Vec::new(),
            item => text(item).into_iter().collect(),
        }
    };
    Ok(Keywords {
        hierarchical: list(HIERARCHICAL),
        subject: list(SUBJECT),
        iptc: list(IPTC),
        pdf: list(PDF),
        xp: text(&tags[XP]).filter(|xp| !xp.is_empty()),
        people: list(PEOPLE),
    })
}

/// A keyword as text: exiftool returns numeric keywords (`2024`) as numbers.
fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::process::Command;

    struct Library {
        dir: tempfile::TempDir,
        exiftool: PathBuf,
    }

    fn library() -> Option<Library> {
        drop(crate::exiftool::tests::exiftool()?);
        Some(Library {
            dir: tempfile::tempdir().unwrap(),
            exiftool: crate::exiftool::find_exiftool().unwrap(),
        })
    }

    impl Library {
        fn path(&self, rel: &str) -> PathBuf {
            self.dir.path().join(rel)
        }

        fn photo(&self, name: &str, args: &[&str]) -> PathBuf {
            let path = self.path(name);
            image::RgbImage::from_pixel(8, 8, image::Rgb([200, 90, 40]))
                .save(&path)
                .unwrap();
            if !args.is_empty() {
                self.tag(&path, args);
            }
            path
        }

        fn video(&self, name: &str) -> PathBuf {
            let path = self.path(name);
            let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/tests/fixtures/Holiday/walk.mp4");
            std::fs::copy(fixture, &path).unwrap();
            path
        }

        fn tag(&self, path: &Path, args: &[&str]) {
            let status = Command::new(&self.exiftool)
                .args(args)
                .args(["-overwrite_original", "-q"])
                .arg(path)
                .status()
                .unwrap();
            assert!(status.success());
        }

        /// The keyword fields of `path`, by group.
        fn keywords(&self, path: &Path) -> Value {
            let output = Command::new(&self.exiftool)
                .args([
                    "-j",
                    "-G1",
                    "-HierarchicalSubject",
                    "-Subject",
                    "-Keywords",
                    "-XPKeywords",
                    "-PersonInImage",
                ])
                .arg(path)
                .output()
                .unwrap();
            let mut read: Value = serde_json::from_slice(&output.stdout).unwrap();
            read[0].as_object_mut().unwrap().remove("SourceFile");
            read[0].take()
        }

        fn write(&self, name: &str, kind: MediaKind, change: Change) -> Result<(), WriteError> {
            write(&self.exiftool, self.dir.path(), name, kind, &change)
        }
    }

    fn add(path: &[&str]) -> Change {
        Change::Add(path.iter().map(|level| level.to_string()).collect())
    }

    /// REQ-026-03: an added tag goes into the photo as its hierarchy and name; the date modified stays.
    #[test]
    fn req_026_03_adds_a_tag_to_a_photo() {
        let Some(lib) = library() else { return };
        let photo = lib.photo("a.jpg", &["-IPTC:Keywords=Beach"]);
        let before = std::fs::metadata(&photo).unwrap().modified().unwrap();

        lib.write("a.jpg", MediaKind::Image, add(&["Places", "Italy", "Rome"]))
            .unwrap();
        assert_eq!(
            lib.keywords(&photo),
            json!({
                "XMP-lr:HierarchicalSubject": "Places|Italy|Rome",
                "XMP-dc:Subject": "Rome",
                "IPTC:Keywords": ["Beach", "Rome"],
            })
        );
        assert_eq!(std::fs::metadata(&photo).unwrap().modified().unwrap(), before);
        assert!(!lib.path("a.jpg.xmp").exists());
    }

    /// REQ-026-07: a file system that loses the new version (#140) leaves the photo and a new sidecar as they were; a
    /// temporary file of an older write doesn't block the next one.
    #[test]
    fn req_026_07_a_failed_write_leaves_the_file_as_it_was() {
        let Some(lib) = library() else { return };
        let photo = lib.photo("a.jpg", &["-XMP-dc:Subject=Beach"]);
        let bytes = std::fs::read(&photo).unwrap();
        let written =
            crate::replace::tests::losing_renames(|| lib.write("a.jpg", MediaKind::Image, add(&["Places", "Rome"])));
        assert!(written.is_err());
        assert_eq!(std::fs::read(&photo).unwrap(), bytes);

        lib.video("walk.mp4");
        let written =
            crate::replace::tests::losing_renames(|| lib.write("walk.mp4", MediaKind::Video, add(&["Places", "Rome"])));
        assert!(written.is_err());
        let mut names: Vec<_> = std::fs::read_dir(lib.dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["a.jpg", "walk.mp4"], "no empty sidecar and no temporary file");

        std::fs::write(lib.path("a.jpg_exiftool_tmp"), b"partial").unwrap();
        lib.write("a.jpg", MediaKind::Image, add(&["Places", "Rome"])).unwrap();
        assert_eq!(lib.keywords(&photo)["XMP-lr:HierarchicalSubject"], "Places|Rome");
    }

    /// REQ-026-03: a video's tag goes into a new sidecar; the video stays as it was.
    #[test]
    fn req_026_03_adds_a_videos_tag_to_its_sidecar() {
        let Some(lib) = library() else { return };
        let video = lib.video("walk.mp4");
        let bytes = std::fs::read(&video).unwrap();
        lib.write("walk.mp4", MediaKind::Video, add(&["People", "Anna"]))
            .unwrap();
        assert_eq!(
            lib.keywords(&lib.path("walk.mp4.xmp")),
            json!({ "XMP-lr:HierarchicalSubject": "People|Anna", "XMP-dc:Subject": "Anna" })
        );
        assert_eq!(std::fs::read(&video).unwrap(), bytes);
    }

    /// REQ-026-03: a removed tag leaves every field of the photo and its sidecar, with its unused levels.
    #[test]
    fn req_026_03_removes_a_tag_everywhere() {
        let Some(lib) = library() else { return };
        let photo = lib.photo(
            "a.jpg",
            &[
                "-XMP-lr:HierarchicalSubject=Places|Italy|Rome",
                "-XMP-dc:Subject=Places",
                "-XMP-dc:Subject=Italy",
                "-XMP-dc:Subject=Rome",
                "-XMP-dc:Subject=Beach",
                "-IPTC:Keywords=Rome",
                "-XPKeywords=Beach;Rome",
            ],
        );
        let sidecar = lib.path("a.xmp");
        lib.tag(&photo, &["-o", sidecar.to_str().unwrap()]);
        lib.tag(&sidecar, &["-XMP-dc:Subject=Rome", "-XMP-dc:Subject=Sea"]);

        lib.write("a.jpg", MediaKind::Image, Change::Remove("rome".into()))
            .unwrap();
        assert_eq!(
            lib.keywords(&photo),
            json!({ "XMP-dc:Subject": "Beach", "IFD0:XPKeywords": "Beach" })
        );
        assert_eq!(lib.keywords(&sidecar), json!({ "XMP-dc:Subject": "Sea" }));
    }

    /// REQ-026-04: a read-only file keeps its tags.
    #[test]
    fn req_026_04_leaves_a_read_only_file_as_it_was() {
        let Some(lib) = library() else { return };
        let photo = lib.photo("a.jpg", &["-XMP-dc:Subject=Beach"]);
        let mut permissions = std::fs::metadata(&photo).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&photo, permissions).unwrap();
        assert!(matches!(
            lib.write("a.jpg", MediaKind::Image, Change::Remove("Beach".into())),
            Err(WriteError::ReadOnly)
        ));
        assert_eq!(lib.keywords(&photo), json!({ "XMP-dc:Subject": "Beach" }));
    }
}
