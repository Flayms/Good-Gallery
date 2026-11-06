//! Turns a photo by a quarter through its orientation tag alone, with exiftool (REQ-009-22).

use crate::rating::{WriteError, library_path, path_arg};
use domain::orientation::{Turn, turned};
use std::path::Path;

/// Turns the photo `rel_path` of the library in `library` by `turn`: its orientation tag, in every group it has one,
/// becomes the one turned a quarter from it; the pixels and the date modified stay. Returns the new tag. A file that
/// can't be written stays as it was.
pub fn turn(exiftool: &Path, library: &Path, rel_path: &str, turn: Turn) -> Result<u8, WriteError> {
    let file = library_path(library, rel_path).ok_or(WriteError::UnsafePath)?;
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    if !domain::orientation::can_turn(name) {
        return Err(WriteError::NoOrientation);
    }
    crate::replace::recover(&file)?;
    if !file.is_file() {
        return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
    }
    if std::fs::metadata(&file)?.permissions().readonly() {
        return Err(WriteError::ReadOnly);
    }
    let args = ["-charset", "filename=utf8", "-j", "-n", "-Orientation"].map(String::from);
    let output = crate::exiftool::run_once(exiftool, None, &[&args[..], &[path_arg(&file)]].concat())
        .map_err(WriteError::Exiftool)?;
    let read: serde_json::Value = serde_json::from_str(&output).unwrap_or_default();
    let now = read[0]["Orientation"]
        .as_u64()
        .and_then(|tag| u8::try_from(tag).ok())
        .unwrap_or(1);
    let next = turned(now, turn);
    // `#` writes the number; exiftool edits the tag in every group that has it, and adds it to EXIF otherwise.
    let args = [
        "-charset".into(),
        "filename=utf8".into(),
        format!("-Orientation#={next}"),
    ];
    crate::replace::write_metadata(exiftool, None, &args, &file)?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::SystemTime;

    fn exiftool() -> Option<PathBuf> {
        drop(crate::exiftool::tests::exiftool()?);
        crate::exiftool::find_exiftool()
    }

    /// An 8 × 4 photo: red on the left half, blue on the right.
    fn photo(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        image::RgbImage::from_fn(8, 4, |x, _| {
            if x < 4 {
                image::Rgb([220, 30, 30])
            } else {
                image::Rgb([30, 30, 220])
            }
        })
        .save(&path)
        .unwrap();
        path
    }

    fn orientation(exiftool: &Path, path: &Path) -> Option<u64> {
        let output = Command::new(exiftool)
            .args(["-j", "-n", "-Orientation"])
            .arg(path)
            .output()
            .unwrap();
        let read: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        read[0]["Orientation"].as_u64()
    }

    /// The stored pixels, not turned.
    fn pixels(path: &Path) -> image::RgbImage {
        image::ImageReader::open(path).unwrap().decode().unwrap().into_rgb8()
    }

    #[test]
    fn req_009_22_changes_only_the_orientation_tag() {
        let Some(exiftool) = exiftool() else { return };
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.jpg", "b.png", "c.webp"] {
            let file = photo(dir.path(), name);
            let modified = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
            std::fs::File::options()
                .write(true)
                .open(&file)
                .unwrap()
                .set_modified(modified)
                .unwrap();
            let before = pixels(&file);

            assert_eq!(turn(&exiftool, dir.path(), name, Turn::Right).unwrap(), 6, "{name}");
            assert_eq!(orientation(&exiftool, &file), Some(6), "{name}");
            assert_eq!(pixels(&file), before, "{name}: the pixels stay");
            assert_eq!(
                std::fs::metadata(&file).unwrap().modified().unwrap(),
                modified,
                "{name}"
            );
            let upright = crate::original::load_original(&file, None, 100, None).unwrap();
            assert_eq!(upright.dimensions(), (4, 8), "{name}: shown turned");
            // Red was on the left; turned right it is on top.
            assert!(upright.get_pixel(2, 1)[0] > 150, "{name}");

            assert_eq!(turn(&exiftool, dir.path(), name, Turn::Left).unwrap(), 1, "{name}");
            assert_eq!(turn(&exiftool, dir.path(), name, Turn::Left).unwrap(), 8, "{name}");
            assert_eq!(orientation(&exiftool, &file), Some(8), "{name}");
        }
    }

    #[test]
    fn req_009_22_a_mirrored_photo_stays_mirrored() {
        let Some(exiftool) = exiftool() else { return };
        let dir = tempfile::tempdir().unwrap();
        let file = photo(dir.path(), "a.jpg");
        let status = Command::new(&exiftool)
            .args(["-n", "-Orientation=2", "-overwrite_original", "-q"])
            .arg(&file)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(turn(&exiftool, dir.path(), "a.jpg", Turn::Right).unwrap(), 7);
        assert_eq!(orientation(&exiftool, &file), Some(7));
    }

    #[test]
    fn req_009_22_a_read_only_file_stays() {
        let Some(exiftool) = exiftool() else { return };
        let dir = tempfile::tempdir().unwrap();
        let file = photo(dir.path(), "a.jpg");
        let bytes = std::fs::read(&file).unwrap();
        let mut permissions = std::fs::metadata(&file).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions.clone()).unwrap();

        assert!(matches!(
            turn(&exiftool, dir.path(), "a.jpg", Turn::Right),
            Err(WriteError::ReadOnly)
        ));
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        #[expect(
            clippy::permissions_set_readonly_false,
            reason = "so the temporary folder can be removed"
        )]
        permissions.set_readonly(false);
        std::fs::set_permissions(&file, permissions).unwrap();
        assert!(
            turn(&exiftool, dir.path(), "../a.jpg", Turn::Right).is_err(),
            "never outside the library"
        );
    }
}
