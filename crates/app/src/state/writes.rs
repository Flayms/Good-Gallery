//! Ratings and tags written into files, one at a time or for many items at once, and deleting (specs 024–026).

use super::AppState;
#[cfg(not(target_arch = "wasm32"))]
use super::{Event, faces, forget_index_entry, ids_roots, lock};
use crate::format;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

/// A rating the user set on a media item (spec 024).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rate {
    pub media: i64,
    pub root_id: i64,
    pub rel_path: String,
    pub kind: domain::media::MediaKind,
    /// `None` removes the rating.
    pub rating: Option<i64>,
}

impl Rate {
    /// What pressing `stars` (a key or a star) does to `item`'s rating (REQ-024-01, REQ-024-02).
    pub fn toggle(item: &db::gallery::GalleryItem, stars: i64) -> Self {
        Self {
            media: item.id,
            root_id: item.root_id,
            rel_path: item.rel_path.clone(),
            kind: item.kind,
            rating: domain::rating::toggled(item.rating, stars),
        }
    }

    /// As [`Self::toggle`], for the item of an info view.
    pub fn toggle_detail(detail: &db::media::MediaDetail, stars: i64) -> Self {
        Self {
            media: detail.id,
            root_id: detail.root_id,
            rel_path: detail.rel_path.clone(),
            kind: detail.kind,
            rating: domain::rating::toggled(detail.rating, stars),
        }
    }
}

/// A change to several items at once: the selection's (REQ-025-07, REQ-026-06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeMany {
    pub ids: Vec<i64>,
    pub what: Many,
}

/// What a [`ChangeMany`] does to each item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Many {
    /// Set the rating; `None` removes it (REQ-025-07).
    Rate(Option<i64>),
    /// Add the tag with this path (REQ-026-06).
    Tag(Vec<String>),
    /// Turn photos a quarter by their orientation tag (REQ-009-22).
    Turn(domain::orientation::Turn),
    /// Delete the files: to the Recycle Bin, or `permanently` when one is on a network drive (REQ-009-23).
    Delete { permanently: bool },
}

impl Many {
    /// The dialog asking before changing `count` (formatted) items: its title, its text and its button.
    pub fn question(&self, count: &str) -> (String, String, &'static str) {
        let changed = || "Their files are changed.".to_owned();
        match self {
            Self::Rate(Some(1)) => (format!("Rate {count} items with 1 star?"), changed(), "Rate"),
            Self::Rate(Some(stars)) => (format!("Rate {count} items with {stars} stars?"), changed(), "Rate"),
            Self::Rate(None) => (
                format!("Remove the rating of {count} items?"),
                changed(),
                "Remove rating",
            ),
            Self::Tag(path) => (format!("Tag {count} items with {}?", tag_name(path)), changed(), "Tag"),
            Self::Turn(domain::orientation::Turn::Left) => {
                (format!("Rotate {count} photos left?"), changed(), "Rotate")
            }
            Self::Turn(domain::orientation::Turn::Right) => {
                (format!("Rotate {count} photos right?"), changed(), "Rotate")
            }
            Self::Delete { permanently } => domain::deletion::question(count, *permanently),
        }
    }
}

fn tag_name(path: &[String]) -> &str {
    path.last().map_or("", String::as_str)
}

/// Changing more items than this asks first (REQ-025-07, REQ-026-06).
pub const CONFIRM_MANY_ABOVE: usize = 100;

/// What changing several items did (REQ-025-07, REQ-026-06).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Debug)]
pub(super) struct ChangedMany {
    what: Many,
    /// The files changed, by library: read again on the next changes scan.
    changed: Vec<(i64, String)>,
    /// The media items changed.
    media: Vec<i64>,
    failed: usize,
    /// The first file that couldn't be changed, and why.
    first_failure: Option<(String, String)>,
}

/// A tag added to or removed from a media item by hand (spec 026).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagChange {
    pub media: i64,
    pub root_id: i64,
    pub rel_path: String,
    pub kind: domain::media::MediaKind,
    pub change: domain::tagging::Change,
}

impl TagChange {
    pub fn of(detail: &db::media::MediaDetail, change: domain::tagging::Change) -> Self {
        Self {
            media: detail.id,
            root_id: detail.root_id,
            rel_path: detail.rel_path.clone(),
            kind: detail.kind,
            change,
        }
    }
}

impl AppState {
    /// Writes a rating into the file in the background, then into the index (REQ-024-03); a file that can't be
    /// written keeps its rating, and the user is told why (REQ-024-04).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn rate(&mut self, rate: Rate) {
        let (Some(exiftool), Some(library)) = (
            self.edit_tools.as_ref().map(|tools| tools.exiftool.clone()),
            self.libraries.iter().find(|library| library.id == rate.root_id),
        ) else {
            self.toasts
                .error("Couldn't save the rating: exiftool, which writes it, wasn't found.");
            return;
        };
        let folder = PathBuf::from(&library.path);
        let db = self.db.clone();
        self.spawn_write(move || {
            let written =
                media::rating::write(&exiftool, &folder, &rate.rel_path, rate.kind, rate.rating).map_err(|error| {
                    tracing::error!("can't rate {}: {error}", rate.rel_path);
                    error.to_string()
                });
            Event::Rated(written.map(|()| {
                if let Some(db) = &db
                    && let Err(error) = db::media::set_rating(&lock(db), rate.media, rate.rating)
                {
                    tracing::error!("can't store the rating of {}: {error}", rate.rel_path);
                }
                (rate.root_id, rate.rel_path)
            }))
        });
    }

    /// The browser version never writes files (REQ-024-05).
    #[cfg(target_arch = "wasm32")]
    pub fn rate(&mut self, _rate: Rate) {}

    /// Rates or tags several items, after the user confirmed if there are many (REQ-025-07, REQ-026-06).
    pub fn ask_change_many(&mut self, change: ChangeMany) {
        // Deleting always asks (REQ-009-23).
        if change.ids.len() > CONFIRM_MANY_ABOVE || matches!(change.what, Many::Delete { .. }) {
            self.confirm_many = Some(change);
        } else {
            self.change_many(change);
        }
    }

    /// Asks before deleting media items `ids` (REQ-009-23): permanently if one of their libraries is on a network drive.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn ask_delete(&mut self, ids: Vec<i64>) {
        let Some(db) = self.db.clone() else { return };
        if ids.is_empty() {
            return;
        }
        let roots: std::collections::BTreeSet<i64> = ids_roots(&lock(&db), &ids);
        let permanently = self
            .libraries
            .iter()
            // Without its libraries known, any of them counts.
            .filter(|library| roots.is_empty() || roots.contains(&library.id))
            .any(|library| media::delete::deletes_permanently(std::path::Path::new(&library.path)));
        self.ask_change_many(ChangeMany {
            ids,
            what: Many::Delete { permanently },
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub fn ask_delete(&mut self, _ids: Vec<i64>) {}

    /// Writes a rating or a tag into the files of several items in the background, one after another, then into
    /// the index; files that can't be written are counted and the first one named (REQ-025-07, REQ-026-06).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn change_many(&mut self, change: ChangeMany) {
        let Some(db) = self.db.clone() else { return };
        // Deleting needs no exiftool (REQ-009-23).
        let exiftool = self.edit_tools.as_ref().map(|tools| tools.exiftool.clone());
        let exiftool = match (exiftool, &change.what) {
            (Some(exiftool), _) => exiftool,
            (None, Many::Delete { .. }) => std::path::PathBuf::new(),
            (None, _) => {
                self.toasts
                    .error("Couldn't save the changes: exiftool, which writes them, wasn't found.");
                return;
            }
        };
        let count = format::count(change.ids.len() as u64);
        self.toasts.success(match &change.what {
            Many::Rate(_) => format!("Rating {count} items…"),
            Many::Tag(_) => format!("Tagging {count} items…"),
            Many::Turn(_) => format!("Rotating {count} photos…"),
            Many::Delete { .. } => format!("Deleting {count} items…"),
        });
        let cache = self.thumbnail_cache();
        let trash = self.trash;
        self.spawn_write(move || {
            let files = db::media::files_of(&lock(&db), &change.ids).map_err(|error| error.to_string());
            Event::ChangedMany(files.map(|files| {
                let mut done = ChangedMany {
                    what: change.what.clone(),
                    changed: Vec::new(),
                    media: Vec::new(),
                    failed: 0,
                    first_failure: None,
                };
                for file in files {
                    let library = std::path::Path::new(&file.root);
                    let written = match &change.what {
                        Many::Rate(rating) => {
                            media::rating::write(&exiftool, library, &file.rel_path, file.kind, *rating)
                                .map(|()| db::media::set_rating(&lock(&db), file.id, *rating))
                        }
                        Many::Tag(path) => {
                            let tag = domain::tagging::Change::Add(path.clone());
                            media::tagging::write(&exiftool, library, &file.rel_path, file.kind, &tag)
                                .map(|()| db::media::mark_for_reading(&lock(&db), file.id))
                        }
                        // A new thumbnail, and the faces looked for again on the turned photo.
                        Many::Turn(turn) => media::rotate::turn(&exiftool, library, &file.rel_path, *turn).map(|_| {
                            forget_index_entry(Some(&db), cache.as_ref(), file.root_id, &file.rel_path);
                            db::faces::look_again(&lock(&db), Some(file.id)).map(|()| true)
                        }),
                        // Out of the index and the thumbnail cache right away (REQ-009-24).
                        Many::Delete { permanently } => {
                            // Of a selection across libraries, those with a Recycle Bin still use it.
                            let permanently = *permanently && media::delete::deletes_permanently(library);
                            let remove = if permanently { media::delete::permanently } else { trash };
                            media::delete::delete(library, &file.rel_path, remove).map(|()| {
                                let key = db::media::remove(&lock(&db), file.id)?;
                                if let (Some(key), Some(cache)) = (key, &cache) {
                                    for width in domain::thumbs::THUMB_WIDTHS {
                                        let _ = std::fs::remove_file(cache.path(&key, width));
                                    }
                                }
                                Ok(true)
                            })
                        }
                    };
                    match written {
                        Ok(indexed) => {
                            if let Err(error) = indexed {
                                tracing::error!("can't update the index for {}: {error}", file.rel_path);
                            }
                            done.changed.push((file.root_id, file.rel_path));
                            done.media.push(file.id);
                        }
                        Err(error) => {
                            tracing::error!("can't change {}: {error}", file.rel_path);
                            done.failed += 1;
                            done.first_failure.get_or_insert((file.file_name, error.to_string()));
                        }
                    }
                }
                done
            }))
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub fn change_many(&mut self, _change: ChangeMany) {}

    /// Writes a tag change into the file in the background; the index reads the file again afterwards
    /// (REQ-026-03). A file that can't be written keeps its tags, and the user is told why (REQ-026-04).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn tag(&mut self, tag: TagChange) {
        let (Some(exiftool), Some(library)) = (
            self.edit_tools.as_ref().map(|tools| tools.exiftool.clone()),
            self.libraries.iter().find(|library| library.id == tag.root_id),
        ) else {
            self.toasts
                .error("Couldn't save the tag: exiftool, which writes it, wasn't found.");
            return;
        };
        let folder = PathBuf::from(&library.path);
        let db = self.db.clone();
        let detector = self.faces.detector.clone();
        self.spawn_write(move || {
            // Removing a person also takes their face regions, or the name would come back from them (REQ-032-13).
            if let (domain::tagging::Change::Remove(name), Some(db), domain::media::MediaKind::Image) =
                (&tag.change, &db, tag.kind)
            {
                let photo = db::faces::photo(&lock(db), tag.media).ok().flatten();
                if let Some(photo) = photo {
                    let removed = faces::remove_person(&exiftool, &photo, name, db, detector.as_deref());
                    if let Err(error) = removed {
                        tracing::error!("can't remove {name}'s regions from {}: {error}", tag.rel_path);
                        return Event::Tagged(Err(error));
                    }
                }
            }
            let written =
                media::tagging::write(&exiftool, &folder, &tag.rel_path, tag.kind, &tag.change).map_err(|error| {
                    tracing::error!("can't tag {}: {error}", tag.rel_path);
                    error.to_string()
                });
            Event::Tagged(written.map(|()| {
                if let Some(db) = &db
                    && let Err(error) = db::media::mark_for_reading(&lock(db), tag.media)
                {
                    tracing::error!("can't mark {} for reading: {error}", tag.rel_path);
                }
                (tag.root_id, tag.rel_path)
            }))
        });
    }

    /// The browser version never writes files (REQ-026-05).
    #[cfg(target_arch = "wasm32")]
    pub fn tag(&mut self, _tag: TagChange) {}

    /// Reads the changed files again and says how changing many items went (REQ-025-07, REQ-026-06).
    pub(super) fn changed_many(&mut self, result: Result<ChangedMany, String>) {
        match result {
            Ok(done) => {
                let mut by_library: std::collections::BTreeMap<i64, Vec<String>> = Default::default();
                for (root_id, rel_path) in &done.changed {
                    by_library.entry(*root_id).or_default().push(rel_path.clone());
                }
                for (root_id, paths) in by_library {
                    self.scans.request_paths(root_id, paths);
                }
                // The viewer shows a turned photo anew, and moves on from a deleted one (REQ-009-24).
                if let (Many::Turn(_) | Many::Delete { .. }, Some(originals)) = (&done.what, &mut self.originals) {
                    for media in &done.media {
                        originals.forget(*media);
                    }
                }
                if let (Many::Delete { .. }, Some(viewer)) = (&done.what, &mut self.viewer) {
                    viewer.deleted(&done.media);
                }
                self.refresh();
                let changed = format::count(done.changed.len() as u64);
                let (did, verb) = match &done.what {
                    Many::Rate(_) => (format!("Rated {changed} items"), "rate"),
                    Many::Tag(path) => (format!("Tagged {changed} items with {}", tag_name(path)), "tag"),
                    Many::Turn(_) => (format!("Rotated {changed} photos"), "rotate"),
                    Many::Delete { permanently: true } => (format!("Deleted {changed} items"), "delete"),
                    Many::Delete { permanently: false } => (
                        format!("Moved {changed} items to the {}", domain::deletion::TRASH),
                        "delete",
                    ),
                };
                match done.first_failure {
                    None => self.toasts.success(did),
                    Some((name, error)) => self.toasts.error(format!(
                        "Couldn't {verb} {} of {} items. {name}: {error}",
                        format::count(done.failed as u64),
                        format::count((done.failed + done.changed.len()) as u64),
                    )),
                }
            }
            Err(error) => self.toasts.error(format!("Couldn't save the changes: {error}")),
        }
    }
}
