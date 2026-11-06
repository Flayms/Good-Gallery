//! Smart albums (spec 034): searches kept under a name, with what the sidebar shows of them.

use crate::Result;
use domain::search::GallerySearch;
use domain::smart_albums::{SmartAlbum, search_from_value, search_to_value};
use rusqlite::{Connection, params};

/// Why a smart album can't find what it was saved for (REQ-034-06).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    Library,
    Folder,
}

/// A smart album as the sidebar lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct AlbumRow {
    pub album: SmartAlbum,
    /// Media items it holds now (REQ-034-02, REQ-034-04).
    pub count: u64,
    pub missing: Option<Missing>,
}

/// All smart albums, by name (REQ-034-02).
pub fn list(conn: &Connection) -> Result<Vec<SmartAlbum>> {
    let mut stmt = conn.prepare_cached("SELECT id, name, search FROM smart_albums ORDER BY name COLLATE NOCASE, id")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut albums = Vec::new();
    for row in rows {
        let (id, name, search) = row?;
        // A value that isn't JSON reads as the default search, like any part that can't be read.
        let search = search_from_value(&serde_json::from_str(&search).unwrap_or_default());
        albums.push(SmartAlbum { id, name, search });
    }
    Ok(albums)
}

/// Keeps `search` under `name`; its id (REQ-034-01).
pub fn insert(conn: &Connection, name: &str, search: &GallerySearch) -> Result<i64> {
    conn.execute(
        "INSERT INTO smart_albums (name, search) VALUES (?1, ?2)",
        params![name, search_to_value(search).to_string()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn rename(conn: &Connection, id: i64, name: &str) -> Result<()> {
    conn.execute("UPDATE smart_albums SET name = ?1 WHERE id = ?2", params![name, id])?;
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM smart_albums WHERE id = ?1", [id])?;
    Ok(())
}

/// All smart albums with their counts, and whether their library or folder is gone (REQ-034-02, REQ-034-06).
pub fn rows(conn: &Connection) -> Result<Vec<AlbumRow>> {
    let mut rows = Vec::new();
    for album in list(conn)? {
        let missing = missing(conn, &album.search)?;
        let count = match missing {
            Some(_) => 0,
            None => crate::gallery::count(conn, &album.search)?,
        };
        rows.push(AlbumRow { album, count, missing });
    }
    Ok(rows)
}

fn missing(conn: &Connection, search: &GallerySearch) -> Result<Option<Missing>> {
    let Some(library) = search.library else {
        return Ok(None);
    };
    let exists: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM library_roots WHERE id = ?1)",
        [library],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(Some(Missing::Library));
    }
    if search.folder_filter().is_some() {
        let folder = GallerySearch {
            library: search.library,
            folder: search.folder.clone(),
            ..GallerySearch::default()
        };
        if crate::gallery::count(conn, &folder)? == 0 {
            return Ok(Some(Missing::Folder));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::libraries::{LibraryStatus, insert as insert_library};
    use crate::writer::{IndexWriter, IndexedFile};
    use domain::media::{MediaKind, MediaMetadata};

    fn file(rel_path: &str, rating: Option<i64>) -> IndexedFile {
        IndexedFile {
            rel_path: rel_path.into(),
            file_name: rel_path.rsplit('/').next().unwrap().into(),
            kind: MediaKind::Image,
            size: 1,
            mtime: 1_000,
            sidecar_mtime: None,
            metadata: MediaMetadata {
                rating,
                ..Default::default()
            },
            read_error: None,
        }
    }

    fn rated(rating: u8) -> GallerySearch {
        GallerySearch {
            ratings: vec![rating],
            ..GallerySearch::default()
        }
    }

    #[test]
    fn req_034_01_req_034_05_req_034_07_keeps_renames_and_deletes_albums_by_name() {
        let conn = crate::open_in_memory().unwrap();
        let search = GallerySearch {
            tags: vec!["Rome".into()],
            descending: false,
            ..GallerySearch::default()
        };
        let rome = insert(&conn, "rome", &search).unwrap();
        let best = insert(&conn, "Best", &rated(5)).unwrap();
        let names = |conn: &Connection| list(conn).unwrap().into_iter().map(|a| a.name).collect::<Vec<_>>();
        assert_eq!(names(&conn), ["Best", "rome"]);
        assert_eq!(list(&conn).unwrap()[1].search, search);

        rename(&conn, best, "Top").unwrap();
        assert_eq!(names(&conn), ["rome", "Top"]);
        delete(&conn, rome).unwrap();
        assert_eq!(names(&conn), ["Top"]);
        assert_eq!(list(&conn).unwrap()[0].search, rated(5));
    }

    #[test]
    fn req_034_02_req_034_04_req_034_06_counts_albums_and_finds_what_is_gone() {
        let conn = crate::open_in_memory().unwrap();
        let root = insert_library(&conn, "D:\\Photos", "Photos", LibraryStatus::Online)
            .unwrap()
            .id;
        let mut writer = IndexWriter::new(conn);
        writer
            .write(
                root,
                &[
                    file("2023/a.jpg", Some(5)),
                    file("2023/b.jpg", None),
                    file("c.jpg", Some(5)),
                ],
            )
            .unwrap();
        let conn: Connection = writer.into();
        let in_folder = |folder: &str| GallerySearch {
            library: Some(root),
            folder: Some(folder.into()),
            ..GallerySearch::default()
        };
        insert(&conn, "Five", &rated(5)).unwrap();
        insert(&conn, "2023", &in_folder("2023")).unwrap();
        insert(&conn, "Gone folder", &in_folder("2019")).unwrap();
        insert(
            &conn,
            "Gone library",
            &GallerySearch {
                library: Some(root + 1),
                ..GallerySearch::default()
            },
        )
        .unwrap();
        let summary = |conn: &Connection| {
            rows(conn)
                .unwrap()
                .into_iter()
                .map(|row| (row.album.name, row.count, row.missing))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            summary(&conn),
            [
                ("2023".to_string(), 2, None),
                ("Five".to_string(), 2, None),
                ("Gone folder".to_string(), 0, Some(Missing::Folder)),
                ("Gone library".to_string(), 0, Some(Missing::Library)),
            ]
        );
        // A new rating changes the count.
        conn.execute("UPDATE media SET rating = 5 WHERE rel_path = '2023/b.jpg'", [])
            .unwrap();
        assert_eq!(summary(&conn)[1].1, 3);
    }
}
