//! Renaming a selection, with undo and recovery after a crash (spec 035). Desktop only.

use super::{AppState, Event};
use std::path::PathBuf;

/// The rename dialog and where renames are journaled.
#[derive(Default)]
pub struct Rename {
    /// The rename dialog, while open (spec 035).
    pub dialog: Option<crate::rename::RenameDialog>,
    /// Where renames are journaled (REQ-035-08, REQ-035-09); renaming is off without it.
    journal: Option<PathBuf>,
}

impl AppState {
    /// Journals renames at `path` (REQ-035-08, REQ-035-09).
    pub fn with_rename_journal(mut self, path: PathBuf) -> Self {
        self.rename.journal = Some(path);
        self
    }

    /// Opens the rename dialog for `ids` (REQ-035-01).
    pub fn open_rename(&mut self, ids: Vec<i64>) {
        let (Some(db), Some(journal)) = (self.db.clone(), &self.rename.journal) else {
            return;
        };
        if ids.is_empty() {
            return;
        }
        let undo = crate::rename::undo_job(&db, journal).is_some();
        self.rename.dialog = Some(crate::rename::RenameDialog::open(db, &self.libraries, ids, undo));
    }

    /// Renames in the background, the index first (REQ-035-06, REQ-035-07).
    pub fn start_rename(&mut self, job: crate::rename::RenameJob) {
        let (Some(db), Some(journal)) = (self.db.clone(), self.rename.journal.clone()) else {
            return;
        };
        let cache = self.thumbnail_cache();
        self.spawn_write(move || Event::Renamed(crate::rename::run(&db, cache.as_ref(), &journal, &job)));
    }

    /// Takes the last rename back (REQ-035-08).
    pub fn undo_rename(&mut self) {
        let (Some(db), Some(journal)) = (self.db.clone(), &self.rename.journal) else {
            return;
        };
        match crate::rename::undo_job(&db, journal) {
            Some(job) => self.start_rename(job),
            None => self
                .toasts
                .error("The last rename can't be undone: its files have other names now."),
        }
    }

    /// Finishes a rename the app was ended in, once its libraries are there (REQ-035-09).
    pub fn recover_rename(&mut self) {
        let (Some(db), Some(journal)) = (self.db.clone(), self.rename.journal.clone()) else {
            return;
        };
        let cache = self.thumbnail_cache();
        self.spawn_write(move || match crate::rename::recover(&db, cache.as_ref(), &journal) {
            Ok(Some(done)) => Event::Renamed(Ok(done)),
            Ok(None) => Event::Renamed(Ok(crate::rename::RenameDone::default())),
            Err(error) => Event::Renamed(Err(error)),
        });
    }

    pub(super) fn renamed(&mut self, result: Result<crate::rename::RenameDone, String>) {
        let done = match result {
            Ok(done) => done,
            Err(error) => {
                self.toasts.error(format!("Couldn't rename: {error}"));
                return;
            }
        };
        for (root_id, paths) in &done.touched {
            self.scans.request_paths(*root_id, paths.clone());
        }
        if done.touched.is_empty() {
            return;
        }
        self.refresh();
        if let Some(gallery) = &mut self.gallery {
            gallery.reload();
        }
        if let Some((name, error, count)) = &done.failed {
            let others = if *count > 1 {
                format!(" and {} more", crate::format::count(*count as u64 - 1))
            } else {
                String::new()
            };
            self.toasts.error(format!("Couldn't rename {name}{others}: {error}"));
        }
        if done.renamed > 0 {
            let files = if done.renamed == 1 {
                "file".to_string()
            } else {
                "files".to_string()
            };
            let text = if done.recovered {
                format!(
                    "Finished renaming {} {files}",
                    crate::format::count(done.renamed as u64)
                )
            } else {
                format!("Renamed {} {files}", crate::format::count(done.renamed as u64))
            };
            self.toasts.success_with(text, crate::toasts::ToastAction::UndoRename);
        }
    }
}
