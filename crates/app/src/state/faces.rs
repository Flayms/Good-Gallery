//! The app's side of faces (spec 032): the models on this computer, downloads, the background search, and what the user
//! decides about a face. Desktop only; the browser version looks for no faces.

use super::{AppState, Event, lock};
use db::faces::{self, FaceState};
use domain::face_models::{CatalogModel, DetectorFormat, InputStyle, ModelKind};
use media::face_models::{Detector, Recognizer};
use media::face_regions::RegionChange;
use media::face_store::{FaceStore, StoreError, StoredModel};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A download or an added model on its way.
#[derive(Clone)]
pub struct Arriving {
    pub bytes: u64,
    pub total: Option<u64>,
    cancel: Arc<AtomicBool>,
}

/// What the _Add a model_ form says the model is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AddedKind {
    Yunet,
    RetinaFace,
    #[default]
    ArcFaceStyle,
    SFaceStyle,
}

impl AddedKind {
    pub const ALL: [Self; 4] = [Self::ArcFaceStyle, Self::SFaceStyle, Self::Yunet, Self::RetinaFace];

    pub fn label(self) -> &'static str {
        match self {
            Self::Yunet => "Detection: YuNet format",
            Self::RetinaFace => "Detection: RetinaFace format",
            Self::ArcFaceStyle => "Recognition: ArcFace style",
            Self::SFaceStyle => "Recognition: SFace style",
        }
    }

    fn kind(self) -> ModelKind {
        match self {
            Self::Yunet => ModelKind::Detect(DetectorFormat::YuNet),
            Self::RetinaFace => ModelKind::Detect(DetectorFormat::RetinaFace),
            Self::ArcFaceStyle => ModelKind::Recognize {
                style: InputStyle::ArcFace,
                threshold: domain::face_models::DEFAULT_THRESHOLD,
            },
            Self::SFaceStyle => ModelKind::Recognize {
                style: InputStyle::SFace,
                threshold: domain::face_models::DEFAULT_THRESHOLD,
            },
        }
    }
}

/// How often the check for a wrongly set up model runs while the settings are open (REQ-032-17).
const MODEL_CHECK_EVERY: Duration = Duration::from_secs(30);

/// Faces' part of the app's state.
#[derive(Default)]
pub struct Faces {
    store: Option<Arc<FaceStore>>,
    service: Option<Arc<indexer::faces::FaceService>>,
    pub models: Vec<StoredModel>,
    pub detector: Option<String>,
    pub recognizer: Option<String>,
    pub paused: bool,
    /// By the catalog model's hash, or `"added"` for one being added.
    arriving: Arc<Mutex<HashMap<String, Arriving>>>,
    pub link: String,
    pub added_kind: AddedKind,
    pub confirm_delete: bool,
    pub model_seems_wrong: bool,
    model_checked: Option<Instant>,
}

impl Faces {
    /// What's on its way, under its hash.
    pub fn arriving(&self) -> HashMap<String, Arriving> {
        lock(&self.arriving).clone()
    }

    pub fn status(&self) -> Option<indexer::faces::FaceStatus> {
        self.service.as_ref().map(|service| service.status())
    }

    pub fn enabled(&self) -> bool {
        self.store.is_some()
    }
}

/// Keeps the name out of what would break a metadata field.
pub fn clean_name(name: &str) -> Option<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    (!name.is_empty()).then_some(name)
}

impl AppState {
    /// Connects the model store and the search; without them the settings show no models.
    pub fn with_faces(mut self, data_folder: &Path, service: Option<Arc<indexer::faces::FaceService>>) -> Self {
        let store = FaceStore::in_data_folder(data_folder);
        self.faces.models = store.models();
        self.faces.store = Some(Arc::new(store));
        self.faces.service = service;
        if let Some(db) = &self.db {
            let db = lock(db);
            self.faces.detector = faces::chosen(&db, faces::DETECTOR_KEY).ok().flatten();
            self.faces.recognizer = faces::chosen(&db, faces::RECOGNIZER_KEY).ok().flatten();
            self.faces.paused = faces::chosen(&db, faces::SEARCH_KEY).ok().flatten().as_deref() == Some("paused");
        }
        self
    }

    fn kick_faces(&self) {
        if let Some(service) = &self.faces.service {
            service.kick();
        }
    }

    fn store_face_setting(&self, key: &str, value: Option<&str>) {
        let Some(db) = &self.db else { return };
        let value = value.map_or(serde_json::Value::Null, serde_json::Value::from);
        if let Err(error) = db::settings::set_value(&lock(db), key, &value) {
            tracing::error!("can't store {key}: {error}");
        }
    }

    /// Chooses a model of its kind, or none (REQ-032-01, REQ-032-16).
    pub fn choose_face_model(&mut self, detect: bool, sha256: Option<String>) {
        if detect {
            self.store_face_setting(faces::DETECTOR_KEY, sha256.as_deref());
            self.faces.detector = sha256;
        } else {
            let threshold = sha256
                .as_deref()
                .and_then(|sha256| self.faces.models.iter().find(|model| model.sha256 == sha256))
                .and_then(|model| match model.kind {
                    ModelKind::Recognize { threshold, .. } => Some(threshold.to_string()),
                    ModelKind::Detect(_) => None,
                });
            self.store_face_setting(faces::THRESHOLD_KEY, threshold.as_deref());
            self.store_face_setting(faces::RECOGNIZER_KEY, sha256.as_deref());
            self.faces.recognizer = sha256;
        }
        self.faces.model_checked = None;
        self.faces.model_seems_wrong = false;
        if let Some(gallery) = &mut self.gallery {
            gallery.forget_faces();
        }
        self.kick_faces();
    }

    /// Switches the search on or off (REQ-032-05).
    pub fn set_face_search(&mut self, paused: bool) {
        self.faces.paused = paused;
        self.store_face_setting(faces::SEARCH_KEY, Some(if paused { "paused" } else { "on" }));
        self.kick_faces();
    }

    fn arriving_progress(&self, key: String) -> impl FnMut(u64, Option<u64>) -> bool + use<> {
        let arriving = self.faces.arriving.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        lock(&arriving).insert(
            key.clone(),
            Arriving {
                bytes: 0,
                total: None,
                cancel: cancel.clone(),
            },
        );
        let repaint = self.repaint.clone();
        let mut shown = Instant::now();
        move |bytes, total| {
            if let Some(entry) = lock(&arriving).get_mut(&key) {
                entry.bytes = bytes;
                entry.total = total;
            }
            if shown.elapsed() > Duration::from_millis(200) {
                shown = Instant::now();
                if let Some(repaint) = &repaint {
                    repaint.request_repaint();
                }
            }
            !cancel.load(Ordering::SeqCst)
        }
    }

    /// Downloads a listed model in the background (REQ-032-02).
    pub fn download_face_model(&mut self, model: &'static CatalogModel) {
        let Some(store) = self.faces.store.clone() else { return };
        let mut progress = self.arriving_progress(model.sha256.to_owned());
        let key = model.sha256.to_owned();
        self.spawn(move || Event::FaceModelArrived {
            key,
            name: model.name.to_owned(),
            result: store.download(model, &mut progress),
        });
    }

    /// Stops a download; the file is dropped with it.
    pub fn cancel_face_model(&self, key: &str) {
        if let Some(entry) = lock(&self.faces.arriving).get(key) {
            entry.cancel.store(true, Ordering::SeqCst);
        }
    }

    /// Checks that a model file runs as the kind the user said it is (REQ-032-03).
    fn checker(kind: ModelKind) -> impl Fn(&Path) -> Result<(), String> {
        move |file| match kind {
            ModelKind::Detect(format) => Detector::load(file, format).map(|_| ()),
            ModelKind::Recognize { style, .. } => Recognizer::load(file, style).map(|_| ()),
        }
    }

    /// Adds a model from the pasted link (REQ-032-03).
    pub fn add_face_link(&mut self) {
        let Some(store) = self.faces.store.clone() else { return };
        let url = self.faces.link.trim().to_owned();
        if url.is_empty() {
            return;
        }
        let kind = self.faces.added_kind.kind();
        let mut progress = self.arriving_progress("added".to_owned());
        self.spawn(move || Event::FaceModelArrived {
            key: "added".to_owned(),
            name: url.clone(),
            result: store.add_link(&url, kind, &Self::checker(kind), &mut progress),
        });
    }

    /// Adds a model from a file the user picks (REQ-032-03).
    pub fn add_face_file(&mut self) {
        let Some(store) = self.faces.store.clone() else { return };
        let picker = self.picker.clone();
        let kind = self.faces.added_kind.kind();
        let mut progress = self.arriving_progress("added".to_owned());
        self.spawn(move || {
            let result = match picker.pick_model_file() {
                Some(file) => store.add_file(&file, kind, &Self::checker(kind), &mut progress),
                None => Err(StoreError::Cancelled),
            };
            Event::FaceModelArrived {
                key: "added".to_owned(),
                name: "the model".to_owned(),
                result,
            }
        });
    }

    /// Adds a model from a file, for tests and for what the dialog answered.
    pub fn add_face_file_at(&mut self, file: PathBuf) {
        let Some(store) = self.faces.store.clone() else { return };
        let kind = self.faces.added_kind.kind();
        let mut progress = self.arriving_progress("added".to_owned());
        self.spawn(move || Event::FaceModelArrived {
            key: "added".to_owned(),
            name: "the model".to_owned(),
            result: store.add_file(&file, kind, &Self::checker(kind), &mut progress),
        });
    }

    /// Removes a model from this computer; what it found stays, for when it comes back.
    pub fn remove_face_model(&mut self, sha256: &str) {
        let Some(store) = self.faces.store.clone() else { return };
        if self.faces.detector.as_deref() == Some(sha256) {
            self.choose_face_model(true, None);
        }
        if self.faces.recognizer.as_deref() == Some(sha256) {
            self.choose_face_model(false, None);
        }
        if let Err(error) = store.remove(sha256) {
            self.toasts.error(format!("Couldn't remove the model: {error}"));
        }
        self.faces.models = store.models();
    }

    pub(super) fn face_model_arrived(&mut self, key: &str, name: &str, result: Result<StoredModel, StoreError>) {
        lock(&self.faces.arriving).remove(key);
        if let Some(store) = &self.faces.store {
            self.faces.models = store.models();
        }
        match result {
            Ok(model) => {
                if key == "added" {
                    self.faces.link.clear();
                }
                self.toasts.success(format!("{} is ready", model.name));
            }
            Err(StoreError::Cancelled) => {}
            Err(error) => {
                tracing::error!("can't get {name}: {error}");
                self.toasts.error(format!("Couldn't get {name}: {error}"));
            }
        }
    }

    /// Shows ignored faces again (REQ-032-11).
    pub fn restore_ignored_faces(&mut self) {
        let Some(db) = self.db.clone() else { return };
        match faces::restore_ignored(&lock(&db)) {
            Ok(count) => self.toasts.success(format!("{count} ignored faces are back")),
            Err(error) => self.toasts.error(format!("Couldn't bring them back: {error}")),
        }
        if let Some(gallery) = &mut self.gallery {
            gallery.forget_faces();
        }
        self.kick_faces();
    }

    /// Deletes everything stored about faces and stops looking; files stay as they are (REQ-032-04).
    pub fn delete_face_data(&mut self) {
        let Some(db) = self.db.clone() else { return };
        self.choose_face_model(true, None);
        self.choose_face_model(false, None);
        if let Err(error) = faces::delete_all(&lock(&db)) {
            self.toasts.error(format!("Couldn't delete the face data: {error}"));
        } else {
            self.toasts.success("Face data deleted");
        }
        if let Some(gallery) = &mut self.gallery {
            gallery.forget_faces();
        }
    }

    /// Looks now and then whether the chosen model tells known people apart (REQ-032-17).
    pub fn check_face_model(&mut self) {
        let (Some(db), Some(detector), Some(recognizer)) = (
            self.db.clone(),
            self.faces.detector.clone(),
            self.faces.recognizer.clone(),
        ) else {
            return;
        };
        if self
            .faces
            .model_checked
            .is_some_and(|at| at.elapsed() < MODEL_CHECK_EVERY)
        {
            return;
        }
        self.faces.model_checked = Some(Instant::now());
        self.spawn(move || {
            Event::FaceModelChecked(faces::model_seems_wrong(&lock(&db), &detector, &recognizer).unwrap_or(false))
        });
    }

    pub(super) fn face_model_checked(&mut self, wrong: bool) {
        self.faces.model_seems_wrong = wrong;
    }

    /// What the user decided about a face (REQ-032-09 … REQ-032-15).
    pub fn face_action(&mut self, action: super::FaceAction) {
        use super::FaceAction as A;
        let Some(db) = self.db.clone() else { return };
        let decide = |face: i64, state: FaceState| faces::set_state(&lock(&db), face, state);
        let decided = match &action {
            A::Ignore { faces } => faces.iter().try_for_each(|face| decide(*face, FaceState::Ignored)),
            A::NotAFace { face } => decide(*face, FaceState::NotAFace),
            A::NotThisPerson { face, name } => faces::reject(&lock(&db), *face, name),
            A::Name { .. } | A::Remove { .. } | A::Rename { .. } => Ok(()),
        };
        if let Err(error) = decided {
            self.toasts.error(format!("Couldn't save that: {error}"));
        }
        let changes: Vec<(i64, RegionChange)> = match action {
            A::Ignore { .. } | A::NotAFace { .. } | A::NotThisPerson { .. } => Vec::new(),
            A::Name { name, places } => places
                .into_iter()
                .map(|(media, rect)| {
                    (
                        media,
                        RegionChange::Add {
                            name: name.clone(),
                            rect,
                        },
                    )
                })
                .collect(),
            A::Remove { media, name, rect } => vec![(media, RegionChange::Remove { name, rect: Some(rect) })],
            A::Rename { media, from, to, rect } => vec![(
                media,
                RegionChange::Rename {
                    from,
                    to,
                    rect: Some(rect),
                },
            )],
        };
        if changes.is_empty() {
            if let Some(gallery) = &mut self.gallery {
                gallery.forget_faces();
            }
        } else {
            self.write_regions(changes);
        }
    }

    /// Writes region changes on the writer thread; the index reads the files again afterwards (REQ-032-14).
    fn write_regions(&mut self, changes: Vec<(i64, RegionChange)>) {
        let (Some(db), Some(exiftool)) = (
            self.db.clone(),
            self.edit_tools.as_ref().map(|tools| tools.exiftool.clone()),
        ) else {
            self.toasts
                .error("Couldn't save the face: exiftool, which writes it, wasn't found.");
            return;
        };
        let detector = self.faces.detector.clone();
        self.spawn_write(move || {
            let mut written = Vec::new();
            let mut failed = None;
            for (media, change) in changes {
                let photo = faces::photo(&lock(&db), media).ok().flatten();
                let Some(photo) = photo else { continue };
                let result = write_region(&exiftool, &photo, &change, &db, detector.as_deref());
                match result {
                    Ok(()) => {
                        if let Err(error) = db::media::mark_for_reading(&lock(&db), media) {
                            tracing::error!("can't mark {} for reading: {error}", photo.rel_path);
                        }
                        written.push((photo.root_id, photo.rel_path));
                    }
                    Err(error) => {
                        tracing::error!("can't change the regions of {}: {error}", photo.rel_path);
                        failed = Some(error);
                    }
                }
            }
            Event::FaceRegionsWritten { written, failed }
        });
    }

    pub(super) fn face_regions_written(&mut self, written: Vec<(i64, String)>, failed: Option<String>) {
        let mut by_root: HashMap<i64, Vec<String>> = HashMap::new();
        for (root, rel_path) in written {
            by_root.entry(root).or_default().push(rel_path);
        }
        for (root, paths) in by_root {
            self.scans.request_paths(root, paths);
        }
        if let Some(error) = failed {
            self.toasts.error(format!("Couldn't save the face: {error}"));
        }
        if let Some(gallery) = &mut self.gallery {
            gallery.forget_faces();
            gallery.forget_detail();
        }
        self.refresh();
    }
}

/// Removes every region of `name` from a photo for its People tag's remove button (REQ-032-13), and remembers that
/// these faces aren't that person.
pub(super) fn remove_person(
    exiftool: &Path,
    photo: &faces::FacePhoto,
    name: &str,
    db: &Mutex<db::rusqlite::Connection>,
    detector: Option<&str>,
) -> Result<(), String> {
    let change = RegionChange::Remove {
        name: name.to_owned(),
        rect: None,
    };
    write_region(exiftool, photo, &change, db, detector)
}

fn write_region(
    exiftool: &Path,
    photo: &faces::FacePhoto,
    change: &RegionChange,
    db: &Mutex<db::rusqlite::Connection>,
    detector: Option<&str>,
) -> Result<(), String> {
    use domain::tagging::Change;
    // Where the regions are, before they change: the faces there shouldn't get this name suggested again.
    let rejected: Vec<(String, [f32; 4])> = match change {
        RegionChange::Add { .. } => Vec::new(),
        RegionChange::Remove { name, rect } | RegionChange::Rename { from: name, rect, .. } => match rect {
            Some(rect) => vec![(name.clone(), *rect)],
            None => faces::regions_named(&lock(db), photo.media, name)
                .unwrap_or_default()
                .into_iter()
                .map(|rect| (name.clone(), rect))
                .collect(),
        },
    };
    let library = PathBuf::from(&photo.root_path);
    media::face_regions::write(exiftool, &library, &photo.rel_path, photo.sidecar_orientation, change)
        .map_err(|error| error.to_string())?;
    if let Some(detector) = detector {
        let db = lock(db);
        for (name, rect) in rejected {
            if let Ok(Some(face)) = faces::face_on_region(&db, photo.media, detector, rect) {
                let _ = faces::reject(&db, face, &name);
            }
        }
    }
    // The old name also leaves the keywords, or it would stay as a tag.
    let old = match change {
        RegionChange::Add { .. } => None,
        RegionChange::Remove { name, .. } | RegionChange::Rename { from: name, .. } => Some(name),
    };
    if let Some(old) = old {
        let still_there = faces::regions_named(&lock(db), photo.media, old)
            .unwrap_or_default()
            .len()
            > 1
            && !matches!(
                change,
                RegionChange::Remove { rect: None, .. } | RegionChange::Rename { rect: None, .. }
            );
        if !still_there {
            media::tagging::write(
                exiftool,
                &library,
                &photo.rel_path,
                domain::media::MediaKind::Image,
                &Change::Remove(old.clone()),
            )
            .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}
