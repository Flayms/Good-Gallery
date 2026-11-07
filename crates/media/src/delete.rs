//! Deleting files (REQ-009-23): each with its sidecars, to the Recycle Bin (the Trash on Linux), or permanently from a
//! network drive, which has none.

use crate::rating::{WriteError, library_path};
use std::io;
use std::path::Path;

/// Whether files of the library in `library` are deleted permanently: it is on a network drive, which has no Recycle
/// Bin. Moving such a file to the Recycle Bin would delete it permanently without saying so.
pub fn deletes_permanently(library: &Path) -> bool {
    domain::deletion::is_network_path(&library.to_string_lossy()) || on_remote_drive(library)
}

/// A mapped network drive (`Z:`).
#[cfg(windows)]
#[allow(unsafe_code)]
fn on_remote_drive(library: &Path) -> bool {
    use std::path::{Component, Prefix};
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    const DRIVE_REMOTE: u32 = 4;
    let Some(Component::Prefix(prefix)) = library.components().next() else {
        return false;
    };
    let (Prefix::Disk(letter) | Prefix::VerbatimDisk(letter)) = prefix.kind() else {
        return false;
    };
    let root: Vec<u16> = format!("{}:\\", char::from(letter)).encode_utf16().chain([0]).collect();
    // SAFETY: `root` is a NUL-terminated wide string that outlives the call.
    unsafe { GetDriveTypeW(root.as_ptr()) == DRIVE_REMOTE }
}

#[cfg(not(windows))]
fn on_remote_drive(_library: &Path) -> bool {
    false
}

/// Removes a file for [`delete`].
pub type Remove = fn(&Path) -> io::Result<()>;

/// Moves a file to the Recycle Bin (the Trash on Linux).
pub fn to_trash(path: &Path) -> io::Result<()> {
    trash::delete(path).map_err(|error| io::Error::other(error.to_string()))
}

/// Deletes a file permanently.
pub fn permanently(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// Deletes the file `rel_path` of the library in `library` with its sidecars, each with `remove` ([`to_trash`] or
/// [`permanently`]). The file goes first; its sidecars only once it is gone. A sidecar that another file of the same
/// name shares (`IMG_1.xmp` beside `IMG_1.jpg` and `IMG_1.mov`) stays. A file that is gone already counts as deleted.
pub fn delete(library: &Path, rel_path: &str, remove: impl Fn(&Path) -> io::Result<()>) -> Result<(), WriteError> {
    let file = library_path(library, rel_path).ok_or(WriteError::UnsafePath)?;
    // Found before the file goes: whether another file shares the stem's sidecar is read from the folder.
    let sidecars: Vec<_> = crate::rename::sidecars(library, rel_path, rel_path)
        .into_iter()
        .filter(|(_, _, shared)| !shared)
        .filter_map(|(sidecar, _, _)| library_path(library, &sidecar))
        .collect();
    if file.symlink_metadata().is_ok() {
        remove(&file)?;
    }
    // On a file system that ignores case, `IMG_1.xmp` and `IMG_1.XMP` are found as one sidecar: the second is gone.
    for sidecar in sidecars
        .into_iter()
        .filter(|sidecar| sidecar.symlink_metadata().is_ok())
    {
        if let Err(error) = remove(&sidecar) {
            tracing::warn!("can't delete the sidecar {}: {error}", sidecar.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A stand-in for the Recycle Bin: moves files into `bin`, as the real one would, without touching the user's.
    fn into(bin: &Path) -> impl Fn(&Path) -> io::Result<()> + '_ {
        |path| std::fs::rename(path, bin.join(path.file_name().unwrap()))
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn library(names: &[&str]) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let (library, bin) = (dir.path().join("library"), dir.path().join("bin"));
        std::fs::create_dir_all(library.join("2024")).unwrap();
        std::fs::create_dir(&bin).unwrap();
        for name in names {
            std::fs::write(library.join(name), name).unwrap();
        }
        (dir, library, bin)
    }

    /// REQ-009-23: the file goes with both kinds of sidecar.
    #[test]
    fn req_009_23_the_file_goes_with_its_sidecars() {
        let (_dir, library, bin) = library(&[
            "2024/IMG_1.jpg",
            "2024/IMG_1.jpg.xmp",
            "2024/IMG_1.XMP",
            "2024/IMG_2.jpg",
        ]);
        delete(&library, "2024/IMG_1.jpg", into(&bin)).unwrap();
        assert_eq!(files(&library.join("2024")), ["IMG_2.jpg"]);
        // Windows finds `IMG_1.XMP` as `IMG_1.xmp`, and the stand-in names it so.
        let lower = |names: Vec<String>| {
            let mut names: Vec<String> = names.iter().map(|name| name.to_lowercase()).collect();
            names.sort();
            names
        };
        assert_eq!(lower(files(&bin)), ["img_1.jpg", "img_1.jpg.xmp", "img_1.xmp"]);
    }

    /// REQ-009-23: a sidecar named after the stem that another file of that name shares stays.
    #[test]
    fn req_009_23_a_shared_sidecar_stays() {
        let (_dir, library, bin) = library(&["IMG_1.jpg", "IMG_1.mov", "IMG_1.xmp"]);
        delete(&library, "IMG_1.jpg", into(&bin)).unwrap();
        assert_eq!(files(&library), ["2024", "IMG_1.mov", "IMG_1.xmp"]);
        assert_eq!(files(&bin), ["IMG_1.jpg"]);
    }

    /// REQ-009-23: a file that can't be deleted stays with its sidecars, and the error says why.
    #[test]
    fn req_009_23_a_file_that_cant_go_stays_with_its_sidecars() {
        let (_dir, library, _bin) = library(&["IMG_1.jpg", "IMG_1.jpg.xmp"]);
        let error = delete(&library, "IMG_1.jpg", |_| Err(io::ErrorKind::PermissionDenied.into())).unwrap_err();
        assert!(matches!(error, WriteError::Io(ref error) if error.kind() == io::ErrorKind::PermissionDenied));
        assert_eq!(files(&library), ["2024", "IMG_1.jpg", "IMG_1.jpg.xmp"]);
    }

    /// REQ-009-23: a file gone already counts as deleted, and its leftover sidecar goes; nothing outside the library is
    /// touched.
    #[test]
    fn req_009_23_a_gone_file_counts_and_paths_stay_inside() {
        let (dir, library, bin) = library(&["IMG_1.jpg.xmp"]);
        delete(&library, "IMG_1.jpg", into(&bin)).unwrap();
        assert_eq!(files(&bin), ["IMG_1.jpg.xmp"]);

        std::fs::write(dir.path().join("outside.jpg"), "x").unwrap();
        assert!(matches!(
            delete(&library, "../outside.jpg", into(&bin)),
            Err(WriteError::UnsafePath)
        ));
        assert!(dir.path().join("outside.jpg").exists());
    }

    /// REQ-009-23: permanently, the files are removed.
    #[test]
    fn req_009_23_permanently_removes_the_files() {
        let (_dir, library, _bin) = library(&["IMG_1.jpg", "IMG_1.jpg.xmp"]);
        delete(&library, "IMG_1.jpg", permanently).unwrap();
        assert_eq!(files(&library), ["2024"]);
    }

    /// REQ-009-23: a local library has a Recycle Bin.
    #[test]
    fn req_009_23_a_local_library_has_a_recycle_bin() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!deletes_permanently(dir.path()));
        assert!(deletes_permanently(Path::new(r"\\nas\photos")));
    }
}
