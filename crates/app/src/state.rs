//! App state and the background jobs that feed it.

use crate::gallery::Gallery;
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

/// Explorer, or a fake in tests (REQ-009-10).
pub trait FileManager: Send + Sync {
    /// Opens the folder of `file` with the file selected.
    fn show_in_folder(&self, file: &std::path::Path) -> std::io::Result<()>;
}

/// The command that shows a file in the system's file manager (REQ-009-10, REQ-018-05).
pub const SHOW_IN_FOLDER: &str = if cfg!(windows) {
    "Show in Explorer"
} else {
    "Show in folder"
};

pub struct Explorer;

impl FileManager for Explorer {
    fn show_in_folder(&self, file: &std::path::Path) -> std::io::Result<()> {
        media::helpers::show_in_explorer(file)
    }
}

/// The running parts that take new settings at once (spec 010): the app's indexer, schedule and thumbnail cache, or a
/// fake in tests.
pub trait SettingsTargets: Send + Sync {
    /// Scheduled quick scans follow this interval from now on, without scanning now (REQ-010-03).
    fn set_quick_check_interval(&self, minutes: u32);
    /// Parallel file reads for the next scans and for rendering thumbnails (REQ-010-04).
    fn set_io_concurrency(&self, reads: usize);
    /// The thumbnail cache's limit in bytes; a lower one evicts in the background (REQ-010-05).
    fn set_cache_limit(&self, bytes: u64);
    /// The cache's size in bytes, `None` while it is still being measured (REQ-010-06). Doesn't block.
    fn cache_usage(&self) -> Option<u64>;
    /// Deletes all cached thumbnails (but those being read) and returns the new size. Blocks; runs on a job thread.
    fn clear_cache(&self) -> u64;
}

/// The thumbnail cache's size as the Settings page shows it (REQ-010-06).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CacheUsage {
    /// Nothing to ask (no thumbnail cache).
    #[default]
    Unknown,
    Measuring,
    Bytes(u64),
}

impl CacheUsage {
    /// "…" while unknown, "unknown" if it can't be determined, else the size.
    pub fn label(self) -> String {
        match self {
            Self::Unknown => "unknown".into(),
            Self::Measuring => "…".into(),
            Self::Bytes(bytes) => crate::format::bytes(i64::try_from(bytes).unwrap_or(i64::MAX)),
        }
    }
}

/// The cache's size is read again this often while the Settings page shows (REQ-010-06).
pub const USAGE_REFRESH: Duration = Duration::from_secs(10);
/// After a change of the limit, the size is read this often for a while, so the eviction shows (REQ-010-05).
const USAGE_REFRESH_AFTER_CHANGE: Duration = Duration::from_secs(1);

/// Bytes of a cache limit in GB (GiB).
pub fn gib_to_bytes(gib: f64) -> u64 {
    (gib * 1024f64.powi(3)).round() as u64
}

/// Results of background jobs, applied on the UI thread.
enum Event {
    LibraryAdded(Result<Library, String>),
    LibraryRemoved(Result<(), String>),
    FolderPicked(Option<PathBuf>),
    /// Explorer couldn't show a file: why.
    ExplorerFailed(Option<String>),
    /// The thumbnail cache was cleared; its new size.
    CacheCleared(u64),
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
    /// _Clear_ waits for confirmation (REQ-010-06).
    pub confirm_clear: bool,
    pub clearing: bool,
    /// The _Parallel file reads_ slider's value while it is dragged; saved on release (REQ-010-04).
    pub concurrency_drag: Option<u32>,
}

pub struct AppState {
    pub page: Page,
    /// The write connection. `None` if the index couldn't be opened; `error` says why.
    pub(crate) db: Option<Arc<Mutex<Connection>>>,
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
    scheduler: Option<Arc<indexer::schedule::Scheduler>>,
    /// Renders thumbnails in the background; the gallery's tiles request theirs from it.
    thumbnails: Option<Arc<indexer::thumbnails::ThumbnailService>>,
    /// `None` if the index couldn't be opened.
    pub gallery: Option<Gallery>,
    pub search_bar: crate::search_bar::SearchBar,
    pub sidebar: crate::sidebar::SidebarState,
    /// The open viewer, over the gallery.
    pub viewer: Option<crate::viewer::Viewer>,
    /// The media item whose details the info sheet beside the gallery shows (REQ-009-11).
    pub info_sheet: Option<i64>,
    pub file_manager: Arc<dyn FileManager>,
    /// Loads the viewer's originals; made with egui textures on first use unless set with [`Self::with_originals`].
    pub originals: Option<crate::originals::Originals>,
    /// Plays the viewer's videos (REQ-009-06); unavailable unless set with [`Self::with_videos`].
    pub videos: crate::video::Videos,
    /// The stored settings (spec 010).
    pub settings: db::settings::Settings,
    settings_targets: Option<Arc<dyn SettingsTargets>>,
    pub cache_usage: CacheUsage,
    /// When the cache's size was last read, and until when it is read more often after a change.
    usage_read_at: Option<Instant>,
    usage_fast_until: Option<Instant>,
}

impl AppState {
    pub fn new(conn: Connection, scans: Arc<dyn ScanRequests>, picker: Arc<dyn FolderPicker>) -> Self {
        let mut state = Self::empty(scans, picker);
        state.settings = db::settings::load(&conn).unwrap_or_else(|error| {
            tracing::error!("can't read the settings: {error}");
            Default::default()
        });
        let db = Arc::new(Mutex::new(conn));
        state.db = Some(db.clone());
        state.refresh();
        state.gallery = Some(Gallery::new(db.clone(), db, None));
        state
    }

    /// The UI's write connection, for parts that store choices through it.
    pub fn db(&self) -> Option<Arc<Mutex<Connection>>> {
        self.db.clone()
    }

    /// Replaces the gallery, e.g. with one that reads through its own connection and streams thumbnails.
    pub fn with_gallery(mut self, gallery: Gallery) -> Self {
        self.gallery = Some(gallery);
        self
    }

    /// Sends scan requests to `indexer` and shows what it reports.
    pub fn with_indexer(mut self, indexer: Arc<dyn ScanRequests>, events: Receiver<IndexerEvent>) -> Self {
        self.scans = indexer;
        self.indexer_events = Some(events);
        self
    }

    /// Runs scheduled scans for as long as the app runs.
    pub fn with_scheduler(mut self, scheduler: Arc<indexer::schedule::Scheduler>) -> Self {
        self.scheduler = Some(scheduler);
        self
    }

    /// Renders thumbnails for as long as the app runs.
    pub fn with_thumbnails(mut self, thumbnails: Arc<indexer::thumbnails::ThumbnailService>) -> Self {
        self.thumbnails = Some(thumbnails);
        self
    }

    /// Hands changed settings to `targets` (the running indexer, schedule and thumbnail cache).
    pub fn with_settings_targets(mut self, targets: Arc<dyn SettingsTargets>) -> Self {
        self.settings_targets = Some(targets);
        self
    }

    /// Validates and stores `settings`, then applies what changed right away (REQ-010-02 … -05). An invalid value
    /// is an error toast, and nothing changes.
    pub fn update_settings(&mut self, settings: db::settings::Settings) {
        let Some(db) = self.db.clone() else { return };
        if let Err(error) = db::settings::save(&lock(&db), &settings) {
            self.toasts.error(error.to_string());
            return;
        }
        let old = std::mem::replace(&mut self.settings, settings);
        let Some(targets) = &self.settings_targets else { return };
        if old.quick_check_interval_minutes != settings.quick_check_interval_minutes {
            targets.set_quick_check_interval(settings.quick_check_interval_minutes);
        }
        if old.io_concurrency != settings.io_concurrency {
            targets.set_io_concurrency(settings.io_concurrency as usize);
        }
        if old.thumb_cache_gib != settings.thumb_cache_gib {
            targets.set_cache_limit(gib_to_bytes(settings.thumb_cache_gib));
            self.usage_read_at = None;
            self.usage_fast_until = Some(Instant::now() + USAGE_REFRESH);
        }
    }

    /// Reads the thumbnail cache's size when it is due (every 10 s, more often right after a change of the limit);
    /// the Settings page calls it every frame. Returns when to read next.
    pub fn refresh_cache_usage(&mut self) -> Duration {
        let now = Instant::now();
        let every = match self.usage_fast_until {
            Some(until) if now < until => USAGE_REFRESH_AFTER_CHANGE,
            _ => USAGE_REFRESH,
        };
        if self.usage_read_at.is_none_or(|at| now.duration_since(at) >= every) {
            self.usage_read_at = Some(now);
            self.cache_usage = match &self.settings_targets {
                None => CacheUsage::Unknown,
                Some(targets) => targets.cache_usage().map_or(CacheUsage::Measuring, CacheUsage::Bytes),
            };
        }
        every
    }

    /// Deletes all cached thumbnails in the background, then shows the new size (REQ-010-06).
    pub fn clear_thumbnail_cache(&mut self) {
        let Some(targets) = self.settings_targets.clone() else {
            return;
        };
        self.settings_page.clearing = true;
        self.spawn(move || Event::CacheCleared(targets.clear_cache()));
    }

    /// Shows files in `file_manager` instead of Explorer.
    pub fn with_file_manager(mut self, file_manager: Arc<dyn FileManager>) -> Self {
        self.file_manager = file_manager;
        self
    }

    /// Opens Explorer on a media file, or says why it can't (REQ-009-10). In the background: on an offline share the
    /// check alone may take many seconds.
    pub fn show_in_explorer(&mut self, file: PathBuf) {
        let file_manager = self.file_manager.clone();
        self.spawn(move || {
            if !file.exists() {
                return Event::ExplorerFailed(Some(format!("{} no longer exists.", file.display())));
            }
            Event::ExplorerFailed(file_manager.show_in_folder(&file).err().map(|error| {
                tracing::error!("can't show {} in the file manager: {error}", file.display());
                let program = if cfg!(windows) { "Explorer" } else { "the file manager" };
                format!("Couldn't open {program}: {error}")
            }))
        });
    }

    /// The file of the media item at gallery position `index`, if its data is loaded.
    pub fn file_at(&mut self, index: usize) -> Option<PathBuf> {
        let item = self.gallery.as_mut()?.item(index)?;
        db::media::file_in(&self.libraries, item.root_id, &item.rel_path)
    }

    /// Loads the viewer's originals with `originals` (wgpu textures in the app).
    pub fn with_originals(mut self, originals: crate::originals::Originals) -> Self {
        self.originals = Some(originals);
        self
    }

    /// Plays the viewer's videos with `videos` (libmpv in the app, a fake in tests).
    pub fn with_videos(mut self, videos: crate::video::Videos) -> Self {
        self.videos = videos;
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
            gallery: None,
            search_bar: Default::default(),
            sidebar: Default::default(),
            viewer: None,
            info_sheet: None,
            file_manager: Arc::new(Explorer),
            originals: None,
            videos: crate::video::Videos::default(),
            settings: Default::default(),
            settings_targets: None,
            cache_usage: CacheUsage::default(),
            usage_read_at: None,
            usage_fast_until: None,
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
            Ok((libraries, count)) => {
                (self.libraries, self.media_count) = (libraries, count);
                // New media items while indexing, or a library removed (REQ-006-11).
                if let Some(gallery) = &mut self.gallery {
                    // A removed library leaves the search, so the gallery doesn't stay empty without a reason.
                    if let Some(library) = gallery.search().library
                        && !self.libraries.iter().any(|known| known.id == library)
                    {
                        let mut search = gallery.search().clone();
                        (search.library, search.folder) = (None, None);
                        gallery.set_search(search);
                    }
                    gallery.reload();
                }
            }
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
        self.is_busy()
            || self.indexer_status.current.is_some()
            || self.written_since_refresh
            || self.gallery.as_ref().is_some_and(Gallery::is_loading)
    }

    /// Waits for all background jobs and applies them (tests, before stepping the UI).
    pub fn wait_for_jobs(&mut self) {
        while self.pending_jobs > 0 {
            let event = self.events.1.recv().expect("job sender dropped");
            self.apply(event);
        }
        if let Some(gallery) = &mut self.gallery {
            gallery.wait_for_loads();
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
            Event::ExplorerFailed(error) => {
                if let Some(error) = error {
                    self.toasts.error(error);
                }
            }
            Event::CacheCleared(bytes) => {
                self.settings_page.clearing = false;
                self.cache_usage = CacheUsage::Bytes(bytes);
                self.usage_read_at = Some(Instant::now());
                self.toasts.success("Thumbnail cache cleared");
            }
            Event::Failed => {
                self.settings_page.adding = false;
                self.settings_page.picking = false;
                self.settings_page.clearing = false;
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
