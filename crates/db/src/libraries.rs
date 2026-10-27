//! Library rows (`library_roots`). Validation and scan requests live in `indexer::libraries`.

use crate::Result;
use rusqlite::{Connection, OptionalExtension, Row};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryStatus {
    Unknown,
    Online,
    Offline,
}

impl LibraryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Online => "online",
            Self::Offline => "offline",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "online" => Self::Online,
            "offline" => Self::Offline,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    pub id: i64,
    /// Absolute, normalized folder path.
    pub path: String,
    pub label: String,
    pub status: LibraryStatus,
    /// Milliseconds since the Unix epoch.
    pub last_scan_at: Option<i64>,
}

const COLUMNS: &str = "id, path, label, status, last_scan_at";

fn from_row(row: &Row) -> rusqlite::Result<Library> {
    Ok(Library {
        id: row.get(0)?,
        path: row.get(1)?,
        label: row.get(2)?,
        status: LibraryStatus::parse(&row.get::<_, String>(3)?),
        last_scan_at: row.get(4)?,
    })
}

/// All libraries, sorted by label.
pub fn list(conn: &Connection) -> Result<Vec<Library>> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {COLUMNS} FROM library_roots ORDER BY label"))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Library>> {
    let sql = format!("SELECT {COLUMNS} FROM library_roots WHERE id = ?1");
    Ok(conn.query_row(&sql, [id], from_row).optional()?)
}

/// The library with this path in any letter case (Windows paths are case-insensitive, beyond ASCII too, so this
/// compares in Rust instead of with SQLite's ASCII-only `lower()`). There are few libraries.
pub fn find_by_path(conn: &Connection, path: &str) -> Result<Option<i64>> {
    let wanted = path.to_lowercase();
    let mut stmt = conn.prepare_cached("SELECT id, path FROM library_roots")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        if row.get::<_, String>(1)?.to_lowercase() == wanted {
            return Ok(Some(row.get(0)?));
        }
    }
    Ok(None)
}

pub fn insert(conn: &Connection, path: &str, label: &str, status: LibraryStatus) -> Result<Library> {
    let sql = format!("INSERT INTO library_roots (path, label, status) VALUES (?1, ?2, ?3) RETURNING {COLUMNS}");
    Ok(conn.query_row(&sql, (path, label, status.as_str()), from_row)?)
}

pub fn set_status(conn: &Connection, id: i64, status: LibraryStatus) -> Result<()> {
    conn.execute(
        "UPDATE library_roots SET status = ?1 WHERE id = ?2",
        (status.as_str(), id),
    )?;
    Ok(())
}

/// Deletes a library with its media items and folders (foreign key cascades). False if there was none.
pub fn delete(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn.execute("DELETE FROM library_roots WHERE id = ?1", [id])? > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_by_label() {
        let conn = crate::open_in_memory().unwrap();
        insert(&conn, "D:\\b", "b", LibraryStatus::Online).unwrap();
        insert(&conn, "D:\\a", "a", LibraryStatus::Offline).unwrap();

        let labels: Vec<_> = list(&conn).unwrap().into_iter().map(|l| (l.label, l.status)).collect();

        assert_eq!(
            labels,
            [
                ("a".into(), LibraryStatus::Offline),
                ("b".into(), LibraryStatus::Online)
            ]
        );
    }

    #[test]
    fn finds_paths_in_any_letter_case() {
        let conn = crate::open_in_memory().unwrap();
        let library = insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Online).unwrap();

        assert_eq!(find_by_path(&conn, "d:\\PHOTOS").unwrap(), Some(library.id));
        assert_eq!(find_by_path(&conn, "d:\\other").unwrap(), None);
    }

    #[test]
    fn req_001_04_finds_paths_in_any_letter_case_beyond_ascii() {
        let conn = crate::open_in_memory().unwrap();
        let library = insert(&conn, "D:\\Fotos\\Ärger", "Ärger", LibraryStatus::Online).unwrap();

        assert_eq!(find_by_path(&conn, "d:\\fotos\\ärger").unwrap(), Some(library.id));
    }

    #[test]
    fn delete_removes_media_and_folders() {
        let conn = crate::open_in_memory().unwrap();
        let library = insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Online).unwrap();
        conn.execute(
            "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime) VALUES (?1, 'a.jpg', 'a.jpg', 'image', 1, 1)",
            [library.id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO folders (root_id, rel_dir, mtime) VALUES (?1, '', 1)",
            [library.id],
        )
        .unwrap();

        assert!(delete(&conn, library.id).unwrap());
        assert!(!delete(&conn, library.id).unwrap());
        let left: i64 = conn
            .query_row(
                "SELECT (SELECT count(*) FROM media) + (SELECT count(*) FROM folders)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
        assert_eq!(get(&conn, library.id).unwrap(), None);
    }
}
