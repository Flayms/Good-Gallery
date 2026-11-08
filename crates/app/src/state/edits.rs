//! Saving and reverting edits, clearing a photo's version history and importing LUTs (specs 021, 043).

use super::AppState;
#[cfg(not(target_arch = "wasm32"))]
use super::{Event, Written, forget_index_entry};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::Arc;

/// A media item whose edit _Revert to original_ would take back (REQ-021-12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevertTarget {
    pub media: i64,
    pub root_id: i64,
    pub rel_path: String,
    pub file_name: String,
}

impl RevertTarget {
    pub fn of(detail: &db::media::MediaDetail) -> Self {
        Self {
            media: detail.id,
            root_id: detail.root_id,
            rel_path: detail.rel_path.clone(),
            file_name: detail.file_name.clone(),
        }
    }
}

/// A photo whose earlier versions the user wants gone (REQ-021-26), with its versions as the index knows them.
#[derive(Debug, Clone, PartialEq)]
pub struct ClearHistory {
    pub target: RevertTarget,
    pub versions: Vec<domain::versions::Version>,
}

impl ClearHistory {
    /// For a photo with more than one version.
    pub fn of(detail: &db::media::MediaDetail) -> Option<Self> {
        let versions = detail.edit.as_ref()?.versions();
        (versions.len() > 1).then(|| Self {
            target: RevertTarget::of(detail),
            versions,
        })
    }
}

impl AppState {
    /// Keeps imported LUTs in `luts` (spec 043).
    pub fn with_luts(mut self, luts: crate::luts::Luts) -> Self {
        self.luts = Arc::new(luts);
        self
    }

    /// Lets the user pick a `.cube` file and imports it as a LUT, in the background (REQ-043-01).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn import_lut(&mut self) {
        let (picker, luts) = (self.picker.clone(), self.luts.clone());
        self.spawn(move || Event::LutImported(picker.pick_lut_file().map(|file| luts.import(&file))));
    }

    /// The browser version doesn't edit (REQ-021-01).
    #[cfg(target_arch = "wasm32")]
    pub fn import_lut(&mut self) {}

    /// Saves edits with exiftool (spec 021).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_edit_tools(mut self, tools: media::edit::Tools) -> Self {
        self.edit_tools = Some(tools);
        self
    }

    /// Saves an edit in the background (REQ-021-09, REQ-021-10); the viewer hears back when it is done.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_edit(&mut self, request: crate::edit::SaveEdit) {
        let media = request.media;
        let (Some(tools), Some(library)) = (
            self.edit_tools.clone(),
            self.libraries.iter().find(|library| library.id == request.root_id),
        ) else {
            self.toasts
                .error("Can't save: exiftool, which copies the metadata, wasn't found.");
            if let Some(viewer) = &mut self.viewer {
                viewer.edit_saved(media, false);
            }
            return;
        };
        let save = media::edit::SaveRequest {
            library: PathBuf::from(&library.path),
            rel_path: request.rel_path,
            source: request.source,
            recipe: request.recipe,
            copy: request.copy,
            keep_original: request.keep_original,
            mtime: self.settings.mtime_on_save,
            subject: request.subject,
            sidecar_orientation: request.sidecar_orientation,
            history: request.history,
            lut: request.lut,
        };
        let (db, cache) = (self.db.clone(), self.thumbnail_cache());
        let (root_id, copy) = (request.root_id, request.copy);
        self.spawn_write(move || {
            let result = media::edit::save(&tools, &save).map_err(|error| {
                tracing::error!("can't save the edit of {}: {error}", save.rel_path);
                error.to_string()
            });
            Event::EditSaved(
                media,
                result.map(|rel_path| {
                    forget_index_entry(db.as_deref(), cache.as_ref(), root_id, &rel_path);
                    Written {
                        media,
                        root_id,
                        rel_path,
                        copy,
                    }
                }),
            )
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub fn save_edit(&mut self, request: crate::edit::SaveEdit) {
        if let Some(viewer) = &mut self.viewer {
            viewer.edit_saved(request.media, false);
        }
    }

    /// Puts the unedited original back in the background (REQ-021-12).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn revert_edit(&mut self, target: RevertTarget) {
        let Some(library) = self.libraries.iter().find(|library| library.id == target.root_id) else {
            return;
        };
        let (folder, mtime) = (PathBuf::from(&library.path), self.settings.mtime_on_save);
        let (db, cache) = (self.db.clone(), self.thumbnail_cache());
        self.spawn_write(move || {
            let result = media::edit::revert(&folder, &target.rel_path, mtime).map_err(|error| {
                tracing::error!("can't revert {}: {error}", target.rel_path);
                error.to_string()
            });
            Event::Reverted(result.map(|()| {
                forget_index_entry(db.as_deref(), cache.as_ref(), target.root_id, &target.rel_path);
                Written {
                    media: target.media,
                    root_id: target.root_id,
                    rel_path: target.rel_path,
                    copy: false,
                }
            }))
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub fn revert_edit(&mut self, _target: RevertTarget) {}

    /// Removes a photo's earlier versions from its file in the background, then reads it again (REQ-021-26).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn clear_history(&mut self, clear: ClearHistory) {
        let (Some(tools), Some(library)) = (
            self.edit_tools.clone(),
            self.libraries.iter().find(|library| library.id == clear.target.root_id),
        ) else {
            self.toasts
                .error("Couldn't clear the version history: exiftool, which writes it, wasn't found.");
            return;
        };
        let folder = PathBuf::from(&library.path);
        let (db, cache) = (self.db.clone(), self.thumbnail_cache());
        self.spawn_write(move || {
            let target = clear.target;
            let result =
                media::edit::clear_history(&tools, &folder, &target.rel_path, &clear.versions).map_err(|error| {
                    tracing::error!("can't clear the versions of {}: {error}", target.rel_path);
                    error.to_string()
                });
            Event::HistoryCleared(result.map(|()| {
                forget_index_entry(db.as_deref(), cache.as_ref(), target.root_id, &target.rel_path);
                Written {
                    media: target.media,
                    root_id: target.root_id,
                    rel_path: target.rel_path,
                    copy: false,
                }
            }))
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub fn clear_history(&mut self, _clear: ClearHistory) {}
}
