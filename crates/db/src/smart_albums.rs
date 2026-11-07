//! Smart albums (spec 034): searches kept under a name, with what the sidebar shows of them.

use crate::Result;
use domain::album_rules::Condition;
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
        let mut search = search_from_value(&serde_json::from_str(&search).unwrap_or_default());
        // The filters bar names rules by their album (REQ-039-05).
        if let Some(rules) = &mut search.rules {
            rules.name = name.clone();
        }
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

/// Gives album `id` a new name and search, from its editor (REQ-039-01).
pub fn update(conn: &Connection, id: i64, name: &str, search: &GallerySearch) -> Result<()> {
    conn.execute(
        "UPDATE smart_albums SET name = ?1, search = ?2 WHERE id = ?3",
        params![name, search_to_value(search).to_string(), id],
    )?;
    Ok(())
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
/// `resolve` turns an album's content words into the items they match (REQ-036-07).
pub fn rows(conn: &Connection, resolve: &dyn Fn(&GallerySearch) -> GallerySearch) -> Result<Vec<AlbumRow>> {
    let mut rows = Vec::new();
    for album in list(conn)? {
        let missing = missing(conn, &album.search)?;
        let count = match missing {
            Some(_) => 0,
            None => crate::gallery::count(conn, &resolve(&album.search))?,
        };
        rows.push(AlbumRow { album, count, missing });
    }
    Ok(rows)
}

fn missing(conn: &Connection, search: &GallerySearch) -> Result<Option<Missing>> {
    // A rule album's libraries and folders count like a search's (REQ-034-06, REQ-039-03).
    if let Some(rules) = &search.rules {
        for condition in &rules.conditions {
            let place = match condition {
                Condition::Library(library) => GallerySearch {
                    library: Some(*library),
                    ..GallerySearch::default()
                },
                Condition::Folder { library, folder } => GallerySearch {
                    library: Some(*library),
                    folder: Some(folder.clone()),
                    ..GallerySearch::default()
                },
                _ => continue,
            };
            if let Some(missing) = missing(conn, &place)? {
                return Ok(Some(missing));
            }
        }
    }
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

    /// REQ-036-07: an album with content words counts the items its resolver says they match.
    #[test]
    fn req_036_07_counts_an_albums_content_matches() {
        let conn = crate::open_in_memory().unwrap();
        let root = insert_library(&conn, "D:\\Photos", "Photos", LibraryStatus::Online)
            .unwrap()
            .id;
        let mut writer = IndexWriter::new(conn);
        writer
            .write(
                root,
                &[file("a.jpg", Some(5)), file("b.jpg", Some(5)), file("c.jpg", None)],
            )
            .unwrap();
        let conn: Connection = writer.into();
        let dogs = GallerySearch {
            content: Some("dogs".into()),
            ..rated(5)
        };
        insert(&conn, "Dogs", &dogs).unwrap();
        let first = crate::media::id_of(&conn, root, "a.jpg").unwrap().unwrap();
        let third = crate::media::id_of(&conn, root, "c.jpg").unwrap().unwrap();
        let resolve = |search: &GallerySearch| match &search.content {
            Some(_) => search.with_content_matches(&[first, third]),
            None => search.clone(),
        };
        let rows = rows(&conn, &resolve).unwrap();
        assert_eq!(rows[0].count, 1, "a.jpg: matched and rated 5");
        assert_eq!(rows[0].album.search.content.as_deref(), Some("dogs"));
    }

    /// REQ-039-02, REQ-039-03, REQ-039-06: rule albums are kept under their name, match all or any of their
    /// conditions, and say when their library is gone.
    #[test]
    fn req_039_02_req_039_03_rule_albums_match_all_or_any() {
        use domain::album_rules::{Bound, Condition, DateField, Days, Orientation, Rules, Unit};
        let conn = crate::open_in_memory().unwrap();
        let root = insert_library(&conn, "D:\\Photos", "Photos", LibraryStatus::Online)
            .unwrap()
            .id;
        let today = domain::time_zone::today();
        let now = domain::search::days_ms(Some(today), None).0.unwrap() + 3_600_000;
        let old = domain::search::days_ms(today.checked_sub_days(chrono::Days::new(400)), None)
            .0
            .unwrap();
        let shaped =
            |rel: &str, size: i64, w: i64, h: i64, taken: i64, kind: MediaKind, rating: Option<i64>| IndexedFile {
                size,
                kind,
                metadata: MediaMetadata {
                    width: Some(w),
                    height: Some(h),
                    taken_at: Some(taken),
                    duration: (kind == MediaKind::Video).then_some(90.0),
                    rating,
                    tags: match rel {
                        "beach.jpg" => vec![vec!["Beach".into()]],
                        "anna.jpg" => vec![vec!["People".into(), "Anna".into()]],
                        _ => Vec::new(),
                    },
                    ..Default::default()
                },
                ..file(rel, None)
            };
        let mut writer = IndexWriter::new(conn);
        writer
            .write(
                root,
                &[
                    shaped("beach.jpg", 5_000_000, 4000, 3000, old, MediaKind::Image, Some(5)),
                    shaped("anna.jpg", 100, 300, 400, now, MediaKind::Image, None),
                    shaped("clip.mp4", 50_000_000, 1920, 1080, now, MediaKind::Video, Some(2)),
                    shaped("square.png", 10, 100, 100, old, MediaKind::Image, Some(3)),
                ],
            )
            .unwrap();
        let conn: Connection = writer.into();
        let names = |rules: Rules| -> Vec<String> {
            let search = GallerySearch {
                rules: Some(Box::new(rules)),
                ..GallerySearch::default()
            };
            let layout = crate::gallery::layout(&conn, &search).unwrap();
            let mut names: Vec<String> = crate::gallery::items(&conn, &layout.ids)
                .unwrap()
                .into_iter()
                .map(|item| item.file_name)
                .collect();
            names.sort();
            names
        };
        let rules = |any: bool, conditions: Vec<Condition>| Rules {
            name: String::new(),
            any,
            conditions,
        };
        let last_30 = Condition::Date {
            field: DateField::Taken,
            days: Days::Last(30, Unit::Days),
        };
        assert_eq!(names(rules(false, vec![last_30.clone()])), ["anna.jpg", "clip.mp4"]);
        assert_eq!(
            names(rules(false, vec![last_30.clone(), Condition::Kind(MediaKind::Video)])),
            ["clip.mp4"]
        );
        assert_eq!(
            names(rules(
                true,
                vec![
                    Condition::Tag {
                        name: "beach".into(),
                        is: true
                    },
                    Condition::HasPeople
                ]
            )),
            ["anna.jpg", "beach.jpg"]
        );
        assert_eq!(names(rules(false, vec![Condition::NoTags])), ["clip.mp4", "square.png"]);
        assert_eq!(
            names(rules(
                false,
                vec![Condition::Tag {
                    name: "Beach".into(),
                    is: false
                }]
            ))
            .len(),
            3
        );
        assert_eq!(
            names(rules(
                false,
                vec![Condition::Tag {
                    name: "nowhere".into(),
                    is: true
                }]
            )),
            Vec::<String>::new()
        );
        assert_eq!(
            names(rules(false, vec![Condition::RatingAtLeast(3)])),
            ["beach.jpg", "square.png"]
        );
        assert_eq!(
            names(rules(false, vec![Condition::RatingIn(vec![0, 2])])),
            ["anna.jpg", "clip.mp4"]
        );
        let at_least = |value| Bound { at_least: true, value };
        assert_eq!(
            names(rules(false, vec![Condition::Size(at_least(1_000_000.0))])),
            ["beach.jpg", "clip.mp4"]
        );
        assert_eq!(
            names(rules(false, vec![Condition::Duration(at_least(60.0))])),
            ["clip.mp4"]
        );
        assert_eq!(
            names(rules(false, vec![Condition::Megapixels(at_least(10.0))])),
            ["beach.jpg"]
        );
        assert_eq!(
            names(rules(false, vec![Condition::Orientation(Orientation::Portrait)])),
            ["anna.jpg"]
        );
        assert_eq!(
            names(rules(false, vec![Condition::Orientation(Orientation::Square)])),
            ["square.png"]
        );
        assert_eq!(
            names(rules(false, vec![Condition::NameContains("BEACH".into())])),
            ["beach.jpg"]
        );
        assert_eq!(
            names(rules(false, vec![Condition::Shows("dogs".into())])),
            Vec::<String>::new()
        );
        assert_eq!(names(rules(true, vec![])).len(), 4, "no conditions: everything");

        // Kept under its name, and its library missing is noticed.
        let search = GallerySearch {
            rules: Some(Box::new(rules(true, vec![Condition::HasPeople]))),
            ..GallerySearch::default()
        };
        let id = insert(&conn, "People", &search).unwrap();
        let listed = list(&conn).unwrap();
        assert_eq!(
            listed[0].search.rules.as_ref().map(|rules| rules.name.as_str()),
            Some("People")
        );
        let gone = GallerySearch {
            rules: Some(Box::new(rules(false, vec![Condition::Library(root + 1)]))),
            ..GallerySearch::default()
        };
        update(&conn, id, "Gone", &gone).unwrap();
        let rows = rows(&conn, &GallerySearch::clone).unwrap();
        assert_eq!(
            (rows[0].album.name.as_str(), rows[0].missing),
            ("Gone", Some(Missing::Library))
        );
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
            rows(conn, &GallerySearch::clone)
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
