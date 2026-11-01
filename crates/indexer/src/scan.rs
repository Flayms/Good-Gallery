//! Syncs one library with the index: new or changed files (size, mtime, sidecar mtime, metadata version) are read
//! again, vanished files and folders removed. Files in folders that couldn't be listed are kept. Read files are written
//! in batches while the scan runs; removals happen once it completes.
//!
//! Threads: the calling thread walks the library and owns all index access; `concurrency` workers stat files and read
//! their metadata.

use crate::retry::{is_transient, mtime_ms, with_retry};
use crate::walk::{Descend, DirListing, Skip, Walk, WalkItem};
use db::libraries::LibraryStatus;
use db::writer::{ExistingEntry, IndexWriter, IndexedFile};
use domain::media::{METADATA_VERSION, MediaKind, MediaMetadata, find_sidecar};
use media::metadata::MetadataSource;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Upper bound of paths per changes scan; more changes than that are cheaper to find with a quick scan.
pub const MAX_CHANGED_PATHS: usize = 5000;

/// What a scan looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanScope {
    /// Lists every folder and stats every file.
    Full,
    /// Only re-lists folders whose mtime changed since they were last listed; a full scan without folder data.
    Quick,
    /// Paths (relative, `/`-separated) the watcher reported: re-lists their folders, stats new files and the
    /// reported ones, and walks new subfolders.
    Changes(Vec<String>),
}

impl ScanScope {
    pub fn kind(&self) -> crate::ScanKind {
        match self {
            Self::Full => crate::ScanKind::Full,
            Self::Quick => crate::ScanKind::Quick,
            Self::Changes(_) => crate::ScanKind::Changes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanOutcome {
    Completed,
    Offline,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScanProgress {
    pub library: i64,
    /// Files seen so far.
    pub scanned: u64,
    /// New or changed files written to the index so far.
    pub indexed: u64,
}

pub struct ScanOptions<'a> {
    /// Parallel file operations (stat + metadata).
    pub concurrency: usize,
    /// Files written per transaction.
    pub batch_size: usize,
    /// Longest time read files wait to be written, so they show up while a slow scan still runs.
    pub flush_interval: Duration,
    pub progress_interval: Duration,
    /// How long a library that wasn't offline gets to answer before it counts as offline (REQ-003-11).
    pub reconnect_grace: Duration,
    /// Set when the library is removed: the scan stops and writes nothing more.
    pub cancel: &'a AtomicBool,
    pub on_progress: &'a mut dyn FnMut(ScanProgress),
}

impl<'a> ScanOptions<'a> {
    pub fn new(concurrency: usize, cancel: &'a AtomicBool, on_progress: &'a mut dyn FnMut(ScanProgress)) -> Self {
        Self {
            concurrency,
            batch_size: 250,
            flush_interval: Duration::from_secs(2),
            progress_interval: Duration::from_millis(250),
            reconnect_grace: crate::reachability::RECONNECT_GRACE,
            cancel,
            on_progress,
        }
    }
}

pub struct ScanDeps<'a> {
    pub writer: &'a mut IndexWriter,
    pub metadata: &'a dyn MetadataSource,
    pub is_reachable: &'a (dyn Fn(&Path) -> bool + Sync),
}

/// Mtime stored for folders that couldn't be listed, so the next quick scan lists them again.
const UNLISTED: i64 = -1;

fn is_within(rel_path: &str, rel_dir: &str) -> bool {
    rel_dir.is_empty() || rel_path.strip_prefix(rel_dir).is_some_and(|rest| rest.starts_with('/'))
}

fn parent_dir(rel_dir: &str) -> &str {
    rel_dir.rfind('/').map_or("", |slash| &rel_dir[..slash])
}

/// Known subfolders by parent folder.
fn child_index(stored: &HashMap<String, i64>) -> HashMap<String, Vec<String>> {
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    for rel_dir in stored.keys().filter(|dir| !dir.is_empty()) {
        children
            .entry(parent_dir(rel_dir).to_string())
            .or_default()
            .push(rel_dir.clone());
    }
    for siblings in children.values_mut() {
        siblings.sort();
    }
    children
}

/// For changes scans: whether a known file must be checked because it or one of its possible sidecars was reported.
/// Compared case-insensitively, which at worst checks a file too many.
fn is_touched(touched: &HashSet<String>, rel_path: &str) -> bool {
    let lower = rel_path.to_lowercase();
    let stem = match lower.rfind('.') {
        Some(dot) if dot > lower.rfind('/').map_or(0, |s| s + 1) => &lower[..dot],
        _ => lower.as_str(),
    };
    touched.contains(&lower) || touched.contains(&format!("{lower}.xmp")) || touched.contains(&format!("{stem}.xmp"))
}

struct Job {
    rel_path: String,
    abs_path: PathBuf,
    file_name: String,
    kind: MediaKind,
    sidecar: Option<PathBuf>,
    known: Option<ExistingEntry>,
}

enum JobResult {
    /// Stat succeeded; `None` when nothing changed. Boxed: most files are unchanged.
    Seen(String, Option<Box<IndexedFile>>),
    /// The file is gone; it stays unseen and is removed.
    Vanished,
    /// Couldn't be read; it keeps its last indexed state.
    Failed(String),
    /// Aborted (cancelled or offline).
    Skipped,
}

fn read_file(job: Job, deps: &WorkerDeps, root_path: &Path) -> JobResult {
    if deps.aborted() {
        return JobResult::Skipped;
    }
    let stats = match with_retry(deps.cancel, || std::fs::metadata(&job.abs_path)) {
        Ok(stats) => stats,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return JobResult::Vanished,
        Err(error) => {
            tracing::warn!("Indexer: cannot read {}: {error}", job.abs_path.display());
            let transient = is_transient(&error);
            if transient {
                deps.check_reachable(root_path);
            }
            return JobResult::Failed(job.rel_path);
        }
    };
    // A sidecar that vanished or can't be read counts as missing.
    let sidecar_stats = job
        .sidecar
        .as_ref()
        .and_then(|sidecar| with_retry(deps.cancel, || std::fs::metadata(sidecar)).ok());
    let size = stats.len() as i64;
    let mtime = mtime_ms(&stats);
    let sidecar_mtime = sidecar_stats.as_ref().map(mtime_ms);
    if let Some(known) = &job.known
        && known.size == size
        && known.mtime == mtime
        && known.sidecar_mtime == sidecar_mtime
        && known.meta_version == METADATA_VERSION
    {
        return JobResult::Seen(job.rel_path, None);
    }
    let sidecar = sidecar_stats.and(job.sidecar.as_deref());
    let metadata = match deps.metadata.read(&job.abs_path, sidecar) {
        Ok(metadata) => metadata,
        Err(error) => {
            deps.check_reachable(root_path);
            if deps.aborted() {
                return JobResult::Skipped;
            }
            tracing::warn!("Indexer: unreadable metadata in {}: {error}", job.abs_path.display());
            MediaMetadata::default()
        }
    };
    let file = IndexedFile {
        rel_path: job.rel_path.clone(),
        file_name: job.file_name,
        kind: job.kind,
        size,
        mtime,
        sidecar_mtime,
        metadata,
    };
    JobResult::Seen(job.rel_path, Some(Box::new(file)))
}

struct WorkerDeps<'a> {
    metadata: &'a dyn MetadataSource,
    is_reachable: &'a (dyn Fn(&Path) -> bool + Sync),
    cancel: &'a AtomicBool,
    offline: AtomicBool,
    /// Serializes reachability probes and remembers when the library last answered.
    last_reachable: std::sync::Mutex<Option<Instant>>,
}

/// After a successful probe, failures within this time are taken as bad files, not a dropped connection.
const PROBE_INTERVAL: Duration = Duration::from_secs(10);

impl WorkerDeps<'_> {
    fn aborted(&self) -> bool {
        self.cancel.load(Ordering::Relaxed) || self.offline.load(Ordering::Relaxed)
    }

    /// Distinguishes a dropped connection from a single bad file; the former stops the scan.
    /// One probe at a time, and none shortly after one succeeded, so many bad files don't flood the share.
    fn check_reachable(&self, root: &Path) {
        let mut last = self.last_reachable.lock().unwrap_or_else(|p| p.into_inner());
        if self.offline.load(Ordering::Relaxed) || last.is_some_and(|at| at.elapsed() < PROBE_INTERVAL) {
            return;
        }
        if (self.is_reachable)(root) {
            *last = Some(Instant::now());
        } else {
            self.offline.store(true, Ordering::Relaxed);
        }
    }
}

/// Syncs library `root_id` with the index. Errors are index failures (the scan counts as failed).
pub fn scan_root(deps: ScanDeps, root_id: i64, requested: &ScanScope, options: ScanOptions) -> db::Result<ScanOutcome> {
    let ScanDeps {
        writer,
        metadata,
        is_reachable,
    } = deps;
    let Some(root) = writer.root(root_id)? else {
        return Ok(ScanOutcome::Cancelled);
    };
    let root_path = PathBuf::from(&root.path);
    // A share that was online may still be reconnecting (after the computer started or woke up), so it gets a moment
    // before it counts as offline; an offline one is checked once, so retries don't hold up the queue (REQ-003-11).
    let reachable = if root.status == LibraryStatus::Offline {
        is_reachable(&root_path)
    } else {
        let cancel = options.cancel;
        crate::reachability::reachable_within(
            options.reconnect_grace,
            || cancel.load(Ordering::Relaxed),
            || is_reachable(&root_path),
        )
    };
    if options.cancel.load(Ordering::Relaxed) {
        return Ok(ScanOutcome::Cancelled);
    }
    if !reachable {
        writer.set_root_status(root_id, LibraryStatus::Offline, None)?;
        return Ok(ScanOutcome::Offline);
    }
    writer.set_root_status(root_id, LibraryStatus::Online, None)?;

    let stored = if *requested == ScanScope::Full {
        HashMap::new()
    } else {
        writer.folders(root_id)?
    };
    // Without folder data (new library, or no scan completed since) only a full scan finds everything.
    let scope = if stored.is_empty() { &ScanScope::Full } else { requested };
    let full = *scope == ScanScope::Full;
    let children = child_index(&stored);
    let touched: Option<HashSet<String>> = match scope {
        ScanScope::Changes(paths) => Some(paths.iter().map(|p| p.to_lowercase()).collect()),
        _ => None,
    };

    let worker_deps = WorkerDeps {
        metadata,
        is_reachable,
        cancel: options.cancel,
        offline: AtomicBool::new(false),
        last_reachable: std::sync::Mutex::new(None),
    };
    // Entries left over after their folder was listed no longer exist. A full scan preloads the whole library, so
    // files in vanished folders are left over too; other scans load each listed folder.
    let mut unseen = if full {
        writer.existing(root_id)?
    } else {
        HashMap::new()
    };
    let mut failed_dirs: Vec<String> = Vec::new();
    let mut removed_dirs: Vec<String> = Vec::new();
    let mut listed: HashMap<String, i64> = HashMap::new();
    let mut sink = Sink {
        writer,
        root_id,
        cancel: options.cancel,
        on_progress: options.on_progress,
        pending: Vec::new(),
        progress: ScanProgress {
            library: root_id,
            ..Default::default()
        },
        batch_size: options.batch_size,
        flush_interval: options.flush_interval,
        progress_interval: options.progress_interval,
        last_flush: Instant::now(),
        last_report: None,
    };

    let skip_quick: Skip = &|rel_dir, mtime| {
        (stored.get(rel_dir) == Some(&mtime)).then(|| children.get(rel_dir).cloned().unwrap_or_default())
    };
    let descend_new: Descend = &|listing: &DirListing| {
        listing
            .subdirs
            .iter()
            .filter(|dir| !stored.contains_key(*dir))
            .cloned()
            .collect()
    };
    let from = match scope {
        ScanScope::Changes(paths) => {
            let mut parents: Vec<String> = Vec::new();
            for path in paths {
                let parent = parent_dir(path).to_string();
                if !parents.contains(&parent) {
                    parents.push(parent);
                }
            }
            parents
        }
        _ => vec![String::new()],
    };
    let walk = Walk::new(&root_path, from, options.cancel);
    let walk = match scope {
        ScanScope::Full => walk,
        ScanScope::Quick => walk.skip(skip_quick),
        ScanScope::Changes(_) => walk.descend(descend_new),
    };

    let result: db::Result<()> = std::thread::scope(|threads| {
        let (job_tx, job_rx) = crossbeam_channel::bounded::<Job>(options.concurrency.max(1) * 4);
        let (result_tx, result_rx) = mpsc::channel::<JobResult>();
        for _ in 0..options.concurrency.max(1) {
            let (job_rx, result_tx) = (job_rx.clone(), result_tx.clone());
            let (worker_deps, root_path) = (&worker_deps, &root_path);
            threads.spawn(move || {
                for job in job_rx {
                    // A panic while reading one file (a decoder bug) costs that file, not the scan (REQ-002-13).
                    let rel_path = job.rel_path.clone();
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        read_file(job, worker_deps, root_path)
                    }))
                    .unwrap_or_else(|_| {
                        tracing::error!("Indexer: reading {rel_path} panicked");
                        JobResult::Failed(rel_path)
                    });
                    if result_tx.send(result).is_err() {
                        return;
                    }
                }
            });
        }
        drop(result_tx);
        let mut in_flight = 0usize;

        let walked = (|| -> db::Result<()> {
            for item in walk {
                if worker_deps.aborted() {
                    return Ok(());
                }
                let listing = match item {
                    WalkItem::Listing(listing) => listing,
                    WalkItem::Error(rel_dir, error) => {
                        if !rel_dir.is_empty() && error.kind() == io::ErrorKind::NotFound {
                            removed_dirs.push(rel_dir);
                            continue;
                        }
                        tracing::warn!("Indexer: cannot list {}\\{rel_dir}: {error}", root_path.display());
                        if is_transient(&error) {
                            worker_deps.check_reachable(&root_path);
                        }
                        listed.insert(rel_dir.clone(), UNLISTED);
                        failed_dirs.push(rel_dir);
                        continue;
                    }
                };
                listed.insert(listing.rel_dir.clone(), listing.mtime);
                if !full {
                    unseen.extend(sink.writer.existing_in(root_id, &listing.rel_dir)?);
                    let present: HashSet<&String> = listing.subdirs.iter().collect();
                    if let Some(known) = children.get(&listing.rel_dir) {
                        removed_dirs.extend(known.iter().filter(|dir| !present.contains(dir)).cloned());
                    }
                }
                for (name, kind) in &listing.files {
                    let rel_path = if listing.rel_dir.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{name}", listing.rel_dir)
                    };
                    let known = unseen.get(&rel_path).cloned();
                    if known.is_some()
                        && let Some(touched) = &touched
                        && !is_touched(touched, &rel_path)
                    {
                        unseen.remove(&rel_path);
                        continue;
                    }
                    let mut job = Job {
                        abs_path: listing.abs_dir.join(name),
                        sidecar: find_sidecar(&listing.sidecars, name).map(|s| listing.abs_dir.join(s)),
                        rel_path,
                        file_name: name.clone(),
                        kind: *kind,
                        known,
                    };
                    // Backpressure: wait for the workers instead of queueing a whole share up front.
                    loop {
                        if worker_deps.aborted() {
                            return Ok(());
                        }
                        match job_tx.try_send(job) {
                            Ok(()) => {
                                in_flight += 1;
                                break;
                            }
                            Err(crossbeam_channel::TrySendError::Full(back)) => {
                                job = back;
                                if let Ok(result) = result_rx.recv_timeout(Duration::from_millis(20)) {
                                    in_flight -= 1;
                                    sink.apply(result, &mut unseen);
                                }
                            }
                            Err(crossbeam_channel::TrySendError::Disconnected(_)) => return Ok(()),
                        }
                    }
                    while let Ok(result) = result_rx.try_recv() {
                        in_flight -= 1;
                        sink.apply(result, &mut unseen);
                    }
                    sink.flush_if_due()?;
                    sink.report();
                }
                // Folders without media (or skipped ones) still let finished reads through on time.
                while let Ok(result) = result_rx.try_recv() {
                    in_flight -= 1;
                    sink.apply(result, &mut unseen);
                }
                sink.flush_if_due()?;
                sink.report();
            }
            Ok(())
        })();
        drop(job_tx);
        // Drain the workers, still writing batches while they finish.
        let mut drained: db::Result<()> = Ok(());
        while in_flight > 0 {
            match result_rx.recv_timeout(options.flush_interval) {
                Ok(result) => {
                    in_flight -= 1;
                    sink.apply(result, &mut unseen);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if walked.is_ok() && drained.is_ok() {
                drained = sink.flush_if_due();
            }
        }
        walked?;
        drained?;
        sink.flush()
    });

    if options.cancel.load(Ordering::Relaxed) {
        return Ok(ScanOutcome::Cancelled);
    }
    result?;
    if worker_deps.offline.load(Ordering::Relaxed) {
        sink.writer.set_root_status(root_id, LibraryStatus::Offline, None)?;
        return Ok(ScanOutcome::Offline);
    }

    let stale: Vec<i64> = unseen
        .iter()
        .filter(|(rel_path, _)| !failed_dirs.iter().any(|dir| is_within(rel_path, dir)))
        .map(|(_, entry)| entry.id)
        .collect();
    sink.writer.remove(&stale)?;
    let vanished: Vec<String> = removed_dirs
        .into_iter()
        .filter(|dir| !listed.contains_key(dir))
        .collect();
    sink.writer.remove_folders(root_id, &vanished)?;
    sink.writer.save_folders(root_id, &listed, full)?;
    sink.writer.prune_tags()?;
    sink.writer
        .set_root_status(root_id, LibraryStatus::Online, Some(now_ms()))?;
    (sink.on_progress)(sink.progress);
    Ok(ScanOutcome::Completed)
}

/// Collects read files and writes them in batches; reports progress.
struct Sink<'a> {
    writer: &'a mut IndexWriter,
    root_id: i64,
    cancel: &'a AtomicBool,
    on_progress: &'a mut dyn FnMut(ScanProgress),
    pending: Vec<IndexedFile>,
    progress: ScanProgress,
    batch_size: usize,
    flush_interval: Duration,
    progress_interval: Duration,
    last_flush: Instant,
    last_report: Option<Instant>,
}

impl Sink<'_> {
    fn apply(&mut self, result: JobResult, unseen: &mut HashMap<String, ExistingEntry>) {
        match result {
            JobResult::Seen(rel_path, file) => {
                unseen.remove(&rel_path);
                self.progress.scanned += 1;
                self.pending.extend(file.map(|file| *file));
            }
            JobResult::Failed(rel_path) => {
                unseen.remove(&rel_path);
            }
            JobResult::Vanished | JobResult::Skipped => {}
        }
    }

    fn flush_if_due(&mut self) -> db::Result<()> {
        if self.pending.len() >= self.batch_size || self.last_flush.elapsed() >= self.flush_interval {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> db::Result<()> {
        self.last_flush = Instant::now();
        // A removed library cancels its scan; its rows are gone, so nothing may be written anymore.
        if self.pending.is_empty() || self.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let batch = std::mem::take(&mut self.pending);
        self.writer.write(self.root_id, &batch)?;
        self.progress.indexed += batch.len() as u64;
        self.last_report = Some(Instant::now());
        (self.on_progress)(self.progress);
        Ok(())
    }

    fn report(&mut self) {
        if self.last_report.is_none_or(|at| at.elapsed() >= self.progress_interval) {
            self.last_report = Some(Instant::now());
            (self.on_progress)(self.progress);
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use db::libraries;
    use media::exiftool::ExifToolError;
    use std::fs;
    use std::sync::{Arc, Mutex};

    /// Returns metadata by file name and records which files were read (`name+xmp` with a sidecar).
    #[derive(Default)]
    struct FakeMetadata {
        by_name: Mutex<HashMap<String, MediaMetadata>>,
        reads: Mutex<Vec<String>>,
        /// Reads of this file wait until the receiver gets a message.
        blocked: Mutex<Option<(String, mpsc::Receiver<()>)>>,
        failing: Mutex<HashSet<String>>,
    }

    impl FakeMetadata {
        fn set(&self, name: &str, metadata: MediaMetadata) {
            self.by_name.lock().unwrap().insert(name.into(), metadata);
        }

        fn take_reads(&self) -> Vec<String> {
            let mut reads = std::mem::take(&mut *self.reads.lock().unwrap());
            reads.sort();
            reads
        }
    }

    impl MetadataSource for FakeMetadata {
        fn read(&self, file: &Path, sidecar: Option<&Path>) -> Result<MediaMetadata, ExifToolError> {
            let name = file.file_name().unwrap().to_str().unwrap().to_string();
            self.reads.lock().unwrap().push(if sidecar.is_some() {
                format!("{name}+xmp")
            } else {
                name.clone()
            });
            let blocked = self.blocked.lock().unwrap().take();
            match blocked {
                Some((blocked_name, rx)) if blocked_name == name => {
                    let _ = rx.recv();
                }
                other => *self.blocked.lock().unwrap() = other,
            }
            if self.failing.lock().unwrap().contains(&name) {
                return Err(ExifToolError::Failed("broken".into()));
            }
            Ok(self.by_name.lock().unwrap().get(&name).cloned().unwrap_or_default())
        }
    }

    struct Fixture {
        dir: tempfile::TempDir,
        writer: IndexWriter,
        root: i64,
        metadata: FakeMetadata,
        reachable: Arc<AtomicBool>,
        /// Short, so tests of unreachable libraries don't wait out the real grace.
        reconnect_grace: Duration,
    }

    /// 2020-01-01.
    const PAST: i64 = 1_577_836_800_000;

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let conn = db::open_in_memory().unwrap();
            let root = libraries::insert(&conn, dir.path().to_str().unwrap(), "Test", LibraryStatus::Unknown)
                .unwrap()
                .id;
            Self {
                dir,
                writer: IndexWriter::new(conn),
                root,
                metadata: FakeMetadata::default(),
                reachable: Arc::new(AtomicBool::new(true)),
                reconnect_grace: Duration::from_millis(100),
            }
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.dir.path().join(rel)
        }

        fn touch(&self, rel: &str, content: &str) {
            let path = self.path(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, content).unwrap();
        }

        fn touch_past(&self, rel: &str, content: &str) {
            self.touch(rel, content);
            set_mtime(&self.path(rel), PAST);
        }

        /// Dates folders back, so later changes in them are noticed by their mtime.
        fn age(&self, rel_dirs: &[&str]) {
            for rel in rel_dirs {
                set_mtime(&self.path(rel), PAST);
            }
        }

        fn scan_with(&mut self, scope: ScanScope, concurrency: usize, flush: Duration) -> (ScanOutcome, Vec<u64>) {
            let cancel = AtomicBool::new(false);
            let mut indexed = Vec::new();
            let mut on_progress = |p: ScanProgress| indexed.push(p.indexed);
            let reachable = self.reachable.clone();
            let is_reachable = move |_: &Path| reachable.load(Ordering::Relaxed);
            let mut options = ScanOptions::new(concurrency, &cancel, &mut on_progress);
            options.flush_interval = flush;
            options.reconnect_grace = self.reconnect_grace;
            let deps = ScanDeps {
                writer: &mut self.writer,
                metadata: &self.metadata,
                is_reachable: &is_reachable,
            };
            let outcome = scan_root(deps, self.root, &scope, options).unwrap();
            (outcome, indexed)
        }

        fn scan(&mut self, scope: ScanScope) -> ScanOutcome {
            self.scan_with(scope, 2, Duration::from_secs(2)).0
        }

        fn paths(&self) -> Vec<String> {
            let mut paths: Vec<String> = self.writer.existing(self.root).unwrap().into_keys().collect();
            paths.sort();
            paths
        }

        fn folders(&self) -> Vec<String> {
            let mut folders: Vec<String> = self.writer.folders(self.root).unwrap().into_keys().collect();
            folders.sort();
            folders
        }

        fn value<T: db::rusqlite::types::FromSql>(&self, sql: &str) -> T {
            self.writer.conn().query_row(sql, [], |r| r.get(0)).unwrap()
        }
    }

    /// Sets a file's or folder's modification time (folders need backup semantics to be opened on Windows).
    fn set_mtime(path: &Path, ms: i64) {
        let time = std::time::UNIX_EPOCH + Duration::from_millis(ms as u64);
        let mut options = fs::File::options();
        // Other systems can't open folders for writing, and setting the times of a file one owns needs no write access.
        options.read(true).write(cfg!(windows));
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
            options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
        }
        options.open(path).unwrap().set_modified(time).unwrap();
    }

    fn with_size(width: i64, height: i64, taken_at: i64) -> MediaMetadata {
        MediaMetadata {
            width: Some(width),
            height: Some(height),
            taken_at: Some(taken_at),
            ..Default::default()
        }
    }

    #[test]
    fn req_002_01_req_002_02_req_002_03_indexes_media_recursively_and_skips_housekeeping_files() {
        let mut f = Fixture::new();
        for rel in [
            "a.jpg",
            "trip/b.MP4",
            "trip/notes.txt",
            "trip/._b.MP4",
            "@eaDir/a.jpg/SYNO.jpg",
            ".hidden/c.jpg",
        ] {
            f.touch(rel, "x");
        }
        f.metadata.set("a.jpg", with_size(30, 20, 1000));

        let (outcome, progress) = f.scan_with(ScanScope::Full, 2, Duration::from_secs(2));

        assert_eq!(outcome, ScanOutcome::Completed);
        assert_eq!(f.paths(), ["a.jpg", "trip/b.MP4"]);
        assert_eq!(
            f.value::<String>("SELECT kind FROM media WHERE rel_path = 'a.jpg'"),
            "image"
        );
        assert_eq!(f.value::<i64>("SELECT width FROM media WHERE rel_path = 'a.jpg'"), 30);
        assert_eq!(
            f.value::<i64>("SELECT sort_date FROM media WHERE rel_path = 'a.jpg'"),
            1000
        );
        assert_eq!(
            f.value::<String>("SELECT kind FROM media WHERE rel_path = 'trip/b.MP4'"),
            "video"
        );
        assert_eq!(progress.last(), Some(&2));
        let library = f.writer.root(f.root).unwrap().unwrap();
        assert_eq!(library.status, LibraryStatus::Online);
        assert!(library.last_scan_at.is_some());
    }

    #[test]
    fn req_002_04_req_002_14_req_004_10_req_005_10_rescans_incrementally() {
        let mut f = Fixture::new();
        for rel in ["same.jpg", "changed.jpg", "sidecar.jpg", "gone.jpg"] {
            f.touch_past(rel, "x");
        }
        let old_tag = MediaMetadata {
            tags: vec![vec!["Old".into()]],
            ..Default::default()
        };
        f.metadata.set("gone.jpg", old_tag);
        f.scan(ScanScope::Full);
        f.writer
            .conn()
            .execute("UPDATE media SET thumb_status = 'ready', thumbhash = 'abc'", [])
            .unwrap();
        f.metadata.take_reads();

        f.touch("changed.jpg", "xy");
        f.touch("sidecar.xmp", "x");
        fs::remove_file(f.path("gone.jpg")).unwrap();
        f.touch("new.png", "x");
        let outcome = f.scan(ScanScope::Full);

        assert_eq!(outcome, ScanOutcome::Completed);
        assert_eq!(f.metadata.take_reads(), ["changed.jpg", "new.png", "sidecar.jpg+xmp"]);
        assert_eq!(f.paths(), ["changed.jpg", "new.png", "same.jpg", "sidecar.jpg"]);
        let status = |rel: &str| f.value::<String>(&format!("SELECT thumb_status FROM media WHERE rel_path = '{rel}'"));
        assert_eq!(status("changed.jpg"), "pending");
        assert_eq!(status("same.jpg"), "ready");
        assert_eq!(status("sidecar.jpg"), "ready");
        assert!(
            f.value::<Option<i64>>("SELECT sidecar_mtime FROM media WHERE rel_path = 'sidecar.jpg'")
                .is_some()
        );
        assert_eq!(f.value::<i64>("SELECT count(*) FROM tags"), 0);
    }

    #[test]
    fn req_002_05_rereads_files_from_an_older_metadata_extractor() {
        let mut f = Fixture::new();
        f.touch("a.jpg", "x");
        f.scan(ScanScope::Full);
        f.writer
            .conn()
            .execute("UPDATE media SET meta_version = 0", [])
            .unwrap();
        f.metadata.take_reads();

        assert_eq!(f.scan(ScanScope::Full), ScanOutcome::Completed);

        assert_eq!(f.metadata.take_reads(), ["a.jpg"]);
        assert_eq!(f.value::<i64>("SELECT meta_version FROM media"), METADATA_VERSION);
    }

    #[test]
    fn req_002_09_keeps_the_index_of_unreachable_libraries() {
        let mut f = Fixture::new();
        f.touch("a.jpg", "x");
        f.scan(ScanScope::Full);
        f.reachable.store(false, Ordering::Relaxed);

        assert_eq!(f.scan(ScanScope::Full), ScanOutcome::Offline);
        assert_eq!(f.paths(), ["a.jpg"]);
        assert_eq!(f.writer.root(f.root).unwrap().unwrap().status, LibraryStatus::Offline);
    }

    #[test]
    fn req_003_11_waits_for_a_library_that_was_online_to_reconnect() {
        let mut f = Fixture::new();
        f.touch("a.jpg", "x");
        f.scan(ScanScope::Full);
        f.touch("b.jpg", "x");
        f.reconnect_grace = Duration::from_secs(10);
        f.reachable.store(false, Ordering::Relaxed);
        let reachable = f.reachable.clone();
        let reconnect = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            reachable.store(true, Ordering::Relaxed);
        });

        let outcome = f.scan(ScanScope::Full);
        reconnect.join().unwrap();

        assert_eq!(outcome, ScanOutcome::Completed);
        assert_eq!(f.paths(), ["a.jpg", "b.jpg"]);
        assert_eq!(f.writer.root(f.root).unwrap().unwrap().status, LibraryStatus::Online);
    }

    #[test]
    fn req_003_11_an_offline_library_is_checked_once() {
        let mut f = Fixture::new();
        f.reconnect_grace = Duration::from_secs(10);
        f.reachable.store(false, Ordering::Relaxed);
        f.writer.set_root_status(f.root, LibraryStatus::Offline, None).unwrap();
        let start = Instant::now();

        assert_eq!(f.scan(ScanScope::Quick), ScanOutcome::Offline);
        assert!(start.elapsed() < Duration::from_secs(2), "{:?}", start.elapsed());
    }

    #[test]
    fn req_002_10_stops_and_keeps_everything_when_the_library_goes_offline_during_a_scan() {
        let mut f = Fixture::new();
        f.touch("a.jpg", "x");
        f.touch("b.jpg", "x");
        f.scan(ScanScope::Full);
        f.touch("b.jpg", "xy");
        // Reading b.jpg fails once the library is gone; the reachability check then says so.
        f.metadata.failing.lock().unwrap().insert("b.jpg".into());
        let (tx, rx) = mpsc::channel();
        *f.metadata.blocked.lock().unwrap() = Some(("b.jpg".into(), rx));
        let reachable = f.reachable.clone();
        let unplug = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            reachable.store(false, Ordering::Relaxed);
            tx.send(()).unwrap();
        });

        let outcome = f.scan(ScanScope::Full);
        unplug.join().unwrap();

        assert_eq!(outcome, ScanOutcome::Offline);
        assert_eq!(f.paths(), ["a.jpg", "b.jpg"]);
        assert_eq!(f.value::<i64>("SELECT size FROM media WHERE rel_path = 'b.jpg'"), 1);
        assert_eq!(f.writer.root(f.root).unwrap().unwrap().status, LibraryStatus::Offline);
    }

    #[test]
    fn req_002_12_indexes_files_with_unreadable_metadata_without_it() {
        let mut f = Fixture::new();
        f.touch("a.jpg", "x");
        f.metadata.failing.lock().unwrap().insert("a.jpg".into());

        assert_eq!(f.scan(ScanScope::Full), ScanOutcome::Completed);
        assert_eq!(f.paths(), ["a.jpg"]);
        assert_eq!(f.value::<Option<i64>>("SELECT width FROM media"), None);
    }

    #[test]
    fn req_002_06_req_006_11_writes_read_files_while_the_scan_still_runs() {
        let mut f = Fixture::new();
        f.touch("a.jpg", "x");
        f.touch("b.jpg", "x");
        let (tx, rx) = mpsc::channel();
        *f.metadata.blocked.lock().unwrap() = Some(("b.jpg".into(), rx));
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            tx.send(()).unwrap();
        });

        let (outcome, progress) = f.scan_with(ScanScope::Full, 1, Duration::from_millis(10));
        release.join().unwrap();

        assert_eq!(outcome, ScanOutcome::Completed);
        // a.jpg was written in a batch of its own before the read of b.jpg finished.
        assert!(progress.contains(&1), "{progress:?}");
        assert_eq!(f.paths(), ["a.jpg", "b.jpg"]);
    }

    #[test]
    fn req_003_08_quick_scans_only_relist_folders_whose_entries_changed() {
        let mut f = Fixture::new();
        for rel in ["keep/a.jpg", "changed/b.jpg", "old/c.jpg", "old/sub/d.jpg"] {
            f.touch_past(rel, "x");
        }
        f.age(&["keep", "changed", "old/sub", "old", ""]);
        f.scan(ScanScope::Full);
        assert_eq!(f.folders(), ["", "changed", "keep", "old", "old/sub"]);
        f.metadata.take_reads();

        // Changed in place: the folder mtime stays, so only a full scan notices.
        f.touch("keep/a.jpg", "xy");
        set_mtime(&f.path("keep"), PAST);
        f.touch("changed/new.jpg", "x");
        fs::remove_file(f.path("changed/b.jpg")).unwrap();
        fs::remove_dir_all(f.path("old")).unwrap();
        f.touch("fresh/deep/e.jpg", "x");

        assert_eq!(f.scan(ScanScope::Quick), ScanOutcome::Completed);
        assert_eq!(f.metadata.take_reads(), ["e.jpg", "new.jpg"]);
        assert_eq!(f.paths(), ["changed/new.jpg", "fresh/deep/e.jpg", "keep/a.jpg"]);
        assert_eq!(f.folders(), ["", "changed", "fresh", "fresh/deep", "keep"]);

        f.scan(ScanScope::Full);
        assert_eq!(f.metadata.take_reads(), ["a.jpg"]);
    }

    #[test]
    fn req_003_08_falls_back_to_a_full_scan_without_stored_folders() {
        let mut f = Fixture::new();
        f.touch("a/b.jpg", "x");

        assert_eq!(f.scan(ScanScope::Quick), ScanOutcome::Completed);
        assert_eq!(f.paths(), ["a/b.jpg"]);
        assert_eq!(f.folders(), ["", "a"]);
    }

    #[test]
    fn req_003_04_checks_only_reported_files_of_changed_folders() {
        let mut f = Fixture::new();
        for rel in ["a/x.jpg", "a/y.jpg", "a/unreported.jpg", "b/z.jpg"] {
            f.touch_past(rel, "x");
        }
        f.scan(ScanScope::Full);
        f.metadata.take_reads();

        f.touch("a/x.jpg", "xy");
        f.touch("a/y.xmp", "x");
        f.touch("a/unreported.jpg", "xy");
        f.touch("a/new.jpg", "x");
        fs::remove_dir_all(f.path("b")).unwrap();
        f.touch("c/w.jpg", "x");
        let paths = ["a/x.jpg", "a/y.xmp", "a/new.jpg", "b", "c"].map(String::from).to_vec();

        assert_eq!(f.scan(ScanScope::Changes(paths)), ScanOutcome::Completed);
        assert_eq!(f.metadata.take_reads(), ["new.jpg", "w.jpg", "x.jpg", "y.jpg+xmp"]);
        assert_eq!(
            f.paths(),
            ["a/new.jpg", "a/unreported.jpg", "a/x.jpg", "a/y.jpg", "c/w.jpg"]
        );
        assert_eq!(f.folders(), ["", "a", "c"]);
    }

    #[test]
    fn req_001_05_writes_nothing_when_cancelled() {
        let mut f = Fixture::new();
        f.touch("a.jpg", "x");
        let cancel = AtomicBool::new(true);
        let mut on_progress = |_| {};
        let is_reachable = |_: &Path| true;
        let deps = ScanDeps {
            writer: &mut f.writer,
            metadata: &f.metadata,
            is_reachable: &is_reachable,
        };

        let outcome = scan_root(
            deps,
            f.root,
            &ScanScope::Full,
            ScanOptions::new(2, &cancel, &mut on_progress),
        );

        assert_eq!(outcome.unwrap(), ScanOutcome::Cancelled);
        assert!(f.paths().is_empty());
    }

    #[test]
    fn touched_files_include_their_sidecars() {
        let touched: HashSet<String> = ["a/y.xmp", "b/z.jpg.xmp", "c/w.jpg"].map(String::from).into();

        assert!(is_touched(&touched, "a/Y.jpg"));
        assert!(is_touched(&touched, "b/z.jpg"));
        assert!(is_touched(&touched, "c/w.jpg"));
        assert!(!is_touched(&touched, "c/v.jpg"));
    }
}
