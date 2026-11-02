//! Replacing a file with a new version without ever losing it (REQ-024-06, REQ-026-07, REQ-021-16, #140).
//!
//! The new version is a new file beside the original (exiftool's `-o`, never `-overwrite_original`), which gets the
//! original's dates and permissions and is synced. Then it takes the original's place, and the original is moved
//! aside first, so nothing is ever renamed over an existing file: an rclone mount on Windows loses a file renamed
//! over another one while it reports success, and exiftool's own replace deletes the original when its rename fails.
//! The file in place is read back afterwards; if it isn't the new version, the original is put back.

use std::hash::Hasher as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("exiftool: {0}")]
    Exiftool(String),
    #[error("exiftool wrote an empty file")]
    Empty,
    #[error("the file system didn't keep the new version, so the file was left as it was")]
    Lost,
    #[error("the file system didn't keep the new version, and the original couldn't be put back: it is in {0}")]
    Stranded(PathBuf),
}

/// A file beside `file` with `role` in its place while it is replaced: `.<name>.gg-<role>.<extension>`. Dot-named,
/// so indexing skips it (REQ-002-02); the extension stays, since exiftool writes the format it names.
pub fn beside(file: &Path, role: &str) -> PathBuf {
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    let extension = file
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    file.with_file_name(format!(".{name}.gg-{role}{extension}"))
}

/// Writes metadata into the existing `file` with exiftool's `args` (without a file): exiftool writes a new version
/// beside it, which takes its place ([`replace`]). The date modified, the date created on Windows and the
/// permissions stay.
pub fn write_metadata(exiftool: &Path, config: Option<&Path>, args: &[String], file: &Path) -> Result<(), Error> {
    recover(file)?;
    let new = beside(file, "write");
    exiftool_into(exiftool, config, args, file, &new)?;
    let replaced = keep_attributes(file, &new)
        .map_err(Error::from)
        .and_then(|()| replace(&new, file));
    if replaced.is_err() {
        let _ = std::fs::remove_file(&new);
    }
    replaced
}

/// Writes a new `file` (a sidecar) with exiftool's `args` (without a file), by way of a new file beside it.
pub fn create(exiftool: &Path, config: Option<&Path>, args: &[String], file: &Path) -> Result<(), Error> {
    let new = beside(file, "write");
    remove_if_exists(&new)?;
    let mut args = args.to_vec();
    args.extend(["-o".into(), path_arg(&new)]);
    if let Err(error) = crate::exiftool::run_once(exiftool, config, &args) {
        let _ = std::fs::remove_file(&new);
        return Err(Error::Exiftool(error));
    }
    replace(&new, file)
}

/// Runs exiftool with `args` on `from` and writes the result into the new file `to` (`-o`). A `to` left behind by an
/// ended exiftool is removed first, since exiftool won't write over it; a failed run leaves no `to`.
pub fn exiftool_into(
    exiftool: &Path,
    config: Option<&Path>,
    args: &[String],
    from: &Path,
    to: &Path,
) -> Result<(), Error> {
    remove_if_exists(to)?;
    let mut args = args.to_vec();
    args.extend(["-o".into(), path_arg(to), path_arg(from)]);
    if let Err(error) = crate::exiftool::run_once(exiftool, config, &args) {
        let _ = std::fs::remove_file(to);
        return Err(Error::Exiftool(error));
    }
    Ok(())
}

/// Puts `new` in the place of `file`, which may not exist yet. `new` is synced first; an existing `file` is moved
/// aside, never renamed over; the file in place is read back, and if it isn't `new`, the original is put back. `new`
/// is gone afterwards, also on failure.
pub fn replace(new: &Path, file: &Path) -> Result<(), Error> {
    let replaced = replace_inner(new, file);
    if replaced.is_err() {
        let _ = std::fs::remove_file(new);
    }
    replaced
}

fn replace_inner(new: &Path, file: &Path) -> Result<(), Error> {
    recover(file)?;
    let expected = digest(new)?;
    if expected.len == 0 {
        return Err(Error::Empty);
    }
    std::fs::File::options().write(true).open(new)?.sync_all()?;
    let old = beside(file, "old");
    let had_original = file.exists();
    if had_original {
        retry(|| std::fs::rename(file, &old))?;
    }
    if let Err(error) = retry(|| std::fs::rename(new, file)) {
        if had_original {
            let _ = retry(|| std::fs::rename(&old, file));
        }
        return Err(error.into());
    }
    #[cfg(test)]
    tests::maybe_lose(file);
    if digest(file).is_ok_and(|actual| actual == expected) {
        sync_folder(file);
        if had_original {
            // A leftover is removed by the next write's `recover`.
            let _ = std::fs::remove_file(&old);
        }
        return Ok(());
    }
    // The file system lost the new version: the original goes back, again without renaming over a file.
    tracing::error!(
        "{} wasn't kept by the file system after it was renamed into place",
        file.display()
    );
    let _ = std::fs::remove_file(file);
    if !had_original {
        return Err(Error::Lost);
    }
    let original = digest(&old);
    let restored = retry(|| std::fs::rename(&old, file)).is_ok()
        && original.is_ok_and(|original| digest(file).is_ok_and(|actual| actual == original));
    if restored {
        Err(Error::Lost)
    } else {
        Err(Error::Stranded(old))
    }
}

/// Finishes a replace of `file` the app was ended in: the original moved aside comes back if `file` is missing, and
/// is dropped if a new version is in place (only the moment between renaming it in and reading it back is lost).
pub fn recover(file: &Path) -> std::io::Result<()> {
    let old = beside(file, "old");
    if !old.exists() {
        return Ok(());
    }
    if file.exists() {
        std::fs::remove_file(&old)
    } else {
        std::fs::rename(&old, file)
    }
}

/// Gives `new` the date modified (to the sub-second), the date created on Windows and the permissions of `original`.
pub fn keep_attributes(original: &Path, new: &Path) -> std::io::Result<()> {
    let metadata = std::fs::metadata(original)?;
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut times = std::fs::FileTimes::new().set_modified(metadata.modified()?);
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTimesExt as _;
        times = times.set_created(metadata.created()?);
    }
    std::fs::File::options().write(true).open(new)?.set_times(times)?;
    std::fs::set_permissions(new, metadata.permissions())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Digest {
    len: u64,
    hash: u64,
}

/// What `path` holds, read to the end: the file system may report the size of a file whose data it lost (#140).
fn digest(path: &Path) -> std::io::Result<Digest> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = std::hash::DefaultHasher::new();
    let mut buffer = vec![0; 1 << 16];
    let mut len = 0;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.write(&buffer[..read]);
        len += read as u64;
    }
    Ok(Digest {
        len,
        hash: hasher.finish(),
    })
}

/// Retries `rename` for ≈ 300 ms while Windows refuses it because another program (Defender, often) has the file open
/// (`docs/known-pitfalls.md`).
fn retry(mut rename: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let mut attempt = 0;
    loop {
        match rename() {
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied && attempt < 10 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(30));
            }
            result => return result,
        }
    }
}

fn remove_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Makes a rename in `file`'s folder durable; Windows has no folder sync (its renames are journaled).
fn sync_folder(file: &Path) {
    #[cfg(unix)]
    if let Some(folder) = file.parent()
        && let Ok(folder) = std::fs::File::open(folder)
    {
        let _ = folder.sync_all();
    }
    #[cfg(not(unix))]
    let _ = file;
}

/// A path for exiftool. Library paths are absolute, so they can't be taken for an option (REQ-011-09).
fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static LOSE: Cell<bool> = const { Cell::new(false) };
    }

    /// Runs `f` on a file system that empties a file renamed into place while it reports success, as an rclone
    /// mount on Windows does (#140).
    pub(crate) fn losing_renames<T>(f: impl FnOnce() -> T) -> T {
        LOSE.set(true);
        let result = f();
        LOSE.set(false);
        result
    }

    pub(super) fn maybe_lose(file: &Path) {
        if LOSE.get() {
            std::fs::File::create(file).unwrap();
        }
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// REQ-024-06: the new version takes the original's place, and nothing else is left.
    #[test]
    fn req_024_06_puts_the_new_version_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.jpg");
        std::fs::write(&file, b"old").unwrap();
        let new = beside(&file, "write");
        std::fs::write(&new, b"new").unwrap();

        replace(&new, &file).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"new");
        assert_eq!(names(dir.path()), ["a.jpg"]);
    }

    /// REQ-024-06: a lost new version brings the original back, byte for byte, and leaves nothing behind.
    #[test]
    fn req_024_06_a_lost_new_version_brings_the_original_back() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.jpg");
        std::fs::write(&file, b"old").unwrap();
        let new = beside(&file, "write");
        std::fs::write(&new, b"new").unwrap();

        let replaced = losing_renames(|| replace(&new, &file));
        assert!(matches!(replaced, Err(Error::Lost)), "{replaced:?}");
        assert_eq!(std::fs::read(&file).unwrap(), b"old");
        assert_eq!(names(dir.path()), ["a.jpg"]);
    }

    /// REQ-024-06: an empty new version never replaces a file.
    #[test]
    fn req_024_06_an_empty_new_version_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.jpg");
        std::fs::write(&file, b"old").unwrap();
        let new = beside(&file, "write");
        std::fs::write(&new, b"").unwrap();

        assert!(matches!(replace(&new, &file), Err(Error::Empty)));
        assert_eq!(std::fs::read(&file).unwrap(), b"old");
        assert_eq!(names(dir.path()), ["a.jpg"]);
    }

    /// REQ-024-06: after the app was ended part-way, the original moved aside comes back, or is dropped once the new
    /// version is in place.
    #[test]
    fn req_024_06_recovers_a_replace_the_app_was_ended_in() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.jpg");
        std::fs::write(beside(&file, "old"), b"old").unwrap();
        recover(&file).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"old");
        assert_eq!(names(dir.path()), ["a.jpg"]);

        std::fs::write(beside(&file, "old"), b"older").unwrap();
        recover(&file).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"old");
        assert_eq!(names(dir.path()), ["a.jpg"]);
    }

    /// REQ-024-06: the new version gets the original's date modified, to the sub-second.
    #[test]
    fn req_024_06_keeps_the_date_modified() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.jpg");
        std::fs::write(&file, b"old").unwrap();
        let modified = std::time::SystemTime::UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_700);
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        let new = beside(&file, "write");
        std::fs::write(&new, b"new").unwrap();

        keep_attributes(&file, &new).unwrap();
        replace(&new, &file).unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), modified);
    }

    #[test]
    fn names_files_beside_with_the_extension() {
        let file = Path::new("photos").join("a b.jpg");
        assert_eq!(
            beside(&file, "write"),
            Path::new("photos").join(".a b.jpg.gg-write.jpg")
        );
        assert_eq!(beside(Path::new("x"), "old"), Path::new(".x.gg-old"));
    }
}
