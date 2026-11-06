//! Renaming files in the index (spec 035): what each media item contributes to its new name, and moving rows to their
//! new names in place, so a media item keeps its id and all that hangs on it (REQ-035-07).

use crate::Result;
use domain::edit::backup_rel_path;
use domain::rename::Item;
use domain::tags::TagCategory;
use rusqlite::{Connection, params};

/// A media item to rename, with what its thumbnails are named by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameItem {
    pub root_id: i64,
    pub size: i64,
    pub item: Item,
}

/// The media items `ids` in that order, with their tags outside People and Places (REQ-035-03); missing ids are left
/// out.
pub fn items(conn: &Connection, ids: &[i64]) -> Result<Vec<RenameItem>> {
    let mut found = Vec::new();
    let mut media = conn.prepare_cached("SELECT root_id, rel_path, size, mtime, taken_at FROM media WHERE id = ?1")?;
    let mut tags = conn.prepare_cached(
        "SELECT tags.id, tags.name FROM media_tags JOIN tags ON tags.id = media_tags.tag_id
         WHERE media_tags.media_id = ?1 ORDER BY media_tags.rowid",
    )?;
    for &id in ids {
        let row = media.query_row([id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        });
        let (root_id, rel_path, size, mtime, taken_at) = match row {
            Ok(row) => row,
            Err(rusqlite::Error::QueryReturnedNoRows) => continue,
            Err(error) => return Err(error.into()),
        };
        let linked: Vec<(i64, String)> = tags
            .query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let ids: Vec<i64> = linked.iter().map(|(id, _)| *id).collect();
        let categories = crate::gallery::categories_of(conn, &ids)?;
        let tags = linked
            .into_iter()
            .filter(|(id, _)| !matches!(categories.get(id), Some(TagCategory::People | TagCategory::Places)))
            .map(|(_, name)| name)
            .collect();
        found.push(RenameItem {
            root_id,
            size,
            item: Item {
                id,
                rel_path,
                taken_at,
                mtime,
                tags,
            },
        });
    }
    Ok(found)
}

/// Moves the rows of `renames` (old and new paths in library `root_id`) to their new paths in one transaction, and
/// edits whose original was one of them to its new path (REQ-035-07). Swaps work: every row leaves its old path first.
pub fn rename(conn: &Connection, root_id: i64, renames: &[(String, String)]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        // Out of the way first, under a path no file can have, so a swap doesn't meet the unique index.
        let mut away = tx.prepare_cached("UPDATE media SET rel_path = ?3 WHERE root_id = ?1 AND rel_path = ?2")?;
        for (i, (old, _)) in renames.iter().enumerate() {
            away.execute(params![root_id, old, format!("\u{0}rename/{i}")])?;
        }
        let mut place =
            tx.prepare_cached("UPDATE media SET rel_path = ?3, file_name = ?4 WHERE root_id = ?1 AND rel_path = ?2")?;
        let mut sources = tx.prepare_cached(
            "UPDATE media_edits SET source = ?3
             WHERE source = ?2 AND media_id IN (SELECT id FROM media WHERE root_id = ?1)",
        )?;
        for (i, (_, new)) in renames.iter().enumerate() {
            let name = new.rsplit('/').next().unwrap_or(new);
            place.execute(params![root_id, format!("\u{0}rename/{i}"), new, name])?;
        }
        // Sources go through placeholders too: `a → b, b → a` would otherwise send both to one.
        for (i, (old, _)) in renames.iter().enumerate() {
            sources.execute(params![root_id, old, format!("\u{0}source/{i}")])?;
            sources.execute(params![root_id, backup_rel_path(old), format!("\u{0}backup/{i}")])?;
        }
        for (i, (_, new)) in renames.iter().enumerate() {
            sources.execute(params![root_id, format!("\u{0}source/{i}"), new])?;
            sources.execute(params![root_id, format!("\u{0}backup/{i}"), backup_rel_path(new)])?;
        }
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::libraries::{LibraryStatus, insert as insert_library};
    use crate::writer::{IndexWriter, IndexedFile};
    use domain::media::{MediaKind, MediaMetadata};

    fn file(rel_path: &str, tags: &[&[&str]]) -> IndexedFile {
        IndexedFile {
            rel_path: rel_path.into(),
            file_name: rel_path.rsplit('/').next().unwrap().into(),
            kind: MediaKind::Image,
            size: 7,
            mtime: 1_000,
            sidecar_mtime: None,
            metadata: MediaMetadata {
                taken_at: Some(5_000),
                tags: tags.iter().map(|p| p.iter().map(|s| s.to_string()).collect()).collect(),
                ..Default::default()
            },
            read_error: None,
        }
    }

    fn index() -> (Connection, i64) {
        let conn = crate::open_in_memory().unwrap();
        let root = insert_library(&conn, "D:\\Photos", "Photos", LibraryStatus::Online)
            .unwrap()
            .id;
        let mut writer = IndexWriter::new(conn);
        writer
            .write(
                root,
                &[
                    file(
                        "trip/a.jpg",
                        &[&["Beach"], &["People", "Anna"], &["Places", "Rome"], &["Summer"]],
                    ),
                    file("trip/b.jpg", &[]),
                    file("c.jpg", &[]),
                ],
            )
            .unwrap();
        (writer.into(), root)
    }

    fn id(conn: &Connection, rel_path: &str) -> Option<i64> {
        crate::media::id_of(conn, 1, rel_path).unwrap()
    }

    #[test]
    fn req_035_03_reads_items_with_their_tags_outside_people_and_places() {
        let (conn, root) = index();
        let (a, b) = (id(&conn, "trip/a.jpg").unwrap(), id(&conn, "trip/b.jpg").unwrap());
        let items = items(&conn, &[b, a, 999]).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].item.rel_path, "trip/b.jpg");
        assert_eq!((items[1].root_id, items[1].size), (root, 7));
        let mut tags = items[1].item.tags.clone();
        tags.sort();
        assert_eq!(tags, ["Beach", "Summer"]);
        assert_eq!(items[1].item.taken_at, Some(5_000));
    }

    #[test]
    fn req_035_07_renames_rows_in_place_also_swapped_and_moves_edits_along() {
        let (conn, root) = index();
        let (a, b) = (id(&conn, "trip/a.jpg").unwrap(), id(&conn, "trip/b.jpg").unwrap());
        // b is an edited copy of a, and c was edited over its original.
        let c = id(&conn, "c.jpg").unwrap();
        conn.execute(
            "INSERT INTO media_edits (media_id, recipe, source) VALUES (?1, '{}', 'trip/a.jpg')",
            [b],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO media_edits (media_id, recipe, source) VALUES (?1, '{}', ?2)",
            params![c, backup_rel_path("c.jpg")],
        )
        .unwrap();

        rename(
            &conn,
            root,
            &[
                ("trip/a.jpg".into(), "trip/b.jpg".into()),
                ("trip/b.jpg".into(), "trip/a.jpg".into()),
                ("c.jpg".into(), "C2.jpg".into()),
            ],
        )
        .unwrap();
        assert_eq!((id(&conn, "trip/b.jpg"), id(&conn, "trip/a.jpg")), (Some(a), Some(b)));
        assert_eq!(id(&conn, "C2.jpg"), Some(c));
        let name: String = conn
            .query_row("SELECT file_name FROM media WHERE id = ?1", [c], |row| row.get(0))
            .unwrap();
        assert_eq!(name, "C2.jpg");
        let source = |media: i64| -> String {
            conn.query_row("SELECT source FROM media_edits WHERE media_id = ?1", [media], |row| {
                row.get(0)
            })
            .unwrap()
        };
        assert_eq!(source(b), "trip/b.jpg", "the copy's original was renamed");
        assert_eq!(source(c), backup_rel_path("C2.jpg"));
        // Search by the new name finds it.
        let found = crate::gallery::count(
            &conn,
            &domain::search::GallerySearch {
                name: Some("c2".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(found, 1);
    }
}
