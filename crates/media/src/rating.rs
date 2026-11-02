//! Writes a rating set by hand into a file or its sidecar with exiftool (spec 024).

use domain::media::MediaKind;
use domain::rating::{MAX, Target, sidecar_name};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("the file is read-only")]
    ReadOnly,
    #[error("the path leaves its library")]
    UnsafePath,
    #[error("exiftool: {0}")]
    Exiftool(String),
    #[error(transparent)]
    Replace(#[from] crate::replace::Error),
}

/// Writes `rating` (`None` removes it) for the media item `rel_path` of the library in `library`: into the file or
/// its sidecar, and into an existing sidecar that has a rating too (REQ-024-03). The date modified stays. A file that
/// can't be written stays as it was (REQ-024-04, REQ-024-06).
pub fn write(
    exiftool: &Path,
    library: &Path,
    rel_path: &str,
    kind: MediaKind,
    rating: Option<i64>,
) -> Result<(), WriteError> {
    let file = library_path(library, rel_path).ok_or(WriteError::UnsafePath)?;
    crate::replace::recover(&file)?;
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let folder = file.parent().ok_or(WriteError::UnsafePath)?;
    let sidecar = existing_sidecar(folder, name)?;
    let file_has_rating = kind == MediaKind::Video && has_rating(exiftool, &file)?;
    let mut targets = vec![match Target::of(kind, file_has_rating) {
        Target::File => file.clone(),
        Target::Sidecar => sidecar.clone().unwrap_or_else(|| folder.join(sidecar_name(name))),
    }];
    if let Some(sidecar) = sidecar
        && !targets.contains(&sidecar)
        && has_rating(exiftool, &sidecar)?
    {
        targets.push(sidecar);
    }
    // Nothing is written unless every target can be: a read-only file isn't replaced behind the user's back.
    for target in &targets {
        if target.exists() && std::fs::metadata(target)?.permissions().readonly() {
            return Err(WriteError::ReadOnly);
        }
    }
    if !file.is_file() {
        return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
    }
    let value = rating.unwrap_or(0).clamp(0, MAX);
    for target in targets {
        let mut args = vec![
            "-charset".into(),
            "filename=utf8".into(),
            format!("-XMP-xmp:Rating={value}"),
        ];
        if target.exists() {
            // Every rating the file already has, so none of them disagrees.
            args.push(format!("-Rating={value}"));
            crate::replace::write_metadata(exiftool, None, &args, &target)?;
        } else {
            crate::replace::create(exiftool, None, &args, &target)?;
        }
    }
    Ok(())
}

pub(crate) fn library_path(library: &Path, rel_path: &str) -> Option<PathBuf> {
    domain::media::is_safe_rel_path(rel_path).then(|| {
        let mut path = library.to_path_buf();
        path.extend(rel_path.split('/'));
        path
    })
}

/// The sidecar of `name` in `folder`, as indexing finds it (REQ-004-10).
pub(crate) fn existing_sidecar(folder: &Path, name: &str) -> std::io::Result<Option<PathBuf>> {
    let mut sidecars = std::collections::HashMap::<String, Vec<String>>::new();
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        let Some(sidecar) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if domain::media::is_sidecar(&sidecar) {
            sidecars.entry(sidecar.to_lowercase()).or_default().push(sidecar);
        }
    }
    Ok(domain::media::find_sidecar(&sidecars, name).map(|sidecar| folder.join(sidecar)))
}

/// Whether `file` has a `Rating` tag of its own. A file exiftool can't read has none.
fn has_rating(exiftool: &Path, file: &Path) -> Result<bool, WriteError> {
    let args = ["-charset", "filename=utf8", "-j", "-Rating"].map(String::from);
    let output = crate::exiftool::run_once(exiftool, None, &[&args[..], &[path_arg(file)]].concat())
        .map_err(WriteError::Exiftool)?;
    let read: serde_json::Value = serde_json::from_str(&output).unwrap_or_default();
    Ok(read[0].get("Rating").is_some())
}

/// A path for exiftool. Library paths are absolute, so they can't be taken for an option (REQ-011-09).
pub(crate) fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::process::Command;
    use std::time::SystemTime;

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

        fn photo(&self, name: &str) -> PathBuf {
            let path = self.path(name);
            image::RgbImage::from_pixel(8, 8, image::Rgb([200, 90, 40]))
                .save(&path)
                .unwrap();
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

        /// Every `Rating` tag in `path`, by group.
        fn ratings(&self, path: &Path) -> Value {
            let output = Command::new(&self.exiftool)
                .args(["-j", "-n", "-G1", "-a", "-Rating"])
                .arg(path)
                .output()
                .unwrap();
            let mut read: Value = serde_json::from_slice(&output.stdout).unwrap();
            read[0].as_object_mut().unwrap().remove("SourceFile");
            read[0].take()
        }

        fn write(&self, name: &str, kind: MediaKind, rating: Option<i64>) -> Result<(), WriteError> {
            write(&self.exiftool, self.dir.path(), name, kind, rating)
        }
    }

    fn mtime(path: &Path) -> SystemTime {
        std::fs::metadata(path).unwrap().modified().unwrap()
    }

    /// REQ-024-03: a photo gets the rating in every `Rating` it has and in XMP; its date modified stays.
    #[test]
    fn req_024_03_writes_a_photos_rating_into_the_photo() {
        let Some(lib) = library() else { return };
        let photo = lib.photo("a.jpg");
        lib.tag(&photo, &["-IFD0:Rating=1"]);
        let before = mtime(&photo);

        lib.write("a.jpg", MediaKind::Image, Some(4)).unwrap();
        assert_eq!(lib.ratings(&photo), json!({ "IFD0:Rating": 4, "XMP-xmp:Rating": 4 }));
        assert_eq!(mtime(&photo), before);
        assert!(!lib.path("a.jpg.xmp").exists(), "no sidecar for a photo");

        let png = lib.photo("b.png");
        lib.write("b.png", MediaKind::Image, Some(2)).unwrap();
        assert_eq!(lib.ratings(&png), json!({ "XMP-xmp:Rating": 2 }));
    }

    /// REQ-024-03: a removed rating is written as 0, and a sidecar's rating follows the file's.
    #[test]
    fn req_024_03_removes_the_rating_also_from_the_sidecar() {
        let Some(lib) = library() else { return };
        let photo = lib.photo("a.jpg");
        lib.tag(&photo, &["-XMP-xmp:Rating=5"]);
        let sidecar = lib.path("a.xmp");
        lib.tag(&photo, &["-o", sidecar.to_str().unwrap()]);
        assert_eq!(lib.ratings(&sidecar), json!({ "XMP-xmp:Rating": 5 }));

        lib.write("a.jpg", MediaKind::Image, None).unwrap();
        assert_eq!(lib.ratings(&photo), json!({ "XMP-xmp:Rating": 0 }));
        assert_eq!(lib.ratings(&sidecar), json!({ "XMP-xmp:Rating": 0 }));
    }

    /// REQ-024-03: a video's rating goes into a new `<file name>.xmp`, or the sidecar it has; the video stays as it was.
    #[test]
    fn req_024_03_writes_a_videos_rating_into_its_sidecar() {
        let Some(lib) = library() else { return };
        let video = lib.video("walk.mp4");
        let bytes = std::fs::read(&video).unwrap();

        lib.write("walk.mp4", MediaKind::Video, Some(3)).unwrap();
        assert_eq!(lib.ratings(&lib.path("walk.mp4.xmp")), json!({ "XMP-xmp:Rating": 3 }));
        assert_eq!(std::fs::read(&video).unwrap(), bytes);

        let other = lib.video("clip.mov");
        lib.tag(&lib.path("clip.xmp"), &["-XMP-dc:Title=Clip"]);
        lib.write("clip.mov", MediaKind::Video, Some(2)).unwrap();
        assert_eq!(lib.ratings(&lib.path("clip.xmp")), json!({ "XMP-xmp:Rating": 2 }));
        assert!(!lib.path("clip.mov.xmp").exists());
        assert_eq!(lib.ratings(&other), json!({}));
    }

    /// REQ-024-03: a video that carries a rating itself is changed in place, since its own rating is read first.
    #[test]
    fn req_024_03_writes_a_videos_own_rating_in_place() {
        let Some(lib) = library() else { return };
        let video = lib.video("walk.mp4");
        lib.tag(&video, &["-XMP-xmp:Rating=1"]);

        lib.write("walk.mp4", MediaKind::Video, Some(5)).unwrap();
        assert_eq!(lib.ratings(&video), json!({ "XMP-xmp:Rating": 5 }));
        assert!(!lib.path("walk.mp4.xmp").exists());
    }

    /// REQ-024-06: temporary files left by an ended exiftool, of this app's or an older write, don't block the next one.
    #[test]
    fn req_024_06_leftover_temporary_files_dont_block_the_next_write() {
        let Some(lib) = library() else { return };
        let photo = lib.photo("a.jpg");
        std::fs::write(lib.path("a.jpg_exiftool_tmp"), b"partial").unwrap();
        std::fs::write(lib.path(".a.jpg.gg-write.jpg"), b"partial").unwrap();

        lib.write("a.jpg", MediaKind::Image, Some(3)).unwrap();
        assert_eq!(lib.ratings(&photo), json!({ "XMP-xmp:Rating": 3 }));
        assert!(!lib.path(".a.jpg.gg-write.jpg").exists());
        // An older version's file may hold the only copy of a photo (#140): it stays for the user.
        assert_eq!(std::fs::read(lib.path("a.jpg_exiftool_tmp")).unwrap(), b"partial");
    }

    /// REQ-024-06: the date modified to the sub-second, the date created and the permissions stay.
    #[test]
    fn req_024_06_keeps_the_dates_and_permissions() {
        let Some(lib) = library() else { return };
        let photo = lib.photo("a.jpg");
        let modified = SystemTime::UNIX_EPOCH + std::time::Duration::new(1_700_000_000, 123_456_700);
        let file = std::fs::File::options().write(true).open(&photo).unwrap();
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut times = std::fs::FileTimes::new().set_modified(modified);
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileTimesExt as _;
            times = times.set_created(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000));
        }
        file.set_times(times).unwrap();
        drop(file);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&photo, std::fs::Permissions::from_mode(0o640)).unwrap();
        }
        let before = std::fs::metadata(&photo).unwrap();

        lib.write("a.jpg", MediaKind::Image, Some(2)).unwrap();
        let after = std::fs::metadata(&photo).unwrap();
        assert_eq!(after.modified().unwrap(), modified);
        #[cfg(windows)]
        assert_eq!(after.created().unwrap(), before.created().unwrap());
        assert_eq!(after.permissions(), before.permissions());
    }

    /// REQ-024-06: a file system that loses the new version while it says it renamed it (#140) leaves the photo as
    /// it was, and the write fails.
    #[test]
    fn req_024_06_a_lost_replacement_leaves_the_file_as_it_was() {
        let Some(lib) = library() else { return };
        let photo = lib.photo("a.jpg");
        lib.tag(&photo, &["-XMP-xmp:Rating=1"]);
        let bytes = std::fs::read(&photo).unwrap();

        let written = crate::replace::tests::losing_renames(|| lib.write("a.jpg", MediaKind::Image, Some(5)));
        assert!(written.is_err());
        assert_eq!(std::fs::read(&photo).unwrap(), bytes);
        let names: Vec<_> = std::fs::read_dir(lib.dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["a.jpg"], "no temporary file is left");
    }

    /// REQ-024-04: a read-only file, a missing one and a path outside the library are left alone, with the reason.
    #[test]
    fn req_024_04_leaves_files_it_cant_write_as_they_were() {
        let Some(lib) = library() else { return };
        let photo = lib.photo("a.jpg");
        lib.tag(&photo, &["-XMP-xmp:Rating=2"]);
        let mut permissions = std::fs::metadata(&photo).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&photo, permissions).unwrap();

        assert!(matches!(
            lib.write("a.jpg", MediaKind::Image, Some(5)),
            Err(WriteError::ReadOnly)
        ));
        assert_eq!(lib.ratings(&photo), json!({ "XMP-xmp:Rating": 2 }));
        assert!(matches!(
            lib.write("gone.jpg", MediaKind::Image, Some(5)),
            Err(WriteError::Io(_))
        ));
        assert!(!lib.path("gone.jpg").exists());
        assert!(matches!(
            lib.write("../a.jpg", MediaKind::Image, Some(5)),
            Err(WriteError::UnsafePath)
        ));
    }
}
