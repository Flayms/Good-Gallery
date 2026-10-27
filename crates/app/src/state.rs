//! App state and the background jobs that feed it.

use crate::toasts::Toasts;
use db::libraries::Library;
use db::rusqlite::Connection;
use indexer::{IndexerEvent, IndexerStatus, ScanRequests};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// While a scan writes, the gallery and counts refresh at most this often (REQ-006-11).
const REFRESH_WHILE_SCANNING: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Gallery,
    Settings,
}

/// The native folder dialog, or a fake in tests. Blocks until the user picks or cancels; runs on a job thread.
pub trait FolderPicker: Send + Sync {
    fn pick_folder(&self) -> Option<PathBuf>;
}

pub struct NativeFolderPicker;

impl FolderPicker for NativeFolderPicker {
    fn pick_folder(&self) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title("Add a library").pick_folder()
    }
}

/// Results of background jobs, applied on the UI thread.
enum Event {
    LibraryAdded(Result<Library, String>),
    LibraryRemoved(Result<(), String>),
    FolderPicked(Option<PathBuf>),
    /// The job panicked; its flags are reset and the user gets an error.
    Failed,
}

/// Inputs and dialogs of the Settings page.
#[derive(Debug, Default)]
pub struct SettingsPage {
    pub path_input: String,
    pub adding: bool,
    pub picking: bool,
    /// The library whose removal waits for confirmation.
    pub confirm_remove: Option<Library>,
}

pub struct AppState {
    pub page: Page,
    /// The write connection. `None` if the index couldn't be opened; `error` says why.
    db: Option<Arc<Mutex<Connection>>>,
    pub error: Option<String>,
    pub libraries: Vec<Library>,
    pub media_count: i64,
    scans: Arc<dyn ScanRequests>,
    picker: Arc<dyn FolderPicker>,
    events: (Sender<Event>, Receiver<Event>),
    pending_jobs: usize,
    pub settings_page: SettingsPage,
    pub toasts: Toasts,
    indexer_events: Option<Receiver<IndexerEvent>>,
    pub indexer_status: IndexerStatus,
    /// When written files were last shown; `None` until the running scan wrote its first files.
    last_written_refresh: Option<Instant>,
    written_since_refresh: bool,
    /// Kept alive for the app's lifetime; dropping it stops the schedule.
    scheduler: Option<indexer::schedule::Scheduler>,
    /// Renders thumbnails in the background; the gallery's tiles request theirs from it (M6).
    thumbnails: Option<Arc<indexer::thumbnails::ThumbnailService>>,
}

impl AppState {
    pub fn new(conn: Connection, scans: Arc<dyn ScanRequests>, picker: Arc<dyn FolderPicker>) -> Self {
        let mut state = Self::empty(scans, picker);
        state.db = Some(Arc::new(Mutex::new(conn)));
        state.refresh();
        state
    }

    /// Sends scan requests to `indexer` and shows what it reports.
    pub fn with_indexer(mut self, indexer: Arc<dyn ScanRequests>, events: Receiver<IndexerEvent>) -> Self {
        self.scans = indexer;
        self.indexer_events = Some(events);
        self
    }

    /// Runs scheduled scans for as long as the app runs.
    pub fn with_scheduler(mut self, scheduler: indexer::schedule::Scheduler) -> Self {
        self.scheduler = Some(scheduler);
        self
    }

    /// Renders thumbnails for as long as the app runs.
    pub fn with_thumbnails(mut self, thumbnails: Arc<indexer::thumbnails::ThumbnailService>) -> Self {
        self.thumbnails = Some(thumbnails);
        self
    }

    /// The index couldn't be opened: the app shows `error` instead of the gallery.
    pub fn failed(error: String) -> Self {
        let mut state = Self::empty(Arc::new(indexer::NoScans), Arc::new(NativeFolderPicker));
        state.error = Some(error);
        state
    }

    fn empty(scans: Arc<dyn ScanRequests>, picker: Arc<dyn FolderPicker>) -> Self {
        Self {
            page: Page::default(),
            db: None,
            error: None,
            libraries: Vec::new(),
            media_count: 0,
            scans,
            picker,
            events: channel(),
            pending_jobs: 0,
            settings_page: SettingsPage::default(),
            toasts: Toasts::default(),
            indexer_events: None,
            indexer_status: IndexerStatus::default(),
            last_written_refresh: None,
            written_since_refresh: false,
            scheduler: None,
            thumbnails: None,
        }
    }

    /// Reloads what the UI shows from the index.
    pub fn refresh(&mut self) {
        let Some(db) = self.db.clone() else { return };
        let conn = lock(&db);
        self.reload(&conn);
    }

    /// [`Self::refresh`] unless the connection is busy; false if it was.
    fn try_refresh(&mut self) -> bool {
        let Some(db) = self.db.clone() else { return true };
        let conn = match db.try_lock() {
            Ok(conn) => conn,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return false,
        };
        self.reload(&conn);
        true
    }

    fn reload(&mut self, conn: &Connection) {
        let result = indexer::libraries::list(conn)
            .map_err(|e| e.to_string())
            .and_then(|libraries| {
                let count = conn
                    .query_row("SELECT count(*) FROM media", [], |row| row.get(0))
                    .map_err(|e| e.to_string())?;
                Ok((libraries, count))
            });
        match result {
            Ok((libraries, count)) => (self.libraries, self.media_count) = (libraries, count),
            Err(error) => self.toasts.error(error),
        }
    }

    /// Runs `job` on its own thread. Every job sends exactly one event, also when it panics.
    fn spawn(&mut self, job: impl FnOnce() -> Event + Send + 'static) {
        let tx = self.events.0.clone();
        self.pending_jobs += 1;
        std::thread::spawn(move || {
            let event = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).unwrap_or(Event::Failed);
            let _ = tx.send(event);
        });
    }

    pub fn is_busy(&self) -> bool {
        self.pending_jobs > 0
    }

    /// Adds the library at `path` in the background (the reachability check may take seconds).
    pub fn add_library(&mut self, path: String) {
        let Some(db) = self.db.clone() else { return };
        let scans = self.scans.clone();
        self.settings_page.adding = true;
        self.spawn(move || {
            // The reachability check may take seconds: only lock the index for the insert.
            let added = indexer::libraries::check_folder(&path, indexer::reachability::TIMEOUT).and_then(|path| {
                let conn = lock(&db);
                indexer::libraries::insert_checked(&conn, &path, scans.as_ref())
            });
            Event::LibraryAdded(added.map_err(|e| e.to_string()))
        });
    }

    /// Opens the folder dialog; the picked folder is then added.
    pub fn browse_for_library(&mut self) {
        let picker = self.picker.clone();
        self.settings_page.picking = true;
        self.spawn(move || Event::FolderPicked(picker.pick_folder()));
    }

    /// Removes a library in the background: deleting the media items of a large library takes a while.
    pub fn remove_library(&mut self, id: i64) {
        let Some(db) = self.db.clone() else { return };
        let scans = self.scans.clone();
        self.spawn(move || {
            let conn = lock(&db);
            Event::LibraryRemoved(indexer::libraries::remove(&conn, id, scans.as_ref()).map_err(|e| e.to_string()))
        });
    }

    pub fn rescan_library(&mut self, id: i64) {
        let Some(db) = &self.db else { return };
        let result = indexer::libraries::rescan(&lock(db), id, self.scans.as_ref());
        match result {
            Ok(()) => self.toasts.success("Rescan queued"),
            Err(error) => self.toasts.error(error.to_string()),
        }
    }

    /// Applies finished background jobs. Called every frame.
    pub fn poll(&mut self) {
        while let Ok(event) = self.events.1.try_recv() {
            self.apply(event);
        }
        let events: Vec<IndexerEvent> = self.indexer_events.iter().flat_map(|rx| rx.try_iter()).collect();
        for event in events {
            self.apply_indexer(event);
        }
        // Written files show right away the first time, then at most every 5 seconds, and once more at the end.
        if self.written_since_refresh
            && self
                .last_written_refresh
                .is_none_or(|at| at.elapsed() >= REFRESH_WHILE_SCANNING)
        {
            // Not while a background job (e.g. removing a large library) holds the connection: try next frame.
            if self.try_refresh() {
                self.written_since_refresh = false;
                self.last_written_refresh = Some(Instant::now());
            }
        }
    }

    fn apply_indexer(&mut self, event: IndexerEvent) {
        match event {
            IndexerEvent::Status(status) => self.indexer_status = status,
            IndexerEvent::Written(_) => self.written_since_refresh = true,
            IndexerEvent::Done(_, result) => {
                if let indexer::ScanResult::Failed(error) = result {
                    self.toasts.error(format!("Indexing failed: {error}"));
                }
                self.written_since_refresh = false;
                self.last_written_refresh = None;
                self.refresh();
            }
        }
    }

    /// Whether the UI must keep polling (jobs or a scan running).
    pub fn is_active(&self) -> bool {
        self.is_busy() || self.indexer_status.current.is_some() || self.written_since_refresh
    }

    /// Waits for all background jobs and applies them (tests, before stepping the UI).
    pub fn wait_for_jobs(&mut self) {
        while self.pending_jobs > 0 {
            let event = self.events.1.recv().expect("job sender dropped");
            self.apply(event);
        }
    }

    fn apply(&mut self, event: Event) {
        self.pending_jobs -= 1;
        match event {
            Event::LibraryAdded(result) => {
                self.settings_page.adding = false;
                match result {
                    Ok(library) => {
                        self.toasts.success(format!("Added {}, indexing…", library.label));
                        self.settings_page.path_input.clear();
                        self.refresh();
                    }
                    Err(error) => self.toasts.error(error),
                }
            }
            Event::LibraryRemoved(result) => {
                if let Err(error) = result {
                    self.toasts.error(error);
                }
                self.refresh();
            }
            Event::FolderPicked(folder) => {
                self.settings_page.picking = false;
                if let Some(folder) = folder {
                    self.add_library(folder.to_string_lossy().into_owned());
                }
            }
            Event::Failed => {
                self.settings_page.adding = false;
                self.settings_page.picking = false;
                self.toasts.error("Something went wrong. Details are in the log.");
                self.refresh();
            }
        }
    }
}

/// The index connection. A job that panicked while holding it poisons the mutex; SQLite rolled its transaction back,
/// so the connection is still usable.
fn lock(db: &Mutex<Connection>) -> std::sync::MutexGuard<'_, Connection> {
    db.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
