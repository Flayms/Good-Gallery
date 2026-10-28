//! Thumbnail rendering in the background and on request: worker threads take jobs from one priority queue. Requests
//! from visible tiles come first, the most recent first, since earlier ones have likely scrolled away (REQ-005-07);
//! media items without thumbnails are rendered in the background, newest first (REQ-005-08).

use crate::thumb_cache::ThumbCache;
use db::libraries::{self, LibraryStatus};
use db::rusqlite::Connection;
use db::thumbs::{self, ThumbSource};
use media::thumbnail::{Renderer, Thumbnails};
use std::collections::HashSet;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Pending media items queued per background pass.
const BACKGROUND_BATCH: usize = 100;
const BACKGROUND_PRIORITY: u64 = 0;

/// Renders a media item's thumbnails; [`Renderer`] or a fake in tests.
pub trait RenderThumbnails: Send + Sync {
    fn render(&self, source: &ThumbSource) -> Result<Thumbnails, String>;
}

impl RenderThumbnails for Renderer {
    fn render(&self, source: &ThumbSource) -> Result<Thumbnails, String> {
        let file = source.file().ok_or("its indexed path leaves its library")?;
        Renderer::render(self, &file, source.kind, source.duration).map_err(|error| error.to_string())
    }
}

/// The answer to a [`ThumbnailService::request`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThumbnailReady {
    pub media: i64,
    /// The thumbnail files are in the cache. False if the item can't be rendered (or its library is offline, or the
    /// files couldn't be stored).
    pub stored: bool,
}

pub struct ThumbnailDeps {
    /// A write connection of its own.
    pub conn: Connection,
    pub cache: ThumbCache,
    pub render: Arc<dyn RenderThumbnails>,
    pub is_reachable: Arc<dyn Fn(&Path) -> bool + Send + Sync>,
    /// Render threads.
    pub workers: usize,
    /// Called on a worker thread for every answered request.
    pub on_ready: Box<dyn Fn(ThumbnailReady) + Send + Sync>,
}

struct Job {
    media: i64,
    priority: u64,
}

#[derive(Default)]
struct Queue {
    jobs: Vec<Job>,
    running: HashSet<i64>,
    /// Media items someone waits for: answered when done, and stored even into a full cache (REQ-005-12).
    requested: HashSet<i64>,
    last_priority: u64,
    /// Pending media items may be waiting: the next background pass looks for them.
    kicked: bool,
    /// Counts kicks, so a pass that started before a kick doesn't clear it.
    kicks: u64,
    refilling: bool,
    /// Libraries this service found unreachable: their queued items are skipped until the next kick (which follows
    /// the scan that finds them back).
    offline: HashSet<i64>,
    shutdown: bool,
}

impl Queue {
    fn idle(&self) -> bool {
        self.jobs.is_empty() && self.running.is_empty() && !self.kicked && !self.refilling
    }

    /// The highest priority job; the earliest of equals, so background jobs keep their order.
    fn take(&mut self) -> Option<Job> {
        let mut best: Option<usize> = None;
        for (index, job) in self.jobs.iter().enumerate() {
            if best.is_none_or(|b| job.priority > self.jobs[b].priority) {
                best = Some(index);
            }
        }
        best.map(|index| self.jobs.remove(index))
    }
}

struct Shared {
    queue: Mutex<Queue>,
    changed: Condvar,
    /// Render threads above the set number wait here, so the queue's notifications don't wake them.
    parked: Condvar,
    conn: Mutex<Connection>,
    cache: ThumbCache,
    render: Arc<dyn RenderThumbnails>,
    is_reachable: Arc<dyn Fn(&Path) -> bool + Send + Sync>,
    on_ready: Box<dyn Fn(ThumbnailReady) + Send + Sync>,
    /// Render threads that may work; threads numbered above wait (REQ-010-04).
    workers: AtomicUsize,
}

pub struct ThumbnailService {
    shared: Arc<Shared>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl ThumbnailService {
    /// Starts the render threads. Background rendering waits for the first [`Self::kick`].
    pub fn start(deps: ThumbnailDeps) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            changed: Condvar::new(),
            parked: Condvar::new(),
            conn: Mutex::new(deps.conn),
            cache: deps.cache,
            render: deps.render,
            is_reachable: deps.is_reachable,
            on_ready: deps.on_ready,
            workers: AtomicUsize::new(deps.workers.max(1)),
        });
        let service = Self {
            shared: shared.clone(),
            workers: Mutex::new(Vec::new()),
        };
        service.set_workers(deps.workers);
        // Background passes wait until the cache is measured (an empty-looking cache would be overfilled), but no
        // render thread waits for it: requests go on meanwhile.
        let weak = Arc::downgrade(&shared);
        let cache = shared.cache.clone();
        std::thread::Builder::new()
            .name("thumbnails start".into())
            .spawn(move || {
                cache.wait_ready();
                if let Some(shared) = weak.upgrade() {
                    drop(shared.lock());
                    shared.changed.notify_all();
                }
            })
            .expect("can't start a thumbnail thread");
        service
    }

    /// Renders with `workers` threads from now on (REQ-010-04): renders running on threads above the new number
    /// finish, then those threads wait.
    pub fn set_workers(&self, workers: usize) {
        let workers = workers.max(1);
        self.shared.workers.store(workers, Ordering::Relaxed);
        let mut threads = self.workers.lock().unwrap_or_else(|p| p.into_inner());
        while threads.len() < workers {
            let (shared, index) = (self.shared.clone(), threads.len());
            threads.push(
                std::thread::Builder::new()
                    .name(format!("thumbnails {index}"))
                    .spawn(move || work(&shared, index))
                    .expect("can't start a thumbnail thread"),
            );
        }
        drop(threads);
        drop(self.shared.lock());
        self.shared.changed.notify_all();
        self.shared.parked.notify_all();
    }

    pub fn cache(&self) -> &ThumbCache {
        &self.shared.cache
    }

    /// Renders a media item's thumbnails ahead of background work, the latest request first (REQ-005-07). Answered
    /// through `on_ready`. Items whose thumbnail failed (`thumb_status = 'error'`) shouldn't be requested again: they
    /// would fail again.
    pub fn request(&self, media: i64) {
        let mut queue = self.shared.lock();
        queue.requested.insert(media);
        queue.last_priority += 1;
        let priority = queue.last_priority;
        if queue.running.contains(&media) {
            return;
        }
        match queue.jobs.iter_mut().find(|job| job.media == media) {
            Some(job) => job.priority = priority,
            None => queue.jobs.push(Job { media, priority }),
        }
        drop(queue);
        self.shared.changed.notify_all();
    }

    /// Looks for media items without thumbnails and renders them in the background (REQ-005-08). Call it at start and
    /// whenever media items were written.
    pub fn kick(&self) {
        let mut queue = self.shared.lock();
        queue.kicked = true;
        queue.kicks += 1;
        queue.offline.clear();
        drop(queue);
        self.shared.changed.notify_all();
    }

    /// Blocks until all queued work is done (for tests and tools).
    pub fn wait_idle(&self) {
        let queue = self.shared.lock();
        drop(
            self.shared
                .changed
                .wait_while(queue, |queue| !queue.idle())
                .unwrap_or_else(|p| p.into_inner()),
        );
    }
}

impl Drop for ThumbnailService {
    fn drop(&mut self) {
        self.shared.lock().shutdown = true;
        self.shared.changed.notify_all();
        self.shared.parked.notify_all();
        // A render can wait on a hung share or a slow ffmpeg; don't hang the app's exit for it.
        let deadline = Instant::now() + Duration::from_secs(2);
        let threads = std::mem::take(self.workers.get_mut().unwrap_or_else(|p| p.into_inner()));
        for worker in threads {
            while !worker.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            if worker.is_finished() {
                let _ = worker.join();
            }
        }
    }
}

struct Rendered {
    source: ThumbSource,
    thumbnails: Thumbnails,
    /// Its files are in the cache.
    stored: bool,
}

enum Work {
    Render(i64),
    Refill(u64),
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The next work for render thread `index`; it waits while its number is above the set number of threads.
    fn next(&self, index: usize) -> Option<Work> {
        let mut queue = self.lock();
        loop {
            if queue.shutdown {
                return None;
            }
            let workers = self.workers.load(Ordering::Relaxed);
            if index >= workers {
                queue = self.parked.wait(queue).unwrap_or_else(|p| p.into_inner());
                continue;
            }
            // Topped up while shorter than the number of workers, so requests never wait behind a long queue.
            if queue.kicked && !queue.refilling && queue.jobs.len() < workers && self.cache.is_measured() {
                queue.refilling = true;
                return Some(Work::Refill(queue.kicks));
            }
            if let Some(job) = queue.take() {
                queue.running.insert(job.media);
                return Some(Work::Render(job.media));
            }
            queue = self.changed.wait(queue).unwrap_or_else(|p| p.into_inner());
        }
    }

    /// Queues the next pending media items.
    fn refill(&self, kicks: u64) {
        let exclude: Vec<i64> = {
            let queue = self.lock();
            queue
                .jobs
                .iter()
                .map(|job| job.media)
                .chain(queue.running.iter().copied())
                .collect()
        };
        let pending = thumbs::pending_thumbs(&self.conn(), BACKGROUND_BATCH, &exclude);
        let mut queue = self.lock();
        queue.refilling = false;
        match pending {
            Ok(ids) => {
                if ids.is_empty() && queue.kicks == kicks {
                    queue.kicked = false;
                }
                for media in ids {
                    if !queue.running.contains(&media) && !queue.jobs.iter().any(|job| job.media == media) {
                        queue.jobs.push(Job {
                            media,
                            priority: BACKGROUND_PRIORITY,
                        });
                    }
                }
            }
            Err(error) => {
                // The next kick tries again.
                tracing::error!("Thumbnails: can't list pending media items: {error}");
                queue.kicked = false;
            }
        }
        drop(queue);
        self.changed.notify_all();
    }

    /// Answers a request for the item. One that arrived while it rendered into a full cache gets its files stored
    /// now: checked under the lock that also ends the job, so no request falls between.
    fn finish(&self, media: i64, mut rendered: Option<Rendered>) {
        let requested = loop {
            let mut queue = self.lock();
            if let Some(late) = rendered.take_if(|r| !r.stored && queue.requested.contains(&media)) {
                drop(queue);
                let stored = self.store_files(&late.source, &late.thumbnails);
                rendered = Some(Rendered { stored, ..late });
                if stored {
                    continue;
                }
                queue = self.lock();
            }
            queue.running.remove(&media);
            break queue.requested.remove(&media);
        };
        self.changed.notify_all();
        if requested {
            let stored = rendered.is_some_and(|r| r.stored);
            (self.on_ready)(ThumbnailReady { media, stored });
        }
    }

    fn store_files(&self, source: &ThumbSource, thumbnails: &Thumbnails) -> bool {
        let written = thumbnails
            .images
            .iter()
            .try_for_each(|(width, data)| self.cache.write(&self.cache.path(&source.key, *width), data));
        // Not the file's fault (e.g. a full disk), so it isn't marked failed.
        written
            .inspect_err(|error| tracing::error!("Thumbnails: can't cache {}: {error}", source.key.rel_path))
            .is_ok()
    }

    /// Renders one media item; `None` if it couldn't be.
    fn render(&self, media: i64) -> Option<Rendered> {
        let source = match thumbs::thumb_source(&self.conn(), media) {
            Ok(Some(source)) => source,
            // Removed meanwhile.
            Ok(None) => return None,
            Err(error) => {
                tracing::error!("Thumbnails: can't read media item {media}: {error}");
                return None;
            }
        };
        // Its library went offline since the item was queued: it stays pending.
        if self.lock().offline.contains(&source.key.root_id) {
            return None;
        }
        let rendered = std::panic::catch_unwind(AssertUnwindSafe(|| self.render.render(&source)))
            .unwrap_or_else(|panic| Err(format!("renderer crashed: {}", crate::indexer::panic_message(&panic))));
        let thumbnails = match rendered {
            Ok(thumbnails) => thumbnails,
            Err(error) => {
                if (self.is_reachable)(Path::new(&source.root_path)) {
                    // Not retried until the file changes (REQ-005-09).
                    tracing::warn!("Thumbnails: can't render {}: {error}", source.key.rel_path);
                    self.store_outcome(&source, None);
                } else {
                    // The item stays pending and is rendered once the library is back.
                    self.lock().offline.insert(source.key.root_id);
                    if let Err(error) = libraries::set_status(&self.conn(), source.key.root_id, LibraryStatus::Offline)
                    {
                        tracing::error!("Thumbnails: can't mark library {} offline: {error}", source.key.root_id);
                    }
                }
                return None;
            }
        };

        // A full cache only takes thumbnails someone is looking at; background work still yields the placeholder
        // (REQ-005-12).
        let requested = self.lock().requested.contains(&media);
        let stored = (requested || self.cache.has_room()) && self.store_files(&source, &thumbnails);
        self.store_outcome(&source, Some(&thumbnails.thumbhash));
        Some(Rendered {
            source,
            thumbnails,
            stored,
        })
    }

    fn store_outcome(&self, source: &ThumbSource, thumbhash: Option<&str>) {
        if let Err(error) = thumbs::set_thumbnail(&self.conn(), source, thumbhash) {
            tracing::error!(
                "Thumbnails: can't store the outcome for {}: {error}",
                source.key.rel_path
            );
        }
    }
}

fn work(shared: &Shared, index: usize) {
    while let Some(work) = shared.next(index) {
        match work {
            Work::Refill(kicks) => shared.refill(kicks),
            Work::Render(media) => {
                let rendered = shared.render(media);
                shared.finish(media, rendered);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{Receiver, Sender, channel};

    /// Records what it renders; can fail, panic, or wait for the test before rendering.
    #[derive(Default)]
    struct FakeRender {
        rendered: Mutex<Vec<String>>,
        fail: Mutex<bool>,
        panic: Mutex<bool>,
        /// Each render first reports its name here, then waits for a message on `gate`.
        started: Mutex<Option<Sender<String>>>,
        gate: Mutex<Option<Receiver<()>>>,
        /// Renders running now, the most at once, and how long each takes.
        active: AtomicUsize,
        peak: AtomicUsize,
        delay: Mutex<Duration>,
    }

    impl RenderThumbnails for FakeRender {
        fn render(&self, source: &ThumbSource) -> Result<Thumbnails, String> {
            let name = source.key.rel_path.clone();
            if let Some(started) = &*self.started.lock().unwrap() {
                started.send(name.clone()).unwrap();
                self.gate.lock().unwrap().as_ref().unwrap().recv().unwrap();
            }
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(active, Ordering::SeqCst);
            std::thread::sleep(*self.delay.lock().unwrap());
            self.active.fetch_sub(1, Ordering::SeqCst);
            self.rendered.lock().unwrap().push(name);
            if *self.panic.lock().unwrap() {
                panic!("decoder crashed");
            }
            if *self.fail.lock().unwrap() {
                return Err("corrupt".into());
            }
            Ok(Thumbnails {
                thumbhash: "aGFzaA==".into(),
                images: vec![(400, b"small".to_vec()), (800, b"large".to_vec())],
            })
        }
    }

    struct Fixture {
        dir: tempfile::TempDir,
        conn: Connection,
        root: i64,
        render: Arc<FakeRender>,
        ready: Receiver<ThumbnailReady>,
        service: ThumbnailService,
    }

    impl Fixture {
        fn new(cache_bytes: u64) -> Self {
            Self::with(cache_bytes, Arc::new(|_: &Path| true), FakeRender::default())
        }

        fn with(cache_bytes: u64, is_reachable: Arc<dyn Fn(&Path) -> bool + Send + Sync>, render: FakeRender) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let db = dir.path().join("gallery.db");
            let conn = db::open(&db).unwrap();
            let root = libraries::insert(&conn, dir.path().to_str().unwrap(), "Test", LibraryStatus::Online)
                .unwrap()
                .id;
            let render = Arc::new(render);
            let (tx, ready) = channel();
            let tx = Mutex::new(tx);
            let service = ThumbnailService::start(ThumbnailDeps {
                conn: db::open(&db).unwrap(),
                cache: ThumbCache::open(dir.path().join("cache"), cache_bytes),
                render: render.clone(),
                is_reachable,
                workers: 1,
                on_ready: Box::new(move |ready| {
                    let _ = tx.lock().unwrap().send(ready);
                }),
            });
            Self {
                dir,
                conn,
                root,
                render,
                ready,
                service,
            }
        }

        fn insert(&self, rel_path: &str, taken_at: i64) -> i64 {
            self.conn
                .query_row(
                    "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime, taken_at)
                     VALUES (?1, ?2, ?2, 'image', 1, 0, ?3) RETURNING id",
                    (self.root, rel_path, taken_at),
                    |row| row.get(0),
                )
                .unwrap()
        }

        fn states(&self) -> Vec<(String, String, Option<String>)> {
            let mut stmt = self
                .conn
                .prepare("SELECT rel_path, thumb_status, thumbhash FROM media ORDER BY rel_path")
                .unwrap();
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        }

        fn cached_files(&self) -> usize {
            fn count(dir: &Path) -> usize {
                std::fs::read_dir(dir).map_or(0, |entries| {
                    entries
                        .flatten()
                        .map(|e| if e.path().is_dir() { count(&e.path()) } else { 1 })
                        .sum()
                })
            }
            count(&self.dir.path().join("cache"))
        }

        fn rendered(&self) -> Vec<String> {
            self.render.rendered.lock().unwrap().clone()
        }

        fn next_ready(&self) -> ThumbnailReady {
            self.ready.recv_timeout(Duration::from_secs(10)).unwrap()
        }
    }

    fn ready(rel_path: &str) -> (String, String, Option<String>) {
        (rel_path.into(), "ready".into(), Some("aGFzaA==".into()))
    }

    #[test]
    fn req_010_04_renders_with_as_many_threads_as_set() {
        let fixture = Fixture::new(1 << 20);
        *fixture.render.delay.lock().unwrap() = Duration::from_millis(30);
        fixture.service.set_workers(3);
        for i in 0..12 {
            fixture.insert(&format!("{i}.jpg"), i);
        }
        fixture.service.kick();
        fixture.service.wait_idle();
        assert_eq!(fixture.render.peak.load(Ordering::SeqCst), 3);

        fixture.service.set_workers(1);
        fixture.render.peak.store(0, Ordering::SeqCst);
        for i in 12..18 {
            fixture.insert(&format!("{i}.jpg"), i);
        }
        fixture.service.kick();
        fixture.service.wait_idle();
        assert_eq!(fixture.render.peak.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.rendered().len(), 18);
    }

    #[test]
    fn req_005_08_req_005_06_renders_pending_media_in_the_background_newest_first() {
        let fixture = Fixture::new(1024);
        fixture.insert("old.jpg", 1);
        fixture.insert("new.jpg", 2);

        fixture.service.kick();
        fixture.service.wait_idle();

        assert_eq!(fixture.rendered(), ["new.jpg", "old.jpg"]);
        assert_eq!(fixture.states(), [ready("new.jpg"), ready("old.jpg")]);
        assert_eq!(fixture.cached_files(), 4);
    }

    /// REQ-005-08: media items written while a scan runs are rendered after the next kick.
    #[test]
    fn req_005_08_picks_up_media_written_later() {
        let fixture = Fixture::new(1024);
        fixture.insert("first.jpg", 1);
        fixture.service.kick();
        fixture.service.wait_idle();

        fixture.insert("later.jpg", 2);
        fixture.service.kick();
        fixture.service.wait_idle();

        assert_eq!(fixture.rendered(), ["first.jpg", "later.jpg"]);
    }

    #[test]
    fn req_005_07_runs_the_latest_request_first() {
        let (started_tx, started) = channel();
        let (gate_tx, gate) = channel();
        let render = FakeRender {
            started: Mutex::new(Some(started_tx)),
            gate: Mutex::new(Some(gate)),
            ..Default::default()
        };
        let fixture = Fixture::with(1024, Arc::new(|_: &Path| true), render);
        let ids: Vec<i64> = ["a.jpg", "b.jpg", "c.jpg", "d.jpg"]
            .iter()
            .enumerate()
            .map(|(i, name)| fixture.insert(name, i as i64))
            .collect();
        fixture.service.kick();
        // d.jpg (the newest) is running when the requests arrive.
        assert_eq!(started.recv_timeout(Duration::from_secs(10)).unwrap(), "d.jpg");

        fixture.service.request(ids[0]);
        fixture.service.request(ids[1]);
        for _ in 0..4 {
            gate_tx.send(()).unwrap();
        }
        fixture.service.wait_idle();

        assert_eq!(fixture.rendered(), ["d.jpg", "b.jpg", "a.jpg", "c.jpg"]);
        let mut answers = vec![fixture.next_ready(), fixture.next_ready()];
        answers.sort_by_key(|answer| answer.media);
        assert_eq!(
            answers,
            [
                ThumbnailReady {
                    media: ids[0],
                    stored: true
                },
                ThumbnailReady {
                    media: ids[1],
                    stored: true
                }
            ]
        );
    }

    #[test]
    fn req_005_09_req_005_14_marks_unrenderable_media_as_failed_but_only_while_the_library_is_reachable() {
        let reachable = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let is_reachable = {
            let reachable = reachable.clone();
            Arc::new(move |_: &Path| reachable.load(std::sync::atomic::Ordering::Relaxed))
        };
        let render = FakeRender {
            fail: Mutex::new(true),
            ..Default::default()
        };
        let fixture = Fixture::with(1024, is_reachable, render);
        let broken = fixture.insert("broken.jpg", 1);

        fixture.service.request(broken);
        assert_eq!(
            fixture.next_ready(),
            ThumbnailReady {
                media: broken,
                stored: false
            }
        );
        assert_eq!(fixture.states()[0].1, "error");

        reachable.store(false, std::sync::atomic::Ordering::Relaxed);
        let offline = fixture.insert("offline.jpg", 2);
        fixture.service.request(offline);
        assert_eq!(
            fixture.next_ready(),
            ThumbnailReady {
                media: offline,
                stored: false
            }
        );
        assert_eq!(fixture.states()[1].1, "pending");
        let library = libraries::get(&fixture.conn, fixture.root).unwrap().unwrap();
        assert_eq!(library.status, LibraryStatus::Offline);
    }

    /// REQ-005-09: once a library is found offline, its other queued items aren't tried (each would wait for the
    /// share), until the next kick.
    #[test]
    fn req_005_09_skips_queued_items_of_a_library_found_offline() {
        let render = FakeRender {
            fail: Mutex::new(true),
            ..Default::default()
        };
        let fixture = Fixture::with(1024, Arc::new(|_: &Path| false), render);
        fixture.insert("a.jpg", 1);
        fixture.insert("b.jpg", 2);

        fixture.service.kick();
        fixture.service.wait_idle();

        assert_eq!(fixture.rendered(), ["b.jpg"]);
        assert!(fixture.states().iter().all(|(_, status, _)| status == "pending"));
    }

    /// REQ-005-09, REQ-002-13: a crashing decoder costs that item only.
    #[test]
    fn req_005_09_a_panic_marks_the_item_failed_and_rendering_goes_on() {
        let render = FakeRender {
            panic: Mutex::new(true),
            ..Default::default()
        };
        let fixture = Fixture::with(1024, Arc::new(|_: &Path| true), render);
        fixture.insert("a.jpg", 1);
        fixture.insert("b.jpg", 2);

        fixture.service.kick();
        fixture.service.wait_idle();

        assert_eq!(fixture.rendered(), ["b.jpg", "a.jpg"]);
        assert!(fixture.states().iter().all(|(_, status, _)| status == "error"));
    }

    #[test]
    fn req_005_12_stores_only_requested_thumbnails_once_the_cache_is_full() {
        // Exactly the size of one rendered pair: full after the first, but not over the cap.
        let fixture = Fixture::new(10);
        let requested = fixture.insert("requested.jpg", 1);
        fixture.insert("first.jpg", 3);
        fixture.insert("second.jpg", 2);
        fixture
            .conn
            .execute("UPDATE media SET thumb_status = 'ready' WHERE id = ?1", [requested])
            .unwrap();

        fixture.service.kick();
        fixture.service.wait_idle();
        assert_eq!(fixture.cached_files(), 2, "only the first background item is stored");
        let states = fixture.states();
        assert_eq!([&states[0], &states[2]], [&ready("first.jpg"), &ready("second.jpg")]);

        fixture.service.request(requested);
        assert_eq!(
            fixture.next_ready(),
            ThumbnailReady {
                media: requested,
                stored: true
            }
        );
        // Now over the cap, so eviction removes the least recently written files; the requested item's last written
        // file is the newest of all.
        let source = thumbs::thumb_source(&fixture.conn, requested).unwrap().unwrap();
        assert!(fixture.service.cache().path(&source.key, 800).exists());
    }

    /// REQ-005-08: background passes go on until nothing is pending, beyond one batch.
    #[test]
    fn req_005_08_renders_more_than_one_batch() {
        let fixture = Fixture::new(1 << 20);
        for i in 0..(BACKGROUND_BATCH as i64 + 30) {
            fixture.insert(&format!("{i}.jpg"), i);
        }

        fixture.service.kick();
        fixture.service.wait_idle();

        assert_eq!(fixture.rendered().len(), BACKGROUND_BATCH + 30);
        assert!(fixture.states().iter().all(|(_, status, _)| status == "ready"));
    }

    #[test]
    fn a_removed_media_item_is_answered_as_not_stored() {
        let fixture = Fixture::new(1024);
        fixture.service.request(12345);
        assert_eq!(
            fixture.next_ready(),
            ThumbnailReady {
                media: 12345,
                stored: false
            }
        );
        assert!(fixture.rendered().is_empty());
    }
}
