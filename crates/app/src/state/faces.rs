//! The app's side of faces (spec 032): the models on this computer, downloads, the background search, and what the user
//! decides about a face. Desktop only; the browser version looks for no faces.

use super::{AppState, Event, lock};
use db::faces::{self, FaceState};
use domain::face_models::{CatalogModel, InputStyle, ModelKind};
use media::face_models::identify;
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

/// How the _Add a model_ form describes a recognition model's pixels, the one thing a file doesn't tell (REQ-032-03).
pub fn style_label(style: InputStyle) -> &'static str {
    match style {
        InputStyle::ArcFace => "From -1 to 1 (most models)",
        InputStyle::SFace => "From 0 to 255 (SFace and models like it)",
    }
}

/// What the search is doing, in words (REQ-032-20).
#[derive(Debug, Clone, PartialEq)]
pub struct SearchSummary {
    pub title: &'static str,
    pub detail: String,
    /// How far it has come, while that means something.
    pub share: Option<f32>,
    /// It is working right now, so the window's corner says so too.
    pub working: bool,
}

/// Puts what the search reports into words.
pub fn search_summary(
    detector: bool,
    recognizer: bool,
    paused: bool,
    status: Option<&indexer::faces::FaceStatus>,
) -> SearchSummary {
    let summary = |title, detail: String, share, working| SearchSummary {
        title,
        detail,
        share,
        working,
    };
    if !detector {
        return summary(
            "Not looking for faces",
            "Choose a detection model below to start.".to_owned(),
            None,
            false,
        );
    }
    let Some(status) = status else {
        return summary("Ready", String::new(), None, false);
    };
    if let Some(problem) = &status.problem {
        return summary("Stopped", problem.clone(), None, false);
    }
    // Only for a moment, while the search counts for the first time: never `0 of 0` (REQ-032-20).
    if !status.counted {
        let title = if paused { "Paused" } else { "Getting ready" };
        return summary(title, String::new(), None, false);
    }
    let progress = status.progress;
    let count = crate::format::count;
    let photos_share = progress.photos_done as f32 / progress.photos.max(1) as f32;
    let done = format!(
        "{} of {} photos done, {} faces found.",
        count(progress.photos_done),
        count(progress.photos),
        count(progress.faces)
    );
    if paused {
        let detail = format!("{done} Everything found so far stays.");
        return summary("Paused", detail, Some(photos_share), false);
    }
    let photos_left = progress.photos_done < progress.photos;
    let faces_left = recognizer && progress.faces_recognized < progress.faces;
    if status.working && photos_left {
        // Each photo gets both steps before the next one (REQ-032-23), so its count stands for both.
        let title = if recognizer {
            "Finding and recognizing faces"
        } else {
            "Finding faces"
        };
        return summary(title, done, Some(photos_share), true);
    }
    if status.working && faces_left {
        let detail = format!(
            "{} of {} faces recognized.",
            count(progress.faces_recognized),
            count(progress.faces)
        );
        let share = progress.faces_recognized as f32 / progress.faces.max(1) as f32;
        return summary("Recognizing faces", detail, Some(share), true);
    }
    if photos_left {
        let left = progress.photos - progress.photos_done;
        let detail = format!(
            "{done} The other {} are in a library that can't be reached right now.",
            count(left)
        );
        return summary("Waiting", detail, Some(photos_share), false);
    }
    let mut detail = format!(
        "All {} photos done, {} faces found.",
        count(progress.photos),
        count(progress.faces)
    );
    if !recognizer {
        detail.push_str(" Choose a recognition model to get suggested names and groups.");
    }
    summary("Finished", detail, None, false)
}

/// How often the check for a wrongly set up model runs while the settings are open (REQ-032-17).
const MODEL_CHECK_EVERY: Duration = Duration::from_secs(30);

/// [`InputStyle`] with what most models use as the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddedStyle(pub InputStyle);

impl Default for AddedStyle {
    fn default() -> Self {
        Self(InputStyle::ArcFace)
    }
}

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
    /// How a recognition model being added expects its pixels.
    pub added_style: AddedStyle,
    pub confirm_delete: bool,
    /// The settings should scroll to this section (REQ-032-20).
    pub reveal: bool,
    pub confirm_look_again: bool,
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

    /// What the search is doing, in words (REQ-032-20).
    pub fn summary(&self) -> SearchSummary {
        search_summary(
            self.detector.is_some(),
            self.recognizer.is_some(),
            self.paused,
            self.status().as_ref(),
        )
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
        // A threshold chosen by an earlier version may have been measured better since.
        let threshold = self.chosen_threshold(self.faces.recognizer.as_deref());
        if threshold.is_some() {
            self.store_face_setting(faces::THRESHOLD_KEY, threshold.as_deref());
        }
        self
    }

    /// The threshold of a stored recognition model, as stored settings hold it.
    fn chosen_threshold(&self, sha256: Option<&str>) -> Option<String> {
        let sha256 = sha256?;
        match self.faces.models.iter().find(|model| model.sha256 == sha256)?.kind {
            ModelKind::Recognize { threshold, .. } => Some(threshold.to_string()),
            ModelKind::Detect(_) => None,
        }
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
            let threshold = self.chosen_threshold(sha256.as_deref());
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

    /// Adds a model from a file the user picks (REQ-032-03).
    pub fn add_face_file(&mut self) {
        let Some(store) = self.faces.store.clone() else { return };
        let picker = self.picker.clone();
        let style = self.faces.added_style.0;
        let mut progress = self.arriving_progress("added".to_owned());
        self.spawn(move || {
            let result = match picker.pick_model_file() {
                Some(file) => store.add_file(&file, &|file| identify(file, style), &mut progress),
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
        let style = self.faces.added_style.0;
        let mut progress = self.arriving_progress("added".to_owned());
        self.spawn(move || Event::FaceModelArrived {
            key: "added".to_owned(),
            name: "the model".to_owned(),
            result: store.add_file(&file, &|file| identify(file, style), &mut progress),
        });
    }

    /// Adds a model from files dropped on the settings: the first, if it can be one (REQ-032-03).
    pub fn add_dropped_face_files(&mut self, files: &[PathBuf]) {
        let Some(file) = files.first() else { return };
        let is_onnx = file
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("onnx"));
        if !is_onnx {
            let name = file.file_name().unwrap_or_default().to_string_lossy();
            self.toasts.error(format!("{name} isn't an ONNX file"));
            return;
        }
        if self.faces.arriving().contains_key("added") {
            self.toasts.error("Another model is still being added");
            return;
        }
        self.add_face_file_at(file.clone());
        if files.len() > 1 {
            self.toasts
                .success("Adding the first file: the app adds one model at a time");
        }
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
                self.toasts.success(format!("{} is ready", model.name));
            }
            Err(StoreError::Cancelled) => {}
            Err(error) => {
                tracing::error!("can't get {name}: {error}");
                self.toasts.error(format!("Couldn't get {name}: {error}"));
            }
        }
    }

    /// Looks through every photo again (REQ-032-22).
    pub fn look_through_all_again(&mut self) {
        let Some(db) = self.db.clone() else { return };
        if let Err(error) = faces::look_again(&lock(&db), None) {
            self.toasts.error(format!("Couldn't look again: {error}"));
        }
        if let Some(gallery) = &mut self.gallery {
            gallery.forget_detail();
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
            A::NotAFace { faces } => faces.iter().try_for_each(|face| decide(*face, FaceState::NotAFace)),
            A::Reopen { faces } => faces.iter().try_for_each(|face| decide(*face, FaceState::Open)),
            A::Delete { face } => faces::delete_face(&lock(&db), *face),
            A::NotThisPerson { face, name } => faces::reject(&lock(&db), *face, name),
            A::Ungroup { face } => faces::ungroup(&lock(&db), *face),
            A::LookAgain { media } => faces::look_again(&lock(&db), Some(*media)),
            A::Name { .. } | A::Remove { .. } | A::Rename { .. } => Ok(()),
        };
        if let Err(error) = decided {
            self.toasts.error(format!("Couldn't save that: {error}"));
        }
        let changes: Vec<(i64, RegionChange)> = match action {
            A::Ignore { .. }
            | A::Ungroup { .. }
            | A::NotAFace { .. }
            | A::Reopen { .. }
            | A::Delete { .. }
            | A::NotThisPerson { .. }
            | A::LookAgain { .. } => Vec::new(),
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
                gallery.forget_detail();
            }
            self.kick_faces();
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

#[cfg(test)]
mod tests {
    use super::*;
    use db::faces::FaceProgress;
    use indexer::faces::FaceStatus;

    fn status(photos_done: u64, photos: u64, faces: u64, faces_recognized: u64, working: bool) -> FaceStatus {
        FaceStatus {
            progress: FaceProgress {
                photos_done,
                photos,
                faces,
                faces_recognized,
            },
            counted: true,
            working,
            problem: None,
        }
    }

    #[test]
    fn req_032_20_says_what_the_search_is_doing() {
        let title = |detector, recognizer, paused, status: Option<&FaceStatus>| {
            search_summary(detector, recognizer, paused, status).title
        };
        assert_eq!(title(false, false, false, None), "Not looking for faces");
        let looking = status(10, 100, 5, 0, true);
        assert_eq!(
            title(true, true, false, Some(&looking)),
            "Finding and recognizing faces"
        );
        assert_eq!(title(true, false, false, Some(&looking)), "Finding faces");
        assert_eq!(title(true, true, true, Some(&looking)), "Paused");
        assert_eq!(
            title(true, true, false, Some(&status(100, 100, 50, 10, true))),
            "Recognizing faces"
        );
        assert_eq!(
            title(true, true, false, Some(&status(90, 100, 50, 50, false))),
            "Waiting"
        );
        assert_eq!(
            title(true, true, false, Some(&status(100, 100, 50, 50, false))),
            "Finished"
        );

        let without = search_summary(true, false, false, Some(&status(100, 100, 50, 0, false)));
        assert!(
            without.detail.contains("Choose a recognition model"),
            "{}",
            without.detail
        );

        let broken = FaceStatus {
            problem: Some("The chosen detection model is gone.".into()),
            ..looking.clone()
        };
        let stopped = search_summary(true, true, false, Some(&broken));
        assert_eq!(
            (stopped.title, stopped.detail.as_str()),
            ("Stopped", "The chosen detection model is gone.")
        );

        let shown = search_summary(true, true, false, Some(&looking));
        assert_eq!(shown.detail, "10 of 100 photos done, 5 faces found.");
        assert_eq!(shown.share, Some(0.1));
    }

    /// REQ-032-20: before the first count there is nothing to say yet, and certainly not `0 of 0`.
    #[test]
    fn req_032_20_never_says_none_of_none() {
        let uncounted = FaceStatus {
            working: true,
            ..FaceStatus::default()
        };
        let starting = search_summary(true, true, false, Some(&uncounted));
        assert_eq!((starting.title, starting.detail.as_str()), ("Getting ready", ""));
        assert_eq!(starting.share, None);
        assert_eq!(search_summary(true, true, true, Some(&uncounted)).title, "Paused");

        let empty = search_summary(true, true, false, Some(&status(0, 0, 0, 0, true)));
        assert_eq!(empty.title, "Finished", "a library without photos is simply done");
    }

    #[test]
    fn req_032_20_a_notice_only_while_it_works() {
        let working = |recognizer, paused, status| search_summary(true, recognizer, paused, Some(&status)).working;
        assert!(working(true, false, status(10, 100, 5, 0, true)));
        assert!(working(true, false, status(100, 100, 5, 0, true)));
        assert!(!working(true, true, status(10, 100, 5, 0, true)), "paused");
        assert!(!working(true, false, status(100, 100, 5, 5, false)), "finished");
        assert!(
            !working(false, false, status(100, 100, 5, 0, true)),
            "nothing left to do"
        );
    }
}
