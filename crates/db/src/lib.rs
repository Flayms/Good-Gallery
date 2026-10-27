//! The index: an SQLite database shared with Good Gallery's Electron version (same schema, same migrations, same
//! migration bookkeeping), so either app can open an index the other one wrote.

pub mod gallery;
pub mod libraries;
mod migrate;
pub mod settings;
pub mod tags;
pub mod thumbs;
pub mod writer;

pub use rusqlite;
use rusqlite::Connection;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration journal: {0}")]
    Journal(String),
    #[error("{0}")]
    InvalidSetting(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Opens (or creates) the index at `path` for writing and brings its schema up to date.
pub fn open(path: &Path) -> Result<Connection> {
    let mut conn = Connection::open(path)?;
    configure(&conn)?;
    migrate::migrate(&mut conn)?;
    Ok(conn)
}

/// Opens the index at `path` read-only. The schema must already be up to date (see [`open`]).
pub fn open_reader(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    // Searches walk a sort index and look up each row in the table: with the default 2 MB page cache every lookup is
    // a read call. Memory-mapped reads and a larger cache make a 500k-row search 4–8× faster (M7a, search-bench).
    // Memory-mapped reads turn an I/O error into a crash instead of an error; the index lives on a local disk (the
    // user profile), where that doesn't happen in practice.
    conn.pragma_update(None, "mmap_size", READER_MMAP)?;
    conn.pragma_update(None, "cache_size", READER_CACHE)?;
    Ok(conn)
}

/// Memory-mapped size of read connections, bytes (the index of 500k media items is ≈ 200 MB).
const READER_MMAP: i64 = 512 << 20;
/// Page cache of read connections; negative means KiB.
const READER_CACHE: i64 = -65_536;

/// A migrated in-memory index, for tests.
pub fn open_in_memory() -> Result<Connection> {
    let mut conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate::migrate(&mut conn)?;
    Ok(conn)
}

/// The reference's pragmas (`src/main/db/index.ts`).
fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reference.db")
    }

    /// A temporary copy of the reference fixture, so tests never change the committed file.
    fn reference_copy(dir: &tempfile::TempDir) -> PathBuf {
        let path = dir.path().join("gallery.db");
        std::fs::copy(fixture(), &path).unwrap();
        path
    }

    fn bookkeeping(conn: &Connection) -> Vec<(String, i64)> {
        let mut stmt = conn
            .prepare("SELECT hash, CAST(created_at AS INTEGER) FROM __drizzle_migrations ORDER BY created_at")
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    }

    fn schema(conn: &Connection) -> BTreeSet<(String, String, String)> {
        let mut stmt = conn
            .prepare("SELECT type, name, coalesce(sql, '') FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'")
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    }

    /// REQ-001-08: an index written by the Electron version opens with its libraries and settings.
    #[test]
    fn req_001_08_opens_an_index_written_by_the_reference() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(&reference_copy(&dir)).unwrap();

        let labels: Vec<String> = libraries::list(&conn).unwrap().into_iter().map(|l| l.label).collect();
        assert_eq!(labels, ["Photos", "archive"]);
        let media: i64 = conn.query_row("SELECT count(*) FROM media", [], |r| r.get(0)).unwrap();
        assert_eq!(media, 3);
        let settings = settings::load(&conn).unwrap();
        assert_eq!(
            (settings.thumb_cache_gib, settings.quick_check_interval_minutes),
            (10.0, 15)
        );
    }

    /// REQ-001-08: opening doesn't migrate an up-to-date reference index again.
    #[test]
    fn req_001_08_leaves_the_reference_bookkeeping_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = reference_copy(&dir);
        let before = bookkeeping(&Connection::open(&path).unwrap());

        let conn = open(&path).unwrap();

        assert_eq!(bookkeeping(&conn), before);
    }

    /// REQ-001-08: a new index has the reference's schema and bookkeeping, so the Electron version opens it as is.
    #[test]
    fn req_001_08_creates_the_reference_schema_and_bookkeeping() {
        let dir = tempfile::tempdir().unwrap();
        let reference = Connection::open(fixture()).unwrap();

        let conn = open(&dir.path().join("new.db")).unwrap();

        assert_eq!(bookkeeping(&conn), bookkeeping(&reference));
        assert_eq!(schema(&conn), schema(&reference));
    }

    #[test]
    fn opening_twice_migrates_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.db");
        drop(open(&path).unwrap());

        let conn = open(&path).unwrap();

        assert_eq!(bookkeeping(&conn).len(), migrate::migrations().unwrap().len());
    }

    #[test]
    fn reader_sees_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.db");
        let writer = open(&path).unwrap();
        libraries::insert(&writer, "D:\\Photos", "Photos", libraries::LibraryStatus::Online).unwrap();

        let reader = open_reader(&path).unwrap();

        assert_eq!(libraries::list(&reader).unwrap().len(), 1);
    }
}
