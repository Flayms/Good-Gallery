//! All index writes of the indexer (the reference's `IndexWriter`). Owns the write connection; one per index.

use crate::Result;
use crate::libraries::{self, Library, LibraryStatus};
use domain::media::{METADATA_VERSION, MediaKind, MediaMetadata};
use domain::tags::{clean_tag, normalize_tag};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::collections::HashMap;

/// What the index knows about a file, to tell whether it changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingEntry {
    pub id: i64,
    pub size: i64,
    pub mtime: i64,
    pub sidecar_mtime: Option<i64>,
    pub meta_version: i64,
}

/// A read file, ready to be written.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexedFile {
    /// `/`-separated, relative to the library.
    pub rel_path: String,
    pub file_name: String,
    pub kind: MediaKind,
    pub size: i64,
    /// Milliseconds since the epoch, truncated.
    pub mtime: i64,
    pub sidecar_mtime: Option<i64>,
    pub metadata: MediaMetadata,
}

const CHUNK: usize = 500;

pub struct IndexWriter {
    conn: Connection,
    // Caches over `tags`; cleared whenever a write might have been rolled back or rows were deleted.
    tag_ids: HashMap<String, i64>,
    parents: HashMap<i64, Option<i64>>,
}

impl IndexWriter {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn,
            tag_ids: HashMap::new(),
            parents: HashMap::new(),
        }
    }

    /// For reads by the indexer and for tests.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn root(&self, id: i64) -> Result<Option<Library>> {
        libraries::get(&self.conn, id)
    }

    /// Sets the library's status, and its last indexed time if given.
    pub fn set_root_status(&self, id: i64, status: LibraryStatus, last_scan_at: Option<i64>) -> Result<()> {
        match last_scan_at {
            Some(at) => self.conn.execute(
                "UPDATE library_roots SET status = ?1, last_scan_at = ?2 WHERE id = ?3",
                params![status.as_str(), at, id],
            )?,
            None => self.conn.execute(
                "UPDATE library_roots SET status = ?1 WHERE id = ?2",
                params![status.as_str(), id],
            )?,
        };
        Ok(())
    }

    /// Indexed files of a library, keyed by relative path.
    pub fn existing(&self, root: i64) -> Result<HashMap<String, ExistingEntry>> {
        self.query_existing("root_id = ?1", params![root])
    }

    /// Indexed files directly in one folder of a library, keyed by relative path.
    pub fn existing_in(&self, root: i64, rel_dir: &str) -> Result<HashMap<String, ExistingEntry>> {
        let dir = if rel_dir.is_empty() {
            String::new()
        } else {
            format!("{rel_dir}/")
        };
        self.query_existing("root_id = ?1 AND dir = ?2", params![root, dir])
    }

    fn query_existing(&self, filter: &str, values: &[&dyn rusqlite::ToSql]) -> Result<HashMap<String, ExistingEntry>> {
        let sql = format!("SELECT id, rel_path, size, mtime, sidecar_mtime, meta_version FROM media WHERE {filter}");
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(values, |row| {
            Ok((
                row.get::<_, String>(1)?,
                ExistingEntry {
                    id: row.get(0)?,
                    size: row.get(2)?,
                    mtime: row.get(3)?,
                    sidecar_mtime: row.get(4)?,
                    meta_version: row.get(5)?,
                },
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Folder mtimes of a library as last listed, keyed by relative folder (`""` for the library itself).
    pub fn folders(&self, root: i64) -> Result<HashMap<String, i64>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT rel_dir, mtime FROM folders WHERE root_id = ?1")?;
        let rows = stmt.query_map([root], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Stores listed folders; `replace` drops all other folders of the library (after a full scan).
    pub fn save_folders(&mut self, root: i64, listed: &HashMap<String, i64>, replace: bool) -> Result<()> {
        let tx = self.conn.transaction()?;
        if replace {
            tx.execute("DELETE FROM folders WHERE root_id = ?1", [root])?;
        }
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO folders (root_id, rel_dir, mtime) VALUES (?1, ?2, ?3)
                 ON CONFLICT (root_id, rel_dir) DO UPDATE SET mtime = excluded.mtime",
            )?;
            for (rel_dir, mtime) in listed {
                stmt.execute(params![root, rel_dir, mtime])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Removes vanished folders with everything below them: subfolders and media items. `0` is the character after
    /// `/`, so `>= 'a/' AND < 'a0'` is everything below `a`.
    pub fn remove_folders(&mut self, root: i64, rel_dirs: &[String]) -> Result<()> {
        let tx = self.conn.transaction()?;
        for rel_dir in rel_dirs {
            if rel_dir.is_empty() {
                // The library folder itself: everything goes.
                tx.execute("DELETE FROM media WHERE root_id = ?1", [root])?;
                tx.execute("DELETE FROM folders WHERE root_id = ?1", [root])?;
                continue;
            }
            let (from, to) = (format!("{rel_dir}/"), format!("{rel_dir}0"));
            tx.execute(
                "DELETE FROM media WHERE root_id = ?1 AND dir >= ?2 AND dir < ?3",
                params![root, from, to],
            )?;
            tx.execute(
                "DELETE FROM folders WHERE root_id = ?1 AND (rel_dir = ?2 OR (rel_dir >= ?3 AND rel_dir < ?4))",
                params![root, rel_dir, from, to],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Upserts files and replaces their tags in one transaction.
    pub fn write(&mut self, root: i64, files: &[IndexedFile]) -> Result<()> {
        let result = (|| {
            let tx = self.conn.transaction()?;
            for file in files {
                write_file(&tx, &mut self.tag_ids, &mut self.parents, root, file)?;
            }
            tx.commit()
        })();
        if result.is_err() {
            self.clear_caches();
        }
        Ok(result?)
    }

    pub fn remove(&mut self, ids: &[i64]) -> Result<()> {
        let tx = self.conn.transaction()?;
        for chunk in ids.chunks(CHUNK) {
            let placeholders = vec!["?"; chunk.len()].join(",");
            tx.execute(
                &format!("DELETE FROM media WHERE id IN ({placeholders})"),
                rusqlite::params_from_iter(chunk),
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Deletes tags without media items or child tags, bottom-up.
    pub fn prune_tags(&mut self) -> Result<()> {
        self.clear_caches();
        loop {
            let deleted = self.conn.execute(
                "DELETE FROM tags WHERE NOT EXISTS (SELECT 1 FROM media_tags WHERE media_tags.tag_id = tags.id)
                 AND NOT EXISTS (SELECT 1 FROM tags AS child WHERE child.parent_id = tags.id)",
                [],
            )?;
            if deleted == 0 {
                return Ok(());
            }
        }
    }

    fn clear_caches(&mut self) {
        self.tag_ids.clear();
        self.parents.clear();
    }
}

fn write_file(
    tx: &Transaction,
    tag_ids: &mut HashMap<String, i64>,
    parents: &mut HashMap<i64, Option<i64>>,
    root: i64,
    file: &IndexedFile,
) -> rusqlite::Result<()> {
    let m = &file.metadata;
    // Thumbnails only go stale when the file itself changed, not its sidecar.
    let id: i64 = tx.prepare_cached(
        "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime, sidecar_mtime, width, height, duration,
                            taken_at, rating, meta_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT (root_id, rel_path) DO UPDATE SET
           file_name = excluded.file_name, kind = excluded.kind, size = excluded.size, mtime = excluded.mtime,
           sidecar_mtime = excluded.sidecar_mtime, width = excluded.width, height = excluded.height,
           duration = excluded.duration, taken_at = excluded.taken_at, rating = excluded.rating,
           meta_version = excluded.meta_version,
           thumbhash = CASE WHEN media.size <> excluded.size OR media.mtime <> excluded.mtime THEN NULL
                            ELSE media.thumbhash END,
           thumb_status = CASE WHEN media.size <> excluded.size OR media.mtime <> excluded.mtime THEN 'pending'
                               ELSE media.thumb_status END
         RETURNING id",
    )?
    .query_row(
        params![
            root,
            file.rel_path,
            file.file_name,
            file.kind.as_str(),
            file.size,
            file.mtime,
            file.sidecar_mtime,
            m.width,
            m.height,
            m.duration,
            m.taken_at,
            m.rating,
            METADATA_VERSION
        ],
        |row| row.get(0),
    )?;

    tx.prepare_cached("DELETE FROM media_tags WHERE media_id = ?1")?
        .execute([id])?;
    let mut linked = Vec::new();
    for path in &m.tags {
        if let Some(tag) = tag_path_id(tx, tag_ids, parents, path)?
            && !linked.contains(&tag)
        {
            linked.push(tag);
        }
    }
    let mut insert = tx.prepare_cached("INSERT INTO media_tags (media_id, tag_id) VALUES (?1, ?2)")?;
    for tag in linked {
        insert.execute([id, tag])?;
    }
    Ok(())
}

/// The id of a tag path's leaf, creating missing tags and parent links. `None` for an empty path.
fn tag_path_id(
    tx: &Transaction,
    tag_ids: &mut HashMap<String, i64>,
    parents: &mut HashMap<i64, Option<i64>>,
    path: &[String],
) -> rusqlite::Result<Option<i64>> {
    let mut leaf = None;
    for name in path {
        let tag = tag_id(tx, tag_ids, parents, name)?;
        if let Some(parent) = leaf {
            link_parent(tx, parents, tag, parent)?;
        }
        leaf = Some(tag);
    }
    Ok(leaf)
}

fn tag_id(
    tx: &Transaction,
    tag_ids: &mut HashMap<String, i64>,
    parents: &mut HashMap<i64, Option<i64>>,
    name: &str,
) -> rusqlite::Result<i64> {
    let norm = normalize_tag(name);
    if let Some(id) = tag_ids.get(&norm) {
        return Ok(*id);
    }
    tx.prepare_cached("INSERT INTO tags (name, name_norm) VALUES (?1, ?2) ON CONFLICT (name_norm) DO NOTHING")?
        .execute(params![clean_tag(name), norm])?;
    let (id, parent): (i64, Option<i64>) = tx
        .prepare_cached("SELECT id, parent_id FROM tags WHERE name_norm = ?1")?
        .query_row([&norm], |row| Ok((row.get(0)?, row.get(1)?)))?;
    tag_ids.insert(norm, id);
    parents.insert(id, parent);
    Ok(id)
}

/// Tag names are globally unique, so the first hierarchy seen for a tag wins. Links that would make a tag its own
/// ancestor (`A|B` and `B|A`) are refused.
fn link_parent(
    tx: &Transaction,
    parents: &mut HashMap<i64, Option<i64>>,
    id: i64,
    parent: i64,
) -> rusqlite::Result<()> {
    if parent_of(tx, parents, id)?.is_some() {
        return Ok(());
    }
    let mut ancestor = Some(parent);
    while let Some(current) = ancestor {
        if current == id {
            return Ok(());
        }
        ancestor = parent_of(tx, parents, current)?;
    }
    tx.prepare_cached("UPDATE tags SET parent_id = ?1 WHERE id = ?2")?
        .execute([parent, id])?;
    parents.insert(id, Some(parent));
    Ok(())
}

fn parent_of(tx: &Transaction, parents: &mut HashMap<i64, Option<i64>>, id: i64) -> rusqlite::Result<Option<i64>> {
    if let Some(parent) = parents.get(&id) {
        return Ok(*parent);
    }
    let parent = tx
        .prepare_cached("SELECT parent_id FROM tags WHERE id = ?1")?
        .query_row([id], |row| row.get::<_, Option<i64>>(0))
        .optional()?
        .flatten();
    parents.insert(id, parent);
    Ok(parent)
}

impl From<IndexWriter> for Connection {
    fn from(writer: IndexWriter) -> Self {
        writer.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writer() -> (IndexWriter, i64) {
        let conn = crate::open_in_memory().unwrap();
        let root = libraries::insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Unknown)
            .unwrap()
            .id;
        (IndexWriter::new(conn), root)
    }

    fn file(rel_path: &str, tags: &[&[&str]]) -> IndexedFile {
        IndexedFile {
            rel_path: rel_path.into(),
            file_name: rel_path.rsplit('/').next().unwrap().into(),
            kind: MediaKind::Image,
            size: 1,
            mtime: 1000,
            sidecar_mtime: None,
            metadata: MediaMetadata {
                tags: tags.iter().map(|p| p.iter().map(|s| s.to_string()).collect()).collect(),
                ..Default::default()
            },
        }
    }

    fn tags_of(writer: &IndexWriter, rel_path: &str) -> Vec<String> {
        let mut stmt = writer
            .conn()
            .prepare(
                "SELECT tags.name FROM media_tags JOIN tags ON tags.id = media_tags.tag_id
                 JOIN media ON media.id = media_tags.media_id WHERE media.rel_path = ?1 ORDER BY tags.name",
            )
            .unwrap();
        stmt.query_map([rel_path], |r| r.get(0))
            .unwrap()
            .map(|row| row.unwrap())
            .collect()
    }

    fn parents(writer: &IndexWriter) -> Vec<(String, Option<String>)> {
        let mut stmt = writer
            .conn()
            .prepare("SELECT t.name, p.name FROM tags t LEFT JOIN tags p ON p.id = t.parent_id ORDER BY t.name")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|row| row.unwrap())
            .collect()
    }

    #[test]
    fn req_004_07_req_004_08_stores_hierarchies_links_the_leaf_and_refuses_cycles() {
        let (mut writer, root) = writer();
        writer
            .write(root, &[file("a.jpg", &[&["Places", "Berlin"], &["Travel"]])])
            .unwrap();
        // A reversed hierarchy must not create a parent cycle.
        writer.write(root, &[file("b.jpg", &[&["berlin", "places"]])]).unwrap();

        assert_eq!(tags_of(&writer, "a.jpg"), ["Berlin", "Travel"]);
        assert_eq!(tags_of(&writer, "b.jpg"), ["Places"]);
        assert_eq!(
            parents(&writer),
            [
                ("Berlin".into(), Some("Places".into())),
                ("Places".into(), None),
                ("Travel".into(), None)
            ]
        );
    }

    #[test]
    fn req_004_08_the_first_parent_seen_wins() {
        let (mut writer, root) = writer();
        writer.write(root, &[file("a.jpg", &[&["Places", "Berlin"]])]).unwrap();
        writer.write(root, &[file("b.jpg", &[&["Cities", "Berlin"]])]).unwrap();

        assert!(parents(&writer).contains(&("Berlin".into(), Some("Places".into()))));
    }

    #[test]
    fn req_004_11_one_tag_per_normalized_name_with_the_first_spelling() {
        let (mut writer, root) = writer();
        writer
            .write(
                root,
                &[
                    file("a.jpg", &[&["Summer  Holidays"]]),
                    file("b.jpg", &[&["summer holidays "]]),
                ],
            )
            .unwrap();

        assert_eq!(tags_of(&writer, "b.jpg"), ["Summer Holidays"]);
    }

    #[test]
    fn req_005_10_rewriting_resets_the_thumbnail_only_when_the_content_changed() {
        let (mut writer, root) = writer();
        writer.write(root, &[file("a.jpg", &[]), file("b.jpg", &[])]).unwrap();
        writer
            .conn()
            .execute("UPDATE media SET thumb_status = 'ready', thumbhash = 'abc'", [])
            .unwrap();

        let mut changed = file("a.jpg", &[]);
        changed.size = 2;
        let mut sidecar_only = file("b.jpg", &[]);
        sidecar_only.sidecar_mtime = Some(5);
        writer.write(root, &[changed, sidecar_only]).unwrap();

        let mut stmt = writer
            .conn()
            .prepare("SELECT thumb_status, thumbhash FROM media ORDER BY rel_path")
            .unwrap();
        let rows: Vec<(String, Option<String>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();
        assert_eq!(rows, [("pending".into(), None), ("ready".into(), Some("abc".into()))]);
    }

    #[test]
    fn req_002_14_prunes_unused_tags_bottom_up() {
        let (mut writer, root) = writer();
        writer
            .write(
                root,
                &[file("a.jpg", &[&["Places", "Berlin"]]), file("b.jpg", &[&["Keep"]])],
            )
            .unwrap();
        let id = writer.existing(root).unwrap()["a.jpg"].id;

        writer.remove(&[id]).unwrap();
        writer.prune_tags().unwrap();

        assert_eq!(parents(&writer), [("Keep".into(), None)]);
    }

    #[test]
    fn req_002_04_removes_vanished_folders_with_everything_below() {
        let (mut writer, root) = writer();
        writer
            .write(
                root,
                &[
                    file("a/x.jpg", &[]),
                    file("a/b/y.jpg", &[]),
                    file("ab/z.jpg", &[]),
                    file("w.jpg", &[]),
                ],
            )
            .unwrap();
        let listed: HashMap<String, i64> = [("", 1), ("a", 1), ("a/b", 1), ("ab", 1)]
            .map(|(d, m)| (d.to_string(), m))
            .into();
        writer.save_folders(root, &listed, true).unwrap();

        writer.remove_folders(root, &["a".to_string()]).unwrap();

        let mut paths: Vec<_> = writer.existing(root).unwrap().into_keys().collect();
        paths.sort();
        assert_eq!(paths, ["ab/z.jpg", "w.jpg"]);
        let mut folders: Vec<_> = writer.folders(root).unwrap().into_keys().collect();
        folders.sort();
        assert_eq!(folders, ["", "ab"]);
    }

    #[test]
    fn existing_in_lists_one_folder_only() {
        let (mut writer, root) = writer();
        writer
            .write(
                root,
                &[file("a/x.jpg", &[]), file("a/b/y.jpg", &[]), file("w.jpg", &[])],
            )
            .unwrap();

        assert_eq!(
            writer.existing_in(root, "a").unwrap().into_keys().collect::<Vec<_>>(),
            ["a/x.jpg"]
        );
        assert_eq!(
            writer.existing_in(root, "").unwrap().into_keys().collect::<Vec<_>>(),
            ["w.jpg"]
        );
    }

    #[test]
    fn status_updates_keep_the_last_indexed_time_unless_given() {
        let (writer, root) = writer();
        writer.set_root_status(root, LibraryStatus::Online, Some(5)).unwrap();
        writer.set_root_status(root, LibraryStatus::Offline, None).unwrap();

        let library = writer.root(root).unwrap().unwrap();
        assert_eq!(
            (library.status, library.last_scan_at),
            (LibraryStatus::Offline, Some(5))
        );
    }
}
