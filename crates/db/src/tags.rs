//! Tag suggestions and the sidebar's lists (specs 007, 008), as the Electron version's `tags`, `media.ratingCounts`
//! and `libraries.folders` queries.

use crate::Result;
use domain::folders::CategoryTag;
use domain::tags::{CATEGORY_ROOTS, TagCategory, normalize_tag};
use rusqlite::Connection;

/// A tag with the number of media items carrying it or one of its descendants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagCount {
    pub id: i64,
    pub name: String,
    pub count: u64,
}

/// Ranks the tags selected by `roots` (SQL selecting `id`s) with their descendants' counts, most used first, unused
/// ones left out (they linger until the indexer prunes them after its next scan).
fn ranked(conn: &Connection, roots: &str, params: &[&dyn rusqlite::ToSql], limit: usize) -> Result<Vec<TagCount>> {
    let sql = format!(
        "WITH RECURSIVE tree(root, id) AS (
           {roots}
           UNION
           SELECT tree.root, tags.id FROM tags JOIN tree ON tags.parent_id = tree.id
         )
         SELECT t.id, t.name, sum(d.media_count) AS count
         FROM tree JOIN tags AS d ON d.id = tree.id JOIN tags AS t ON t.id = tree.root
         GROUP BY tree.root
         HAVING count > 0
         ORDER BY count DESC, t.name_norm
         LIMIT {limit}"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params, |row| {
        Ok(TagCount {
            id: row.get(0)?,
            name: row.get(1)?,
            count: row.get::<_, i64>(2)?.max(0) as u64,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Tags whose normalized name starts with the normalized `prefix`, most used first (REQ-007-04). A range scan on the
/// unique `name_norm` index, which `LIKE` can't use.
pub fn autocomplete(conn: &Connection, prefix: &str, limit: usize) -> Result<Vec<TagCount>> {
    let prefix = normalize_tag(prefix);
    if prefix.is_empty() {
        return ranked(conn, "SELECT id, id FROM tags", &[], limit);
    }
    let end = format!("{prefix}\u{10FFFF}");
    ranked(
        conn,
        "SELECT id, id FROM tags WHERE name_norm >= ?1 AND name_norm < ?2",
        &[&prefix, &end],
        limit,
    )
}

/// The `WITH` part selecting a category's root tags (by any of its root names) and their descendants.
fn category_cte(category: TagCategory) -> String {
    let names = CATEGORY_ROOTS
        .iter()
        .find(|(c, _)| *c == category)
        .map(|(_, names)| *names)
        .unwrap_or_default();
    let list = names
        .iter()
        .map(|name| format!("'{name}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "roots(id) AS (SELECT id FROM tags WHERE parent_id IS NULL AND name_norm IN ({list})),
         descendants(id) AS (
           SELECT id FROM tags WHERE parent_id IN (SELECT id FROM roots)
           UNION
           SELECT tags.id FROM tags JOIN descendants ON tags.parent_id = descendants.id
         )"
    )
}

/// Every tag of a category except its root, most used first, with counts including descendants (REQ-008-03).
pub fn category(conn: &Connection, category: TagCategory) -> Result<Vec<CategoryTag>> {
    let sql = format!(
        "WITH RECURSIVE {},
         tree(root, id) AS (
           SELECT id, id FROM descendants
           UNION
           SELECT tree.root, tags.id FROM tags JOIN tree ON tags.parent_id = tree.id
         )
         SELECT t.id, t.name, CASE WHEN t.parent_id IN (SELECT id FROM roots) THEN NULL ELSE t.parent_id END,
                sum(d.media_count) AS count
         FROM tree JOIN tags AS d ON d.id = tree.id JOIN tags AS t ON t.id = tree.root
         GROUP BY tree.root
         HAVING count > 0
         ORDER BY count DESC, t.name_norm",
        category_cte(category)
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(CategoryTag {
            id: row.get(0)?,
            name: row.get(1)?,
            parent_id: row.get(2)?,
            count: row.get::<_, i64>(3)?.max(0) as u64,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The most used tags outside both categories (and their roots), for the sidebar's _Tags_ group (REQ-008-05).
pub fn popular(conn: &Connection, limit: usize) -> Result<Vec<TagCount>> {
    let categorized: Vec<String> = [TagCategory::People, TagCategory::Places]
        .into_iter()
        .map(|category| {
            format!(
                "SELECT id FROM (WITH RECURSIVE {} SELECT id FROM roots UNION SELECT id FROM descendants)",
                category_cte(category)
            )
        })
        .collect();
    let roots = format!(
        "SELECT id, id FROM tags WHERE id NOT IN ({})",
        categorized.join(" UNION ")
    );
    ranked(conn, &roots, &[], limit)
}

/// Media items per rating (`0` = unrated) across all libraries, highest first; ratings without media are left out
/// (REQ-008-04).
pub fn rating_counts(conn: &Connection) -> Result<Vec<(u8, u64)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT coalesce(rating, 0), count(*) FROM media GROUP BY rating ORDER BY rating DESC NULLS LAST",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?.clamp(0, 5) as u8,
            row.get::<_, i64>(1)?.max(0) as u64,
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Folders of a library that directly contain media items, with their direct counts (`/`-separated, no trailing
/// `/`), for the folder tree (REQ-008-02).
pub fn library_folders(conn: &Connection, library: i64) -> Result<Vec<(String, u64)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT rtrim(dir, '/'), count(*) FROM media WHERE root_id = ?1 AND dir <> '' GROUP BY dir ORDER BY dir",
    )?;
    let rows = stmt.query_map([library], |row| Ok((row.get(0)?, row.get::<_, i64>(1)?.max(0) as u64)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::libraries::{self, LibraryStatus};

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

        fn media(&self, rel_path: &str, rating: Option<i64>) -> i64 {
            self.conn
                .query_row(
                    "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime, rating)
                     VALUES (?1, ?2, ?2, 'image', 1, 0, ?3) RETURNING id",
                    (self.root, rel_path, rating),
                    |row| row.get(0),
                )
                .unwrap()
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

        fn link(&self, media: i64, tag: i64) {
            self.conn
                .execute(
                    "INSERT INTO media_tags (media_id, tag_id) VALUES (?1, ?2)",
                    (media, tag),
                )
                .unwrap();
        }
    }

    fn count(id: i64, name: &str, count: u64) -> TagCount {
        TagCount {
            id,
            name: name.into(),
            count,
        }
    }

    #[test]
    fn req_007_04_matches_normalized_prefixes_ordered_by_usage() {
        let index = Index::new();
        let (berlin, bern, paris) = (
            index.tag("Berlin", None),
            index.tag("Bern", None),
            index.tag("Paris", None),
        );
        let files: Vec<i64> = (1..=3).map(|i| index.media(&format!("{i}.jpg"), None)).collect();
        for &file in &files {
            index.link(file, bern);
        }
        index.link(files[0], berlin);
        index.link(files[0], paris);

        assert_eq!(
            autocomplete(&index.conn, " BER", 20).unwrap(),
            [count(bern, "Bern", 3), count(berlin, "Berlin", 1)]
        );
        assert_eq!(autocomplete(&index.conn, "", 1).unwrap(), [count(bern, "Bern", 3)]);
    }

    #[test]
    fn req_002_14_req_007_04_counts_descendants_and_follows_deletions() {
        let index = Index::new();
        let places = index.tag("Places", None);
        let paris = index.tag("Paris", Some(places));
        let files: Vec<i64> = (1..=2).map(|i| index.media(&format!("{i}.jpg"), None)).collect();
        for &file in &files {
            index.link(file, paris);
        }

        assert_eq!(
            autocomplete(&index.conn, "pl", 20).unwrap(),
            [count(places, "Places", 2)]
        );
        index
            .conn
            .execute("DELETE FROM media WHERE id = ?1", [files[0]])
            .unwrap();
        assert_eq!(
            autocomplete(&index.conn, "", 20).unwrap(),
            [count(paris, "Paris", 1), count(places, "Places", 1)]
        );
        index.conn.execute("DELETE FROM media", []).unwrap();
        assert!(autocomplete(&index.conn, "", 20).unwrap().is_empty());
    }

    #[test]
    fn req_008_04_counts_media_per_rating_0_meaning_unrated() {
        let index = Index::new();
        for (path, rating) in [("a", Some(5)), ("b", Some(5)), ("c", Some(2)), ("d", None)] {
            index.media(path, rating);
        }
        assert_eq!(rating_counts(&index.conn).unwrap(), [(5, 2), (2, 1), (0, 1)]);
    }

    #[test]
    fn req_008_03_groups_by_the_top_level_tag_whatever_its_exact_name_with_counts_including_descendants() {
        let index = Index::new();
        let people = index.tag("People", None);
        let anton = index.tag("Anton", Some(people));
        let persons = index.tag("Persons", None);
        let bea = index.tag("Bea", Some(persons));
        let cat = index.tag("Cat", None);
        let files: Vec<i64> = (1..=2).map(|i| index.media(&format!("{i}.jpg"), None)).collect();
        index.link(files[0], anton);
        index.link(files[1], bea);
        index.link(files[0], cat);

        let tag = |id, name: &str| CategoryTag {
            id,
            name: name.into(),
            parent_id: None,
            count: 1,
        };
        assert_eq!(
            category(&index.conn, TagCategory::People).unwrap(),
            [tag(anton, "Anton"), tag(bea, "Bea")]
        );
        assert!(category(&index.conn, TagCategory::Places).unwrap().is_empty());
    }

    #[test]
    fn req_008_03_keeps_the_hierarchy_below_the_root() {
        let index = Index::new();
        let places = index.tag("Locations", None);
        let france = index.tag("France", Some(places));
        let paris = index.tag("Paris", Some(france));
        let file = index.media("1.jpg", None);
        index.link(file, paris);

        let listed = category(&index.conn, TagCategory::Places).unwrap();
        let shape: Vec<(&str, Option<i64>, u64)> =
            listed.iter().map(|t| (t.name.as_str(), t.parent_id, t.count)).collect();
        assert_eq!(shape, [("France", None, 1), ("Paris", Some(france), 1)]);
    }

    #[test]
    fn req_008_05_excludes_category_tags_and_their_roots() {
        let index = Index::new();
        let people = index.tag("People", None);
        let anton = index.tag("Anton", Some(people));
        let cat = index.tag("Cat", None);
        let files: Vec<i64> = (1..=2).map(|i| index.media(&format!("{i}.jpg"), None)).collect();
        index.link(files[0], anton);
        index.link(files[1], cat);

        assert_eq!(popular(&index.conn, 20).unwrap(), [count(cat, "Cat", 1)]);
    }

    #[test]
    fn req_008_02_lists_folders_with_direct_media_counts() {
        let index = Index::new();
        for path in ["top.jpg", "a/1.jpg", "a/2.jpg", "a/b/3.jpg"] {
            index.media(path, None);
        }
        assert_eq!(
            library_folders(&index.conn, index.root).unwrap(),
            [("a".to_string(), 2), ("a/b".to_string(), 1)]
        );
    }
}
