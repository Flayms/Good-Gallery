//! The indexer: a scan queue and a thread that runs one scan at a time with the write connection. Commands are method
//! calls from any thread; results come back as [`IndexerEvent`]s.

use crate::scan::{MAX_CHANGED_PATHS, ScanDeps, ScanOptions, ScanOutcome, ScanProgress, ScanScope, scan_root};
use crate::watcher::{LibraryWatcher, WatchEvent};
use crate::{CurrentScan, IndexerEvent, IndexerStatus, ScanKind, ScanRequests, ScanResult};
use db::writer::IndexWriter;
use media::metadata::MetadataSource;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

/// A scope covering both scans of a library.
pub fn merge_scopes(a: ScanScope, b: ScanScope) -> ScanScope {
    match (a, b) {
        (ScanScope::Changes(mut paths), ScanScope::Changes(more)) => {
            for path in more {
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
            if paths.len() <= MAX_CHANGED_PATHS {
                ScanScope::Changes(paths)
            } else {
                ScanScope::Quick
            }
        }
        (a, b) => {
            if a.kind() >= b.kind() {
                a
            } else {
                b
            }
        }
    }
}

struct Job {
    library: i64,
    scope: ScanScope,
}

struct Running {
    library: i64,
    kind: ScanKind,
    cancel: Arc<AtomicBool>,
    progress: ScanProgress,
}

#[derive(Default)]
struct State {
    jobs: Vec<Job>,
    running: Option<Running>,
    shutdown: bool,
}

impl State {
    fn status(&self) -> IndexerStatus {
        IndexerStatus {
            current: self.running.as_ref().map(|running| CurrentScan {
                library: running.library,
                kind: running.kind,
                scanned: running.progress.scanned,
                indexed: running.progress.indexed,
            }),
            queue: self.jobs.iter().map(|job| job.library).collect(),
        }
    }
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    events: Mutex<Sender<IndexerEvent>>,
    /// Tells the UI to look at the events (e.g. requests a repaint).
    notify: Box<dyn Fn() + Send + Sync>,
    on_written: Box<dyn Fn() + Send + Sync>,
    concurrency: AtomicUsize,
    watchers: Mutex<HashMap<i64, LibraryWatcher>>,
}

impl Shared {
    fn enqueue(&self, library: i64, scope: ScanScope, coverable: bool) {
        let mut state = self.lock();
        // Changes may be in folders the running scan already passed, so only other requests can be covered by it.
        if let Some(running) = &state.running
            && running.library == library
            && coverable
            && running.kind >= scope.kind()
        {
            return;
        }
        if let Some(queued) = state.jobs.iter_mut().find(|job| job.library == library) {
            let current = std::mem::replace(&mut queued.scope, ScanScope::Full);
            queued.scope = merge_scopes(current, scope);
            return;
        }
        state.jobs.push(Job { library, scope });
        let status = state.status();
        drop(state);
        self.wake.notify_all();
        self.send(IndexerEvent::Status(status));
    }

    /// Starts watching a library after a completed scan; its reports become changes scans (REQ-003-01, -04, -05,
    /// -06). Failing to start leaves only the scheduled quick scans to notice changes.
    /// Runs on the indexer thread only, so no other start can race this one.
    fn watch(self: &Arc<Self>, library: i64, root: &Path) {
        if self
            .watchers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(&library)
        {
            return;
        }
        let weak = Arc::downgrade(self);
        // Not under the lock: setting up a recursive watch on a slow share takes a while.
        let started = LibraryWatcher::start(root, move |event| {
            if let Some(shared) = weak.upgrade() {
                shared.on_watch_event(library, event);
            }
        });
        match started {
            Ok(watcher) => {
                self.watchers
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(library, watcher);
            }
            // Queueing a scan here would follow every completed scan; the next scheduled quick scan tries again
            // (REQ-003-06).
            Err(error) => tracing::warn!("Indexer: can't watch {}: {error}", root.display()),
        }
    }

    /// Turns a watcher's report into a scan (REQ-003-04 … 06).
    fn on_watch_event(&self, library: i64, event: WatchEvent) {
        match event {
            WatchEvent::Changed(Some(paths)) if !paths.is_empty() && paths.len() <= MAX_CHANGED_PATHS => {
                self.enqueue(library, ScanScope::Changes(paths), false);
            }
            WatchEvent::Changed(_) => self.enqueue(library, ScanScope::Quick, false),
            WatchEvent::Failed(error) => {
                tracing::warn!("Indexer: watching library {library} failed: {error}");
                self.unwatch(library);
                // Changes may have been missed; the scan also notices if the library went offline.
                self.enqueue(library, ScanScope::Quick, false);
            }
        }
    }

    fn unwatch(&self, library: i64) {
        let watcher = self.watchers.lock().unwrap_or_else(|p| p.into_inner()).remove(&library);
        drop(watcher);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn send(&self, event: IndexerEvent) {
        let _ = self.events.lock().unwrap_or_else(|p| p.into_inner()).send(event);
        (self.notify)();
    }
}

/// Everything a scan needs besides the queue.
pub struct IndexerDeps {
    pub writer: IndexWriter,
    pub metadata: Arc<dyn MetadataSource>,
    pub is_reachable: Arc<dyn Fn(&Path) -> bool + Send + Sync>,
    /// Parallel file reads.
    pub concurrency: usize,
    pub notify: Box<dyn Fn() + Send + Sync>,
    /// Called after a scan wrote files and after every scan, on the indexer thread: starts background rendering of
    /// new thumbnails (`ThumbnailService::kick`).
    pub on_written: Box<dyn Fn() + Send + Sync>,
}

pub struct Indexer {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl Indexer {
    /// Starts the indexer thread. Events arrive on the returned receiver.
    pub fn start(deps: IndexerDeps) -> (Self, Receiver<IndexerEvent>) {
        let (tx, rx) = channel();
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            events: Mutex::new(tx),
            notify: deps.notify,
            on_written: deps.on_written,
            concurrency: AtomicUsize::new(deps.concurrency.max(1)),
            watchers: Mutex::new(HashMap::new()),
        });
        let worker_shared = shared.clone();
        let (writer, metadata, is_reachable) = (deps.writer, deps.metadata, deps.is_reachable);
        let worker = std::thread::Builder::new()
            .name("indexer".into())
            .spawn(move || run(&worker_shared, writer, metadata.as_ref(), is_reachable.as_ref()))
            .expect("can't start the indexer thread");
        (
            Self {
                shared,
                worker: Some(worker),
            },
            rx,
        )
    }

    pub fn status(&self) -> IndexerStatus {
        self.shared.lock().status()
    }

    /// Queues a scan: one waiting scan per library, merged with an earlier request; dropped if the running scan of
    /// the library already covers it (REQ-002-07).
    pub fn request(&self, library: i64, scope: ScanScope) {
        let coverable = !matches!(scope, ScanScope::Changes(_));
        self.enqueue(library, scope, coverable);
    }

    /// Unknown changes (lost watcher events): a quick scan, which a running scan doesn't cover, since it may already
    /// have passed the changed folders.
    pub fn request_unknown_changes(&self, library: i64) {
        self.enqueue(library, ScanScope::Quick, false);
    }

    fn enqueue(&self, library: i64, scope: ScanScope, coverable: bool) {
        self.shared.enqueue(library, scope, coverable);
    }

    /// Whether the library is being watched for changes (after a completed scan).
    #[cfg(test)]
    fn is_watching(&self, library: i64) -> bool {
        self.shared
            .watchers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(&library)
    }

    /// Parallel file reads of the next scan; a running scan keeps its value (REQ-010-04).
    pub fn set_concurrency(&self, concurrency: usize) {
        self.shared.concurrency.store(concurrency.max(1), Ordering::Relaxed);
    }
}

impl ScanRequests for Indexer {
    fn request_paths(&self, library: i64, paths: Vec<String>) {
        self.request(library, ScanScope::Changes(paths));
    }

    fn request_scan(&self, library: i64, kind: ScanKind) {
        match kind {
            ScanKind::Full => self.request(library, ScanScope::Full),
            ScanKind::Quick => self.request(library, ScanScope::Quick),
            // Changes without paths are unknown changes: a quick scan finds them.
            ScanKind::Changes => self.request_unknown_changes(library),
        }
    }

    /// Drops the library's queued scan and cancels its running one, which then writes nothing more.
    fn remove_library(&self, library: i64) {
        let mut state = self.shared.lock();
        state.jobs.retain(|job| job.library != library);
        if let Some(running) = state.running.as_ref().filter(|running| running.library == library) {
            running.cancel.store(true, Ordering::Relaxed);
        }
        let status = state.status();
        // Wait until a running scan of the library has stopped writing.
        let state = self
            .shared
            .wake
            .wait_while(state, |state| {
                state.running.as_ref().is_some_and(|r| r.library == library)
            })
            .unwrap_or_else(|p| p.into_inner());
        drop(state);
        // After the scan: one that completed anyway has started its watcher by now. Not under the state lock, since
        // ending a watch can block.
        self.shared.unwatch(library);
        self.shared.send(IndexerEvent::Status(status));
    }
}

impl Drop for Indexer {
    fn drop(&mut self) {
        {
            let mut state = self.shared.lock();
            state.shutdown = true;
            if let Some(running) = &state.running {
                running.cancel.store(true, Ordering::Relaxed);
            }
        }
        self.shared.wake.notify_all();
        // A scan blocked in a read on a hung share can take long to notice the cancel; don't hang the app's exit
        // for it. The thread ends with the process.
        if let Some(worker) = self.worker.take() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !worker.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            if worker.is_finished() {
                let _ = worker.join();
            }
        }
    }
}

/// The indexer thread: takes the next job, runs it, reports, repeats.
fn run(
    shared: &Arc<Shared>,
    mut writer: IndexWriter,
    metadata: &dyn MetadataSource,
    is_reachable: &(dyn Fn(&Path) -> bool + Sync),
) {
    loop {
        let (job, cancel) = {
            let mut state = shared.lock();
            loop {
                if state.shutdown {
                    return;
                }
                if !state.jobs.is_empty() {
                    break;
                }
                state = shared.wake.wait(state).unwrap_or_else(|p| p.into_inner());
            }
            let job = state.jobs.remove(0);
            let cancel = Arc::new(AtomicBool::new(false));
            state.running = Some(Running {
                library: job.library,
                kind: job.scope.kind(),
                cancel: cancel.clone(),
                progress: ScanProgress {
                    library: job.library,
                    ..Default::default()
                },
            });
            let status = state.status();
            drop(state);
            shared.send(IndexerEvent::Status(status));
            (job, cancel)
        };

        let mut last_indexed = 0;
        let mut on_progress = |progress: ScanProgress| {
            let status = {
                let mut state = shared.lock();
                if let Some(running) = &mut state.running {
                    running.progress = progress;
                }
                state.status()
            };
            shared.send(IndexerEvent::Status(status));
            if progress.indexed > last_indexed {
                last_indexed = progress.indexed;
                shared.send(IndexerEvent::Written(job.library));
                (shared.on_written)();
            }
        };
        let concurrency = shared.concurrency.load(Ordering::Relaxed);
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let deps = ScanDeps {
                writer: &mut writer,
                metadata,
                is_reachable,
            };
            scan_root(
                deps,
                job.library,
                &job.scope,
                ScanOptions::new(concurrency, &cancel, &mut on_progress),
            )
        }));
        let result = match outcome {
            Ok(Ok(ScanOutcome::Completed)) => ScanResult::Completed,
            Ok(Ok(ScanOutcome::Offline)) => ScanResult::Offline,
            Ok(Ok(ScanOutcome::Cancelled)) => ScanResult::Cancelled,
            Ok(Err(error)) => ScanResult::Failed(error.to_string()),
            Err(panic) => ScanResult::Failed(panic_message(&panic)),
        };
        match &result {
            // A cancelled library is being removed: don't watch it again.
            ScanResult::Completed if !cancel.load(Ordering::Relaxed) => {
                if let Ok(Some(library)) = writer.root(job.library) {
                    shared.watch(job.library, Path::new(&library.path));
                }
            }
            ScanResult::Offline => shared.unwatch(job.library),
            _ => {}
        }
        if let ScanResult::Failed(error) = &result {
            tracing::error!("Indexer: scan of library {} failed: {error}", job.library);
            // A panic may have left the writer's tag caches out of step with the rolled-back transaction.
            writer.clear_caches();
        }
        let status = {
            let mut state = shared.lock();
            state.running = None;
            state.status()
        };
        // Wakes `remove_library` calls waiting for this scan to stop.
        shared.wake.notify_all();
        // A library back online may have media items waiting for thumbnails.
        (shared.on_written)();
        shared.send(IndexerEvent::Done(job.library, result));
        shared.send(IndexerEvent::Status(status));
    }
}

pub(crate) fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use db::libraries::{self, LibraryStatus};
    use domain::media::MediaMetadata;
    use media::exiftool::ExifToolError;
    use std::time::Duration;

    #[test]
    fn req_002_07_merges_scopes_keeping_the_more_thorough_kind() {
        let changes = |paths: &[&str]| ScanScope::Changes(paths.iter().map(|p| p.to_string()).collect());
        assert_eq!(merge_scopes(ScanScope::Quick, ScanScope::Full), ScanScope::Full);
        assert_eq!(merge_scopes(ScanScope::Full, changes(&["a"])), ScanScope::Full);
        assert_eq!(merge_scopes(changes(&["a"]), ScanScope::Quick), ScanScope::Quick);
        assert_eq!(
            merge_scopes(changes(&["a", "b"]), changes(&["b", "c"])),
            changes(&["a", "b", "c"])
        );
        let many: Vec<String> = (0..MAX_CHANGED_PATHS).map(|i| i.to_string()).collect();
        assert_eq!(
            merge_scopes(ScanScope::Changes(many), changes(&["x"])),
            ScanScope::Quick
        );
    }

    /// Metadata that panics for one file name and blocks reads of another until released.
    #[derive(Default)]
    struct Scripted {
        panic_on: Mutex<Option<String>>,
        gate: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    }

    impl MetadataSource for Scripted {
        fn read(&self, file: &Path, _: Option<&Path>) -> Result<MediaMetadata, ExifToolError> {
            let name = file.file_name().unwrap().to_string_lossy().into_owned();
            if self.panic_on.lock().unwrap().as_deref() == Some(name.as_str()) {
                panic!("decoder crashed on {name}");
            }
            if name == "slow.jpg"
                && let Some(gate) = self.gate.lock().unwrap().take()
            {
                let _ = gate.recv();
            }
            Ok(MediaMetadata::default())
        }
    }

    struct Fixture {
        dirs: Vec<tempfile::TempDir>,
        libraries: Vec<i64>,
        indexer: Indexer,
        events: Receiver<IndexerEvent>,
        metadata: Arc<Scripted>,
        db: tempfile::TempDir,
    }

    impl Fixture {
        fn new(count: usize) -> Self {
            Self::with_reachability(count, Arc::new(|_: &Path| true))
        }

        fn with_reachability(count: usize, is_reachable: Arc<dyn Fn(&Path) -> bool + Send + Sync>) -> Self {
            let db = tempfile::tempdir().unwrap();
            let conn = db::open(&db.path().join("gallery.db")).unwrap();
            let dirs: Vec<_> = (0..count).map(|_| tempfile::tempdir().unwrap()).collect();
            let libraries = dirs
                .iter()
                .map(|dir| {
                    std::fs::write(dir.path().join("a.jpg"), "x").unwrap();
                    libraries::insert(&conn, dir.path().to_str().unwrap(), "L", LibraryStatus::Unknown)
                        .unwrap()
                        .id
                })
                .collect();
            let metadata = Arc::new(Scripted::default());
            let (indexer, events) = Indexer::start(IndexerDeps {
                writer: IndexWriter::new(conn),
                metadata: metadata.clone(),
                is_reachable,
                concurrency: 2,
                notify: Box::new(|| {}),
                on_written: Box::new(|| {}),
            });
            Self {
                dirs,
                libraries,
                indexer,
                events,
                metadata,
                db,
            }
        }

        /// The results of the next `n` finished scans, in order.
        fn done(&self, n: usize) -> Vec<(i64, ScanResult)> {
            let mut done = Vec::new();
            while done.len() < n {
                let event = self
                    .events
                    .recv_timeout(Duration::from_secs(10))
                    .expect("indexer stalled");
                if let IndexerEvent::Done(library, result) = event {
                    done.push((library, result));
                }
            }
            done
        }

        fn media_count(&self) -> i64 {
            let conn = db::open_reader(&self.db.path().join("gallery.db")).unwrap();
            conn.query_row("SELECT count(*) FROM media", [], |r| r.get(0)).unwrap()
        }
    }

    #[test]
    fn req_002_07_runs_scans_one_at_a_time_in_request_order() {
        let f = Fixture::new(3);
        let (tx, rx) = std::sync::mpsc::channel();
        *f.metadata.gate.lock().unwrap() = Some(rx);
        std::fs::write(f.dirs[0].path().join("slow.jpg"), "x").unwrap();

        f.indexer.request(f.libraries[0], ScanScope::Full);
        std::thread::sleep(Duration::from_millis(100));
        f.indexer.request(f.libraries[2], ScanScope::Quick);
        f.indexer.request(f.libraries[1], ScanScope::Full);
        f.indexer.request(f.libraries[2], ScanScope::Full);
        // Covered by the running full scan of the first library.
        f.indexer.request(f.libraries[0], ScanScope::Quick);
        assert_eq!(f.indexer.status().queue, [f.libraries[2], f.libraries[1]]);
        assert_eq!(
            f.indexer.status().current.map(|c| (c.library, c.kind)),
            Some((f.libraries[0], ScanKind::Full))
        );
        tx.send(()).unwrap();

        let order: Vec<i64> = f.done(3).into_iter().map(|(library, _)| library).collect();
        assert_eq!(order, [f.libraries[0], f.libraries[2], f.libraries[1]]);
        assert_eq!(f.media_count(), 4);
    }

    #[test]
    fn req_002_13_a_panicking_scan_counts_as_finished_and_later_scans_still_run() {
        // The first library's reachability check panics, which takes down its scan.
        let doomed: Arc<Mutex<Option<std::path::PathBuf>>> = Arc::default();
        let check = doomed.clone();
        let f = Fixture::with_reachability(
            2,
            Arc::new(move |path: &Path| {
                assert!(
                    check.lock().unwrap().as_deref() != Some(path),
                    "reachability check crashed"
                );
                true
            }),
        );
        *doomed.lock().unwrap() = Some(f.dirs[0].path().to_path_buf());

        f.indexer.request(f.libraries[0], ScanScope::Full);
        f.indexer.request(f.libraries[1], ScanScope::Full);

        let done = f.done(2);
        assert!(
            matches!(&done[0], (library, ScanResult::Failed(error)) if *library == f.libraries[0] && error.contains("crashed")),
            "{done:?}"
        );
        assert_eq!(done[1], (f.libraries[1], ScanResult::Completed));
    }

    #[test]
    fn req_002_13_a_panic_while_reading_one_file_costs_only_that_file() {
        let f = Fixture::new(1);
        std::fs::write(f.dirs[0].path().join("crash.jpg"), "x").unwrap();
        *f.metadata.panic_on.lock().unwrap() = Some("crash.jpg".into());

        f.indexer.request(f.libraries[0], ScanScope::Full);

        assert_eq!(f.done(1), [(f.libraries[0], ScanResult::Completed)]);
        assert_eq!(f.media_count(), 1);
    }

    #[test]
    fn req_003_01_req_003_04_watches_after_a_completed_scan_and_scans_reported_changes() {
        let f = Fixture::new(1);
        let library = f.libraries[0];
        f.indexer.request(library, ScanScope::Full);
        f.done(1);
        assert!(f.indexer.is_watching(library));

        std::fs::write(f.dirs[0].path().join("b.jpg"), "x").unwrap();

        // Settles after 2 s, then a changes scan picks the file up.
        let mut kinds = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            let event = f
                .events
                .recv_timeout(deadline - std::time::Instant::now())
                .expect("no changes scan");
            match event {
                IndexerEvent::Status(status) => kinds.extend(status.current.map(|current| current.kind)),
                IndexerEvent::Done(_, ScanResult::Completed) => break,
                _ => {}
            }
        }
        assert!(kinds.contains(&ScanKind::Changes), "{kinds:?}");
        assert_eq!(f.media_count(), 2);

        f.indexer.remove_library(library);
        assert!(!f.indexer.is_watching(library));
    }

    #[test]
    fn req_003_01_req_003_10_stops_watching_an_offline_library_and_watches_it_again_after_a_reconnect() {
        let online = Arc::new(AtomicBool::new(true));
        let check = online.clone();
        let f = Fixture::with_reachability(1, Arc::new(move |_: &Path| check.load(Ordering::Relaxed)));
        let library = f.libraries[0];
        f.indexer.request(library, ScanScope::Full);
        f.done(1);
        assert!(f.indexer.is_watching(library));

        online.store(false, Ordering::Relaxed);
        f.indexer.request(library, ScanScope::Quick);

        assert_eq!(f.done(1), [(library, ScanResult::Offline)]);
        assert!(!f.indexer.is_watching(library));

        online.store(true, Ordering::Relaxed);
        f.indexer.request(library, ScanScope::Quick);
        assert_eq!(f.done(1), [(library, ScanResult::Completed)]);
        assert!(f.indexer.is_watching(library));
    }

    /// Kinds of the scans started until `n` scans finished.
    fn started_kinds(f: &Fixture, n: usize) -> Vec<ScanKind> {
        let (mut kinds, mut done) = (Vec::new(), 0);
        while done < n {
            match f.events.recv_timeout(Duration::from_secs(10)).expect("indexer stalled") {
                IndexerEvent::Status(status) => {
                    if let Some(current) = status.current
                        && kinds.last() != Some(&current.kind)
                    {
                        kinds.push(current.kind);
                    }
                }
                IndexerEvent::Done(..) => done += 1,
                _ => {}
            }
        }
        kinds
    }

    #[test]
    fn req_003_06_a_failed_watch_stops_and_queues_a_quick_scan_that_watches_again() {
        let f = Fixture::new(2);
        let (library, busy) = (f.libraries[0], f.libraries[1]);
        f.indexer.request(library, ScanScope::Full);
        f.done(1);
        assert!(f.indexer.is_watching(library));
        // A scan of another library keeps the indexer busy until released.
        let (tx, rx) = std::sync::mpsc::channel();
        *f.metadata.gate.lock().unwrap() = Some(rx);
        std::fs::write(f.dirs[1].path().join("slow.jpg"), "x").unwrap();
        f.indexer.request(busy, ScanScope::Full);

        f.indexer
            .shared
            .on_watch_event(library, WatchEvent::Failed("share disconnected".into()));
        assert!(!f.indexer.is_watching(library));
        // The busy scan may not have started yet.
        assert_eq!(f.indexer.status().queue.last(), Some(&library));
        tx.send(()).unwrap();

        assert_eq!(started_kinds(&f, 2), [ScanKind::Full, ScanKind::Quick]);
        assert!(f.indexer.is_watching(library));
    }

    #[test]
    fn req_003_05_more_than_5000_changed_paths_queue_a_quick_scan() {
        let f = Fixture::new(1);
        let library = f.libraries[0];
        let many: Vec<String> = (0..=MAX_CHANGED_PATHS).map(|i| format!("{i}.jpg")).collect();

        f.indexer
            .shared
            .on_watch_event(library, WatchEvent::Changed(Some(many)));
        assert_eq!(started_kinds(&f, 1), [ScanKind::Quick]);

        f.indexer.shared.on_watch_event(library, WatchEvent::Changed(None));
        assert_eq!(started_kinds(&f, 1), [ScanKind::Quick]);

        f.indexer
            .shared
            .on_watch_event(library, WatchEvent::Changed(Some(vec!["a.jpg".into()])));
        assert_eq!(started_kinds(&f, 1), [ScanKind::Changes]);
    }

    /// REQ-003-07: changes reported while the library is being scanned queue a changes scan after it.
    #[test]
    fn req_003_07_changes_during_a_scan_are_scanned_after_it() {
        let f = Fixture::new(1);
        let (tx, rx) = std::sync::mpsc::channel();
        *f.metadata.gate.lock().unwrap() = Some(rx);
        std::fs::write(f.dirs[0].path().join("slow.jpg"), "x").unwrap();
        let library = f.libraries[0];
        f.indexer.request(library, ScanScope::Full);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while f.indexer.status().current.is_none() {
            assert!(std::time::Instant::now() < deadline, "the scan didn't start");
            std::thread::sleep(Duration::from_millis(10));
        }

        f.indexer.request(library, ScanScope::Changes(vec!["a.jpg".into()]));
        assert_eq!(f.indexer.status().queue, [library]);
        tx.send(()).unwrap();

        assert_eq!(
            f.done(2),
            [(library, ScanResult::Completed), (library, ScanResult::Completed)]
        );
    }

    #[test]
    fn req_001_05_removing_a_library_drops_its_queued_scan_and_cancels_the_running_one() {
        let f = Fixture::new(2);
        let (tx, rx) = std::sync::mpsc::channel();
        *f.metadata.gate.lock().unwrap() = Some(rx);
        std::fs::write(f.dirs[0].path().join("slow.jpg"), "x").unwrap();
        f.indexer.request(f.libraries[0], ScanScope::Full);
        std::thread::sleep(Duration::from_millis(100));
        f.indexer.request(f.libraries[1], ScanScope::Full);

        f.indexer.remove_library(f.libraries[1]);
        // Removing waits for the running scan to stop, which needs its blocked read to finish.
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            tx.send(()).unwrap();
        });
        f.indexer.remove_library(f.libraries[0]);
        assert!(
            f.indexer.status().current.is_none(),
            "removal returns once the scan has stopped"
        );
        release.join().unwrap();

        assert_eq!(f.done(1), [(f.libraries[0], ScanResult::Cancelled)]);
        assert!(f.indexer.status().queue.is_empty());
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            f.events
                .try_iter()
                .all(|event| !matches!(event, IndexerEvent::Done(..)))
        );
    }
}
