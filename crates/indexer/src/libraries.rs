//! Adding, removing and rescanning libraries (spec 001).

use crate::reachability::{self, is_reachable_directory};
use crate::{ScanKind, ScanRequests};
use db::libraries::{self as rows, Library, LibraryStatus};
use db::rusqlite::Connection;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("Path must be absolute")]
    NotAbsolute,
    #[error("Path is too long")]
    TooLong,
    #[error("Folder not found or not reachable")]
    NotReachable,
    #[error("Library already added")]
    AlreadyAdded,
    #[error("Library not found")]
    NotFound,
    #[error(transparent)]
    Db(#[from] db::Error),
}

pub type Result<T> = std::result::Result<T, LibraryError>;

/// All libraries, sorted by label.
pub fn list(conn: &Connection) -> Result<Vec<Library>> {
    Ok(rows::list(conn)?)
}

/// Adds the folder at `path` as an `online` library and queues a full scan. Waits up to 5 seconds for the folder.
pub fn add(conn: &Connection, path: &str, scans: &dyn ScanRequests) -> Result<Library> {
    add_with_timeout(conn, path, scans, reachability::TIMEOUT)
}

pub fn add_with_timeout(conn: &Connection, path: &str, scans: &dyn ScanRequests, timeout: Duration) -> Result<Library> {
    let path = check_folder(path, timeout)?;
    insert_checked(conn, &path, scans)
}

/// The longest path accepted.
const MAX_PATH_LENGTH: usize = 1024;

/// The first half of [`add`], without the index: the normalized path of a reachable folder. Waits up to `timeout`
/// for the folder, so callers holding the index connection should call it before locking.
pub fn check_folder(path: &str, timeout: Duration) -> Result<String> {
    let path = path.trim();
    if path.chars().count() > MAX_PATH_LENGTH {
        return Err(LibraryError::TooLong);
    }
    if !is_absolute(path) {
        return Err(LibraryError::NotAbsolute);
    }
    let path = normalize(path).ok_or(LibraryError::NotAbsolute)?;
    if !is_reachable_directory(Path::new(&path), timeout) {
        return Err(LibraryError::NotReachable);
    }
    Ok(path)
}

/// The second half of [`add`]: stores a path from [`check_folder`] and queues a full scan.
pub fn insert_checked(conn: &Connection, path: &str, scans: &dyn ScanRequests) -> Result<Library> {
    if rows::find_by_path(conn, path)?.is_some() {
        return Err(LibraryError::AlreadyAdded);
    }
    let library = rows::insert(conn, path, &label_for(path), LibraryStatus::Online).map_err(|error| {
        // Another writer added the same path since the check above.
        if is_unique_violation(&error) {
            LibraryError::AlreadyAdded
        } else {
            error.into()
        }
    })?;
    scans.request_scan(library.id, ScanKind::Full);
    Ok(library)
}

fn is_unique_violation(error: &db::Error) -> bool {
    matches!(
        error,
        db::Error::Sqlite(db::rusqlite::Error::SqliteFailure(e, _))
            if e.code == db::rusqlite::ErrorCode::ConstraintViolation
    )
}

/// Removes a library with its media items from the index and stops its scans. Files on disk aren't touched.
pub fn remove(conn: &Connection, id: i64, scans: &dyn ScanRequests) -> Result<()> {
    if rows::get(conn, id)?.is_none() {
        return Err(LibraryError::NotFound);
    }
    // Stop its scan first (this waits for a running scan to end), so nothing writes rows of a deleted library.
    scans.remove_library(id);
    if !rows::delete(conn, id)? {
        return Err(LibraryError::NotFound);
    }
    Ok(())
}

/// Queues a full scan, which also finds files changed in place while the app wasn't running.
pub fn rescan(conn: &Connection, id: i64, scans: &dyn ScanRequests) -> Result<()> {
    if rows::get(conn, id)?.is_none() {
        return Err(LibraryError::NotFound);
    }
    scans.request_scan(id, ScanKind::Full);
    Ok(())
}

fn is_separator(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

/// Windows rules, as Node's `path.win32.isAbsolute`: rooted (`\x`, `\\server\share`) or a drive with a root (`D:\`).
/// Elsewhere a path from the root, `/…`.
fn is_absolute(path: &str) -> bool {
    if !cfg!(windows) {
        return path.starts_with('/');
    }
    let mut chars = path.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some(first), _, _) if is_separator(first) => true,
        (Some(drive), Some(':'), Some(root)) => drive.is_ascii_alphabetic() && is_separator(root),
        _ => false,
    }
}

/// Resolves `.` and `..`, unifies separators and drops a trailing separator, except at a drive or share root (`/`
/// outside Windows).
fn normalize(path: &str) -> Option<String> {
    if !cfg!(windows) {
        // Lexically, as on Windows: `std::path::absolute` keeps `..` on other systems.
        let mut parts = Vec::new();
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                part => parts.push(part),
            }
        }
        return Some(format!("/{}", parts.join("/")));
    }
    let full = std::path::absolute(path).ok()?;
    let full = full.to_str()?.replace('/', "\\");
    let trimmed = full.trim_end_matches('\\');
    Some(if is_root(trimmed) {
        format!("{trimmed}\\")
    } else {
        trimmed.to_string()
    })
}

/// `D:` or `\\server\share` (without the trailing separator).
fn is_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    if bytes.len() == 2 && bytes[1] == b':' {
        return true;
    }
    path.strip_prefix("\\\\")
        .is_some_and(|rest| rest.split('\\').filter(|part| !part.is_empty()).count() <= 2)
}

/// The folder's name, or the whole path for a drive root (`/`).
fn label_for(path: &str) -> String {
    let without_drive = if cfg!(windows) && path.as_bytes().get(1) == Some(&b':') {
        &path[2..]
    } else {
        path
    };
    match without_drive.trim_end_matches(is_separator).rsplit(is_separator).next() {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoScans;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorded {
        scans: Mutex<Vec<(i64, ScanKind)>>,
        removed: Mutex<Vec<i64>>,
    }

    impl ScanRequests for Recorded {
        fn request_scan(&self, library: i64, kind: ScanKind) {
            self.scans.lock().unwrap().push((library, kind));
        }

        fn remove_library(&self, library: i64) {
            self.removed.lock().unwrap().push(library);
        }
    }

    fn path_of(dir: &tempfile::TempDir) -> String {
        dir.path().to_str().unwrap().to_string()
    }

    #[test]
    fn req_001_01_adds_a_reachable_folder_and_queues_a_full_scan() {
        let conn = db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let scans = Recorded::default();

        let library = add(&conn, &path_of(&dir), &scans).unwrap();

        assert_eq!(library.status, LibraryStatus::Online);
        assert_eq!(library.label, dir.path().file_name().unwrap().to_str().unwrap());
        assert_eq!(list(&conn).unwrap(), std::slice::from_ref(&library));
        assert_eq!(*scans.scans.lock().unwrap(), [(library.id, ScanKind::Full)]);
    }

    #[test]
    fn req_001_01_normalizes_the_path() {
        let conn = db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let messy = format!("  {}/sub/../sub/  ", path_of(&dir));

        let library = add(&conn, &messy, &NoScans).unwrap();

        assert_eq!(library.path, dir.path().join("sub").to_str().unwrap());
        assert_eq!(library.label, "sub");
    }

    #[test]
    fn req_001_03_rejects_relative_and_missing_paths() {
        let conn = db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing").to_str().unwrap().to_owned();

        for relative in ["photos", "C:photos", ""] {
            let error = add(&conn, relative, &NoScans).unwrap_err();
            assert_eq!(error.to_string(), "Path must be absolute", "{relative}");
        }
        assert_eq!(
            add(&conn, &missing, &NoScans).unwrap_err().to_string(),
            "Folder not found or not reachable"
        );
        let long = format!("D:\\{}", "a".repeat(1100));
        assert_eq!(add(&conn, &long, &NoScans).unwrap_err().to_string(), "Path is too long");
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn req_001_04_reports_a_concurrent_duplicate_as_already_added() {
        let conn = db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = check_folder(&path_of(&dir), reachability::TIMEOUT).unwrap();
        // Another writer inserts the same path between the check and the insert.
        rows::insert(&conn, &path, "other", LibraryStatus::Online).unwrap();

        let error = rows::insert(&conn, &path, "x", LibraryStatus::Online).unwrap_err();

        assert!(is_unique_violation(&error));
        assert!(matches!(
            insert_checked(&conn, &path, &NoScans),
            Err(LibraryError::AlreadyAdded)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn req_001_04_rejects_duplicates_case_insensitively() {
        let conn = db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        add(&conn, &path_of(&dir), &NoScans).unwrap();

        let error = add(&conn, &path_of(&dir).to_uppercase(), &NoScans).unwrap_err();

        assert_eq!(error.to_string(), "Library already added");
        assert_eq!(list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn req_001_05_removes_a_library_with_its_media_and_stops_its_scans() {
        let conn = db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("a.jpg");
        std::fs::write(&photo, "jpeg").unwrap();
        let scans = Recorded::default();
        let library = add(&conn, &path_of(&dir), &scans).unwrap();
        conn.execute(
            "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime) VALUES (?1, 'a.jpg', 'a.jpg', 'image', 4, 1)",
            [library.id],
        )
        .unwrap();

        remove(&conn, library.id, &scans).unwrap();

        assert!(list(&conn).unwrap().is_empty());
        let media: i64 = conn.query_row("SELECT count(*) FROM media", [], |r| r.get(0)).unwrap();
        assert_eq!(media, 0);
        assert_eq!(*scans.removed.lock().unwrap(), [library.id]);
        assert_eq!(std::fs::read_to_string(&photo).unwrap(), "jpeg");
        assert!(matches!(remove(&conn, library.id, &scans), Err(LibraryError::NotFound)));
    }

    #[test]
    fn req_001_06_rescan_queues_a_full_scan() {
        let conn = db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let scans = Recorded::default();
        let library = add(&conn, &path_of(&dir), &NoScans).unwrap();

        rescan(&conn, library.id, &scans).unwrap();

        assert_eq!(*scans.scans.lock().unwrap(), [(library.id, ScanKind::Full)]);
        assert!(matches!(rescan(&conn, 999, &scans), Err(LibraryError::NotFound)));
    }

    /// REQ-001-04: outside Windows, paths are case-sensitive: only the same path is a duplicate.
    #[cfg(not(windows))]
    #[test]
    fn req_001_04_rejects_the_same_path_but_tells_letter_cases_apart() {
        let conn = db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (lower, upper) = (dir.path().join("photos"), dir.path().join("Photos"));
        std::fs::create_dir(&lower).unwrap();
        std::fs::create_dir(&upper).unwrap();
        add(&conn, lower.to_str().unwrap(), &NoScans).unwrap();

        let again = format!("{}/", lower.to_str().unwrap());
        assert!(matches!(add(&conn, &again, &NoScans), Err(LibraryError::AlreadyAdded)));
        add(&conn, upper.to_str().unwrap(), &NoScans).unwrap();
        assert_eq!(list(&conn).unwrap().len(), 2);
    }

    #[cfg(not(windows))]
    #[test]
    fn posix_paths_are_absolute_from_the_root_and_named_after_their_folder() {
        assert!(is_absolute("/media/Photos") && !is_absolute("media/Photos") && !is_absolute("D:\\Photos"));
        assert_eq!(normalize("/media//Photos/./2024/../").as_deref(), Some("/media/Photos"));
        assert_eq!(normalize("/..").as_deref(), Some("/"));
        assert_eq!(normalize("/a\\b").as_deref(), Some("/a\\b"));
        assert_eq!(label_for("/media/Photos"), "Photos");
        assert_eq!(label_for("/a\\b"), "a\\b");
        assert_eq!(label_for("/"), "/");
    }

    #[cfg(windows)]
    #[test]
    fn labels_are_folder_names_or_the_whole_drive() {
        assert_eq!(label_for("D:\\Photos\\2024"), "2024");
        assert_eq!(label_for("D:\\"), "D:\\");
        assert_eq!(label_for("\\\\nas\\photos"), "photos");
        assert_eq!(label_for("\\\\nas\\photos\\"), "photos");
    }

    #[cfg(windows)]
    #[test]
    fn roots_keep_their_separator() {
        assert_eq!(normalize("D:\\").as_deref(), Some("D:\\"));
        assert_eq!(normalize("D:/Photos/").as_deref(), Some("D:\\Photos"));
        assert_eq!(normalize("\\\\nas\\share\\").as_deref(), Some("\\\\nas\\share\\"));
        assert_eq!(normalize("\\\\nas\\share\\a\\").as_deref(), Some("\\\\nas\\share\\a"));
    }
}
