//! App state and the background jobs that feed it.

use crate::format;
use crate::gallery::Gallery;
use crate::toasts::Toasts;
use db::libraries::Library;
use db::rusqlite::Connection;
use indexer::{IndexerEvent, IndexerStatus, ScanRequests};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use web_time::Instant;

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

#[cfg(not(target_arch = "wasm32"))]
pub struct NativeFolderPicker;

#[cfg(not(target_arch = "wasm32"))]
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
    #[cfg(not(target_arch = "wasm32"))]
    fn show_in_folder(&self, file: &std::path::Path) -> std::io::Result<()> {
        media::helpers::show_in_explorer(file)
    }

    /// The browser has no file manager to open (REQ-019-13).
    #[cfg(target_arch = "wasm32")]
    fn show_in_folder(&self, _file: &std::path::Path) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
}

/// No folder dialog: the browser's picker is asynchronous (`web::pick_folder`), and a failed index has none.
pub struct NoFolderPicker;

impl FolderPicker for NoFolderPicker {
    fn pick_folder(&self) -> Option<PathBuf> {
        None
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

/// A rating for several items at once (REQ-025-07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateMany {
    pub ids: Vec<i64>,
    /// `None` removes the rating.
    pub rating: Option<i64>,
}

/// Rating more items than this asks first (REQ-025-07).
pub const CONFIRM_RATING_ABOVE: usize = 100;

/// What rating several items did (REQ-025-07).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Debug)]
struct RatedMany {
    /// The files rated, by library: read again on the next changes scan.
    rated: Vec<(i64, String)>,
    failed: usize,
    /// The first file that couldn't be rated, and why.
    first_failure: Option<(String, String)>,
}

/// A file the app wrote into a library (spec 021).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct Written {
    /// The media item that was edited.
    media: i64,
    root_id: i64,
    /// The file written, relative to the library: the item's own, or a new copy.
    rel_path: String,
    copy: bool,
}

/// Results of background jobs, applied on the UI thread. The browser has no background threads or folder dialog, so
/// some never happen there.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum Event {
    LibraryAdded(Result<Library, String>),
    /// The browser's folder picker closed: the library it added or made readable again, or `None` when cancelled
    /// (REQ-019-02, REQ-019-09).
    #[cfg(target_arch = "wasm32")]
    FolderAdded(Result<Option<crate::web::folders::Picked>, String>),
    LibraryRemoved(Result<(), String>),
    FolderPicked(Option<PathBuf>),
    /// Explorer couldn't show a file: why.
    ExplorerFailed(Option<String>),
    /// The thumbnail cache was cleared; its new size.
    CacheCleared(u64),
    /// An edit was saved (REQ-021-09, REQ-021-10), or why not.
    EditSaved(i64, Result<Written, String>),
    /// An original was put back (REQ-021-12), or why not.
    Reverted(Result<Written, String>),
    /// A rating was written into the file and the index (REQ-024-03), or why not (REQ-024-04).
    Rated(Result<(i64, String), String>),
    /// Several items were rated (REQ-025-07), or why none could be.
    RatedMany(Result<RatedMany, String>),
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
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
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
    #[cfg(not(target_arch = "wasm32"))]
    scheduler: Option<Arc<indexer::schedule::Scheduler>>,
    /// Renders thumbnails in the background; the gallery's tiles request theirs from it.
    #[cfg(not(target_arch = "wasm32"))]
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
    /// exiftool and its `gg` configuration for saving edits (spec 021); `None` without exiftool.
    #[cfg(not(target_arch = "wasm32"))]
    edit_tools: Option<media::edit::Tools>,
    /// _Revert to original_ waits for confirmation (REQ-021-12).
    pub confirm_revert: Option<RevertTarget>,
    /// Rating many items, waiting for the user to confirm (REQ-025-07).
    pub confirm_rate_many: Option<RateMany>,
    /// A saved copy to show in the viewer once it is indexed: its library and path (REQ-021-09).
    pending_copy: Option<(i64, String)>,
    /// Files whose metadata couldn't be read, for the toolbar's notice (REQ-002-16).
    pub unreadable: crate::unreadable::Unreadable,
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
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_scheduler(mut self, scheduler: Arc<indexer::schedule::Scheduler>) -> Self {
        self.scheduler = Some(scheduler);
        self
    }

    /// Renders thumbnails for as long as the app runs.
    #[cfg(not(target_arch = "wasm32"))]
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
            mtime: self.settings.mtime_on_save,
            subject: request.subject,
            sidecar_orientation: request.sidecar_orientation,
        };
        let (db, cache) = (self.db.clone(), self.thumbnail_cache());
        let (root_id, copy) = (request.root_id, request.copy);
        self.spawn(move || {
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
        self.spawn(move || {
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
        self.spawn(move || {
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

    /// Rates several items, after the user confirmed if there are many (REQ-025-07).
    pub fn ask_rate_many(&mut self, rate: RateMany) {
        if rate.ids.len() > CONFIRM_RATING_ABOVE {
            self.confirm_rate_many = Some(rate);
        } else {
            self.rate_many(rate);
        }
    }

    /// Writes a rating into the files of several items in the background, one after another, then into the index;
    /// files that can't be written are counted and the first one named (REQ-025-07, REQ-024-04).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn rate_many(&mut self, rate: RateMany) {
        let (Some(exiftool), Some(db)) = (
            self.edit_tools.as_ref().map(|tools| tools.exiftool.clone()),
            self.db.clone(),
        ) else {
            self.toasts
                .error("Couldn't save the ratings: exiftool, which writes them, wasn't found.");
            return;
        };
        let count = format::count(rate.ids.len() as u64);
        self.toasts.success(format!("Rating {count} items…"));
        self.spawn(move || {
            let files = db::media::files_of(&lock(&db), &rate.ids).map_err(|error| error.to_string());
            Event::RatedMany(files.map(|files| {
                let mut done = RatedMany {
                    rated: Vec::new(),
                    failed: 0,
                    first_failure: None,
                };
                for file in files {
                    let written = media::rating::write(
                        &exiftool,
                        std::path::Path::new(&file.root),
                        &file.rel_path,
                        file.kind,
                        rate.rating,
                    );
                    match written {
                        Ok(()) => {
                            if let Err(error) = db::media::set_rating(&lock(&db), file.id, rate.rating) {
                                tracing::error!("can't store the rating of {}: {error}", file.rel_path);
                            }
                            done.rated.push((file.root_id, file.rel_path));
                        }
                        Err(error) => {
                            tracing::error!("can't rate {}: {error}", file.rel_path);
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
    pub fn rate_many(&mut self, _rate: RateMany) {}

    #[cfg(not(target_arch = "wasm32"))]
    fn thumbnail_cache(&self) -> Option<indexer::thumb_cache::ThumbCache> {
        self.thumbnails.as_ref().map(|service| service.cache().clone())
    }

    /// After a file was written: the index reads it again, the viewer shows it anew (REQ-021-10, REQ-021-12).
    fn written(&mut self, written: &Written) {
        self.scans
            .request_paths(written.root_id, vec![written.rel_path.clone()]);
        if let Some(originals) = &mut self.originals {
            originals.forget(written.media);
        }
        if written.copy {
            self.pending_copy = Some((written.root_id, written.rel_path.clone()));
        }
        self.refresh();
    }

    /// Shows a saved copy in the viewer once the index has it (REQ-021-09).
    fn show_pending_copy(&mut self) {
        let (Some((root_id, rel_path)), Some(db)) = (&self.pending_copy, &self.db) else {
            return;
        };
        let Ok(Some(id)) = db::media::id_of(&lock(db), *root_id, rel_path) else {
            return;
        };
        self.pending_copy = None;
        if let Some(viewer) = &mut self.viewer {
            viewer.show_when_listed(id);
        }
    }

    /// The index couldn't be opened: the app shows `error` instead of the gallery.
    pub fn failed(error: String) -> Self {
        let mut state = Self::empty(Arc::new(indexer::NoScans), Arc::new(NoFolderPicker));
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
            #[cfg(not(target_arch = "wasm32"))]
            scheduler: None,
            #[cfg(not(target_arch = "wasm32"))]
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
            #[cfg(not(target_arch = "wasm32"))]
            edit_tools: None,
            confirm_revert: None,
            confirm_rate_many: None,
            pending_copy: None,
            unreadable: Default::default(),
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
                let unreadable = db::media::unreadable(conn, crate::unreadable::LISTED).map_err(|e| e.to_string())?;
                Ok((libraries, count, unreadable))
            });
        match result {
            Ok((libraries, count, (unreadable, files))) => {
                (self.libraries, self.media_count) = (libraries, count);
                self.unreadable.load(unreadable, files, &self.libraries);
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
    #[cfg(not(target_arch = "wasm32"))]
    fn spawn(&mut self, job: impl FnOnce() -> Event + Send + 'static) {
        let tx = self.events.0.clone();
        self.pending_jobs += 1;
        std::thread::spawn(move || {
            let event = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).unwrap_or(Event::Failed);
            let _ = tx.send(event);
        });
    }

    /// The browser has no threads: runs `job` now; its event applies with the next [`Self::poll`].
    #[cfg(target_arch = "wasm32")]
    fn spawn(&mut self, job: impl FnOnce() -> Event + Send + 'static) {
        self.pending_jobs += 1;
        let _ = self.events.0.send(job());
    }

    pub fn is_busy(&self) -> bool {
        self.pending_jobs > 0
    }

    /// Adds the library at `path` in the background (the reachability check may take seconds).
    #[cfg(not(target_arch = "wasm32"))]
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

    /// The browser has no paths to type (REQ-019-13).
    #[cfg(target_arch = "wasm32")]
    pub fn add_library(&mut self, _path: String) {}

    /// Opens the folder dialog; the picked folder is then added.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn browse_for_library(&mut self) {
        let picker = self.picker.clone();
        self.settings_page.picking = true;
        self.spawn(move || Event::FolderPicked(picker.pick_folder()));
    }

    /// Opens the browser's folder picker; the picked folder becomes a library and is scanned (REQ-019-02).
    #[cfg(target_arch = "wasm32")]
    pub fn browse_for_library(&mut self) {
        let Some(db) = self.db.clone() else { return };
        let (tx, scans) = (self.events.0.clone(), self.scans.clone());
        self.settings_page.picking = true;
        self.pending_jobs += 1;
        crate::web::folders::pick_folder(db, scans, move |added| {
            let _ = tx.send(Event::FolderAdded(added));
        });
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

    /// Reads unreadable files again right away: a changes scan of their paths per library (REQ-002-17).
    pub fn read_again(&mut self, asked: crate::unreadable::ReadAgain) {
        let files: Vec<db::media::UnreadableFile> = match asked {
            crate::unreadable::ReadAgain::Files(ids) => self
                .unreadable
                .rows
                .iter()
                .filter(|row| ids.contains(&row.file.id))
                .map(|row| row.file.clone())
                .collect(),
            // Listed or not.
            crate::unreadable::ReadAgain::All => {
                let Some(db) = &self.db else { return };
                match db::media::unreadable(&lock(db), usize::MAX) {
                    Ok((_, files)) => files,
                    Err(error) => return self.toasts.error(error.to_string()),
                }
            }
        };
        let mut by_library: Vec<(i64, Vec<String>)> = Vec::new();
        for file in &files {
            match by_library.iter_mut().find(|(root, _)| *root == file.root_id) {
                Some((_, paths)) => paths.push(file.rel_path.clone()),
                None => by_library.push((file.root_id, vec![file.rel_path.clone()])),
            }
        }
        for (library, paths) in by_library {
            self.scans.request_paths(library, paths);
        }
        self.unreadable
            .start_reading(files.iter().map(|file| (file.id, file.root_id)));
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
            IndexerEvent::Done(library, result) => {
                self.unreadable.done(library);
                if let indexer::ScanResult::Failed(error) = result {
                    self.toasts.error(format!("Indexing failed: {error}"));
                }
                self.written_since_refresh = false;
                self.last_written_refresh = None;
                self.refresh();
                self.show_pending_copy();
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
                // Its thumbnails are gone (or going): show the cache's new size soon, not in 10 s.
                self.usage_read_at = None;
                self.usage_fast_until = Some(Instant::now() + USAGE_REFRESH);
                self.refresh();
            }
            #[cfg(target_arch = "wasm32")]
            Event::FolderAdded(result) => {
                self.settings_page.picking = false;
                match result {
                    Ok(Some(picked)) => {
                        let label = &picked.library.label;
                        self.toasts.success(if picked.added {
                            format!("Added {label}, indexing…")
                        } else {
                            format!("{label} is readable again, checking for changes…")
                        });
                        self.refresh();
                    }
                    Ok(None) => {}
                    Err(error) => self.toasts.error(error),
                }
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
            Event::EditSaved(media, result) => {
                let saved = result.is_ok();
                match result {
                    Ok(written) => {
                        self.toasts.success(if written.copy { "Saved a copy" } else { "Saved" });
                        self.written(&written);
                    }
                    Err(error) => self.toasts.error(format!("Couldn't save: {error}")),
                }
                if let Some(viewer) = &mut self.viewer {
                    viewer.edit_saved(media, saved);
                }
            }
            Event::Reverted(result) => match result {
                Ok(written) => {
                    self.toasts.success("Reverted to the original");
                    self.written(&written);
                }
                Err(error) => self.toasts.error(format!("Couldn't revert: {error}")),
            },
            Event::Rated(result) => match result {
                Ok((root_id, rel_path)) => {
                    // A new sidecar is noted on the next read; the rating read then is the same.
                    self.scans.request_paths(root_id, vec![rel_path]);
                    self.refresh();
                }
                Err(error) => self.toasts.error(format!("Couldn't save the rating: {error}")),
            },
            Event::RatedMany(result) => match result {
                Ok(done) => {
                    let mut by_library: std::collections::BTreeMap<i64, Vec<String>> = Default::default();
                    for (root_id, rel_path) in &done.rated {
                        by_library.entry(*root_id).or_default().push(rel_path.clone());
                    }
                    for (root_id, paths) in by_library {
                        self.scans.request_paths(root_id, paths);
                    }
                    self.refresh();
                    let rated = format::count(done.rated.len() as u64);
                    match done.first_failure {
                        None => self.toasts.success(format!("Rated {rated} items")),
                        Some((name, error)) => self.toasts.error(format!(
                            "Couldn't rate {} of {} items. {name}: {error}",
                            format::count(done.failed as u64),
                            format::count((done.failed + done.rated.len()) as u64),
                        )),
                    }
                }
                Err(error) => self.toasts.error(format!("Couldn't save the ratings: {error}")),
            },
            Event::Failed => {
                if let Some(viewer) = &mut self.viewer
                    && let Some(media) = viewer.editing().filter(|session| session.saving).map(|s| s.media())
                {
                    viewer.edit_saved(media, false);
                }
                self.settings_page.adding = false;
                self.settings_page.picking = false;
                self.settings_page.clearing = false;
                self.toasts.error("Something went wrong. Details are in the log.");
                self.refresh();
            }
        }
    }
}

/// Marks a file the app rewrote, so the index reads it again, and deletes the thumbnails of its old version: with the
/// date modified kept, a same-sized file would keep its cache key (REQ-021-15).
#[cfg(not(target_arch = "wasm32"))]
fn forget_index_entry(
    db: Option<&Mutex<Connection>>,
    cache: Option<&indexer::thumb_cache::ThumbCache>,
    root_id: i64,
    rel_path: &str,
) {
    let Some(db) = db else { return };
    let key = match db::media::mark_rewritten(&lock(db), root_id, rel_path) {
        Ok(key) => key,
        Err(error) => {
            tracing::error!("can't mark {rel_path} for reading again: {error}");
            None
        }
    };
    if let (Some(key), Some(cache)) = (key, cache) {
        for width in domain::thumbs::THUMB_WIDTHS {
            let _ = std::fs::remove_file(cache.path(&key, width));
        }
    }
}

/// The index connection. A job that panicked while holding it poisons the mutex; SQLite rolled its transaction back,
/// so the connection is still usable.
fn lock(db: &Mutex<Connection>) -> std::sync::MutexGuard<'_, Connection> {
    db.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
