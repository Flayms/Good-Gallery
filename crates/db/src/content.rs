//! Content search's vectors (spec 036): one per media item and model, in the version of the file the model saw.

use crate::Result;
use domain::media::MediaKind;
use domain::thumbs::ThumbKey;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::PathBuf;

/// The chosen model's id (`domain::content::CATALOG`), unset while content search is off (REQ-036-03).
pub const MODEL_KEY: &str = "content.model";
/// `"paused"` while the user paused the pass (REQ-036-04).
pub const PASS_KEY: &str = "content.pass";

/// A photo or video waiting for a model.
#[derive(Debug, Clone, PartialEq)]
pub struct ContentItem {
    pub media: i64,
    pub kind: MediaKind,
    pub root_path: String,
    pub rel_path: String,
    pub size: i64,
    pub mtime: i64,
    pub sidecar_orientation: Option<u8>,
    pub root_id: i64,
    /// Whether its thumbnails were rendered (they may have been evicted since).
    pub has_thumbnail: bool,
    /// Its library is offline.
    pub offline: bool,
}

impl ContentItem {
    /// The file; `None` if its path would leave the library (REQ-011-08).
    pub fn file(&self) -> Option<PathBuf> {
        crate::media::absolute_path(&self.root_path, &self.rel_path)
    }

    /// The key of its cached thumbnails.
    pub fn thumb_key(&self) -> ThumbKey {
        ThumbKey {
            root_id: self.root_id,
            rel_path: self.rel_path.clone(),
            size: self.size,
            mtime: self.mtime,
        }
    }
}

/// Photos and videos `model` hasn't looked at in their current version, newest first, past the first `skip` of them
/// (those the pass couldn't read this time). In an offline library only those with a thumbnail count (REQ-036-04).
pub fn pending(conn: &Connection, model: &str, limit: usize, skip: usize) -> Result<Vec<ContentItem>> {
    let items = conn
        .prepare_cached(
            "SELECT m.id, m.kind, r.path, m.rel_path, m.size, m.mtime, m.sidecar_orientation, m.root_id,
                    m.thumb_status = 'done', r.status = 'offline'
             FROM media m JOIN library_roots r ON r.id = m.root_id
             LEFT JOIN content_vectors v ON v.media_id = m.id AND v.model = ?1
             WHERE (v.media_id IS NULL OR v.size <> m.size OR v.mtime <> m.mtime)
               AND (r.status <> 'offline' OR m.thumb_status = 'done')
             ORDER BY m.sort_date DESC LIMIT ?2 OFFSET ?3",
        )?
        .query_map(params![model, limit as i64, skip as i64], |row| {
            Ok(ContentItem {
                media: row.get(0)?,
                kind: MediaKind::parse(&row.get::<_, String>(1)?).unwrap_or(MediaKind::Image),
                root_path: row.get(2)?,
                rel_path: row.get(3)?,
                size: row.get(4)?,
                mtime: row.get(5)?,
                sidecar_orientation: row.get(6)?,
                root_id: row.get(7)?,
                has_thumbnail: row.get(8)?,
                offline: row.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(items)
}

/// Stores what `model` made of `item`; an empty vector marks a file that couldn't be read, so it isn't tried again
/// until it changes (REQ-036-04).
pub fn store(conn: &Connection, item: &ContentItem, model: &str, vector: &[i8]) -> Result<()> {
    let bytes: Vec<u8> = vector.iter().map(|value| value.to_ne_bytes()[0]).collect();
    conn.prepare_cached(
        "INSERT OR REPLACE INTO content_vectors (media_id, model, size, mtime, vector) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?
    .execute(params![item.media, model, item.size, item.mtime, bytes])?;
    Ok(())
}

/// How far `model` has come: media items looked at in their current version, and all of them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContentProgress {
    pub done: u64,
    pub total: u64,
}

pub fn progress(conn: &Connection, model: &str) -> Result<ContentProgress> {
    let total: i64 = conn
        .prepare_cached("SELECT count(*) FROM media")?
        .query_row([], |row| row.get(0))?;
    let done: i64 = conn
        .prepare_cached(
            "SELECT count(*) FROM content_vectors v JOIN media m ON m.id = v.media_id
             WHERE v.model = ?1 AND v.size = m.size AND v.mtime = m.mtime",
        )?
        .query_row([model], |row| row.get(0))?;
    Ok(ContentProgress {
        done: done as u64,
        total: total as u64,
    })
}

/// Calls `each` with every readable vector of `model`, in blocks so a large library isn't held in memory at once
/// (REQ-036-06).
pub fn each_vector(conn: &Connection, model: &str, mut each: impl FnMut(i64, &[i8])) -> Result<()> {
    const BLOCK: i64 = 10_000;
    let mut after = i64::MIN;
    let mut stmt = conn.prepare_cached(
        "SELECT media_id, vector FROM content_vectors
         WHERE model = ?1 AND media_id > ?2 AND length(vector) > 0 ORDER BY media_id LIMIT ?3",
    )?;
    loop {
        let mut rows = stmt.query(params![model, after, BLOCK])?;
        let mut read = 0;
        while let Some(row) = rows.next()? {
            let media: i64 = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            let vector: Vec<i8> = blob.into_iter().map(|byte| i8::from_ne_bytes([byte])).collect();
            each(media, &vector);
            after = media;
            read += 1;
        }
        if read < BLOCK {
            return Ok(());
        }
    }
}

/// How many vectors `model` has, readable or not: a cheap sign that matches may have changed.
pub fn count(conn: &Connection, model: &str) -> Result<u64> {
    let count: i64 = conn
        .prepare_cached("SELECT count(*) FROM content_vectors WHERE model = ?1")?
        .query_row([model], |row| row.get(0))?;
    Ok(count as u64)
}

/// Removes everything content search stored (REQ-036-03).
pub fn delete_all(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM content_vectors", [])?;
    Ok(())
}

/// The stored setting, if set.
pub fn chosen(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(crate::settings::value(conn, key)?.and_then(|value| value.as_str().map(str::to_owned)))
}

/// Whether `media` has a vector of `model` (for the details).
pub fn has_vector(conn: &Connection, media: i64, model: &str) -> Result<bool> {
    Ok(conn
        .prepare_cached("SELECT 1 FROM content_vectors WHERE media_id = ?1 AND model = ?2")?
        .query_row(params![media, model], |_| Ok(()))
        .optional()?
        .is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::libraries::{self, LibraryStatus};

    const ROOT: &str = if cfg!(windows) { "D:\\Photos" } else { "/media/Photos" };

    fn setup() -> (Connection, i64) {
        let conn = crate::open_in_memory().unwrap();
        let root = libraries::insert(&conn, ROOT, "Photos", LibraryStatus::Online)
            .unwrap()
            .id;
        (conn, root)
    }

    fn insert(conn: &Connection, root: i64, rel_path: &str, kind: &str, sort_date: i64) -> i64 {
        conn.query_row(
            "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime, taken_at)
             VALUES (?1, ?2, ?2, ?3, 7, 1000, ?4) RETURNING id",
            params![root, rel_path, kind, sort_date],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn ids(items: &[ContentItem]) -> Vec<i64> {
        items.iter().map(|item| item.media).collect()
    }

    /// REQ-036-04: photos and videos wait newest first, once looked at they don't, a changed file waits again, and a
    /// file that couldn't be read doesn't either.
    #[test]
    fn req_036_04_photos_and_videos_wait_until_looked_at_in_their_version() {
        let (conn, root) = setup();
        let old = insert(&conn, root, "old.jpg", "image", 1);
        let video = insert(&conn, root, "clip.mp4", "video", 2);
        let new = insert(&conn, root, "new.jpg", "image", 3);
        let waiting = pending(&conn, "m", 10, 0).unwrap();
        assert_eq!(ids(&waiting), [new, video, old]);
        assert_eq!(
            ids(&pending(&conn, "m", 10, 1).unwrap()),
            [video, old],
            "past the skipped"
        );

        store(&conn, &waiting[0], "m", &[1, -2, 3]).unwrap();
        store(&conn, &waiting[1], "m", &[]).unwrap();
        assert_eq!(ids(&pending(&conn, "m", 10, 0).unwrap()), [old]);
        assert_eq!(
            ids(&pending(&conn, "other", 10, 0).unwrap()),
            [new, video, old],
            "per model"
        );
        assert_eq!(progress(&conn, "m").unwrap(), ContentProgress { done: 2, total: 3 });

        conn.execute("UPDATE media SET mtime = 2000 WHERE id = ?1", [new])
            .unwrap();
        assert_eq!(ids(&pending(&conn, "m", 10, 0).unwrap()), [new, old], "changed");
        assert_eq!(progress(&conn, "m").unwrap().done, 1);
    }

    /// REQ-036-04: in an offline library only what has a thumbnail waits.
    #[test]
    fn req_036_04_an_offline_library_waits_only_with_thumbnails() {
        let (conn, root) = setup();
        let with = insert(&conn, root, "with.jpg", "image", 1);
        insert(&conn, root, "without.jpg", "image", 2);
        conn.execute("UPDATE media SET thumb_status = 'done' WHERE id = ?1", [with])
            .unwrap();
        conn.execute("UPDATE library_roots SET status = 'offline'", []).unwrap();
        let waiting = pending(&conn, "m", 10, 0).unwrap();
        assert_eq!(ids(&waiting), [with]);
        assert!(waiting[0].offline && waiting[0].has_thumbnail);
    }

    /// REQ-036-06, REQ-036-08: vectors come back per model, without unreadable ones, across blocks; deleting removes
    /// all of them.
    #[test]
    fn req_036_06_req_036_08_reads_back_each_models_vectors() {
        let (conn, root) = setup();
        let a = insert(&conn, root, "a.jpg", "image", 1);
        let b = insert(&conn, root, "b.jpg", "image", 2);
        let items = pending(&conn, "m", 10, 0).unwrap();
        store(&conn, &items[0], "m", &[-127, 0, 127]).unwrap();
        store(&conn, &items[1], "m", &[]).unwrap();
        store(&conn, &items[1], "other", &[5, 5, 5]).unwrap();
        let mut seen = Vec::new();
        each_vector(&conn, "m", |media, vector| seen.push((media, vector.to_vec()))).unwrap();
        assert_eq!(seen, [(b, vec![-127, 0, 127])]);
        assert_eq!(count(&conn, "m").unwrap(), 2);
        assert!(has_vector(&conn, a, "m").unwrap() && !has_vector(&conn, a, "nope").unwrap());

        delete_all(&conn).unwrap();
        assert_eq!(count(&conn, "m").unwrap() + count(&conn, "other").unwrap(), 0);
        assert_eq!(pending(&conn, "m", 10, 0).unwrap().len(), 2);
    }
}
