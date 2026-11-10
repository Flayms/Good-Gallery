//! Fixing the date taken of a selection, with undo (spec 038). Desktop only.

use super::{AppState, Event};
use crate::format;
use std::path::PathBuf;
use std::sync::Arc;

/// The date fix dialog, where date fixes are journaled, and what reads dates for it.
#[derive(Default)]
pub struct FixDates {
    /// The date fix dialog, while open (spec 038).
    pub dialog: Option<crate::fix_dates::FixDatesDialog>,
    /// Where date fixes are journaled (REQ-038-09); fixing dates is off without it.
    journal: Option<PathBuf>,
    /// exiftool processes that read dates for the date fix dialog, started when it first opens.
    reader: Option<Arc<media::exiftool::ExifTool>>,
}

impl AppState {
    /// Journals date fixes at `path` (REQ-038-09).
    pub fn with_date_journal(mut self, path: PathBuf) -> Self {
        self.dates.journal = Some(path);
        self
    }

    /// Opens the date fix dialog for `ids` (REQ-038-01).
    pub fn open_fix_dates(&mut self, ids: Vec<i64>) {
        let (Some(db), Some(journal)) = (self.db.clone(), &self.dates.journal) else {
            return;
        };
        if ids.is_empty() {
            return;
        }
        let Some(path) = self.edit_tools.as_ref().map(|tools| tools.exiftool.clone()) else {
            self.toasts
                .error("Dates can't be fixed: exiftool, which reads and writes them, wasn't found.");
            return;
        };
        let reader = self
            .dates
            .reader
            .get_or_insert_with(|| Arc::new(media::exiftool::ExifTool::new(path, 2)))
            .clone();
        let undo = crate::fix_dates::can_undo(journal);
        self.dates.dialog = Some(crate::fix_dates::FixDatesDialog::open(
            db,
            reader,
            &self.libraries,
            ids,
            undo,
        ));
    }

    /// Writes the dates of `job` in the background (REQ-038-08).
    pub fn start_fix_dates(&mut self, job: crate::fix_dates::FixJob) {
        let (Some(db), Some(journal), Some(exiftool)) = (
            self.db.clone(),
            self.dates.journal.clone(),
            self.edit_tools.as_ref().map(|tools| tools.exiftool.clone()),
        ) else {
            return;
        };
        let mtime = self.settings.mtime_on_save;
        let count = job.changes.len();
        self.toasts
            .success(format!("Fixing the dates of {} photos…", format::count(count as u64)));
        self.spawn_write(move || {
            Event::to(
                Self::dates_fixed,
                crate::fix_dates::run(&db, &exiftool, &journal, &job, mtime),
            )
        });
    }

    /// Takes the last date fix back (REQ-038-09).
    pub fn undo_fix_dates(&mut self) {
        let (Some(db), Some(journal), Some(exiftool)) = (
            self.db.clone(),
            self.dates.journal.clone(),
            self.edit_tools.as_ref().map(|tools| tools.exiftool.clone()),
        ) else {
            return;
        };
        self.spawn_write(move || Event::to(Self::dates_fixed, crate::fix_dates::undo(&db, &exiftool, &journal)));
    }

    pub(super) fn dates_fixed(&mut self, result: Result<crate::fix_dates::DatesDone, String>) {
        let done = match result {
            Ok(done) => done,
            Err(error) => {
                self.toasts.error(format!("Couldn't fix the dates: {error}"));
                return;
            }
        };
        for (root_id, paths) in &done.touched {
            self.scans.request_paths(*root_id, paths.clone());
        }
        if !done.touched.is_empty() {
            self.refresh();
            if let Some(gallery) = &mut self.gallery {
                gallery.reload();
            }
        }
        if let Some((name, error, count)) = &done.failed {
            let others = if *count > 1 {
                format!(" and {} more", format::count(*count as u64 - 1))
            } else {
                String::new()
            };
            self.toasts
                .error(format!("Couldn't fix the date of {name}{others}: {error}"));
        }
        let photos = |count: usize| {
            let count = format::count(count as u64);
            if count == "1" {
                "1 photo".to_string()
            } else {
                format!("{count} photos")
            }
        };
        if done.undone {
            let mut text = format!("Put back the dates of {}", photos(done.fixed));
            if done.skipped > 0 {
                text.push_str(&format!("; {} changed since were left alone", photos(done.skipped)));
            }
            self.toasts.success(text);
        } else if done.fixed > 0 {
            self.toasts.success_with(
                format!("Fixed the dates of {}", photos(done.fixed)),
                crate::toasts::ToastAction::UndoDates,
            );
        }
    }
}
