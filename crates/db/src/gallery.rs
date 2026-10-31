//! What the gallery shows: the whole ordered result up front (ids and aspects, so the layout is exact before any item
//! is loaded) and tile data for the visible range.

use crate::Result;
use domain::date_groups::{DateGroup, date_groups};
use domain::layout::tile_aspect;
use domain::media::MediaKind;
use domain::tags::{CATEGORY_ROOTS, TagCategory};
use rusqlite::Connection;
use std::collections::HashMap;

pub use domain::search::{GallerySearch, SortBy};

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

/// Query shapes by (estimated) matches of an included tag, measured on 500k synthetic media items: up to `DRIVING_MAX`,
/// the rarest tag drives (its media are looked up by id and sorted); otherwise the sort index is walked and rows are
/// checked against each tag's materialized media ids; beyond `MATERIALIZE_MAX`, a primary key probe per row is cheaper
/// than building the list.
const DRIVING_MAX: u64 = 5_000;
const MATERIALIZE_MAX: u64 = 100_000;

/// A tag with all its descendants, and an upper bound of the media items carrying any of them.
struct Subtree {
    ids: Vec<i64>,
    estimate: u64,
}

/// Subtrees of the given tags (normalized names, without duplicates); unknown tags are missing.
fn subtrees(conn: &Connection, names: &[String]) -> Result<HashMap<String, Subtree>> {
    let mut result: HashMap<String, Subtree> = HashMap::new();
    if names.is_empty() {
        return Ok(result);
    }
    // UNION (not UNION ALL) also ends on parent cycles.
    let mut stmt = conn.prepare_cached(
        "WITH RECURSIVE tree(name, id) AS (
           SELECT name_norm, id FROM tags WHERE name_norm IN (SELECT value FROM json_each(?1))
           UNION
           SELECT tree.name, tags.id FROM tags JOIN tree ON tags.parent_id = tree.id
         )
         SELECT tree.name, tree.id, tags.media_count FROM tree JOIN tags ON tags.id = tree.id",
    )?;
    let names_json = serde_json::to_string(names).expect("names serialize");
    let rows = stmt.query_map([names_json], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
    })?;
    for row in rows {
        let (name, id, count) = row?;
        let subtree = result.entry(name).or_insert(Subtree {
            ids: Vec::new(),
            estimate: 0,
        });
        subtree.ids.push(id);
        subtree.estimate += count.max(0) as u64;
    }
    Ok(result)
}

fn normalized(names: &[String]) -> Vec<String> {
    let mut seen = Vec::new();
    for name in names.iter().map(|name| domain::tags::normalize_tag(name)) {
        if !name.is_empty() && !seen.contains(&name) {
            seen.push(name);
        }
    }
    seen
}

fn id_list(ids: &[i64]) -> String {
    ids.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")
}

/// Media items carrying one of `ids`.
fn tagged_media(ids: &[i64]) -> String {
    format!("SELECT media_id FROM media_tags WHERE tag_id IN ({})", id_list(ids))
}

/// Correlated check via the primary key, per candidate row. Unary `+` pins the plan: `+media.id` stops SQLite from
/// rewriting EXISTS into an id lookup, `+tag_id` from probing once per id of a large subtree; instead it reads the few
/// tags of each media item and checks the list. Keep them (`docs/known-pitfalls.md`).
fn links_to(ids: &[i64]) -> String {
    format!(
        "SELECT 1 FROM media_tags WHERE media_tags.media_id = +media.id AND +media_tags.tag_id IN ({})",
        id_list(ids)
    )
}

/// Conditions on `media` for including all of `include` and none of `exclude` (tag names, with descendants), or
/// `None` if an included tag doesn't exist, so nothing can match (REQ-007-01 … -03). Tag ids are written into the
/// SQL as numbers, never text.
fn tag_conditions(conn: &Connection, include: &[String], exclude: &[String]) -> Result<Option<Vec<String>>> {
    let include_names = normalized(include);
    let included = subtrees(conn, &include_names)?;
    if included.len() < include_names.len() {
        return Ok(None);
    }
    let mut subtrees_by_size: Vec<&Subtree> = included.values().collect();
    subtrees_by_size.sort_by_key(|subtree| subtree.estimate);
    let mut conditions = Vec::new();
    if let Some((driving, rest)) = subtrees_by_size.split_first() {
        if driving.estimate <= DRIVING_MAX {
            conditions.push(format!("media.id IN ({})", tagged_media(&driving.ids)));
            for subtree in rest {
                conditions.push(format!("EXISTS ({})", links_to(&subtree.ids)));
            }
        } else {
            for subtree in &subtrees_by_size {
                conditions.push(if subtree.estimate <= MATERIALIZE_MAX {
                    // `+media.id` rules out id lookups, so SQLite keeps the sort index and builds the id list once.
                    format!("+media.id IN ({})", tagged_media(&subtree.ids))
                } else {
                    format!("EXISTS ({})", links_to(&subtree.ids))
                });
            }
        }
    }
    let excluded: Vec<i64> = subtrees(conn, &normalized(exclude))?
        .into_values()
        .flat_map(|subtree| subtree.ids)
        .collect();
    if !excluded.is_empty() {
        conditions.push(format!("NOT EXISTS ({})", links_to(&excluded)));
    }
    Ok(Some(conditions))
}

/// The WHERE clause of a search with its parameters (REQ-007-01 … -11).
fn filters(conn: &Connection, search: &GallerySearch) -> Result<(String, Vec<rusqlite::types::Value>)> {
    use rusqlite::types::Value;
    let mut conditions = Vec::new();
    let mut params: Vec<Value> = Vec::new();
    let mut param = |value: Value| {
        params.push(value);
        format!("?{}", params.len())
    };
    if let Some(library) = search.library {
        conditions.push(format!("media.root_id = {}", param(library.into())));
    }
    // `dir` ends with `/`, and `0` is the character after `/`: a range scan over the folder and its subfolders.
    if let Some(folder) = search.folder_filter() {
        let from = param(format!("{folder}/").into());
        let to = param(format!("{folder}0").into());
        conditions.push(format!("media.dir >= {from} AND media.dir < {to}"));
    }
    if let Some(kind) = search.kind {
        conditions.push(format!("media.kind = {}", param(kind.as_str().to_string().into())));
    }
    let (from, to) = search.date_range();
    if let Some(from) = from {
        conditions.push(format!("media.sort_date >= {}", param(from.into())));
    }
    if let Some(to) = to {
        conditions.push(format!("media.sort_date < {}", param(to.into())));
    }
    // Only 0 (unrated) to 5 are ratings; others match nothing.
    if !search.ratings.is_empty() {
        let ratings: Vec<String> = search
            .ratings
            .iter()
            .filter(|&&rating| rating <= 5)
            .map(|&rating| {
                if rating == 0 {
                    "media.rating IS NULL".to_string()
                } else {
                    format!("media.rating = {}", param(i64::from(rating).into()))
                }
            })
            .collect();
        conditions.push(format!("({})", ratings.join(" OR ")));
    }
    // `lower()` on both sides: SQLite's ASCII-only folding must match the generated `file_name_lower` column.
    if let Some(name) = search.name.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
        conditions.push(format!(
            "instr(media.file_name_lower, lower({})) > 0",
            param(name.to_string().into())
        ));
    }
    match tag_conditions(conn, &search.tags, &search.exclude)? {
        Some(tags) => conditions.extend(tags),
        None => conditions.push("0".into()),
    }
    let clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    Ok((clause, params))
}

/// Ids and aspects of every match in sort order, with the day groups of a date sort.
pub fn layout(conn: &Connection, search: &GallerySearch) -> Result<GalleryLayout> {
    let date = match search.sort_by {
        SortBy::Taken => "sort_date",
        SortBy::Modified => "mtime",
        _ => "0",
    };
    let (clause, params) = filters(conn, search)?;
    let sql = format!(
        "SELECT id, width, height, kind = 'video', {date} FROM media {clause} ORDER BY {}",
        order_by(search)
    );
    // Tag ids are part of the text: such searches are mostly new statements, others come back (sorts, refreshes).
    let (mut cached, mut fresh);
    let stmt: &mut rusqlite::Statement = if search.tags.is_empty() && search.exclude.is_empty() {
        cached = conn.prepare_cached(&sql)?;
        &mut cached
    } else {
        fresh = conn.prepare(&sql)?;
        &mut fresh
    };
    let mut rows = stmt.query(rusqlite::params_from_iter(params))?;
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
    /// Turns a photo without an orientation of its own (REQ-004-12).
    pub sidecar_orientation: Option<u8>,
}

/// Tile data of the given media items, in no particular order; ids that don't exist are left out.
pub fn items(conn: &Connection, ids: &[i64]) -> Result<Vec<GalleryItem>> {
    let ids_json = serde_json::to_string(ids).expect("ids serialize");
    let mut stmt = conn.prepare_cached(
        "SELECT id, root_id, rel_path, file_name, kind, size, mtime, taken_at, width, height, rating, duration,
                thumbhash, thumb_status, sidecar_orientation
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
                sidecar_orientation: row.get(14)?,
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
                ..Default::default()
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

    /// Search tests (`layout` with every filter).
    mod search {
        use super::*;
        use chrono::{Local, NaiveDate, TimeZone};

        struct Index {
            conn: Connection,
            root: i64,
        }

        impl Index {
            fn new() -> Self {
                let conn = crate::open_in_memory().unwrap();
                let root = libraries::insert(&conn, "C:\\Photos", "Photos", LibraryStatus::Online)
                    .unwrap()
                    .id;
                Self { conn, root }
            }

            /// A media item; `extra` sets columns as SQL (`"taken_at = 5"`).
            fn media(&self, root: i64, rel_path: &str, extra: &str) -> i64 {
                let file_name = rel_path.rsplit('/').next().unwrap();
                let id: i64 = self
                    .conn
                    .query_row(
                        "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime)
                         VALUES (?1, ?2, ?3, 'image', 1, 0) RETURNING id",
                        (root, rel_path, file_name),
                        |row| row.get(0),
                    )
                    .unwrap();
                if !extra.is_empty() {
                    self.conn
                        .execute(&format!("UPDATE media SET {extra} WHERE id = ?1"), [id])
                        .unwrap();
                }
                id
            }

            fn tag(&self, name: &str, parent: Option<i64>) -> i64 {
                self.conn
                    .query_row(
                        "INSERT INTO tags (name, name_norm, parent_id) VALUES (?1, lower(?1), ?2) RETURNING id",
                        (name, parent),
                        |row| row.get(0),
                    )
                    .unwrap()
            }

            fn link(&self, media: i64, tags: &[i64]) {
                for tag in tags {
                    self.conn
                        .execute(
                            "INSERT INTO media_tags (media_id, tag_id) VALUES (?1, ?2)",
                            (media, tag),
                        )
                        .unwrap();
                }
            }

            fn ids(&self, search: &GallerySearch) -> Vec<i64> {
                layout(&self.conn, search).unwrap().ids
            }
        }

        fn sorted(sort_by: SortBy, descending: bool) -> GallerySearch {
            GallerySearch {
                sort_by,
                descending,
                ..Default::default()
            }
        }

        #[test]
        fn req_007_12_orders_by_date_taken_newest_first_with_the_id_as_tie_breaker() {
            let index = Index::new();
            let ids: Vec<i64> = [5, 3, 3, 9, 1]
                .iter()
                .enumerate()
                .map(|(i, taken)| index.media(index.root, &format!("{i}.jpg"), &format!("taken_at = {taken}")))
                .collect();

            assert_eq!(
                index.ids(&GallerySearch::default()),
                [ids[3], ids[0], ids[2], ids[1], ids[4]]
            );
            assert_eq!(
                index.ids(&sorted(SortBy::Taken, false)),
                [ids[4], ids[1], ids[2], ids[0], ids[3]]
            );
        }

        #[test]
        fn req_007_12_sorts_by_every_field_in_both_directions() {
            let index = Index::new();
            let a = index.media(index.root, "z/a.jpg", "size = 1, mtime = 2, taken_at = 1");
            let b = index.media(index.root, "a/B.jpg", "size = 3, mtime = 3, taken_at = 3");
            let c = index.media(index.root, "z/c.jpg", "size = 2, mtime = 1, taken_at = 2");
            let cases = [
                (SortBy::Name, false, [a, b, c]),
                (SortBy::Name, true, [c, b, a]),
                (SortBy::Modified, false, [c, a, b]),
                (SortBy::Size, true, [b, c, a]),
                (SortBy::Path, false, [b, a, c]),
                (SortBy::Taken, false, [a, c, b]),
            ];
            for (sort_by, descending, expected) in cases {
                assert_eq!(
                    index.ids(&sorted(sort_by, descending)),
                    expected,
                    "{sort_by:?} {descending}"
                );
            }
        }

        #[test]
        fn req_007_07_req_007_10_falls_back_to_mtime_and_filters_by_kind_and_library() {
            let index = Index::new();
            let other = libraries::insert(&index.conn, "D:\\Other", "Other", LibraryStatus::Online)
                .unwrap()
                .id;
            let video = index.media(index.root, "b.mp4", "kind = 'video', mtime = 10");
            index.media(index.root, "a.jpg", "");
            index.media(other, "c.mp4", "kind = 'video'");

            let search = GallerySearch {
                library: Some(index.root),
                kind: Some(MediaKind::Video),
                ..Default::default()
            };
            assert_eq!(index.ids(&search), [video]);
        }

        #[test]
        fn req_007_10_filters_by_folder_including_subfolders() {
            let index = Index::new();
            let ids: Vec<i64> = ["a/1.jpg", "a/b/2.jpg", "ab/3.jpg", "4.jpg"]
                .iter()
                .enumerate()
                .map(|(i, path)| index.media(index.root, path, &format!("mtime = {}", -(i as i64))))
                .collect();
            let folder = |library, folder: &str| GallerySearch {
                library,
                folder: Some(folder.into()),
                ..Default::default()
            };

            assert_eq!(index.ids(&folder(Some(index.root), "a")), [ids[0], ids[1]]);
            assert_eq!(index.ids(&folder(Some(index.root), "a/b/")), [ids[1]]);
            assert_eq!(
                index.ids(&folder(None, "a")).len(),
                4,
                "a folder without a library is ignored"
            );
        }

        #[test]
        fn req_007_08_filters_by_local_days_both_inclusive() {
            let index = Index::new();
            let at = |d, h| {
                Local
                    .with_ymd_and_hms(2024, 3, d, h, 0, 0)
                    .earliest()
                    .unwrap()
                    .timestamp_millis()
            };
            let ids: Vec<i64> = [at(19, 12), at(20, 12), at(21, 0), at(21, 12), at(22, 12)]
                .iter()
                .enumerate()
                .map(|(i, taken)| index.media(index.root, &format!("{i}.jpg"), &format!("taken_at = {taken}")))
                .collect();
            let day = |d| NaiveDate::from_ymd_opt(2024, 3, d);
            let range = |from, to| GallerySearch {
                from,
                to,
                ..sorted(SortBy::Taken, false)
            };

            assert_eq!(index.ids(&range(day(20), day(21))), [ids[1], ids[2], ids[3]]);
            assert_eq!(index.ids(&range(day(21), None)), [ids[2], ids[3], ids[4]]);
            assert_eq!(index.ids(&range(None, day(19))), [ids[0]]);
        }

        /// Tree: places > france > paris; flat: cat, dog.
        fn tagged() -> (Index, [i64; 4]) {
            let index = Index::new();
            let places = index.tag("Places", None);
            let france = index.tag("France", Some(places));
            let paris = index.tag("Paris", Some(france));
            let (cat, dog) = (index.tag("Cat", None), index.tag("Dog", None));
            let file = |name: &str, tags: &[i64], mtime: i64| {
                let id = index.media(index.root, name, &format!("mtime = {mtime}"));
                index.link(id, tags);
                id
            };
            let ids = [
                file("paris-cat.jpg", &[paris, cat], 5),
                file("france-dog.jpg", &[france, dog], 4),
                file("cat.jpg", &[cat], 3),
                file("none.jpg", &[], 2),
            ];
            (index, ids)
        }

        fn tags(include: &[&str], exclude: &[&str]) -> GallerySearch {
            GallerySearch {
                tags: include.iter().map(|s| s.to_string()).collect(),
                exclude: exclude.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            }
        }

        #[test]
        fn req_007_01_requires_all_included_tags_matching_descendants() {
            let (index, [paris_cat, france_dog, _, _]) = tagged();
            assert_eq!(index.ids(&tags(&["places"], &[])), [paris_cat, france_dog]);
            assert_eq!(index.ids(&tags(&[" FRANCE ", "cat"], &[])), [paris_cat]);
            assert!(index.ids(&tags(&["paris", "dog"], &[])).is_empty());
        }

        #[test]
        fn req_007_02_excludes_tags_with_their_descendants() {
            let (index, [_, _, cat, untagged]) = tagged();
            assert_eq!(index.ids(&tags(&[], &["france"])), [cat, untagged]);
            assert_eq!(index.ids(&tags(&["cat"], &["paris"])), [cat]);
        }

        #[test]
        fn req_007_03_matches_nothing_for_unknown_included_tags_and_ignores_unknown_exclusions() {
            let (index, [paris_cat, _, cat, _]) = tagged();
            assert!(index.ids(&tags(&["cat", "unknown"], &[])).is_empty());
            assert_eq!(index.ids(&tags(&["cat"], &["unknown"])), [paris_cat, cat]);
        }

        /// REQ-007-01: the query shapes for common tags (materialized ids, primary key probes) give the same result.
        #[test]
        fn req_007_01_common_tags_take_the_other_query_shapes_with_the_same_result() {
            let index = Index::new();
            let (common, rare) = (index.tag("Common", None), index.tag("Rare", None));
            let mut both = Vec::new();
            for i in 0..6000 {
                let id = index.media(index.root, &format!("{i}.jpg"), &format!("mtime = {i}"));
                index.link(id, &[common]);
                if i % 1000 == 0 {
                    index.link(id, &[rare]);
                    both.push(id);
                }
            }
            // Items without the tag, which every query shape must leave out.
            for i in 0..10 {
                index.media(index.root, &format!("untagged {i}.jpg"), "");
            }
            both.reverse();
            assert_eq!(index.ids(&tags(&["common", "rare"], &[])), both);
            assert_eq!(index.ids(&tags(&["common"], &[])).len(), 6000);
            // As if the tag were very common, beyond the materialized list.
            index
                .conn
                .execute("UPDATE tags SET media_count = 200000 WHERE name = 'Common'", [])
                .unwrap();
            assert_eq!(index.ids(&tags(&["common"], &["rare"])).len(), 5994);
        }

        #[test]
        fn req_007_09_filters_by_exact_rating_0_meaning_unrated() {
            let index = Index::new();
            let five = index.media(index.root, "a.jpg", "rating = 5");
            let three = index.media(index.root, "b.jpg", "rating = 3");
            let unrated = index.media(index.root, "c.jpg", "");
            let rated = |ratings: &[u8]| {
                let mut ids = index.ids(&GallerySearch {
                    ratings: ratings.to_vec(),
                    ..Default::default()
                });
                ids.sort();
                ids
            };
            assert_eq!(rated(&[5]), [five]);
            assert_eq!(rated(&[0]), [unrated]);
            assert_eq!(rated(&[3, 5]), [five.min(three), five.max(three)]);
            assert_eq!(rated(&[]).len(), 3);
        }

        #[test]
        fn req_007_06_req_007_11_matches_a_case_insensitive_file_name_substring_combined_with_other_filters() {
            let index = Index::new();
            let beach = index.media(index.root, "IMG_Beach_01.jpg", "rating = 5");
            let beach_video = index.media(index.root, "beach.mp4", "kind = 'video'");
            index.media(index.root, "mountain.jpg", "");
            let name = |name: &str, kind, ratings: &[u8]| {
                let mut ids = index.ids(&GallerySearch {
                    name: Some(name.into()),
                    kind,
                    ratings: ratings.to_vec(),
                    ..Default::default()
                });
                ids.sort();
                ids
            };
            assert_eq!(name("BEACH", None, &[]), [beach, beach_video]);
            assert_eq!(name("h_0", None, &[]), [beach]);
            assert_eq!(name("beach", Some(MediaKind::Video), &[]), [beach_video]);
            assert_eq!(name("beach", None, &[5]), [beach]);
            assert!(name("%", None, &[]).is_empty());
        }
    }
}
