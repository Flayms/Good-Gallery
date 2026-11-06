//! Looking for faces in the background (spec 032): one thread works through the photos the chosen detection model
//! hasn't seen, then the faces the chosen recognition model hasn't. It reads the choice from the settings each round,
//! so changing models needs only a [`FaceService::kick`] (REQ-032-16).

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

    /// How many photos [`FaceWork::detect_many`] and [`FaceWork::recognize_many`] take at once (REQ-037-02).
    fn batch(&self) -> usize {
        1
    }

    /// The faces in each of `photos`, in their order. A panic costs only its photo.
    fn detect_many(&mut self, detector: &str, photos: &[FacePhoto]) -> Vec<Result<Vec<Detection>, WorkError>> {
        photos
            .iter()
            .map(|photo| {
                std::panic::catch_unwind(AssertUnwindSafe(|| self.detect(detector, photo)))
                    .unwrap_or_else(|_| Err(WorkError::Photo("reading it failed unexpectedly".into())))
            })
            .collect()
    }

    /// The vectors of each photo's faces, as [`FaceWork::recognize`], in their order. A panic costs only its photo.
    fn recognize_many(&mut self, recognizer: &str, photos: &[(FacePhoto, Vec<Landmarks>)]) -> Vec<Recognized> {
        photos
            .iter()
            .map(|(photo, faces)| {
                std::panic::catch_unwind(AssertUnwindSafe(|| self.recognize(recognizer, photo, faces)))
                    .unwrap_or_else(|_| Err(WorkError::Photo("reading it failed unexpectedly".into())))
            })
            .collect()
    }

    /// Nothing is left to do for now: what's only kept for the next question can go.
    fn rest(&mut self) {}
}

/// A face's five landmarks, in fractions of the upright photo.
pub type Landmarks = [[f32; 2]; 5];

/// One vector per face of a photo, or why there are none.
pub type Recognized = Result<Vec<Vec<f32>>, WorkError>;

#[derive(Debug, Clone, PartialEq)]
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
    /// Whether `progress` has been counted yet (REQ-032-20).
    pub counted: bool,
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
    // Whether the status still shows the count made after the last round.
    let mut counted = false;
    loop {
        {
            let mut control = shared.lock();
            if control.shutdown {
                return;
            }
            control.kicked = false;
            control.status.working = true;
        }
        // Before the first photo, which also loads the models, so the numbers show from the start (REQ-032-20).
        if !counted {
            let progress = count(&deps.conn);
            let mut control = shared.lock();
            control.status.counted = progress.is_some();
            control.status.progress = progress.unwrap_or_default();
            drop(control);
            (deps.on_change)();
        }
        let round = std::panic::catch_unwind(AssertUnwindSafe(|| round(shared, &mut deps)));
        let (more, problem) = match round {
            Ok(Ok(more)) => (more, None),
            Ok(Err(problem)) => (false, Some(problem)),
            Err(_) => (false, Some("Looking for faces stopped unexpectedly.".to_owned())),
        };
        let progress = count(&deps.conn);
        counted = more;
        {
            let mut control = shared.lock();
            control.status.counted = progress.is_some();
            control.status.progress = progress.unwrap_or_default();
            control.status.problem = problem;
            control.status.working = more;
            shared.changed.notify_all();
        }
        (deps.on_change)();
        if more {
            continue;
        }
        // A large model takes hundreds of megabytes, which an idle search shouldn't hold.
        deps.work.rest();
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

/// How far the search has come; `None` without a chosen detection model or if it can't be counted.
fn count(conn: &Connection) -> Option<FaceProgress> {
    let detector = chosen(conn, faces::DETECTOR_KEY)?;
    let recognizer = chosen(conn, faces::RECOGNIZER_KEY);
    faces::progress(conn, &detector, recognizer.as_deref()).ok()
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
    let recognizer = chosen(&deps.conn, faces::RECOGNIZER_KEY);
    let photos = faces::pending_photos(&deps.conn, &detector, PHOTOS_PER_ROUND).map_err(|error| error.to_string())?;
    let mut unreachable: Vec<String> = Vec::new();
    for photo in &photos {
        if !unreachable.contains(&photo.root_path) && !(deps.is_reachable)(Path::new(&photo.root_path)) {
            unreachable.push(photo.root_path.clone());
        }
    }
    let reachable: Vec<FacePhoto> = photos
        .iter()
        .filter(|photo| !unreachable.contains(&photo.root_path))
        .cloned()
        .collect();
    let batch = deps.work.batch().max(1);
    let mut did_some = false;
    // Several photos at once; a pause or a kick stops between batches (REQ-037-02, REQ-037-04).
    for photos in reachable.chunks(batch) {
        if shared.interrupted() {
            return Ok(true);
        }
        let work = &mut deps.work;
        let detected = std::panic::catch_unwind(AssertUnwindSafe(|| work.detect_many(&detector, photos)))
            .unwrap_or_else(|_| {
                photos
                    .iter()
                    .map(|_| Err(WorkError::Photo("reading it failed unexpectedly".into())))
                    .collect()
            });
        for (photo, detected) in photos.iter().zip(detected) {
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
        // Right away, while the photos are still decoded (REQ-032-23).
        if let Some(recognizer) = &recognizer {
            let mut groups = Vec::new();
            for photo in photos {
                let waiting = faces::pending_faces_in(&deps.conn, photo.media, &detector, recognizer)
                    .map_err(|error| error.to_string())?;
                groups.push((photo.clone(), waiting));
            }
            recognize_photos(deps, recognizer, &groups)?;
        }
    }
    if !photos.is_empty() && did_some {
        return Ok(true);
    }
    let Some(recognizer) = recognizer else {
        return Ok(false);
    };
    let waiting =
        faces::pending_faces(&deps.conn, &detector, &recognizer, FACES_PER_ROUND).map_err(|error| error.to_string())?;
    let mut by_photo: Vec<(FacePhoto, Vec<faces::FaceToRecognize>)> = Vec::new();
    for face in waiting {
        match by_photo.iter_mut().find(|(photo, _)| photo.media == face.photo.media) {
            Some((_, faces)) => faces.push(face),
            None => by_photo.push((face.photo.clone(), vec![face])),
        }
    }
    by_photo.retain(|(photo, _)| {
        if unreachable.contains(&photo.root_path) {
            return false;
        }
        if !(deps.is_reachable)(Path::new(&photo.root_path)) {
            unreachable.push(photo.root_path.clone());
            return false;
        }
        true
    });
    for groups in by_photo.chunks(batch) {
        if shared.interrupted() {
            return Ok(true);
        }
        recognize_photos(deps, &recognizer, groups)?;
        did_some = true;
    }
    Ok(did_some)
}

/// Recognizes the faces of several photos at once and stores what comes out. A file that can't be read costs only its
/// faces.
fn recognize_photos(
    deps: &mut FaceDeps,
    recognizer: &str,
    groups: &[(FacePhoto, Vec<faces::FaceToRecognize>)],
) -> Result<(), String> {
    let groups: Vec<&(FacePhoto, Vec<faces::FaceToRecognize>)> =
        groups.iter().filter(|(_, faces)| !faces.is_empty()).collect();
    if groups.is_empty() {
        return Ok(());
    }
    let jobs: Vec<(FacePhoto, Vec<Landmarks>)> = groups
        .iter()
        .map(|(photo, faces)| (photo.clone(), faces.iter().map(|face| face.landmarks).collect()))
        .collect();
    let work = &mut deps.work;
    let recognized = std::panic::catch_unwind(AssertUnwindSafe(|| work.recognize_many(recognizer, &jobs)))
        .unwrap_or_else(|_| {
            jobs.iter()
                .map(|_| Err(WorkError::Photo("reading it failed unexpectedly".into())))
                .collect()
        });
    for ((photo, group), recognized) in groups.into_iter().zip(recognized) {
        let vectors = match recognized {
            Ok(vectors) => vectors,
            Err(WorkError::Photo(reason)) => {
                tracing::warn!("no faces recognized in {}: {reason}", photo.rel_path);
                vec![Vec::new(); group.len()]
            }
            Err(WorkError::Model(reason)) => return Err(reason),
        };
        for (face, vector) in group.iter().zip(vectors) {
            faces::store_embedding(&deps.conn, face.face, recognizer, &vector).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// The longest side a photo is decoded at; more shows no more faces, and a panorama shouldn't fill the memory.
const MAX_SIDE: u32 = 8000;

/// A photo as the models see it: upright, at most [`MAX_SIDE`], in RGB.
fn decode(photo: &FacePhoto, ffmpeg: Option<&Path>) -> Result<image::RgbImage, WorkError> {
    let file = photo
        .file()
        .ok_or_else(|| WorkError::Photo("its indexed path leaves its library".into()))?;
    let rgba = media::original::load_original(&file, ffmpeg, MAX_SIDE, photo.sidecar_orientation)
        .map_err(|error| WorkError::Photo(error.to_string()))?;
    Ok(image::DynamicImage::ImageRgba8(rgba).to_rgb8())
}

/// Which version of which photo: a changed file is decoded again.
fn key_of(photo: &FacePhoto) -> (i64, i64) {
    (photo.media, photo.mtime)
}

/// The real work: models from the store, photos from the library, the last photos kept for the next question. Several
/// photos are decoded and looked at at once, each on a thread of its own with the same loaded model (REQ-037-02).
pub struct RunModels {
    store: FaceStore,
    ffmpeg: Option<PathBuf>,
    detectors: HashMap<String, Detector>,
    recognizers: HashMap<String, Recognizer>,
    photo: Option<(i64, i64, image::RgbImage)>,
    /// The photos of the last batch, decoded, for their faces' recognition (REQ-032-23).
    kept: Vec<((i64, i64), image::RgbImage)>,
    threads: usize,
}

impl RunModels {
    pub fn new(data_folder: &Path, ffmpeg: Option<PathBuf>) -> Self {
        Self {
            store: FaceStore::in_data_folder(data_folder),
            ffmpeg,
            detectors: HashMap::new(),
            recognizers: HashMap::new(),
            photo: None,
            kept: Vec::new(),
            threads: crate::analysis::threads(),
        }
    }

    /// Looks at up to `threads` photos at once.
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = threads.max(1);
        self
    }

    fn decoded(&mut self, photo: &FacePhoto) -> Result<&image::RgbImage, WorkError> {
        let is_current = |kept: &(i64, i64, image::RgbImage)| kept.0 == photo.media && kept.1 == photo.mtime;
        if !self.photo.as_ref().is_some_and(is_current) {
            self.photo = None;
            self.photo = Some((photo.media, photo.mtime, decode(photo, self.ffmpeg.as_deref())?));
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
    fn rest(&mut self) {
        self.detectors.clear();
        self.recognizers.clear();
        self.photo = None;
        self.kept.clear();
    }

    fn batch(&self) -> usize {
        self.threads
    }

    fn detect_many(&mut self, detector: &str, photos: &[FacePhoto]) -> Vec<Result<Vec<Detection>, WorkError>> {
        if let Err(error) = self.detector(detector) {
            return photos.iter().map(|_| Err(error.clone())).collect();
        }
        // The last batch goes first, so no more photos than one batch are held decoded.
        self.kept.clear();
        self.photo = None;
        let (Some(model), ffmpeg) = (self.detectors.get(detector), self.ffmpeg.as_deref()) else {
            return Vec::new();
        };
        let looked = crate::analysis::map_in_order(photos, self.threads, |photo| {
            std::panic::catch_unwind(AssertUnwindSafe(|| {
                let decoded = decode(photo, ffmpeg)?;
                let (width, height) = (decoded.width() as f32, decoded.height() as f32);
                let found = model.find(&decoded).map_err(WorkError::Model)?;
                let found: Vec<Detection> = found.into_iter().map(|face| fractions(face, width, height)).collect();
                Ok((decoded, found))
            }))
            .unwrap_or_else(|_| Err(WorkError::Photo("reading it failed unexpectedly".into())))
        });
        photos
            .iter()
            .zip(looked)
            .map(|(photo, looked)| {
                looked.map(|(decoded, found)| {
                    self.kept.push((key_of(photo), decoded));
                    found
                })
            })
            .collect()
    }

    fn recognize_many(&mut self, recognizer: &str, photos: &[(FacePhoto, Vec<Landmarks>)]) -> Vec<Recognized> {
        if let Err(error) = self.recognizer(recognizer) {
            return photos.iter().map(|_| Err(error.clone())).collect();
        }
        let (Some(model), kept, ffmpeg) = (self.recognizers.get(recognizer), &self.kept, self.ffmpeg.as_deref()) else {
            return Vec::new();
        };
        crate::analysis::map_in_order(photos, self.threads, |(photo, faces)| {
            std::panic::catch_unwind(AssertUnwindSafe(|| {
                let decoded_now;
                let decoded = match kept.iter().find(|(key, _)| *key == key_of(photo)) {
                    Some((_, decoded)) => decoded,
                    None => {
                        decoded_now = decode(photo, ffmpeg)?;
                        &decoded_now
                    }
                };
                let (width, height) = (decoded.width() as f32, decoded.height() as f32);
                faces
                    .iter()
                    .map(|landmarks| {
                        let pixels = landmarks.map(|[x, y]| [x * width, y * height]);
                        model.embed(decoded, &pixels).map_err(WorkError::Model)
                    })
                    .collect()
            }))
            .unwrap_or_else(|_| Err(WorkError::Photo("reading it failed unexpectedly".into())))
        })
    }

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
        /// Keeps the search at its first photo while set.
        held: Arc<AtomicBool>,
        /// What happened to which photo, in order.
        steps: Arc<std::sync::Mutex<Vec<String>>>,
        /// Photos looked at at once.
        batch: usize,
        /// How many photos each batch had.
        batches: Arc<std::sync::Mutex<Vec<usize>>>,
    }

    impl FaceWork for Fake {
        fn detect(&mut self, _detector: &str, photo: &FacePhoto) -> Result<Vec<Detection>, WorkError> {
            while self.held.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(2));
            }
            if self.broken_model.load(Ordering::SeqCst) {
                return Err(WorkError::Model("broken".into()));
            }
            self.detected.fetch_add(1, Ordering::SeqCst);
            self.steps.lock().unwrap().push(format!("detect {}", photo.rel_path));
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

        fn recognize(
            &mut self,
            _: &str,
            photo: &FacePhoto,
            faces: &[[[f32; 2]; 5]],
        ) -> Result<Vec<Vec<f32>>, WorkError> {
            self.recognized.fetch_add(faces.len(), Ordering::SeqCst);
            self.steps.lock().unwrap().push(format!("recognize {}", photo.rel_path));
            Ok(vec![vec![1.0, 0.0]; faces.len()])
        }

        fn batch(&self) -> usize {
            self.batch.max(1)
        }

        fn detect_many(&mut self, detector: &str, photos: &[FacePhoto]) -> Vec<Result<Vec<Detection>, WorkError>> {
            self.batches.lock().unwrap().push(photos.len());
            photos
                .iter()
                .map(|photo| {
                    std::panic::catch_unwind(AssertUnwindSafe(|| self.detect(detector, photo)))
                        .unwrap_or_else(|_| Err(WorkError::Photo("reading it failed unexpectedly".into())))
                })
                .collect()
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
    fn req_032_04_looks_for_nothing_without_a_chosen_model() {
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

    /// REQ-032-20: the numbers are there while the first photo, and with it the loading of the models, still takes time.
    #[test]
    fn req_032_20_counts_the_photos_before_the_first_is_done() {
        let (_dir, conn, path) = index();
        for name in ["a.jpg", "b.jpg", "c.jpg"] {
            add_photo(&conn, name);
        }
        choose(&conn, faces::DETECTOR_KEY, "d");
        let fake = Fake::default();
        fake.held.store(true, Ordering::SeqCst);
        let service = start(&path, &fake, true);
        let mut status = service.status();
        for _ in 0..1000 {
            if status.counted {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
            status = service.status();
        }
        fake.held.store(false, Ordering::SeqCst);
        assert!(status.counted, "still not counted");
        assert_eq!(status.progress.photos, 3);
        assert_eq!(status.progress.photos_done, 0);
        assert!(status.working);
        service.wait_idle();
        assert_eq!(service.status().progress.photos_done, 3);
    }

    #[test]
    fn req_032_05_works_through_all_photos_and_a_bad_file_costs_only_itself() {
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

    /// REQ-032-23: a photo's faces are recognized before the next photo, so suggestions show during the first pass and
    /// the photo is still decoded.
    #[test]
    fn req_032_23_recognizes_a_photos_faces_right_after_looking_at_it() {
        let (_dir, conn, path) = index();
        for name in ["a.jpg", "bad.jpg", "c.jpg"] {
            add_photo(&conn, name);
        }
        choose(&conn, faces::DETECTOR_KEY, "d");
        choose(&conn, faces::RECOGNIZER_KEY, "r");
        let fake = Fake::default();
        let service = start(&path, &fake, true);
        service.wait_idle();
        let steps = fake.steps.lock().unwrap().clone();
        assert_eq!(steps.len(), 5, "{steps:?}");
        for name in ["a.jpg", "c.jpg"] {
            let detect = steps.iter().position(|step| step == &format!("detect {name}")).unwrap();
            assert_eq!(steps[detect + 1], format!("recognize {name}"), "{steps:?}");
        }
    }

    /// REQ-032-23: without a recognition model the search only looks for faces, and recognizes them once one is chosen.
    #[test]
    fn req_032_23_only_looks_without_a_recognition_model_and_catches_up_with_one() {
        let (_dir, conn, path) = index();
        add_photo(&conn, "a.jpg");
        choose(&conn, faces::DETECTOR_KEY, "d");
        let fake = Fake::default();
        let service = start(&path, &fake, true);
        service.wait_idle();
        assert_eq!(*fake.steps.lock().unwrap(), ["detect a.jpg"]);

        choose(&conn, faces::RECOGNIZER_KEY, "r");
        service.kick();
        service.wait_idle();
        assert_eq!(*fake.steps.lock().unwrap(), ["detect a.jpg", "recognize a.jpg"]);
    }

    #[test]
    fn req_032_05_stops_for_an_unreachable_share_or_when_paused() {
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
    fn req_032_05_a_model_that_cant_run_says_so_and_a_panic_costs_only_that_file() {
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
    fn req_032_16_another_recognition_model_recognizes_again_and_going_back_repeats_nothing() {
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

    /// REQ-032-05, REQ-032-06: everything for real but the network: a model in the store, a photo stored on its side, and
    /// the search finding its face where the upright photo shows it.
    #[test]
    fn req_032_06_finds_the_face_of_a_turned_photo_with_the_real_model() {
        let data = tempfile::tempdir().unwrap();
        let library = tempfile::tempdir().unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../media/tests/fixtures/faces");
        let store = FaceStore::in_data_folder(data.path());
        let kind = ModelKind::Detect(DetectorFormat::YuNet);
        let model = store
            .add_file(
                &fixtures.join("face_detection_yunet_2023mar.onnx"),
                &|_| Ok(kind),
                &mut |_, _| true,
            )
            .unwrap();
        // The pixels turned a quarter to the left, with the tag saying how to turn them back.
        let upright = image::open(fixtures.join("astronaut.jpg")).unwrap();
        upright.rotate270().save(library.path().join("turned.jpg")).unwrap();
        let Some(exiftool) = media::exiftool::find_exiftool() else {
            return;
        };
        let tagged = std::process::Command::new(exiftool)
            .args(["-q", "-overwrite_original", "-Orientation#=6"])
            .arg(library.path().join("turned.jpg"))
            .status()
            .unwrap();
        assert!(tagged.success());

        let path = data.path().join("gallery.db");
        let conn = db::open(&path).unwrap();
        libraries::insert(&conn, library.path().to_str().unwrap(), "Photos", LibraryStatus::Online).unwrap();
        add_photo(&conn, "turned.jpg");
        choose(&conn, faces::DETECTOR_KEY, &model.sha256);
        let service = FaceService::start(FaceDeps {
            conn: db::open(&path).unwrap(),
            work: Box::new(RunModels::new(data.path(), None)),
            is_reachable: Arc::new(|_| true),
            on_change: Box::new(|| {}),
        });
        service.wait_idle();
        assert_eq!(service.status().problem, None);

        let found = faces::unknown_faces_on(&conn, 1, &model.sha256, "").unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        // By eye, her face's center is at about 225, 95 of the upright photo's 512 × 512 pixels.
        let [x, y, w, h] = found[0].rect;
        let (cx, cy) = ((x + w / 2.0) * 512.0, (y + h / 2.0) * 512.0);
        assert!((cx - 225.0).abs() < 40.0 && (cy - 95.0).abs() < 40.0, "{cx}, {cy}");
    }

    /// REQ-037-02: the search looks at photos in batches of as many as the work takes at once, then recognizes their
    /// faces while they are still decoded.
    #[test]
    fn req_037_02_looks_at_several_photos_at_once() {
        let (_dir, conn, path) = index();
        for name in ["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg", "f.jpg", "g.jpg"] {
            add_photo(&conn, name);
        }
        choose(&conn, faces::DETECTOR_KEY, "d");
        choose(&conn, faces::RECOGNIZER_KEY, "r");
        let fake = Fake {
            batch: 3,
            ..Fake::default()
        };
        let service = start(&path, &fake, true);
        service.wait_idle();
        assert_eq!(*fake.batches.lock().unwrap(), [3, 3, 1]);
        assert_eq!(fake.recognized.load(Ordering::SeqCst), 7);
        let steps = fake.steps.lock().unwrap().clone();
        let at = |step: &str| steps.iter().position(|done| done.starts_with(step)).unwrap();
        // Newest first: g, f and e together, then their faces, then the next batch.
        assert!(
            at("detect e") < at("recognize g") && at("recognize e") < at("detect d"),
            "{steps:?}"
        );
    }

    /// REQ-037-04: pausing stops the search between batches; the batch in flight is stored, the rest stays waiting.
    #[test]
    fn req_037_04_stops_between_batches() {
        let (_dir, conn, path) = index();
        for name in ["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg"] {
            add_photo(&conn, name);
        }
        choose(&conn, faces::DETECTOR_KEY, "d");
        let fake = Fake {
            batch: 2,
            ..Fake::default()
        };
        fake.held.store(true, Ordering::SeqCst);
        let service = start(&path, &fake, true);
        while fake.batches.lock().unwrap().is_empty() {
            std::thread::sleep(Duration::from_millis(2));
        }
        choose(&conn, faces::SEARCH_KEY, "paused");
        service.kick();
        fake.held.store(false, Ordering::SeqCst);
        service.wait_idle();
        let scanned: i64 = conn
            .query_row("SELECT count(*) FROM faces_scanned", [], |row| row.get(0))
            .unwrap();
        assert_eq!(scanned, 2, "the batch in flight");
        assert_eq!(*fake.batches.lock().unwrap(), [2]);
    }

    /// REQ-037-01: several photos at once find the same faces as one at a time, with the real model.
    #[test]
    fn req_037_01_several_at_once_find_the_same_faces_with_the_real_model() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../media/tests/fixtures/faces");
        let library = tempfile::tempdir().unwrap();
        let astronaut = image::open(fixtures.join("astronaut.jpg")).unwrap();
        let names = ["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg"];
        for (i, name) in names.iter().enumerate() {
            let photo = match i {
                0 => astronaut.clone(),
                1 => astronaut.fliph(),
                2 => astronaut.resize(300, 300, image::imageops::FilterType::Triangle),
                3 => astronaut.crop_imm(100, 0, 400, 300),
                _ => astronaut.rotate180(),
            };
            photo.save(library.path().join(name)).unwrap();
        }
        let found = |threads: usize| {
            let data = tempfile::tempdir().unwrap();
            let store = FaceStore::in_data_folder(data.path());
            let kind = ModelKind::Detect(DetectorFormat::YuNet);
            let model = store
                .add_file(
                    &fixtures.join("face_detection_yunet_2023mar.onnx"),
                    &|_| Ok(kind),
                    &mut |_, _| true,
                )
                .unwrap();
            let path = data.path().join("gallery.db");
            let conn = db::open(&path).unwrap();
            libraries::insert(&conn, library.path().to_str().unwrap(), "Photos", LibraryStatus::Online).unwrap();
            for name in names {
                add_photo(&conn, name);
            }
            choose(&conn, faces::DETECTOR_KEY, &model.sha256);
            let service = FaceService::start(FaceDeps {
                conn: db::open(&path).unwrap(),
                work: Box::new(RunModels::new(data.path(), None).with_threads(threads)),
                is_reachable: Arc::new(|_| true),
                on_change: Box::new(|| {}),
            });
            service.wait_idle();
            assert_eq!(service.status().problem, None);
            (1..=names.len() as i64)
                .map(|media| {
                    faces::unknown_faces_on(&conn, media, &model.sha256, "")
                        .unwrap()
                        .into_iter()
                        .map(|face| face.rect)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        let one_at_a_time = found(1);
        assert!(
            one_at_a_time.iter().filter(|faces| !faces.is_empty()).count() >= 3,
            "{one_at_a_time:?}"
        );
        assert_eq!(found(3), one_at_a_time);
    }

    #[test]
    fn req_032_06_detections_become_fractions_of_the_upright_photo() {
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
