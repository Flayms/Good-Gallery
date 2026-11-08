//! App state and the background jobs that feed it.

#[cfg(not(target_arch = "wasm32"))]
pub mod content;
#[cfg(not(target_arch = "wasm32"))]
pub mod faces;

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
    /// Groups of unknown faces (REQ-032-15).
    People,
    /// The world map of the photos with a position (spec 044).
    Map,
}

/// The native folder dialog, or a fake in tests. Blocks until the user picks or cancels; runs on a job thread.
pub trait FolderPicker: Send + Sync {
    fn pick_folder(&self) -> Option<PathBuf>;

    /// An ONNX model file (REQ-032-03).
    fn pick_model_file(&self) -> Option<PathBuf> {
        None
    }

    /// A `.cube` LUT (REQ-043-01).
    fn pick_lut_file(&self) -> Option<PathBuf> {
        None
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub struct NativeFolderPicker;

#[cfg(not(target_arch = "wasm32"))]
impl FolderPicker for NativeFolderPicker {
    fn pick_folder(&self) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title("Add a library").pick_folder()
    }

    fn pick_model_file(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Add a face model")
            .add_filter("ONNX model", &["onnx"])
            .pick_file()
    }

    fn pick_lut_file(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Import a LUT")
            .add_filter("Cube LUT", &["cube"])
            .pick_file()
    }
}

/// Explorer, or a fake in tests (REQ-009-10).
pub trait FileManager: Send + Sync {
    /// Opens the folder of `file` with the file selected.
    fn show_in_folder(&self, file: &std::path::Path) -> std::io::Result<()>;
    /// Opens `url` in the default browser (REQ-010-07).
    fn open_in_browser(&self, url: &str) -> std::io::Result<()>;
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

    #[cfg(not(target_arch = "wasm32"))]
    fn open_in_browser(&self, url: &str) -> std::io::Result<()> {
        media::helpers::open_in_browser(url)
    }

    /// The browser has no file manager to open (REQ-019-13).
    #[cfg(target_arch = "wasm32")]
    fn show_in_folder(&self, _file: &std::path::Path) -> std::io::Result<()> {
        Err(std::io::ErrorKind::Unsupported.into())
    }

    /// The browser version opens links through egui (`settings_page::about`).
    #[cfg(target_arch = "wasm32")]
    fn open_in_browser(&self, _url: &str) -> std::io::Result<()> {
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

/// How long a window started with a file waits for the viewer before it shows the gallery (REQ-027-07).
const OPEN_AT_START_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Changing more items than this asks first (REQ-025-07, REQ-026-06).
pub const CONFIRM_MANY_ABOVE: usize = 100;

/// What changing several items did (REQ-025-07, REQ-026-06).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Debug)]
struct ChangedMany {
    what: Many,
    /// The files changed, by library: read again on the next changes scan.
    changed: Vec<(i64, String)>,
    /// The media items changed.
    media: Vec<i64>,
    failed: usize,
    /// The first file that couldn't be changed, and why.
    first_failure: Option<(String, String)>,
}

/// What the user decided about a face (spec 032).
#[derive(Debug, Clone, PartialEq)]
pub enum FaceAction {
    /// This person is at these places (media item and rectangle): one face, a drawn one, or a group's.
    Name {
        name: String,
        places: Vec<(i64, [f32; 4])>,
    },
    NotThisPerson {
        face: i64,
        name: String,
    },
    Ignore {
        faces: Vec<i64>,
    },
    /// Out of the groups of unknown faces, still unknown (REQ-032-25).
    Ungroup {
        face: i64,
    },
    NotAFace {
        faces: Vec<i64>,
    },
    /// An ignored face or a non-face back among the unknown faces (REQ-032-30).
    Reopen {
        faces: Vec<i64>,
    },
    /// Forget a face until its photo is looked at again (REQ-032-30).
    Delete {
        face: i64,
    },
    Remove {
        media: i64,
        name: String,
        rect: [f32; 4],
    },
    Rename {
        media: i64,
        from: String,
        to: String,
        rect: [f32; 4],
    },
    /// Look at this photo again (REQ-032-22).
    LookAgain {
        media: i64,
    },
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

/// The longest side of a photo copied to the clipboard, in pixels: a 45 MP photo fits; larger ones are scaled down
/// (REQ-009-25).
#[cfg(not(target_arch = "wasm32"))]
const COPY_MAX_SIDE: u32 = 8192;

/// Results of background jobs, applied on the UI thread. The browser has no background threads or folder dialog, so
/// some never happen there.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum Event {
    /// A LUT the user picked was imported, or why not (REQ-043-01); `None` when they cancelled.
    LutImported(Option<Result<crate::luts::LutEntry, String>>),
    #[cfg(not(target_arch = "wasm32"))]
    FaceModelArrived {
        key: String,
        name: String,
        result: Result<media::face_store::StoredModel, media::face_store::StoreError>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    FaceModelChecked(bool),
    #[cfg(not(target_arch = "wasm32"))]
    ContentModelArrived {
        model: &'static domain::content::ContentModel,
        result: Result<(), media::face_store::StoreError>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    FaceRegionsWritten {
        written: Vec<(i64, String)>,
        failed: Option<String>,
    },
    /// The latest published version, or `None` if the check failed (REQ-031-03).
    UpdateChecked(Option<domain::update::Version>),
    /// The installer runs, or why it doesn't (REQ-031-05, REQ-031-06).
    UpdateInstalled(Result<(), String>),
    LibraryAdded(Result<Library, String>),
    /// The browser's folder picker closed: the library it added or made readable again, or `None` when cancelled
    /// (REQ-019-02, REQ-019-09).
    #[cfg(target_arch = "wasm32")]
    FolderAdded(Result<Option<crate::web::folders::Picked>, String>),
    LibraryRemoved(Result<(), String>),
    FolderPicked(Option<PathBuf>),
    /// Explorer couldn't show a file: why.
    ExplorerFailed(Option<String>),
    /// A photo decoded for the clipboard, or why it couldn't be (REQ-009-25).
    #[cfg(not(target_arch = "wasm32"))]
    ImageCopied(Result<eframe::egui::ColorImage, String>),
    /// The thumbnail cache was cleared; its new size.
    CacheCleared(u64),
    /// An edit was saved (REQ-021-09, REQ-021-10), or why not.
    EditSaved(i64, Result<Written, String>),
    /// An original was put back (REQ-021-12), or why not.
    Reverted(Result<Written, String>),
    /// A photo's earlier versions were removed (REQ-021-26), or why not.
    HistoryCleared(Result<Written, String>),
    /// A rating was written into the file and the index (REQ-024-03), or why not (REQ-024-04).
    Rated(Result<(i64, String), String>),
    /// Files were renamed (spec 035), or why not.
    #[cfg(not(target_arch = "wasm32"))]
    Renamed(Result<crate::rename::RenameDone, String>),
    /// Dates taken were fixed or taken back (spec 038), or why not.
    #[cfg(not(target_arch = "wasm32"))]
    DatesFixed(Result<crate::fix_dates::DatesDone, String>),
    /// A tag was written into the file (REQ-026-03), or why not (REQ-026-04).
    Tagged(Result<(i64, String), String>),
    /// Several items were rated or tagged (REQ-025-07, REQ-026-06), or why none could be.
    ChangedMany(Result<ChangedMany, String>),
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
    /// Face models and the search for faces (spec 032).
    #[cfg(not(target_arch = "wasm32"))]
    pub faces: faces::Faces,
    /// Content search (spec 036).
    #[cfg(not(target_arch = "wasm32"))]
    pub content: content::Content,
    /// Wakes the UI from a background job's progress.
    #[cfg(not(target_arch = "wasm32"))]
    repaint: Option<eframe::egui::Context>,
    /// The write connection. `None` if the index couldn't be opened; `error` says why.
    pub(crate) db: Option<Arc<Mutex<Connection>>>,
    pub error: Option<String>,
    pub libraries: Vec<Library>,
    pub media_count: i64,
    scans: Arc<dyn ScanRequests>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    picker: Arc<dyn FolderPicker>,
    /// The LUTs the user imported, for editing (spec 043).
    pub luts: Arc<crate::luts::Luts>,
    events: (Sender<Event>, Receiver<Event>),
    pending_jobs: usize,
    /// A photo decoded for the clipboard, put there in the next frame (REQ-009-25).
    copied: Option<eframe::egui::ColorImage>,
    /// The writer thread's queue: every write into a library file, one after another (REQ-024-07). Started with
    /// the first write.
    #[cfg(not(target_arch = "wasm32"))]
    writer: Option<Sender<Box<dyn FnOnce() + Send>>>,
    pub settings_page: SettingsPage,
    pub people_page: crate::people_page::PeoplePage,
    pub toasts: Toasts,
    /// The rename dialog, while open (spec 035).
    #[cfg(not(target_arch = "wasm32"))]
    pub rename_dialog: Option<crate::rename::RenameDialog>,
    /// Where renames are journaled (REQ-035-08, REQ-035-09); renaming is off without it.
    #[cfg(not(target_arch = "wasm32"))]
    rename_journal: Option<PathBuf>,
    #[cfg(not(target_arch = "wasm32"))]
    pub date_dialog: Option<crate::fix_dates::FixDatesDialog>,
    /// Where date fixes are journaled (REQ-038-09); fixing dates is off without it.
    #[cfg(not(target_arch = "wasm32"))]
    date_journal: Option<PathBuf>,
    /// exiftool processes that read dates for the date fix dialog, started when it first opens.
    #[cfg(not(target_arch = "wasm32"))]
    date_reader: Option<Arc<media::exiftool::ExifTool>>,
    /// Moves a deleted file to the Recycle Bin (REQ-009-23); tests put their own in its place.
    #[cfg(not(target_arch = "wasm32"))]
    trash: media::delete::Remove,
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
    /// A newer version, if there is one (spec 031).
    pub updates: crate::updates::Updates,
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
    /// _Clear version history_ waits for confirmation (REQ-021-26).
    pub confirm_clear_history: Option<ClearHistory>,
    /// Rating many items, waiting for the user to confirm (REQ-025-07).
    pub confirm_many: Option<ChangeMany>,
    /// The day the counts and the gallery were last worked out for: a new one moves relative dates (REQ-039-04).
    today: Option<chrono::NaiveDate>,
    /// A saved copy to show in the viewer once it is indexed: its library and path (REQ-021-09).
    pending_copy: Option<(i64, String)>,
    /// Files whose metadata couldn't be read, for the toolbar's notice (REQ-002-16).
    pub unreadable: crate::unreadable::Unreadable,
    /// Files later starts of the app ask it to show (REQ-027-02).
    #[cfg(not(target_arch = "wasm32"))]
    open_requests: Option<Receiver<crate::instance::Request>>,
    /// A later start asked for the window: it comes to the front in the next frame (REQ-027-02).
    pub bring_to_front: bool,
    /// A file opened with the app, shown in the viewer once the gallery lists it (REQ-027-01).
    pending_open: Option<PendingOpen>,
    /// The window has drawn a frame.
    window_shown: bool,
    /// A group's photos from _Unconfirmed faces_ in the viewer, and the search to go back to (REQ-032-26).
    face_photos: Option<FacePhotos>,
    /// The installed Windows app's _Open with_ entries, and whether they are on (REQ-027-06).
    open_with: Option<(Arc<dyn crate::open_with::OpenWithEntries>, bool)>,
    /// A file whose folder is being added as a library from the viewer, opened once the library is there (REQ-033-04).
    #[cfg(not(target_arch = "wasm32"))]
    open_when_added: Option<PathBuf>,
    /// Where files outside every library are read into scratch indexes (spec 033).
    #[cfg(not(target_arch = "wasm32"))]
    loose_files: Option<crate::loose::LooseFiles>,
    /// A file outside every library, shown from its scratch index (spec 033).
    #[cfg(not(target_arch = "wasm32"))]
    pub loose: Option<crate::loose::Loose>,
}

/// A group's photos shown in the viewer from _Unconfirmed faces_ (REQ-032-26).
struct FacePhotos {
    /// The photo to open on, until the viewer is open.
    at: Option<i64>,
    /// The gallery's search before.
    back: domain::search::GallerySearch,
}

/// A file opened with the app (REQ-027-01): where it is, and its media item once the index has it.
#[derive(Debug, Clone)]
struct PendingOpen {
    library: i64,
    rel_path: String,
    id: Option<i64>,
    /// Opened before the window first showed, then: nothing else shows until the viewer does (REQ-027-07).
    at_start: Option<Instant>,
    /// A changes scan of the file was asked for, since the index didn't have it.
    scanned: bool,
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
        if old.check_for_updates != settings.check_for_updates {
            self.updates.enabled_changed();
        }
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

    /// Copies a photo to the clipboard, decoded on a thread of its own as the viewer shows it (REQ-009-25).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn copy_image(&mut self, file: PathBuf, sidecar_orientation: Option<u8>) {
        self.spawn(move || {
            let ffmpeg = media::ffmpeg::find_ffmpeg();
            let image = media::original::load_original(&file, ffmpeg.as_deref(), COPY_MAX_SIDE, sidecar_orientation);
            Event::ImageCopied(match image {
                Ok(image) => Ok(eframe::egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                )),
                Err(error) => {
                    tracing::error!("can't copy {}: {error}", file.display());
                    Err(format!("Couldn't copy the photo: {error}"))
                }
            })
        });
    }

    /// The photo to put on the clipboard this frame, if one was decoded (REQ-009-25).
    pub fn take_copied(&mut self) -> Option<eframe::egui::ColorImage> {
        self.copied.take()
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

    /// Opens `url` in the default browser, or says why it can't (REQ-010-07).
    pub fn open_in_browser(&mut self, url: impl Into<String>) {
        let file_manager = self.file_manager.clone();
        let url = url.into();
        self.spawn(move || {
            Event::ExplorerFailed(file_manager.open_in_browser(&url).err().map(|error| {
                tracing::error!("can't open {url}: {error}");
                format!("Couldn't open the browser: {error}")
            }))
        });
    }

    /// Checks for updates through `updater` (REQ-031-01).
    pub fn with_updater(mut self, updater: Arc<dyn crate::updates::Updater>) -> Self {
        self.updates = crate::updates::Updates::new(updater);
        self
    }

    /// _Faces settings_ in the menu of what's running (REQ-032-20).
    pub fn open_face_settings(&mut self) {
        self.page = Page::Settings;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.faces.reveal = true;
        }
    }

    /// Asks for the latest version when the app starts, without blocking (REQ-031-01).
    fn check_for_updates(&mut self) {
        if self.updates.start_check(self.settings.check_for_updates) {
            self.spawn_update_check();
        }
    }

    /// Asks for the latest version because the user said so (REQ-031-11).
    pub fn check_for_updates_now(&mut self) {
        if self.updates.ask_check() {
            self.spawn_update_check();
        }
    }

    fn spawn_update_check(&mut self) {
        let Some(updater) = self.updates.updater() else { return };
        self.spawn(move || {
            Event::UpdateChecked(
                updater
                    .latest()
                    .map_err(|error| tracing::info!("update check: {error}"))
                    .ok(),
            )
        });
    }

    /// The user asked to see the update, or to install it (REQ-031-04, REQ-031-05).
    pub fn answer_update(&mut self, asked: crate::updates::Asked, ctx: &eframe::egui::Context) {
        use crate::updates::{Asked, Phase};
        let Phase::Available(version) = self.updates.phase else {
            return;
        };
        match asked {
            Asked::SeeWhatsNew if crate::WEB => ctx.open_url(eframe::egui::OpenUrl::new_tab(version.release_url())),
            Asked::SeeWhatsNew => self.open_in_browser(version.release_url()),
            Asked::Install => {
                let Some(updater) = self.updates.updater() else { return };
                self.updates.phase = Phase::Installing(version);
                self.spawn(move || Event::UpdateInstalled(updater.install(&version)));
            }
        }
    }

    /// The file of the media item at gallery position `index`, if its data is loaded.
    pub fn file_at(&mut self, index: usize) -> Option<PathBuf> {
        let item = self.gallery.as_mut()?.item(index)?;
        db::media::file_in(&self.libraries, item.root_id, &item.rel_path)
    }

    /// Wakes the UI when a download gets further.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_repaint(mut self, ctx: eframe::egui::Context) -> Self {
        self.repaint = Some(ctx);
        self
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
    /// A new day refreshes the sidebar's counts and the gallery, so smart albums with relative dates follow
    /// (REQ-039-04).
    pub fn notice_new_day(&mut self) {
        let today = domain::time_zone::today();
        if self.today == Some(today) {
            return;
        }
        let first = self.today.is_none();
        self.today = Some(today);
        if first {
            return;
        }
        if let Some(gallery) = &mut self.gallery {
            gallery.reload_sidebar();
            if gallery.search().rules.as_ref().is_some_and(|rules| rules.is_relative()) {
                gallery.reload();
            }
        }
    }

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

    /// The browser version looks for no faces (spec 032).
    #[cfg(target_arch = "wasm32")]
    pub fn face_action(&mut self, _action: FaceAction) {}

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

    /// Offers _Open with_ in the settings, for the installed Windows app (REQ-027-06).
    pub fn with_open_with(mut self, entries: Arc<dyn crate::open_with::OpenWithEntries>) -> Self {
        let on = entries.is_on();
        self.open_with = Some((entries, on));
        self
    }

    /// Whether the app is offered under _Open with_; `None` where the settings don't offer the switch (REQ-027-06).
    pub fn open_with(&self) -> Option<bool> {
        self.open_with.as_ref().map(|(_, on)| *on)
    }

    /// Adds or removes the _Open with_ entries, or says why it couldn't (REQ-027-06).
    pub fn set_open_with(&mut self, on: bool) {
        let Some((entries, shown)) = &mut self.open_with else {
            return;
        };
        match entries.set(on) {
            Ok(()) => *shown = on,
            Err(error) => {
                tracing::error!("can't change Open with: {error}");
                self.toasts.error(format!("Couldn't change \"Open with\": {error}"));
            }
        }
    }

    /// Shows files outside every library from scratch indexes in `scratch`, reading their metadata with `metadata`
    /// (spec 033).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_loose_files(mut self, scratch: PathBuf, metadata: Arc<dyn media::metadata::MetadataSource>) -> Self {
        self.loose_files = Some(crate::loose::LooseFiles { scratch, metadata });
        self
    }

    /// Takes the files later starts of the app hand over (REQ-027-02).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_open_requests(mut self, requests: Receiver<crate::instance::Request>) -> Self {
        self.open_requests = Some(requests);
        self
    }

    /// Shows a file opened with the app (REQ-027-01): in the viewer if it is in a library, after indexing it if the
    /// index doesn't have it yet; outside every library from a scratch index (REQ-033-01); anything else is an error
    /// message (REQ-027-04).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_file(&mut self, file: PathBuf) {
        let name = file.file_name().map_or_else(
            || file.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let problem = match std::fs::metadata(&file) {
            Err(_) => Some("it doesn't exist"),
            Ok(metadata) if !metadata.is_file() => Some("it isn't a file"),
            Ok(_) if domain::media::media_kind(&name).is_none() => Some("it isn't a photo or video Good Gallery shows"),
            Ok(_) => None,
        };
        if let Some(problem) = problem {
            self.toasts.error(format!("Couldn't open {name}: {problem}"));
            return;
        }
        let path = file.to_string_lossy();
        let found = self
            .libraries
            .iter()
            .filter_map(|library| {
                let rel_path = domain::media::rel_path_in(&library.path, &path, cfg!(windows))?;
                Some((library.path.len(), library.id, rel_path))
            })
            .max();
        let Some((_, library, rel_path)) = found else {
            self.open_loose(file);
            return;
        };
        self.page = Page::Gallery;
        // Its folder was just added from the viewer: it stays shown until the index has it (REQ-033-04).
        let kept = self
            .loose
            .as_ref()
            .is_some_and(|loose| loose.shown && loose.file == file);
        if !kept {
            self.viewer = None;
            self.loose = None;
        }
        self.info_sheet = None;
        if let Some(gallery) = &mut self.gallery {
            let search = domain::search::GallerySearch {
                library: Some(library),
                sort_by: gallery.search().sort_by,
                descending: gallery.search().descending,
                ..Default::default()
            };
            if search != *gallery.search() {
                gallery.set_search(search);
            }
        }
        self.pending_open = Some(PendingOpen {
            library,
            rel_path,
            id: None,
            at_start: (!self.window_shown).then(Instant::now),
            scanned: false,
        });
        self.find_pending_open();
    }

    /// Shows a file outside every library from a scratch index (REQ-033-01), or says why it can't.
    #[cfg(not(target_arch = "wasm32"))]
    fn open_loose(&mut self, file: PathBuf) {
        let name = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let (Some(files), Some(db)) = (&self.loose_files, &self.db) else {
            self.toasts
                .error(format!("Couldn't open {name}: it isn't in a library"));
            return;
        };
        match crate::loose::Loose::open(files, &file, db.clone(), self.repaint.clone()) {
            Ok(loose) => {
                self.page = Page::Gallery;
                self.viewer = None;
                self.info_sheet = None;
                self.pending_open = None;
                self.loose = Some(loose);
            }
            Err(error) => {
                tracing::error!("can't show {}: {error}", file.display());
                self.toasts.error(format!("Couldn't open {name}: {error}"));
            }
        }
    }

    /// Opens the viewer on a file outside every library once its scratch index has it (REQ-033-01).
    #[cfg(not(target_arch = "wasm32"))]
    fn show_loose(&mut self) {
        let Some(loose) = &mut self.loose else { return };
        loose.poll();
        if loose.shown {
            return;
        }
        let Some(index) = loose.position() else {
            // Read and loaded without it: the file couldn't be read (REQ-027-04).
            if !loose.is_busy() {
                let name = loose
                    .file
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                self.loose = None;
                self.toasts.error(format!("Couldn't open {name}: it couldn't be read"));
            }
            return;
        };
        if let Some(viewer) = crate::viewer::Viewer::open_loose(&loose.gallery, index) {
            self.viewer = Some(viewer);
            loose.shown = true;
        }
    }

    /// Adds the folder of the file shown from a scratch index as a library; the viewer switches to the indexed item
    /// once the library's first scan has it (REQ-033-04).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn add_loose_folder(&mut self) {
        let Some(file) = self.loose.as_ref().map(|loose| loose.file.clone()) else {
            return;
        };
        let Some(folder) = file.parent() else { return };
        self.add_library(folder.to_string_lossy().into_owned());
        self.open_when_added = Some(file);
    }

    /// The viewer closed: a file shown from a scratch index goes with it, and isn't opened later (REQ-033-05).
    pub fn viewer_closed(&mut self) {
        self.face_photos_closed();
        #[cfg(not(target_arch = "wasm32"))]
        if self.loose.take().is_some() {
            self.pending_open = None;
            self.open_when_added = None;
        }
    }

    /// Looks up the media item of the file opened with the app; asks for a scan of it ahead of the others the first
    /// time the index doesn't have it, and gives up once that scan found nothing (a skipped folder).
    fn find_pending_open(&mut self) {
        let (Some(pending), Some(db)) = (&mut self.pending_open, &self.db) else {
            return;
        };
        if pending.id.is_some() {
            return;
        }
        if let Ok(Some(id)) = db::media::id_of(&lock(db), pending.library, &pending.rel_path) {
            pending.id = Some(id);
            return;
        }
        let library = pending.library;
        if !pending.scanned {
            pending.scanned = true;
            // Ahead of other scans, e.g. the library's own first scan (REQ-002-18).
            self.scans.request_paths_first(library, vec![pending.rel_path.clone()]);
            let name = pending.rel_path.rsplit('/').next().unwrap_or_default();
            // Not while the viewer shows it already, from a scratch index (REQ-033-04).
            #[cfg(not(target_arch = "wasm32"))]
            let shown = self.loose.as_ref().is_some_and(|loose| loose.shown);
            #[cfg(target_arch = "wasm32")]
            let shown = false;
            if !shown {
                self.toasts.success(format!("{name} opens once it's indexed"));
            }
            return;
        }
        let waiting = self.indexer_status.current.is_some_and(|scan| scan.library == library)
            || self.indexer_status.queue.contains(&library);
        if !waiting {
            let rel_path = pending.rel_path.clone();
            let name = rel_path.rsplit('/').next().unwrap_or_default().to_owned();
            self.pending_open = None;
            // In a folder its library skips: shown as a file outside every library (REQ-033-06).
            #[cfg(not(target_arch = "wasm32"))]
            if self.loose_files.is_some()
                && let Some(folder) = self.libraries.iter().find(|root| root.id == library)
            {
                let file = std::path::Path::new(&folder.path).join(rel_path);
                self.open_loose(file);
                return;
            }
            self.toasts.error(format!("Couldn't open {name}: it wasn't indexed"));
        }
    }

    /// Whether the window waits for the viewer to show the file it was started with, showing nothing else
    /// (REQ-027-07). Marks the window as shown.
    pub fn waiting_to_open(&mut self) -> bool {
        self.window_shown = true;
        self.viewer.is_none()
            && self.pending_open.as_ref().is_some_and(|pending| {
                pending.id.is_some()
                    && pending
                        .at_start
                        .is_some_and(|since| since.elapsed() < OPEN_AT_START_WAIT)
            })
    }

    /// Opens the viewer on the file opened with the app once the gallery lists it (REQ-027-01). Called every frame.
    pub fn show_pending_open(&mut self) {
        let (Some(PendingOpen { id: Some(id), .. }), Some(gallery)) = (&self.pending_open, &self.gallery) else {
            return;
        };
        if gallery.is_loading() {
            return;
        }
        let Some(index) = gallery.position_of(*id) else { return };
        let at_start = self
            .pending_open
            .as_ref()
            .is_some_and(|pending| pending.at_start.is_some());
        // Started with it: once the item is there, so the viewer draws the photo in its first frame (REQ-027-07).
        let Some(gallery) = &mut self.gallery else { return };
        if at_start && gallery.item(index).is_none() {
            return;
        }
        if let Some(mut viewer) = crate::viewer::Viewer::open(gallery, index) {
            // In its place at once, not out of a tile the window never showed (REQ-027-07).
            if at_start {
                viewer.open_at_once();
            }
            self.viewer = Some(viewer);
            self.pending_open = None;
            // The indexed item takes the place of the one from a scratch index (REQ-033-04).
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.loose = None;
            }
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
            #[cfg(not(target_arch = "wasm32"))]
            faces: faces::Faces::default(),
            #[cfg(not(target_arch = "wasm32"))]
            content: content::Content::default(),
            #[cfg(not(target_arch = "wasm32"))]
            repaint: None,
            db: None,
            error: None,
            libraries: Vec::new(),
            media_count: 0,
            scans,
            picker,
            luts: Arc::default(),
            events: channel(),
            pending_jobs: 0,
            copied: None,
            #[cfg(not(target_arch = "wasm32"))]
            writer: None,
            settings_page: SettingsPage::default(),
            people_page: crate::people_page::PeoplePage::default(),
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
            updates: Default::default(),
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
            confirm_clear_history: None,
            confirm_many: None,
            today: None,
            #[cfg(not(target_arch = "wasm32"))]
            rename_dialog: None,
            #[cfg(not(target_arch = "wasm32"))]
            rename_journal: None,
            #[cfg(not(target_arch = "wasm32"))]
            date_dialog: None,
            #[cfg(not(target_arch = "wasm32"))]
            date_journal: None,
            #[cfg(not(target_arch = "wasm32"))]
            date_reader: None,
            #[cfg(not(target_arch = "wasm32"))]
            trash: media::delete::to_trash,
            pending_copy: None,
            #[cfg(not(target_arch = "wasm32"))]
            open_requests: None,
            bring_to_front: false,
            pending_open: None,
            window_shown: false,
            face_photos: None,
            open_with: None,
            #[cfg(not(target_arch = "wasm32"))]
            open_when_added: None,
            #[cfg(not(target_arch = "wasm32"))]
            loose_files: None,
            #[cfg(not(target_arch = "wasm32"))]
            loose: None,
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

    /// Moves deleted files with `trash` instead of to the Recycle Bin (REQ-009-23): tests leave the user's alone.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_trash(mut self, trash: media::delete::Remove) -> Self {
        self.trash = trash;
        self
    }

    /// Journals date fixes at `path` (REQ-038-09).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_date_journal(mut self, path: PathBuf) -> Self {
        self.date_journal = Some(path);
        self
    }

    /// Opens the date fix dialog for `ids` (REQ-038-01).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_fix_dates(&mut self, ids: Vec<i64>) {
        let (Some(db), Some(journal)) = (self.db.clone(), &self.date_journal) else {
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
            .date_reader
            .get_or_insert_with(|| Arc::new(media::exiftool::ExifTool::new(path, 2)))
            .clone();
        let undo = crate::fix_dates::can_undo(journal);
        self.date_dialog = Some(crate::fix_dates::FixDatesDialog::open(
            db,
            reader,
            &self.libraries,
            ids,
            undo,
        ));
    }

    /// Writes the dates of `job` in the background (REQ-038-08).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_fix_dates(&mut self, job: crate::fix_dates::FixJob) {
        let (Some(db), Some(journal), Some(exiftool)) = (
            self.db.clone(),
            self.date_journal.clone(),
            self.edit_tools.as_ref().map(|tools| tools.exiftool.clone()),
        ) else {
            return;
        };
        let mtime = self.settings.mtime_on_save;
        let count = job.changes.len();
        self.toasts
            .success(format!("Fixing the dates of {} photos…", format::count(count as u64)));
        self.spawn_write(move || Event::DatesFixed(crate::fix_dates::run(&db, &exiftool, &journal, &job, mtime)));
    }

    /// Takes the last date fix back (REQ-038-09).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn undo_fix_dates(&mut self) {
        let (Some(db), Some(journal), Some(exiftool)) = (
            self.db.clone(),
            self.date_journal.clone(),
            self.edit_tools.as_ref().map(|tools| tools.exiftool.clone()),
        ) else {
            return;
        };
        self.spawn_write(move || Event::DatesFixed(crate::fix_dates::undo(&db, &exiftool, &journal)));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn dates_fixed(&mut self, result: Result<crate::fix_dates::DatesDone, String>) {
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

    /// Journals renames at `path` (REQ-035-08, REQ-035-09).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_rename_journal(mut self, path: PathBuf) -> Self {
        self.rename_journal = Some(path);
        self
    }

    /// Opens the rename dialog for `ids` (REQ-035-01).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_rename(&mut self, ids: Vec<i64>) {
        let (Some(db), Some(journal)) = (self.db.clone(), &self.rename_journal) else {
            return;
        };
        if ids.is_empty() {
            return;
        }
        let undo = crate::rename::undo_job(&db, journal).is_some();
        self.rename_dialog = Some(crate::rename::RenameDialog::open(db, &self.libraries, ids, undo));
    }

    /// Renames in the background, the index first (REQ-035-06, REQ-035-07).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_rename(&mut self, job: crate::rename::RenameJob) {
        let (Some(db), Some(journal)) = (self.db.clone(), self.rename_journal.clone()) else {
            return;
        };
        let cache = self.thumbnail_cache();
        self.spawn_write(move || Event::Renamed(crate::rename::run(&db, cache.as_ref(), &journal, &job)));
    }

    /// Takes the last rename back (REQ-035-08).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn undo_rename(&mut self) {
        let (Some(db), Some(journal)) = (self.db.clone(), &self.rename_journal) else {
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
    #[cfg(not(target_arch = "wasm32"))]
    pub fn recover_rename(&mut self) {
        let (Some(db), Some(journal)) = (self.db.clone(), self.rename_journal.clone()) else {
            return;
        };
        let cache = self.thumbnail_cache();
        self.spawn_write(move || match crate::rename::recover(&db, cache.as_ref(), &journal) {
            Ok(Some(done)) => Event::Renamed(Ok(done)),
            Ok(None) => Event::Renamed(Ok(crate::rename::RenameDone::default())),
            Err(error) => Event::Renamed(Err(error)),
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn renamed(&mut self, result: Result<crate::rename::RenameDone, String>) {
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

    /// Runs `job`, a write into library files, on the writer thread after the writes queued before it, so changes to
    /// one file are written in the order they were made and never at the same time (REQ-024-07). Sends exactly one
    /// event, also when it panics.
    #[cfg(not(target_arch = "wasm32"))]
    fn spawn_write(&mut self, job: impl FnOnce() -> Event + Send + 'static) {
        let tx = self.events.0.clone();
        self.pending_jobs += 1;
        let writer = self.writer.get_or_insert_with(|| {
            let (jobs, queue) = channel::<Box<dyn FnOnce() + Send>>();
            std::thread::Builder::new()
                .name("writer".into())
                .spawn(move || queue.into_iter().for_each(|job| job()))
                .expect("can't start the writer thread");
            jobs
        });
        let _ = writer.send(Box::new(move || {
            let event = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).unwrap_or(Event::Failed);
            let _ = tx.send(event);
        }));
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
        self.check_for_updates();
        while let Ok(event) = self.events.1.try_recv() {
            self.apply(event);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let requests: Vec<_> = self.open_requests.iter().flat_map(|rx| rx.try_iter()).collect();
            for request in requests {
                self.bring_to_front = true;
                if let crate::instance::Request::Open(file) = request {
                    self.open_file(file);
                }
            }
            self.show_loose();
            // The folder added from the viewer is a library now (REQ-033-04).
            if let Some(file) = &self.open_when_added {
                let path = file.to_string_lossy();
                if self
                    .libraries
                    .iter()
                    .any(|library| domain::media::rel_path_in(&library.path, &path, cfg!(windows)).is_some())
                {
                    let file = file.clone();
                    self.open_when_added = None;
                    self.open_file(file);
                }
            }
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
                if self
                    .pending_open
                    .as_ref()
                    .is_some_and(|pending| pending.library == library)
                {
                    self.find_pending_open();
                }
            }
        }
    }

    /// Whether the UI must keep polling (jobs or a scan running).
    pub fn is_active(&self) -> bool {
        self.is_busy()
            || self.indexer_status.current.is_some()
            || self.written_since_refresh
            || self.gallery.as_ref().is_some_and(Gallery::is_loading)
            || self.pending_open.is_some()
            || self.loose_busy()
    }

    /// A file outside every library is still being read, or waits for the viewer (spec 033).
    fn loose_busy(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return self.loose.as_ref().is_some_and(|loose| loose.is_busy() || !loose.shown);
        #[cfg(target_arch = "wasm32")]
        false
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
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(loose) = &mut self.loose {
            loose.wait_filled();
        }
    }

    fn apply(&mut self, event: Event) {
        self.pending_jobs -= 1;
        match event {
            #[cfg(not(target_arch = "wasm32"))]
            Event::FaceModelArrived { key, name, result } => self.face_model_arrived(&key, &name, result),
            Event::LutImported(Some(Ok(entry))) => self.toasts.success(format!("Imported the LUT {}", entry.name)),
            Event::LutImported(Some(Err(error))) => self.toasts.error(error),
            Event::LutImported(None) => {}
            #[cfg(not(target_arch = "wasm32"))]
            Event::FaceModelChecked(wrong) => self.face_model_checked(wrong),
            #[cfg(not(target_arch = "wasm32"))]
            Event::ContentModelArrived { model, result } => self.content_model_arrived(model, result),
            #[cfg(not(target_arch = "wasm32"))]
            Event::FaceRegionsWritten { written, failed } => self.face_regions_written(written, failed),
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
            Event::UpdateChecked(latest) => self.updates.checked(latest, self.settings.check_for_updates),
            Event::UpdateInstalled(result) => {
                use crate::updates::Phase;
                match result {
                    Ok(()) => self.updates.close = true,
                    Err(error) => {
                        tracing::error!("can't install the update: {error}");
                        self.toasts.error(format!("Couldn't install the update: {error}"));
                        if let Phase::Installing(version) = self.updates.phase {
                            self.updates.phase = Phase::Available(version);
                        }
                    }
                }
            }
            Event::ExplorerFailed(error) => {
                if let Some(error) = error {
                    self.toasts.error(error);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Event::ImageCopied(result) => match result {
                Ok(image) => {
                    self.copied = Some(image);
                    self.toasts.success("Copied");
                }
                Err(error) => self.toasts.error(error),
            },
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
            Event::HistoryCleared(result) => match result {
                Ok(written) => {
                    self.toasts.success("Version history cleared");
                    self.written(&written);
                }
                Err(error) => self
                    .toasts
                    .error(format!("Couldn't clear the version history: {error}")),
            },
            #[cfg(not(target_arch = "wasm32"))]
            Event::Renamed(result) => self.renamed(result),
            #[cfg(not(target_arch = "wasm32"))]
            Event::DatesFixed(result) => self.dates_fixed(result),
            Event::Rated(result) => match result {
                Ok((root_id, rel_path)) => {
                    // A new sidecar is noted on the next read; the rating read then is the same.
                    self.scans.request_paths(root_id, vec![rel_path]);
                    self.refresh();
                }
                Err(error) => self.toasts.error(format!("Couldn't save the rating: {error}")),
            },
            Event::ChangedMany(result) => match result {
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
            },
            Event::Tagged(result) => match result {
                // The index reads the file again, which updates the tags everywhere.
                Ok((root_id, rel_path)) => self.scans.request_paths(root_id, vec![rel_path]),
                Err(error) => self.toasts.error(format!("Couldn't save the tag: {error}")),
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

/// The libraries of media items `ids`.
#[cfg(not(target_arch = "wasm32"))]
fn ids_roots(conn: &Connection, ids: &[i64]) -> std::collections::BTreeSet<i64> {
    match db::media::files_of(conn, ids) {
        Ok(files) => files.into_iter().map(|file| file.root_id).collect(),
        Err(error) => {
            tracing::error!("can't find the libraries of the items to delete: {error}");
            std::collections::BTreeSet::new()
        }
    }
}

/// The index connection. A job that panicked while holding it poisons the mutex; SQLite rolled its transaction back,
/// so the connection is still usable.
fn lock<T>(db: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    db.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl AppState {
    /// Shows `photos` in the gallery, chosen on the map, with the rest of the search (REQ-044-03).
    pub fn show_map_photos(&mut self, photos: Vec<i64>) {
        let Some(gallery) = &mut self.gallery else { return };
        let mut media = photos;
        media.sort_unstable();
        media.dedup();
        let search = domain::search::GallerySearch {
            media,
            ..gallery.search().clone()
        };
        gallery.set_search(search);
        self.page = Page::Gallery;
    }

    /// Opens the viewer on photo `at` with its faces shown, going through `photos` only; closing it brings back the
    /// gallery's search (REQ-032-26).
    pub fn show_face_photos(&mut self, photos: &[i64], at: i64) {
        let Some(gallery) = &mut self.gallery else { return };
        let back = match self.face_photos.take() {
            Some(open) => open.back,
            None => gallery.search().clone(),
        };
        let mut media: Vec<i64> = photos.to_vec();
        media.sort_unstable();
        media.dedup();
        gallery.set_search(domain::search::GallerySearch { media, ..back.clone() });
        self.face_photos = Some(FacePhotos { at: Some(at), back });
    }

    /// Opens the viewer once the gallery lists the photo (REQ-032-26). Called every frame.
    pub fn show_face_photo(&mut self) {
        let (Some(FacePhotos { at: Some(at), .. }), Some(gallery)) = (&self.face_photos, &self.gallery) else {
            return;
        };
        if gallery.is_loading() {
            return;
        }
        let at = *at;
        let viewer = gallery
            .position_of(at)
            .and_then(|index| crate::viewer::Viewer::open(gallery, index));
        if let Some(face_photos) = &mut self.face_photos {
            face_photos.at = None;
        }
        match viewer {
            Some(mut viewer) => {
                viewer.show_faces();
                self.viewer = Some(viewer);
            }
            // Gone meanwhile: back to the search as it was.
            None => self.face_photos_closed(),
        }
    }

    /// The viewer closed: the gallery's search as it was before a group's photos (REQ-032-26).
    fn face_photos_closed(&mut self) {
        if let Some(open) = self.face_photos.take()
            && let Some(gallery) = &mut self.gallery
        {
            gallery.set_search(open.back);
        }
    }
}
