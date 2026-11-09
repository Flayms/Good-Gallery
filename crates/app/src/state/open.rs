//! Files opened with the app (spec 027), from inside a library or outside every library (spec 033).

#[cfg(not(target_arch = "wasm32"))]
use super::Page;
use super::{AppState, lock};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::Receiver;
use web_time::Instant;

/// How long a window started with a file waits for the viewer before it shows the gallery (REQ-027-07).
const OPEN_AT_START_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// A file opened with the app (REQ-027-01): where it is, and its media item once the index has it.
#[derive(Debug, Clone)]
pub(super) struct PendingOpen {
    library: i64,
    rel_path: String,
    id: Option<i64>,
    /// Opened before the window first showed, then: nothing else shows until the viewer does (REQ-027-07).
    at_start: Option<Instant>,
    /// A changes scan of the file was asked for, since the index didn't have it.
    scanned: bool,
}

/// Files on their way to the viewer: opened with the app, or from outside every library.
#[derive(Default)]
pub(super) struct Opening {
    /// Files later starts of the app ask it to show (REQ-027-02).
    #[cfg(not(target_arch = "wasm32"))]
    requests: Option<Receiver<crate::instance::Request>>,
    /// A file opened with the app, shown in the viewer once the gallery lists it (REQ-027-01).
    pub(super) pending: Option<PendingOpen>,
    /// The window has drawn a frame.
    window_shown: bool,
    /// A file whose folder is being added as a library from the viewer, opened once the library is there (REQ-033-04).
    #[cfg(not(target_arch = "wasm32"))]
    when_added: Option<PathBuf>,
    /// Where files outside every library are read into scratch indexes (spec 033).
    #[cfg(not(target_arch = "wasm32"))]
    loose_files: Option<crate::loose::LooseFiles>,
}

impl AppState {
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
        self.opening.loose_files = Some(crate::loose::LooseFiles { scratch, metadata });
        self
    }

    /// Takes the files later starts of the app hand over (REQ-027-02).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_open_requests(mut self, requests: Receiver<crate::instance::Request>) -> Self {
        self.opening.requests = Some(requests);
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
        self.opening.pending = Some(PendingOpen {
            library,
            rel_path,
            id: None,
            at_start: (!self.opening.window_shown).then(Instant::now),
            scanned: false,
        });
        self.find_pending_open();
    }

    /// Shows a file outside every library from a scratch index (REQ-033-01), or says why it can't.
    #[cfg(not(target_arch = "wasm32"))]
    fn open_loose(&mut self, file: PathBuf) {
        let name = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let (Some(files), Some(db)) = (&self.opening.loose_files, &self.db) else {
            self.toasts
                .error(format!("Couldn't open {name}: it isn't in a library"));
            return;
        };
        match crate::loose::Loose::open(files, &file, db.clone(), self.repaint.clone()) {
            Ok(loose) => {
                self.page = Page::Gallery;
                self.viewer = None;
                self.info_sheet = None;
                self.opening.pending = None;
                self.loose = Some(loose);
            }
            Err(error) => {
                tracing::error!("can't show {}: {error}", file.display());
                self.toasts.error(format!("Couldn't open {name}: {error}"));
            }
        }
    }

    /// Opens the files later starts of the app hand over (REQ-027-02), shows a file from a scratch index once it is
    /// read (REQ-033-01), and the file whose folder was added from the viewer once it is a library (REQ-033-04).
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn poll_opening(&mut self) {
        let requests: Vec<_> = self.opening.requests.iter().flat_map(|rx| rx.try_iter()).collect();
        for request in requests {
            self.bring_to_front = true;
            if let crate::instance::Request::Open(file) = request {
                self.open_file(file);
            }
        }
        self.show_loose();
        // The folder added from the viewer is a library now (REQ-033-04).
        if let Some(file) = &self.opening.when_added {
            let path = file.to_string_lossy();
            if self
                .libraries
                .iter()
                .any(|library| domain::media::rel_path_in(&library.path, &path, cfg!(windows)).is_some())
            {
                let file = file.clone();
                self.opening.when_added = None;
                self.open_file(file);
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
        self.opening.when_added = Some(file);
    }

    /// The viewer closed: a file shown from a scratch index goes with it, and isn't opened later (REQ-033-05).
    pub fn viewer_closed(&mut self) {
        self.face_photos_closed();
        #[cfg(not(target_arch = "wasm32"))]
        if self.loose.take().is_some() {
            self.opening.pending = None;
            self.opening.when_added = None;
        }
    }

    /// A scan of `library` finished: the file opened with the app may be indexed now.
    pub(super) fn library_scanned(&mut self, library: i64) {
        if self
            .opening
            .pending
            .as_ref()
            .is_some_and(|pending| pending.library == library)
        {
            self.find_pending_open();
        }
    }

    /// Looks up the media item of the file opened with the app; asks for a scan of it ahead of the others the first
    /// time the index doesn't have it, and gives up once that scan found nothing (a skipped folder).
    fn find_pending_open(&mut self) {
        let (Some(pending), Some(db)) = (&mut self.opening.pending, &self.db) else {
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
            self.opening.pending = None;
            // In a folder its library skips: shown as a file outside every library (REQ-033-06).
            #[cfg(not(target_arch = "wasm32"))]
            if self.opening.loose_files.is_some()
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
        self.opening.window_shown = true;
        self.viewer.is_none()
            && self.opening.pending.as_ref().is_some_and(|pending| {
                pending.id.is_some()
                    && pending
                        .at_start
                        .is_some_and(|since| since.elapsed() < OPEN_AT_START_WAIT)
            })
    }

    /// Opens the viewer on the file opened with the app once the gallery lists it (REQ-027-01). Called every frame.
    pub fn show_pending_open(&mut self) {
        let (Some(PendingOpen { id: Some(id), .. }), Some(gallery)) = (&self.opening.pending, &self.gallery) else {
            return;
        };
        if gallery.is_loading() {
            return;
        }
        let Some(index) = gallery.position_of(*id) else { return };
        let at_start = self
            .opening
            .pending
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
            self.opening.pending = None;
            // The indexed item takes the place of the one from a scratch index (REQ-033-04).
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.loose = None;
            }
        }
    }
}
