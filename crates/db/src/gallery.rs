//! What the gallery shows: the whole ordered result up front (ids and aspects, so the layout is exact before any
//! item is loaded) and tile data for the visible range, as the Electron version's `media.layout` and `media.byIds`.

use crate::Result;
use domain::date_groups::{DateGroup, date_groups};
use domain::layout::tile_aspect;
use domain::media::MediaKind;
use domain::tags::{CATEGORY_ROOTS, TagCategory};
use rusqlite::Connection;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    Name,
    /// Date taken, else the date modified (`sort_date`).
    #[default]
    Taken,
    Modified,
    Path,
    Size,
}

impl SortBy {
    /// Date sorts group by day (REQ-016-04).
    pub fn is_date(self) -> bool {
        matches!(self, Self::Taken | Self::Modified)
    }
}

/// The search the gallery shows. Filters come with spec 007 (M7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GallerySearch {
    pub sort_by: SortBy,
    pub descending: bool,
}

impl Default for GallerySearch {
    fn default() -> Self {
        Self {
            sort_by: SortBy::Taken,
            descending: true,
        }
    }
}

/// The ordered result of a search.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GalleryLayout {
    pub ids: Vec<i64>,
    /// Height / width of each tile, clamped (`tile_aspect`).
    pub aspects: Vec<f64>,
    /// Day groups of a date sort (spec 016), whether the gallery shows them or not; empty for other sorts.
    pub groups: Vec<DateGroup>,
}

fn order_by(search: &GallerySearch) -> String {
    let dir = if search.descending { "DESC" } else { "ASC" };
    let columns: &[&str] = match search.sort_by {
        SortBy::Name => &["file_name_lower"],
        SortBy::Taken => &["sort_date"],
        SortBy::Modified => &["mtime"],
        // Unique, so it needs no id tiebreak.
        SortBy::Path => &["root_id", "rel_path"],
        SortBy::Size => &["size"],
    };
    let mut terms: Vec<String> = columns.iter().map(|c| format!("{c} {dir}")).collect();
    if search.sort_by != SortBy::Path {
        terms.push(format!("id {dir}"));
    }
    terms.join(", ")
}

/// Ids and aspects of every match in sort order, with the day groups of a date sort.
pub fn layout(conn: &Connection, search: &GallerySearch) -> Result<GalleryLayout> {
    let date = match search.sort_by {
        SortBy::Taken => "sort_date",
        SortBy::Modified => "mtime",
        _ => "0",
    };
    let sql = format!(
        "SELECT id, width, height, kind = 'video', {date} FROM media ORDER BY {}",
        order_by(search)
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query([])?;
    let (mut ids, mut aspects, mut videos, mut dates) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    while let Some(row) = rows.next()? {
        ids.push(row.get(0)?);
        aspects.push(tile_aspect(row.get(1)?, row.get(2)?));
        videos.push(row.get::<_, bool>(3)?);
        dates.push(row.get::<_, i64>(4)?);
    }
    let groups = if search.sort_by.is_date() {
        date_groups(&dates, |i| videos[i])
    } else {
        Vec::new()
    };
    Ok(GalleryLayout { ids, aspects, groups })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThumbStatus {
    #[default]
    Pending,
    Ready,
    Error,
}

/// Whether a media item has tags of each category, or other tags (REQ-017-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tagged {
    pub people: bool,
    pub places: bool,
    pub other: bool,
}

/// What a tile or list row shows of a media item.
#[derive(Debug, Clone, PartialEq)]
pub struct GalleryItem {
    pub id: i64,
    pub root_id: i64,
    pub rel_path: String,
    pub file_name: String,
    pub kind: MediaKind,
    pub size: i64,
    /// Milliseconds since the Unix epoch.
    pub mtime: i64,
    pub taken_at: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub rating: Option<i64>,
    /// Seconds.
    pub duration: Option<f64>,
    pub thumbhash: Option<String>,
    pub thumb_status: ThumbStatus,
    pub tagged: Tagged,
}

/// Tile data of the given media items, in no particular order; ids that don't exist are left out.
pub fn items(conn: &Connection, ids: &[i64]) -> Result<Vec<GalleryItem>> {
    let ids_json = serde_json::to_string(ids).expect("ids serialize");
    let mut stmt = conn.prepare_cached(
        "SELECT id, root_id, rel_path, file_name, kind, size, mtime, taken_at, width, height, rating, duration,
                thumbhash, thumb_status
         FROM media WHERE id IN (SELECT value FROM json_each(?1))",
    )?;
    let mut items: Vec<GalleryItem> = stmt
        .query_map([&ids_json], |row| {
            Ok(GalleryItem {
                id: row.get(0)?,
                root_id: row.get(1)?,
                rel_path: row.get(2)?,
                file_name: row.get(3)?,
                kind: MediaKind::parse(&row.get::<_, String>(4)?).unwrap_or(MediaKind::Image),
                size: row.get(5)?,
                mtime: row.get(6)?,
                taken_at: row.get(7)?,
                width: row.get(8)?,
                height: row.get(9)?,
                rating: row.get(10)?,
                duration: row.get(11)?,
                thumbhash: row.get(12)?,
                thumb_status: match row.get::<_, String>(13)?.as_str() {
                    "ready" => ThumbStatus::Ready,
                    "error" => ThumbStatus::Error,
                    _ => ThumbStatus::Pending,
                },
                tagged: Tagged::default(),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut links_stmt = conn.prepare_cached(
        "SELECT media_id, tag_id FROM media_tags WHERE media_id IN (SELECT value FROM json_each(?1))",
    )?;
    let links: Vec<(i64, i64)> = links_stmt
        .query_map([&ids_json], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut tag_ids: Vec<i64> = links.iter().map(|(_, tag)| *tag).collect();
    tag_ids.sort_unstable();
    tag_ids.dedup();
    let categories = categories_of(conn, &tag_ids)?;
    let index: HashMap<i64, usize> = items.iter().enumerate().map(|(i, item)| (item.id, i)).collect();
    for (media, tag) in links {
        if let Some(&i) = index.get(&media) {
            let tagged = &mut items[i].tagged;
            match categories.get(&tag) {
                Some(TagCategory::People) => tagged.people = true,
                Some(TagCategory::Places) => tagged.places = true,
                None => tagged.other = true,
            }
        }
    }
    Ok(items)
}

/// The category of each given tag: a tag belongs to a category when its outermost ancestor (or itself) is one of the
/// category's root names. Tags outside any category are left out.
pub fn categories_of(conn: &Connection, tag_ids: &[i64]) -> Result<HashMap<i64, TagCategory>> {
    let mut result = HashMap::new();
    if tag_ids.is_empty() {
        return Ok(result);
    }
    let mut stmt = conn.prepare_cached(
        "WITH RECURSIVE ancestors(start, id) AS (
           SELECT id, id FROM tags WHERE id IN (SELECT value FROM json_each(?1))
           UNION
           SELECT ancestors.start, t.parent_id FROM tags AS t
             JOIN ancestors ON t.id = ancestors.id WHERE t.parent_id IS NOT NULL
         )
         SELECT ancestors.start, t.name_norm FROM ancestors JOIN tags AS t ON t.id = ancestors.id
         WHERE t.parent_id IS NULL",
    )?;
    let ids_json = serde_json::to_string(tag_ids).expect("ids serialize");
    let rows = stmt.query_map([&ids_json], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
    for row in rows {
        let (id, root) = row?;
        if let Some((category, _)) = CATEGORY_ROOTS.iter().find(|(_, names)| names.contains(&root.as_str())) {
            result.insert(id, *category);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::libraries::{self, LibraryStatus};
    use crate::writer::{IndexWriter, IndexedFile};
    use domain::media::MediaMetadata;

    fn file(rel_path: &str, size: (i64, i64), taken_at: Option<i64>, tags: &[&[&str]]) -> IndexedFile {
        IndexedFile {
            rel_path: rel_path.into(),
            file_name: rel_path.rsplit('/').next().unwrap().into(),
            kind: if rel_path.ends_with(".mp4") {
                MediaKind::Video
            } else {
                MediaKind::Image
            },
            size: 10,
            mtime: 1_000,
            sidecar_mtime: None,
            metadata: MediaMetadata {
                width: Some(size.0),
                height: Some(size.1),
                taken_at,
                tags: tags.iter().map(|p| p.iter().map(|s| s.to_string()).collect()).collect(),
                ..Default::default()
            },
        }
    }

    fn index(files: &[IndexedFile]) -> Connection {
        let conn = crate::open_in_memory().unwrap();
        let root = libraries::insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Online)
            .unwrap()
            .id;
        let mut writer = IndexWriter::new(conn);
        writer.write(root, files).unwrap();
        writer.into()
    }

    fn id_of(conn: &Connection, rel_path: &str) -> i64 {
        conn.query_row("SELECT id FROM media WHERE rel_path = ?1", [rel_path], |r| r.get(0))
            .unwrap()
    }

    const DAY: i64 = 24 * 3600 * 1000;

    /// REQ-006-08, REQ-016-01: every match in sort order (newest first, ties by id), with clamped aspects and the
    /// day groups.
    #[test]
    fn req_006_08_req_016_01_lists_every_match_in_date_order_with_aspects_and_groups() {
        let noon = 1_780_000_000_000 / DAY * DAY + DAY / 2;
        let conn = index(&[
            file("a.jpg", (400, 300), Some(noon), &[]),
            file("b.jpg", (100, 1000), Some(noon + DAY), &[]),
            file("c.mp4", (0, 0), Some(noon + DAY), &[]),
            file("undated.jpg", (10, 10), None, &[]),
        ]);

        let layout = layout(&conn, &GallerySearch::default()).unwrap();

        let (a, b, c) = (id_of(&conn, "a.jpg"), id_of(&conn, "b.jpg"), id_of(&conn, "c.mp4"));
        let undated = id_of(&conn, "undated.jpg");
        assert_eq!(layout.ids, [c, b, a, undated]);
        assert_eq!(layout.aspects, [1.0, 3.0, 0.75, 1.0]);
        let groups: Vec<(usize, u32, u32)> = layout.groups.iter().map(|g| (g.start, g.photos, g.videos)).collect();
        assert_eq!(groups, [(0, 1, 1), (2, 1, 0), (3, 1, 0)]);

        let by_name = super::layout(
            &conn,
            &GallerySearch {
                sort_by: SortBy::Name,
                descending: false,
            },
        )
        .unwrap();
        assert_eq!(by_name.ids, [a, b, c, undated]);
        assert!(by_name.groups.is_empty(), "REQ-016-04: no groups without a date sort");
    }

    #[test]
    fn req_017_01_loads_items_with_the_categories_of_their_tags() {
        let conn = index(&[
            file("people.jpg", (1, 1), None, &[&["People", "Anna"]]),
            file("place.jpg", (1, 1), None, &[&["Locations", "Italy", "Rome"]]),
            file("plain.jpg", (1, 1), None, &[&["Beach"]]),
            file("none.jpg", (1, 1), None, &[]),
        ]);
        let ids: Vec<i64> = ["people.jpg", "place.jpg", "plain.jpg", "none.jpg"]
            .iter()
            .map(|p| id_of(&conn, p))
            .collect();

        let mut loaded = items(&conn, &[ids.clone(), vec![9999]].concat()).unwrap();
        loaded.sort_by_key(|item| item.id);

        let tagged: Vec<(&str, Tagged)> = loaded.iter().map(|i| (i.file_name.as_str(), i.tagged)).collect();
        let t = |people, places, other| Tagged { people, places, other };
        assert_eq!(
            tagged,
            [
                ("people.jpg", t(true, false, false)),
                ("place.jpg", t(false, true, false)),
                ("plain.jpg", t(false, false, true)),
                ("none.jpg", t(false, false, false)),
            ]
        );
        assert_eq!(loaded[0].thumb_status, ThumbStatus::Pending);
        assert_eq!((loaded[0].width, loaded[0].height), (Some(1), Some(1)));
    }
}
