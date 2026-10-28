//! One media item's details for the viewer's info panel and the info sheet (REQ-009-08).

use crate::Result;
use domain::media::MediaKind;
use domain::tags::{TagCategory, category_of_root};
use rusqlite::{Connection, OptionalExtension as _};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct MediaDetail {
    pub id: i64,
    pub root_id: i64,
    pub library_label: String,
    /// The file: the library's folder joined with the relative path.
    pub path: PathBuf,
    pub file_name: String,
    pub kind: MediaKind,
    pub size: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    /// Seconds.
    pub duration: Option<f64>,
    /// Milliseconds since the Unix epoch.
    pub taken_at: Option<i64>,
    pub mtime: i64,
    pub thumbhash: Option<String>,
    pub rating: Option<i64>,
    /// The item's own tags (not their ancestors), by normalized name.
    pub tags: Vec<DetailTag>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailTag {
    pub id: i64,
    pub name: String,
    /// People or Places, by the tag's top-level ancestor; `None` for other tags.
    pub category: Option<TagCategory>,
}

/// The library folder joined with a relative path stored with `/` separators; `None` for a relative path that would
/// leave the library (REQ-011-08), so the item is treated as missing.
pub fn absolute_path(root: &str, rel_path: &str) -> Option<PathBuf> {
    if !domain::media::is_safe_rel_path(rel_path) {
        return None;
    }
    let mut path = PathBuf::from(root);
    path.extend(rel_path.split('/'));
    Some(path)
}

/// The file of a media item in `libraries`, or `None` if its library isn't among them or its path leaves the library.
pub fn file_in(libraries: &[crate::libraries::Library], root_id: i64, rel_path: &str) -> Option<PathBuf> {
    let library = libraries.iter().find(|library| library.id == root_id)?;
    absolute_path(&library.path, rel_path)
}

/// The details of media item `id`, or `None` if it isn't indexed (REQ-009-12) or its path leaves its library
/// (REQ-011-08).
pub fn detail(conn: &Connection, id: i64) -> Result<Option<MediaDetail>> {
    let row = conn
        .prepare_cached(
            "SELECT media.id, media.root_id, library_roots.label, library_roots.path, media.rel_path, media.file_name,
                    media.kind, media.size, media.width, media.height, media.duration, media.taken_at, media.mtime,
                    media.thumbhash, media.rating
             FROM media JOIN library_roots ON library_roots.id = media.root_id
             WHERE media.id = ?1",
        )?
        .query_row([id], |row| {
            let Some(path) = absolute_path(&row.get::<_, String>(3)?, &row.get::<_, String>(4)?) else {
                return Ok(None);
            };
            Ok(Some(MediaDetail {
                id: row.get(0)?,
                root_id: row.get(1)?,
                library_label: row.get(2)?,
                path,
                file_name: row.get(5)?,
                kind: MediaKind::parse(&row.get::<_, String>(6)?).unwrap_or(MediaKind::Image),
                size: row.get(7)?,
                width: row.get(8)?,
                height: row.get(9)?,
                duration: row.get(10)?,
                taken_at: row.get(11)?,
                mtime: row.get(12)?,
                thumbhash: row.get(13)?,
                rating: row.get(14)?,
                tags: Vec::new(),
            }))
        })
        .optional()?
        .flatten();
    let Some(mut detail) = row else { return Ok(None) };
    let mut stmt = conn.prepare_cached(
        "SELECT tags.id, tags.name FROM media_tags JOIN tags ON tags.id = media_tags.tag_id
         WHERE media_tags.media_id = ?1 ORDER BY tags.name_norm",
    )?;
    let tags: Vec<(i64, String)> = stmt
        .query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let categories = categories_of(conn, id)?;
    detail.tags = tags
        .into_iter()
        .map(|(id, name)| DetailTag {
            category: categories.get(&id).copied(),
            id,
            name,
        })
        .collect();
    Ok(Some(detail))
}

/// The category of each tag of media item `media`, by the name of its top-level ancestor.
fn categories_of(conn: &Connection, media: i64) -> Result<HashMap<i64, TagCategory>> {
    let mut stmt = conn.prepare_cached(
        "WITH RECURSIVE ancestors(start, id) AS (
           SELECT tag_id, tag_id FROM media_tags WHERE media_id = ?1
           UNION
           SELECT ancestors.start, t.parent_id FROM tags AS t
             JOIN ancestors ON t.id = ancestors.id WHERE t.parent_id IS NOT NULL
         )
         SELECT ancestors.start, t.name FROM ancestors JOIN tags AS t ON t.id = ancestors.id
         WHERE t.parent_id IS NULL",
    )?;
    let rows = stmt.query_map([media], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
    let mut categories = HashMap::new();
    for row in rows {
        let (tag, root) = row?;
        if let Some(category) = category_of_root(&root) {
            categories.insert(tag, category);
        }
    }
    Ok(categories)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::libraries::{self, LibraryStatus};

    fn tag(conn: &Connection, name: &str, parent: Option<i64>) -> i64 {
        conn.query_row(
            "INSERT INTO tags (name, name_norm, parent_id) VALUES (?1, lower(?1), ?2) RETURNING id",
            (name, parent),
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn req_009_08_returns_the_details_with_tags_by_category() {
        let conn = crate::open_in_memory().unwrap();
        let root = libraries::insert(&conn, "D:\\Photos", "Holiday photos", LibraryStatus::Online)
            .unwrap()
            .id;
        let media: i64 = conn
            .query_row(
                "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime, width, height, rating, taken_at)
                 VALUES (?1, '2024/rome/a.jpg', 'a.jpg', 'image', 2048, 5, 4000, 3000, 4, 7) RETURNING id",
                [root],
                |row| row.get(0),
            )
            .unwrap();
        let people = tag(&conn, "People", None);
        let anna = tag(&conn, "Anna", Some(people));
        let places = tag(&conn, "Places", None);
        let italy = tag(&conn, "Italy", Some(places));
        let rome = tag(&conn, "Rome", Some(italy));
        let beach = tag(&conn, "Beach", None);
        for tag in [anna, rome, beach] {
            conn.execute(
                "INSERT INTO media_tags (media_id, tag_id) VALUES (?1, ?2)",
                (media, tag),
            )
            .unwrap();
        }

        let detail = detail(&conn, media).unwrap().unwrap();

        assert_eq!(detail.library_label, "Holiday photos");
        assert_eq!(detail.path, PathBuf::from("D:\\Photos\\2024\\rome\\a.jpg"));
        assert_eq!(
            (detail.width, detail.height, detail.rating, detail.taken_at),
            (Some(4000), Some(3000), Some(4), Some(7))
        );
        let tags: Vec<(&str, Option<TagCategory>)> = detail
            .tags
            .iter()
            .map(|tag| (tag.name.as_str(), tag.category))
            .collect();
        assert_eq!(
            tags,
            [
                ("Anna", Some(TagCategory::People)),
                ("Beach", None),
                ("Rome", Some(TagCategory::Places)),
            ]
        );
    }

    #[test]
    fn req_011_08_treats_an_item_whose_path_leaves_its_library_as_missing() {
        let conn = crate::open_in_memory().unwrap();
        let library = libraries::insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Online).unwrap();
        let media: i64 = conn
            .query_row(
                "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime)
                 VALUES (?1, '../../Windows/win.ini', 'win.ini', 'image', 1, 0) RETURNING id",
                [library.id],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(detail(&conn, media).unwrap(), None);
        let libraries = [library];
        assert_eq!(file_in(&libraries, libraries[0].id, "../../Windows/win.ini"), None);
        assert_eq!(file_in(&libraries, libraries[0].id, "C:/Windows/win.ini"), None);
        assert_eq!(
            file_in(&libraries, libraries[0].id, "2024/a.jpg"),
            Some(PathBuf::from("D:\\Photos\\2024\\a.jpg"))
        );
        let source = crate::thumbs::thumb_source(&conn, media).unwrap().unwrap();
        assert_eq!(source.file(), None);
    }

    #[test]
    fn req_009_12_has_no_details_for_an_item_that_isnt_indexed() {
        let conn = crate::open_in_memory().unwrap();
        assert_eq!(detail(&conn, 42).unwrap(), None);
    }
}
