//! Content search's pass (spec 036): one thread works through the photos and videos the chosen model hasn't looked at,
//! newest first, from their cached thumbnails. It reads the choice from the settings each round, so choosing another
//! model needs only a [`ContentService::kick`] (REQ-036-08).

use crate::thumb_cache::ThumbCache;
use db::content::{self, ContentItem, ContentProgress};
use db::rusqlite::Connection;
use media::content::{ContentStore, ImageHalf};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const ITEMS_PER_ROUND: usize = 20;
/// How long to wait for a kick when there is nothing to do; a share may have come back.
const IDLE: Duration = Duration::from_secs(60);

/// Runs the model. [`RunContent`] or a fake in tests.
pub trait ContentWork: Send {
    /// What `model` makes of `item`.
    fn vector(&mut self, model: &str, item: &ContentItem) -> Result<Vec<f32>, WorkError>;

    /// Nothing is left to do for now: what's only kept for the next item can go.
    fn rest(&mut self) {}
}

#[derive(Debug, PartialEq)]
pub enum WorkError {
    /// This file can't be read; it isn't tried again until it changes.
    Item(String),
    /// Neither a thumbnail nor the file is there now; tried again later.
    Away,
    /// The model can't run, so nothing can.
    Model(String),
}

/// What the pass is doing, for the settings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ContentStatus {
    pub progress: ContentProgress,
    /// Whether `progress` has been counted yet.
    pub counted: bool,
    /// Working on something right now.
    pub working: bool,
    pub problem: Option<String>,
}

pub struct ContentDeps {
    /// A write connection of its own.
    pub conn: Connection,
    pub work: Box<dyn ContentWork>,
    /// Called on its thread when the status changes.
    pub on_change: Box<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
struct Control {
    kicked: bool,
    shutdown: bool,
    status: ContentStatus,
}

struct Shared {
    control: Mutex<Control>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Control> {
        self.control.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn interrupted(&self) -> bool {
        let control = self.lock();
        control.kicked || control.shutdown
    }
}

pub struct ContentService {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl ContentService {
    pub fn start(deps: ContentDeps) -> Self {
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
                .name("content".into())
                .spawn(move || run(&shared, deps))
                .expect("can't start the content thread")
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

    pub fn status(&self) -> ContentStatus {
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

impl Drop for ContentService {
    fn drop(&mut self) {
        self.shared.lock().shutdown = true;
        self.shared.changed.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(shared: &Shared, mut deps: ContentDeps) {
    // Items that were away this time, passed over until the next kick.
    let mut skip = 0;
    loop {
        {
            let mut control = shared.lock();
            if control.shutdown {
                return;
            }
            if control.kicked {
                skip = 0;
            }
            control.kicked = false;
            control.status.working = true;
            let progress = count(&deps.conn);
            control.status.counted = progress.is_some();
            control.status.progress = progress.unwrap_or_default();
        }
        (deps.on_change)();
        let round = std::panic::catch_unwind(AssertUnwindSafe(|| round(shared, &mut deps, &mut skip)));
        let (more, problem) = match round {
            Ok(Ok(more)) => (more, None),
            Ok(Err(problem)) => (false, Some(problem)),
            Err(_) => (false, Some("Content search stopped unexpectedly.".to_owned())),
        };
        let progress = count(&deps.conn);
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
        // A model takes hundreds of megabytes, which an idle pass shouldn't hold.
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

/// How far the pass has come; `None` without a chosen model or if it can't be counted.
fn count(conn: &Connection) -> Option<ContentProgress> {
    let model = content::chosen(conn, content::MODEL_KEY).ok().flatten()?;
    content::progress(conn, &model).ok()
}

/// One round of work; whether there may be more.
fn round(shared: &Shared, deps: &mut ContentDeps, skip: &mut usize) -> Result<bool, String> {
    let chosen = |key| content::chosen(&deps.conn, key).ok().flatten();
    if chosen(content::PASS_KEY).as_deref() == Some("paused") {
        return Ok(false);
    }
    let Some(model) = chosen(content::MODEL_KEY) else {
        return Ok(false);
    };
    let items = content::pending(&deps.conn, &model, ITEMS_PER_ROUND, *skip).map_err(|error| error.to_string())?;
    for item in &items {
        if shared.interrupted() {
            return Ok(true);
        }
        let work = &mut deps.work;
        let made = std::panic::catch_unwind(AssertUnwindSafe(|| work.vector(&model, item)))
            .unwrap_or_else(|_| Err(WorkError::Item("reading it failed unexpectedly".into())));
        let stored = match made {
            Ok(vector) => domain::content::quantize(&vector),
            Err(WorkError::Item(reason)) => {
                tracing::warn!("content not looked at in {}: {reason}", item.rel_path);
                Vec::new()
            }
            Err(WorkError::Away) => {
                *skip += 1;
                continue;
            }
            Err(WorkError::Model(reason)) => return Err(reason),
        };
        content::store(&deps.conn, item, &model, &stored).map_err(|error| error.to_string())?;
    }
    Ok(items.len() == ITEMS_PER_ROUND)
}

/// The thumbnail width the pass reads: the smaller one is larger than any model's input.
const THUMB_WIDTH: u32 = domain::thumbs::THUMB_WIDTHS[0];
/// The longest side a file is decoded at without a thumbnail.
const MAX_SIDE: u32 = 512;

/// The real work: the model from the store, each item from its cached thumbnail, else its file.
pub struct RunContent {
    store: ContentStore,
    thumbs: ThumbCache,
    ffmpeg: Option<PathBuf>,
    is_reachable: Arc<dyn Fn(&Path) -> bool + Send + Sync>,
    loaded: Option<(String, ImageHalf)>,
}

impl RunContent {
    pub fn new(
        data_folder: &Path,
        thumbs: ThumbCache,
        ffmpeg: Option<PathBuf>,
        is_reachable: Arc<dyn Fn(&Path) -> bool + Send + Sync>,
    ) -> Self {
        Self {
            store: ContentStore::in_data_folder(data_folder),
            thumbs,
            ffmpeg,
            is_reachable,
            loaded: None,
        }
    }

    fn image_half(&mut self, id: &str) -> Result<&ImageHalf, WorkError> {
        if self.loaded.as_ref().is_none_or(|(loaded, _)| loaded != id) {
            let model = domain::content::model(id)
                .filter(|model| self.store.has(model))
                .ok_or_else(|| WorkError::Model("The chosen model isn't on this computer.".into()))?;
            self.loaded = None;
            let half = ImageHalf::load(&self.store, model).map_err(WorkError::Model)?;
            self.loaded = Some((id.to_owned(), half));
        }
        self.loaded
            .as_ref()
            .map(|(_, half)| half)
            .ok_or_else(|| WorkError::Model("The chosen model isn't on this computer.".into()))
    }

    /// The item as the model should see it: its thumbnail, else its file if its library is reachable.
    fn picture(&self, item: &ContentItem) -> Result<image::RgbImage, WorkError> {
        if item.has_thumbnail {
            let path = self.thumbs.path(&item.thumb_key(), THUMB_WIDTH);
            if let Ok(Some(data)) = self.thumbs.read(&path)
                && let Ok(image) = image::load_from_memory_with_format(&data, image::ImageFormat::WebP)
            {
                return Ok(image.into_rgb8());
            }
        }
        if item.offline || !(self.is_reachable)(Path::new(&item.root_path)) {
            return Err(WorkError::Away);
        }
        if item.kind != domain::media::MediaKind::Image {
            // A video without its thumbnail waits for the thumbnail.
            return Err(WorkError::Away);
        }
        let file = item
            .file()
            .ok_or_else(|| WorkError::Item("its indexed path leaves its library".into()))?;
        let rgba = media::original::load_original(&file, self.ffmpeg.as_deref(), MAX_SIDE, item.sidecar_orientation)
            .map_err(|error| WorkError::Item(error.to_string()))?;
        Ok(image::DynamicImage::ImageRgba8(rgba).to_rgb8())
    }
}

impl ContentWork for RunContent {
    fn vector(&mut self, model: &str, item: &ContentItem) -> Result<Vec<f32>, WorkError> {
        self.image_half(model)?;
        let picture = self.picture(item)?;
        let half = self.image_half(model)?;
        half.vector(&picture).map_err(WorkError::Model)
    }

    fn rest(&mut self) {
        self.loaded = None;
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

    fn add(conn: &Connection, name: &str, kind: &str, taken: i64) {
        conn.execute(
            "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime, taken_at)
             VALUES (1, ?1, ?1, ?2, 7, 1000, ?3)",
            db::rusqlite::params![name, kind, taken],
        )
        .unwrap();
    }

    fn choose(conn: &Connection, key: &str, value: &str) {
        db::settings::set_value(conn, key, &serde_json::json!(value)).unwrap();
    }

    /// The same vector for every item; `bad` ones can't be read, `away` ones are away, `panic` ones panic.
    #[derive(Clone, Default)]
    struct Fake {
        done: Arc<Mutex<Vec<String>>>,
        broken: Arc<AtomicBool>,
        calls: Arc<AtomicUsize>,
    }

    impl ContentWork for Fake {
        fn vector(&mut self, _: &str, item: &ContentItem) -> Result<Vec<f32>, WorkError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.broken.load(Ordering::SeqCst) {
                return Err(WorkError::Model("broken".into()));
            }
            if item.rel_path.contains("away") {
                return Err(WorkError::Away);
            }
            if item.rel_path.contains("panic") {
                panic!("a hostile file");
            }
            self.done.lock().unwrap().push(item.rel_path.clone());
            if item.rel_path.contains("bad") {
                return Err(WorkError::Item("unreadable".into()));
            }
            Ok(vec![0.6, 0.8])
        }
    }

    fn start(path: &Path, fake: &Fake) -> ContentService {
        ContentService::start(ContentDeps {
            conn: db::open(path).unwrap(),
            work: Box::new(fake.clone()),
            on_change: Box::new(|| {}),
        })
    }

    fn vectors(conn: &Connection, model: &str) -> u64 {
        content::count(conn, model).unwrap()
    }

    /// REQ-036-03: without a chosen model nothing is looked at or stored.
    #[test]
    fn req_036_03_looks_at_nothing_without_a_model() {
        let (_dir, conn, path) = index();
        add(&conn, "a.jpg", "image", 1);
        let fake = Fake::default();
        let service = start(&path, &fake);
        service.wait_idle();
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        assert_eq!(vectors(&conn, "m"), 0);
        assert!(!service.status().counted);
    }

    /// REQ-036-04: photos and videos newest first, a bad file costs only itself, an away one is passed over and taken
    /// up after a kick, and a restart carries on.
    #[test]
    fn req_036_04_works_through_photos_and_videos_newest_first() {
        let (_dir, conn, path) = index();
        add(&conn, "old.jpg", "image", 1);
        add(&conn, "away.jpg", "image", 2);
        add(&conn, "bad.jpg", "image", 3);
        add(&conn, "clip.mp4", "video", 4);
        add(&conn, "panic.jpg", "image", 5);
        choose(&conn, content::MODEL_KEY, "m");
        let fake = Fake::default();
        let service = start(&path, &fake);
        service.wait_idle();
        assert_eq!(*fake.done.lock().unwrap(), ["clip.mp4", "bad.jpg", "old.jpg"]);
        let status = service.status();
        assert_eq!(status.progress, ContentProgress { done: 4, total: 5 });
        assert_eq!(status.problem, None);
        let mut stored = Vec::new();
        content::each_vector(&conn, "m", |_, vector| stored.push(vector.to_vec())).unwrap();
        assert_eq!(
            stored,
            [vec![76, 102], vec![76, 102]],
            "normalized, in 8 bits; bad and panic are empty"
        );

        let calls = fake.calls.load(Ordering::SeqCst);
        drop(service);
        let service = start(&path, &fake);
        service.wait_idle();
        assert_eq!(fake.calls.load(Ordering::SeqCst), calls + 1, "only the away one again");
        assert_eq!(service.status().progress.done, 4);
    }

    /// REQ-036-04, REQ-036-08: paused, nothing happens; another model looks again, and the first one's work stays.
    #[test]
    fn req_036_04_req_036_08_pauses_and_follows_the_chosen_model() {
        let (_dir, conn, path) = index();
        add(&conn, "a.jpg", "image", 1);
        choose(&conn, content::MODEL_KEY, "m");
        choose(&conn, content::PASS_KEY, "paused");
        let fake = Fake::default();
        let service = start(&path, &fake);
        service.wait_idle();
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);

        choose(&conn, content::PASS_KEY, "running");
        service.kick();
        service.wait_idle();
        assert_eq!(vectors(&conn, "m"), 1);

        choose(&conn, content::MODEL_KEY, "other");
        service.kick();
        service.wait_idle();
        assert_eq!((vectors(&conn, "m"), vectors(&conn, "other")), (1, 1));
        choose(&conn, content::MODEL_KEY, "m");
        service.kick();
        service.wait_idle();
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2, "going back repeats nothing");
    }

    /// REQ-036-04: a model that can't run says so and stops the pass.
    #[test]
    fn req_036_04_a_model_that_cant_run_says_so() {
        let (_dir, conn, path) = index();
        add(&conn, "a.jpg", "image", 1);
        choose(&conn, content::MODEL_KEY, "m");
        let fake = Fake::default();
        fake.broken.store(true, Ordering::SeqCst);
        let service = start(&path, &fake);
        service.wait_idle();
        assert_eq!(service.status().problem.as_deref(), Some("broken"));
        assert_eq!(vectors(&conn, "m"), 0);
    }

    /// REQ-036-04: the real work reads an item's cached thumbnail; without one, an offline item or a video is away.
    #[test]
    fn req_036_04_reads_the_cached_thumbnail_first() {
        let dir = tempfile::tempdir().unwrap();
        let thumbs = ThumbCache::open(dir.path().join("thumbs"), 1 << 30);
        let work = RunContent::new(dir.path(), thumbs.clone(), None, Arc::new(|_| false));
        let item = ContentItem {
            media: 1,
            kind: domain::media::MediaKind::Image,
            root_path: ROOT.into(),
            rel_path: "a.jpg".into(),
            size: 7,
            mtime: 1000,
            sidecar_orientation: None,
            root_id: 1,
            has_thumbnail: true,
            offline: true,
        };
        let mut webp = Vec::new();
        image::RgbImage::from_pixel(40, 30, image::Rgb([200, 10, 10]))
            .write_to(&mut std::io::Cursor::new(&mut webp), image::ImageFormat::WebP)
            .unwrap();
        thumbs
            .write(&thumbs.path(&item.thumb_key(), THUMB_WIDTH), &webp)
            .unwrap();
        let picture = work.picture(&item).unwrap();
        assert_eq!((picture.width(), picture.height()), (40, 30));
        assert_eq!(picture.get_pixel(5, 5)[0], 200);

        let gone = ContentItem {
            rel_path: "b.jpg".into(),
            ..item.clone()
        };
        assert_eq!(
            work.picture(&gone).err(),
            Some(WorkError::Away),
            "offline, no thumbnail"
        );
        let video = ContentItem {
            kind: domain::media::MediaKind::Video,
            has_thumbnail: false,
            offline: false,
            ..gone
        };
        assert_eq!(
            work.picture(&video).err(),
            Some(WorkError::Away),
            "a video waits for its thumbnail"
        );
    }
}
