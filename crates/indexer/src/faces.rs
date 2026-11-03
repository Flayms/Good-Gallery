//! Looking for faces in the background (spec 031): one thread works through the photos the chosen detection model
//! hasn't seen, then the faces the chosen recognition model hasn't. It reads the choice from the settings each round,
//! so changing models needs only a [`FaceService::kick`] (REQ-031-16).

use db::faces::{self, FacePhoto, FaceProgress};
use db::rusqlite::Connection;
use domain::face_models::Detection;
use domain::face_models::{DetectorFormat, InputStyle, ModelKind};
use media::face_models::{Detector, Recognizer};
use media::face_store::FaceStore;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const PHOTOS_PER_ROUND: usize = 20;
const FACES_PER_ROUND: usize = 50;
/// How long to wait for a kick when there is nothing to do; a share may have come back.
const IDLE: Duration = Duration::from_secs(60);

/// Runs the models. [`RunModels`] or a fake in tests.
pub trait FaceWork: Send {
    /// The faces in `photo`, in fractions of the upright photo. An error costs only this file.
    fn detect(&mut self, detector: &str, photo: &FacePhoto) -> Result<Vec<Detection>, WorkError>;
    /// One vector per landmarks given, which are in fractions of the upright photo; empty if a face can't be read.
    fn recognize(
        &mut self,
        recognizer: &str,
        photo: &FacePhoto,
        faces: &[[[f32; 2]; 5]],
    ) -> Result<Vec<Vec<f32>>, WorkError>;
}

#[derive(Debug, PartialEq)]
pub enum WorkError {
    /// This photo can't be read; it isn't tried again until it changes.
    Photo(String),
    /// The model can't run, so nothing can.
    Model(String),
}

/// What the search is doing, for the settings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FaceStatus {
    pub progress: FaceProgress,
    /// Working on something right now.
    pub working: bool,
    pub problem: Option<String>,
}

pub struct FaceDeps {
    /// A write connection of its own.
    pub conn: Connection,
    pub work: Box<dyn FaceWork>,
    pub is_reachable: Arc<dyn Fn(&Path) -> bool + Send + Sync>,
    /// Called on its thread when the status changes.
    pub on_change: Box<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
struct Control {
    kicked: bool,
    shutdown: bool,
    status: FaceStatus,
}

struct Shared {
    control: Mutex<Control>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Control> {
        self.control.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether to stop this round, because something changed or the app is ending.
    fn interrupted(&self) -> bool {
        let control = self.lock();
        control.kicked || control.shutdown
    }
}

pub struct FaceService {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl FaceService {
    pub fn start(deps: FaceDeps) -> Self {
        let shared = Arc::new(Shared {
            control: Mutex::new(Control {
                kicked: true,
                ..Control::default()
            }),
            changed: Condvar::new(),
        });
        let thread = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("faces".into())
                .spawn(move || run(&shared, deps))
                .expect("can't start the faces thread")
        };
        Self {
            shared,
            thread: Some(thread),
        }
    }

    /// The settings or the libraries changed: look again now.
    pub fn kick(&self) {
        self.shared.lock().kicked = true;
        self.shared.changed.notify_all();
    }

    pub fn status(&self) -> FaceStatus {
        self.shared.lock().status.clone()
    }

    /// Waits until nothing is left to do (for tests).
    pub fn wait_idle(&self) {
        let mut control = self.shared.lock();
        while control.kicked || control.status.working {
            control = self
                .shared
                .changed
                .wait_timeout(control, Duration::from_millis(20))
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
    }
}

impl Drop for FaceService {
    fn drop(&mut self) {
        self.shared.lock().shutdown = true;
        self.shared.changed.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(shared: &Shared, mut deps: FaceDeps) {
    loop {
        {
            let mut control = shared.lock();
            if control.shutdown {
                return;
            }
            control.kicked = false;
            control.status.working = true;
        }
        let round = std::panic::catch_unwind(AssertUnwindSafe(|| round(shared, &mut deps)));
        let (more, problem) = match round {
            Ok(Ok(more)) => (more, None),
            Ok(Err(problem)) => (false, Some(problem)),
            Err(_) => (false, Some("Looking for faces stopped unexpectedly.".to_owned())),
        };
        let progress = chosen(&deps.conn, faces::DETECTOR_KEY)
            .and_then(|detector| faces::progress(&deps.conn, &detector).ok())
            .unwrap_or_default();
        {
            let mut control = shared.lock();
            control.status.progress = progress;
            control.status.problem = problem;
            control.status.working = more;
            shared.changed.notify_all();
        }
        (deps.on_change)();
        if more {
            continue;
        }
        let control = shared.lock();
        if !control.kicked && !control.shutdown {
            drop(
                shared
                    .changed
                    .wait_timeout_while(control, IDLE, |control| !control.kicked && !control.shutdown)
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            );
        }
    }
}

fn chosen(conn: &Connection, key: &str) -> Option<String> {
    faces::chosen(conn, key).ok().flatten()
}

/// One round of work; whether there may be more.
fn round(shared: &Shared, deps: &mut FaceDeps) -> Result<bool, String> {
    if chosen(&deps.conn, faces::SEARCH_KEY).as_deref() == Some("paused") {
        return Ok(false);
    }
    let Some(detector) = chosen(&deps.conn, faces::DETECTOR_KEY) else {
        return Ok(false);
    };
    let photos = faces::pending_photos(&deps.conn, &detector, PHOTOS_PER_ROUND).map_err(|error| error.to_string())?;
    let mut unreachable: Vec<String> = Vec::new();
    let mut did_some = false;
    for photo in &photos {
        if shared.interrupted() {
            return Ok(true);
        }
        if unreachable.contains(&photo.root_path) {
            continue;
        }
        if !(deps.is_reachable)(Path::new(&photo.root_path)) {
            unreachable.push(photo.root_path.clone());
            continue;
        }
        let work = &mut deps.work;
        let detected = std::panic::catch_unwind(AssertUnwindSafe(|| work.detect(&detector, photo)))
            .unwrap_or_else(|_| Err(WorkError::Photo("reading it failed unexpectedly".into())));
        let found = match detected {
            Ok(found) => found,
            Err(WorkError::Photo(reason)) => {
                tracing::warn!("no faces looked for in {}: {reason}", photo.rel_path);
                Vec::new()
            }
            Err(WorkError::Model(reason)) => return Err(reason),
        };
        faces::store_scan(&mut deps.conn, photo, &detector, &found).map_err(|error| error.to_string())?;
        did_some = true;
    }
    if !photos.is_empty() && did_some {
        return Ok(true);
    }
    let Some(recognizer) = chosen(&deps.conn, faces::RECOGNIZER_KEY) else {
        return Ok(false);
    };
    let waiting =
        faces::pending_faces(&deps.conn, &detector, &recognizer, FACES_PER_ROUND).map_err(|error| error.to_string())?;
    let mut by_photo: Vec<(&FacePhoto, Vec<&faces::FaceToRecognize>)> = Vec::new();
    for face in &waiting {
        match by_photo.iter_mut().find(|(photo, _)| photo.media == face.photo.media) {
            Some((_, faces)) => faces.push(face),
            None => by_photo.push((&face.photo, vec![face])),
        }
    }
    for (photo, group) in by_photo {
        if shared.interrupted() {
            return Ok(true);
        }
        if unreachable.contains(&photo.root_path) {
            continue;
        }
        if !(deps.is_reachable)(Path::new(&photo.root_path)) {
            unreachable.push(photo.root_path.clone());
            continue;
        }
        let landmarks: Vec<[[f32; 2]; 5]> = group.iter().map(|face| face.landmarks).collect();
        let work = &mut deps.work;
        let recognized = std::panic::catch_unwind(AssertUnwindSafe(|| work.recognize(&recognizer, photo, &landmarks)))
            .unwrap_or_else(|_| Err(WorkError::Photo("reading it failed unexpectedly".into())));
        let vectors = match recognized {
            Ok(vectors) => vectors,
            Err(WorkError::Photo(reason)) => {
                tracing::warn!("no faces recognized in {}: {reason}", photo.rel_path);
                vec![Vec::new(); group.len()]
            }
            Err(WorkError::Model(reason)) => return Err(reason),
        };
        for (face, vector) in group.iter().zip(vectors) {
            faces::store_embedding(&deps.conn, face.face, &recognizer, &vector).map_err(|error| error.to_string())?;
        }
        did_some = true;
    }
    Ok(did_some)
}

/// The longest side a photo is decoded at; more shows no more faces, and a panorama shouldn't fill the memory.
const MAX_SIDE: u32 = 8000;

/// The real work: models from the store, photos from the library, the last photo kept for the next question.
pub struct RunModels {
    store: FaceStore,
    ffmpeg: Option<PathBuf>,
    detectors: HashMap<String, Detector>,
    recognizers: HashMap<String, Recognizer>,
    photo: Option<(i64, i64, image::RgbImage)>,
}

impl RunModels {
    pub fn new(data_folder: &Path, ffmpeg: Option<PathBuf>) -> Self {
        Self {
            store: FaceStore::in_data_folder(data_folder),
            ffmpeg,
            detectors: HashMap::new(),
            recognizers: HashMap::new(),
            photo: None,
        }
    }

    fn decoded(&mut self, photo: &FacePhoto) -> Result<&image::RgbImage, WorkError> {
        let is_current = |kept: &(i64, i64, image::RgbImage)| kept.0 == photo.media && kept.1 == photo.mtime;
        if !self.photo.as_ref().is_some_and(is_current) {
            let file = photo
                .file()
                .ok_or_else(|| WorkError::Photo("its indexed path leaves its library".into()))?;
            let rgba =
                media::original::load_original(&file, self.ffmpeg.as_deref(), MAX_SIDE, photo.sidecar_orientation)
                    .map_err(|error| WorkError::Photo(error.to_string()))?;
            self.photo = Some((
                photo.media,
                photo.mtime,
                image::DynamicImage::ImageRgba8(rgba).to_rgb8(),
            ));
        }
        match &self.photo {
            Some((_, _, decoded)) => Ok(decoded),
            None => Err(WorkError::Photo("not decoded".into())),
        }
    }

    fn detector(&mut self, sha256: &str) -> Result<&Detector, WorkError> {
        if !self.detectors.contains_key(sha256) {
            let format: DetectorFormat = match self.store.model(sha256).map(|model| model.kind) {
                Some(ModelKind::Detect(format)) => format,
                _ => return Err(WorkError::Model("The chosen detection model is gone.".into())),
            };
            let detector = Detector::load(&self.store.file(sha256), format).map_err(WorkError::Model)?;
            self.detectors.clear();
            self.detectors.insert(sha256.to_owned(), detector);
        }
        self.detectors
            .get(sha256)
            .ok_or_else(|| WorkError::Model("The chosen detection model is gone.".into()))
    }

    fn recognizer(&mut self, sha256: &str) -> Result<&Recognizer, WorkError> {
        if !self.recognizers.contains_key(sha256) {
            let style: InputStyle = match self.store.model(sha256).map(|model| model.kind) {
                Some(ModelKind::Recognize { style, .. }) => style,
                _ => return Err(WorkError::Model("The chosen recognition model is gone.".into())),
            };
            let recognizer = Recognizer::load(&self.store.file(sha256), style).map_err(WorkError::Model)?;
            self.recognizers.clear();
            self.recognizers.insert(sha256.to_owned(), recognizer);
        }
        self.recognizers
            .get(sha256)
            .ok_or_else(|| WorkError::Model("The chosen recognition model is gone.".into()))
    }
}

impl FaceWork for RunModels {
    fn detect(&mut self, detector: &str, photo: &FacePhoto) -> Result<Vec<Detection>, WorkError> {
        self.detector(detector)?;
        self.decoded(photo)?;
        let (Some(model), Some((_, _, decoded))) = (self.detectors.get(detector), &self.photo) else {
            return Err(WorkError::Model("The chosen detection model is gone.".into()));
        };
        let (width, height) = (decoded.width() as f32, decoded.height() as f32);
        let found = model.find(decoded).map_err(WorkError::Model)?;
        Ok(found.into_iter().map(|face| fractions(face, width, height)).collect())
    }

    fn recognize(
        &mut self,
        recognizer: &str,
        photo: &FacePhoto,
        faces: &[[[f32; 2]; 5]],
    ) -> Result<Vec<Vec<f32>>, WorkError> {
        self.recognizer(recognizer)?;
        self.decoded(photo)?;
        let (Some(model), Some((_, _, decoded))) = (self.recognizers.get(recognizer), &self.photo) else {
            return Err(WorkError::Model("The chosen recognition model is gone.".into()));
        };
        let (width, height) = (decoded.width() as f32, decoded.height() as f32);
        faces
            .iter()
            .map(|landmarks| {
                let pixels = landmarks.map(|[x, y]| [x * width, y * height]);
                model.embed(decoded, &pixels).map_err(WorkError::Model)
            })
            .collect()
    }
}

/// A detection in photo pixels, as fractions of the photo, clamped to it.
fn fractions(face: Detection, width: f32, height: f32) -> Detection {
    let [x, y, w, h] = face.rect;
    let (left, top) = ((x / width).clamp(0.0, 1.0), (y / height).clamp(0.0, 1.0));
    let (right, bottom) = (((x + w) / width).clamp(0.0, 1.0), ((y + h) / height).clamp(0.0, 1.0));
    Detection {
        rect: [left, top, right - left, bottom - top],
        landmarks: face.landmarks.map(|[px, py]| [px / width, py / height]),
        score: face.score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use db::libraries::{self, LibraryStatus};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    const ROOT: &str = if cfg!(windows) { "D:\\Photos" } else { "/media/Photos" };

    fn index() -> (tempfile::TempDir, Connection, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gallery.db");
        let conn = db::open(&path).unwrap();
        libraries::insert(&conn, ROOT, "Photos", LibraryStatus::Online).unwrap();
        (dir, conn, path)
    }

    fn add_photo(conn: &Connection, name: &str) {
        conn.execute(
            "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime, taken_at)
             VALUES (1, ?1, ?1, 'image', 7, 1000, 1)",
            [name],
        )
        .unwrap();
    }

    fn choose(conn: &Connection, key: &str, value: &str) {
        db::settings::set_value(conn, key, &serde_json::json!(value)).unwrap();
    }

    /// One face per photo, except for those named bad; the same vector for all.
    #[derive(Clone, Default)]
    struct Fake {
        detected: Arc<AtomicUsize>,
        recognized: Arc<AtomicUsize>,
        broken_model: Arc<AtomicBool>,
    }

    impl FaceWork for Fake {
        fn detect(&mut self, _detector: &str, photo: &FacePhoto) -> Result<Vec<Detection>, WorkError> {
            if self.broken_model.load(Ordering::SeqCst) {
                return Err(WorkError::Model("broken".into()));
            }
            self.detected.fetch_add(1, Ordering::SeqCst);
            if photo.rel_path.contains("bad") {
                return Err(WorkError::Photo("unreadable".into()));
            }
            if photo.rel_path.contains("panic") {
                panic!("a hostile file");
            }
            Ok(vec![Detection {
                rect: [0.1, 0.1, 0.2, 0.2],
                landmarks: [[0.2, 0.2]; 5],
                score: 0.9,
            }])
        }

        fn recognize(&mut self, _: &str, _: &FacePhoto, faces: &[[[f32; 2]; 5]]) -> Result<Vec<Vec<f32>>, WorkError> {
            self.recognized.fetch_add(faces.len(), Ordering::SeqCst);
            Ok(vec![vec![1.0, 0.0]; faces.len()])
        }
    }

    fn start(path: &Path, fake: &Fake, reachable: bool) -> FaceService {
        FaceService::start(FaceDeps {
            conn: db::open(path).unwrap(),
            work: Box::new(fake.clone()),
            is_reachable: Arc::new(move |_| reachable),
            on_change: Box::new(|| {}),
        })
    }

    #[test]
    fn req_031_04_looks_for_nothing_without_a_chosen_model() {
        let (_dir, conn, path) = index();
        add_photo(&conn, "a.jpg");
        let fake = Fake::default();
        let service = start(&path, &fake, true);
        service.wait_idle();
        assert_eq!(fake.detected.load(Ordering::SeqCst), 0);
        let stored: i64 = conn
            .query_row("SELECT count(*) FROM faces_scanned", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored, 0);
    }

    #[test]
    fn req_031_05_works_through_all_photos_and_a_bad_file_costs_only_itself() {
        let (_dir, conn, path) = index();
        for name in ["a.jpg", "bad.jpg", "c.jpg"] {
            add_photo(&conn, name);
        }
        choose(&conn, faces::DETECTOR_KEY, "d");
        choose(&conn, faces::RECOGNIZER_KEY, "r");
        let fake = Fake::default();
        let service = start(&path, &fake, true);
        service.wait_idle();
        assert_eq!(fake.detected.load(Ordering::SeqCst), 3);
        assert_eq!(fake.recognized.load(Ordering::SeqCst), 2);
        let status = service.status();
        assert_eq!(status.progress.photos_done, 3);
        assert_eq!(status.progress.faces, 2);
        assert_eq!(status.problem, None);
        drop(service);

        // A restart carries on: nothing is looked at again.
        let service = start(&path, &fake, true);
        service.wait_idle();
        assert_eq!(fake.detected.load(Ordering::SeqCst), 3);
        assert_eq!(fake.recognized.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn req_031_05_stops_for_an_unreachable_share_or_when_paused() {
        let (_dir, conn, path) = index();
        add_photo(&conn, "a.jpg");
        choose(&conn, faces::DETECTOR_KEY, "d");
        let fake = Fake::default();
        let service = start(&path, &fake, false);
        service.wait_idle();
        assert_eq!(fake.detected.load(Ordering::SeqCst), 0);
        drop(service);

        choose(&conn, faces::SEARCH_KEY, "paused");
        let service = start(&path, &fake, true);
        service.wait_idle();
        assert_eq!(fake.detected.load(Ordering::SeqCst), 0);

        choose(&conn, faces::SEARCH_KEY, "on");
        service.kick();
        service.wait_idle();
        assert_eq!(fake.detected.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn req_031_05_a_model_that_cant_run_says_so_and_a_panic_costs_only_that_file() {
        let (_dir, conn, path) = index();
        add_photo(&conn, "a.jpg");
        choose(&conn, faces::DETECTOR_KEY, "d");
        let fake = Fake::default();
        fake.broken_model.store(true, Ordering::SeqCst);
        let service = start(&path, &fake, true);
        service.wait_idle();
        assert_eq!(service.status().problem.as_deref(), Some("broken"));
        drop(service);

        fake.broken_model.store(false, Ordering::SeqCst);
        add_photo(&conn, "panic.jpg");
        add_photo(&conn, "z.jpg");
        let service = start(&path, &fake, true);
        service.wait_idle();
        let status = service.status();
        assert_eq!(status.problem, None);
        assert_eq!(
            status.progress.photos_done, 3,
            "the rest go on, and it isn't tried again"
        );
    }

    #[test]
    fn req_031_16_another_recognition_model_recognizes_again_and_going_back_repeats_nothing() {
        let (_dir, conn, path) = index();
        add_photo(&conn, "a.jpg");
        choose(&conn, faces::DETECTOR_KEY, "d");
        choose(&conn, faces::RECOGNIZER_KEY, "r1");
        let fake = Fake::default();
        let service = start(&path, &fake, true);
        service.wait_idle();
        assert_eq!(fake.recognized.load(Ordering::SeqCst), 1);

        choose(&conn, faces::RECOGNIZER_KEY, "r2");
        service.kick();
        service.wait_idle();
        assert_eq!(fake.recognized.load(Ordering::SeqCst), 2);
        assert_eq!(fake.detected.load(Ordering::SeqCst), 1, "the boxes stay");

        choose(&conn, faces::RECOGNIZER_KEY, "r1");
        service.kick();
        service.wait_idle();
        assert_eq!(fake.recognized.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn req_031_06_detections_become_fractions_of_the_upright_photo() {
        let face = Detection {
            rect: [-10.0, 50.0, 110.0, 100.0],
            landmarks: [[50.0, 100.0]; 5],
            score: 0.9,
        };
        let placed = fractions(face, 200.0, 400.0);
        assert_eq!(placed.rect, [0.0, 0.125, 0.5, 0.25]);
        assert_eq!(placed.landmarks[0], [0.25, 0.25]);
    }
}
