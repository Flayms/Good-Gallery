//! Thumbnail state of media items (`thumbhash`, `thumb_status`), as the thumbnail service reads and writes it.

use crate::Result;
use domain::media::MediaKind;
use domain::thumbs::ThumbKey;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

/// Everything needed to render a media item's thumbnails and to store them under the right cache key.
#[derive(Debug, Clone, PartialEq)]
pub struct ThumbSource {
    pub id: i64,
    pub key: ThumbKey,
    pub root_path: String,
    pub kind: MediaKind,
    /// Seconds.
    pub duration: Option<f64>,
}

impl ThumbSource {
    /// The media file: the library folder joined with the `/`-separated relative path; `None` if that would leave
    /// the library (REQ-011-08).
    pub fn file(&self) -> Option<std::path::PathBuf> {
        crate::media::absolute_path(&self.root_path, &self.key.rel_path)
    }
}

pub fn thumb_source(conn: &Connection, id: i64) -> Result<Option<ThumbSource>> {
    let source = conn
        .prepare_cached(
            "SELECT m.id, m.root_id, r.path, m.rel_path, m.kind, m.size, m.mtime, m.duration
             FROM media m JOIN library_roots r ON r.id = m.root_id WHERE m.id = ?1",
        )?
        .query_row([id], |row| {
            Ok(ThumbSource {
                id: row.get(0)?,
                key: ThumbKey {
                    root_id: row.get(1)?,
                    rel_path: row.get(3)?,
                    size: row.get(5)?,
                    mtime: row.get(6)?,
                },
                root_path: row.get(2)?,
                kind: MediaKind::parse(&row.get::<_, String>(4)?).unwrap_or(MediaKind::Image),
                duration: row.get(7)?,
            })
        })
        .optional()?;
    Ok(source)
}

/// The newest media items (by date taken, else mtime) still waiting for thumbnails, skipping offline libraries and
/// `exclude` (REQ-005-08).
pub fn pending_thumbs(conn: &Connection, limit: usize, exclude: &[i64]) -> Result<Vec<i64>> {
    let not_in = if exclude.is_empty() {
        String::new()
    } else {
        format!("AND m.id NOT IN ({})", vec!["?"; exclude.len()].join(","))
    };
    let sql = format!(
        "SELECT m.id FROM media m JOIN library_roots r ON r.id = m.root_id
         WHERE m.thumb_status = 'pending' AND r.status <> 'offline' {not_in}
         ORDER BY m.sort_date DESC LIMIT {limit}"
    );
    // Not cached: the statement changes with the number of excluded items and would push the others out.
    let mut stmt = conn.prepare(&sql)?;
    let ids = stmt.query_map(params_from_iter(exclude), |row| row.get(0))?;
    Ok(ids.collect::<rusqlite::Result<_>>()?)
}

/// Stores the outcome (`None` marks a failure, REQ-005-09) unless the file changed while it was rendered.
pub fn set_thumbnail(conn: &Connection, source: &ThumbSource, thumbhash: Option<&str>) -> Result<()> {
    conn.prepare_cached(
        "UPDATE media SET thumbhash = ?1, thumb_status = ?2 WHERE id = ?3 AND size = ?4 AND mtime = ?5",
    )?
    .execute(params![
        thumbhash,
        if thumbhash.is_some() { "ready" } else { "error" },
        source.id,
        source.key.size,
        source.key.mtime
    ])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::libraries::{self, LibraryStatus};

    fn insert(conn: &Connection, root: i64, rel_path: &str, taken_at: i64) -> i64 {
        conn.query_row(
            "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime, taken_at, duration)
             VALUES (?1, ?2, ?2, 'video', 7, 1000, ?3, 2.5) RETURNING id",
            params![root, rel_path, taken_at],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn status(conn: &Connection, id: i64) -> (String, Option<String>) {
        conn.query_row("SELECT thumb_status, thumbhash FROM media WHERE id = ?1", [id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap()
    }

    #[test]
    fn reads_the_source_of_a_media_item() {
        let conn = crate::open_in_memory().unwrap();
        let root = libraries::insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Online)
            .unwrap()
            .id;
        let id = insert(&conn, root, "trip/a.mp4", 1);

        let source = thumb_source(&conn, id).unwrap().unwrap();

        assert_eq!(
            source,
            ThumbSource {
                id,
                key: ThumbKey {
                    root_id: root,
                    rel_path: "trip/a.mp4".into(),
                    size: 7,
                    mtime: 1000
                },
                root_path: "D:\\Photos".into(),
                kind: MediaKind::Video,
                duration: Some(2.5),
            }
        );
        assert_eq!(source.file().unwrap(), std::path::Path::new("D:\\Photos\\trip\\a.mp4"));
        assert_eq!(thumb_source(&conn, id + 1).unwrap(), None);
    }

    #[test]
    fn req_005_08_lists_pending_media_newest_first_without_offline_libraries() {
        let conn = crate::open_in_memory().unwrap();
        let online = libraries::insert(&conn, "D:\\A", "A", LibraryStatus::Online)
            .unwrap()
            .id;
        let offline = libraries::insert(&conn, "D:\\B", "B", LibraryStatus::Offline)
            .unwrap()
            .id;
        let old = insert(&conn, online, "old.jpg", 1);
        let new = insert(&conn, online, "new.jpg", 3);
        let middle = insert(&conn, online, "middle.jpg", 2);
        let done = insert(&conn, online, "done.jpg", 4);
        insert(&conn, offline, "away.jpg", 5);
        conn.execute("UPDATE media SET thumb_status = 'ready' WHERE id = ?1", [done])
            .unwrap();

        assert_eq!(pending_thumbs(&conn, 10, &[]).unwrap(), [new, middle, old]);
        assert_eq!(pending_thumbs(&conn, 2, &[]).unwrap(), [new, middle]);
        assert_eq!(pending_thumbs(&conn, 10, &[new, old]).unwrap(), [middle]);
    }

    /// REQ-005-09, REQ-005-10: the outcome is stored, but not for a file that changed while it was rendered.
    #[test]
    fn stores_the_outcome_unless_the_file_changed() {
        let conn = crate::open_in_memory().unwrap();
        let root = libraries::insert(&conn, "D:\\A", "A", LibraryStatus::Online)
            .unwrap()
            .id;
        let (ok, broken, changed) = (
            insert(&conn, root, "a.jpg", 1),
            insert(&conn, root, "b.jpg", 1),
            insert(&conn, root, "c.jpg", 1),
        );
        let source = |id| thumb_source(&conn, id).unwrap().unwrap();
        let stale = source(changed);
        conn.execute("UPDATE media SET mtime = 2000 WHERE id = ?1", [changed])
            .unwrap();

        set_thumbnail(&conn, &source(ok), Some("aGFzaA==")).unwrap();
        set_thumbnail(&conn, &source(broken), None).unwrap();
        set_thumbnail(&conn, &stale, Some("aGFzaA==")).unwrap();

        assert_eq!(status(&conn, ok), ("ready".into(), Some("aGFzaA==".into())));
        assert_eq!(status(&conn, broken), ("error".into(), None));
        assert_eq!(status(&conn, changed), ("pending".into(), None));
    }
}
