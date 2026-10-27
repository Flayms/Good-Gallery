//! Watches a library for changes between scans (the reference's `RootWatcher`): `notify` (ReadDirectoryChangesW on
//! Windows, which SMB shares support), housekeeping paths filtered out, changes reported once they settle.

use domain::media::is_ignored_dir;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

/// Quiet period before changes are reported, so a copy of many files becomes one report.
pub const DEBOUNCE: Duration = Duration::from_secs(2);
/// Longest delay of a report while changes keep coming.
pub const MAX_DELAY: Duration = Duration::from_secs(10);

/// A changed path relative to the library (any separator) as a `/`-separated path; `None` for housekeeping paths
/// (`Thumbs.db`, `desktop.ini`, dotfiles, anything in a housekeeping folder).
pub fn changed_path(relative: &str) -> Option<String> {
    let parts: Vec<&str> = relative.split(['\\', '/']).filter(|part| !part.is_empty()).collect();
    let name = parts.last()?;
    // `is_ignored_dir` also covers dotfiles and changed housekeeping folders themselves.
    let ignored_file = matches!(name.to_lowercase().as_str(), "thumbs.db" | "desktop.ini");
    if ignored_file || parts.iter().any(|part| is_ignored_dir(part)) {
        return None;
    }
    Some(parts.join("/"))
}

/// Collects changes and decides when to report them: [`DEBOUNCE`] after the last change, at most [`MAX_DELAY`] after
/// the first.
#[derive(Debug)]
pub struct Debouncer {
    paths: Vec<String>,
    unknown: bool,
    first: Option<Instant>,
    last: Option<Instant>,
    debounce: Duration,
    max_delay: Duration,
}

impl Debouncer {
    pub fn new(debounce: Duration, max_delay: Duration) -> Self {
        Self {
            paths: Vec::new(),
            unknown: false,
            first: None,
            last: None,
            debounce,
            max_delay,
        }
    }

    /// A changed path, or `None` when changes were lost (unknown changes).
    pub fn change(&mut self, path: Option<String>, now: Instant) {
        match path {
            Some(path) => {
                if !self.paths.contains(&path) {
                    self.paths.push(path);
                }
            }
            None => self.unknown = true,
        }
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    /// When the collected changes are due, if there are any.
    pub fn deadline(&self) -> Option<Instant> {
        Some((self.last? + self.debounce).min(self.first? + self.max_delay))
    }

    /// The report if it is due: the changed paths, or `None` for unknown changes.
    pub fn take_due(&mut self, now: Instant) -> Option<Option<Vec<String>>> {
        if self.deadline()? > now {
            return None;
        }
        let paths = std::mem::take(&mut self.paths);
        let unknown = std::mem::replace(&mut self.unknown, false);
        self.first = None;
        self.last = None;
        Some((!unknown).then_some(paths))
    }
}

/// What a watcher reports.
pub enum WatchEvent {
    /// Changed paths, or `None` when the changes are unknown.
    Changed(Option<Vec<String>>),
    /// Watching broke (e.g. the share disconnected); the watcher has stopped.
    Failed(String),
}

/// A running watch of one library. Dropping it stops watching.
pub struct LibraryWatcher {
    // Dropping the watcher closes its channel, which ends the reporting thread.
    _watcher: RecommendedWatcher,
}

impl LibraryWatcher {
    /// Starts watching `root` recursively; `report` runs on the watcher's own thread.
    pub fn start(root: &Path, report: impl Fn(WatchEvent) + Send + 'static) -> notify::Result<Self> {
        let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let mut watcher = notify::recommended_watcher(tx)?;
        watcher.watch(root, RecursiveMode::Recursive)?;
        let root: PathBuf = root.to_path_buf();
        std::thread::Builder::new()
            .name("watcher".into())
            .spawn(move || {
                let mut debouncer = Debouncer::new(DEBOUNCE, MAX_DELAY);
                loop {
                    let received = match debouncer.deadline() {
                        Some(deadline) => rx.recv_timeout(deadline.saturating_duration_since(Instant::now())),
                        None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                    };
                    match received {
                        Ok(Ok(event)) => {
                            let now = Instant::now();
                            if event.need_rescan() {
                                debouncer.change(None, now);
                            }
                            for path in event.paths {
                                let relative = path.strip_prefix(&root).unwrap_or(&path);
                                if let Some(changed) = relative.to_str().and_then(changed_path) {
                                    debouncer.change(Some(changed), now);
                                }
                            }
                        }
                        Ok(Err(error)) => {
                            report(WatchEvent::Failed(error.to_string()));
                            return;
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                    if let Some(paths) = debouncer.take_due(Instant::now()) {
                        report(WatchEvent::Changed(paths));
                    }
                }
            })
            .map_err(notify::Error::io)?;
        Ok(Self { _watcher: watcher })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn req_003_03_normalizes_separators_and_skips_housekeeping_paths() {
        assert_eq!(
            changed_path("trip\\day 1\\IMG_1.jpg").as_deref(),
            Some("trip/day 1/IMG_1.jpg")
        );
        assert_eq!(changed_path("trip").as_deref(), Some("trip"));
        assert_eq!(changed_path("@eaDir\\IMG_1.jpg\\SYNOFILE_THUMB_M.jpg"), None);
        assert_eq!(changed_path("trip\\._IMG_1.jpg"), None);
        assert_eq!(changed_path("trip\\Thumbs.db"), None);
        assert_eq!(changed_path("trip\\DESKTOP.INI"), None);
    }

    fn debouncer() -> (Debouncer, Instant) {
        (
            Debouncer::new(Duration::from_millis(100), Duration::from_millis(1000)),
            Instant::now(),
        )
    }

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn req_003_02_reports_the_changed_paths_once_changes_settle() {
        let (mut d, t0) = debouncer();
        d.change(changed_path("a\\1.jpg"), t0);
        d.change(changed_path("a\\1.jpg"), at(t0, 50));
        d.change(Some("b".into()), at(t0, 50));
        assert_eq!(d.take_due(at(t0, 149)), None);

        assert_eq!(
            d.take_due(at(t0, 150)),
            Some(Some(vec!["a/1.jpg".to_string(), "b".into()]))
        );
        assert_eq!(d.deadline(), None);
    }

    #[test]
    fn req_003_02_reports_within_the_maximum_delay_while_changes_keep_coming() {
        let (mut d, t0) = debouncer();
        let mut reports = Vec::new();
        for elapsed in (0..1000).step_by(50) {
            d.change(Some(format!("a/{elapsed}.jpg")), at(t0, elapsed));
            reports.extend(d.take_due(at(t0, elapsed + 49)));
        }
        reports.extend(d.take_due(at(t0, 1000)));

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].as_ref().map(Vec::len), Some(20));
    }

    #[test]
    fn req_003_05_reports_unknown_changes_when_events_were_lost() {
        let (mut d, t0) = debouncer();
        d.change(Some("a/1.jpg".into()), t0);
        d.change(None, t0);

        assert_eq!(d.take_due(at(t0, 100)), Some(None));
    }

    /// REQ-003-01: a real watch reports a new file once changes settle.
    #[test]
    fn req_003_01_reports_new_files_in_a_watched_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("trip")).unwrap();
        let reports: Arc<Mutex<Vec<Option<Vec<String>>>>> = Arc::default();
        let sink = reports.clone();
        let _watcher = LibraryWatcher::start(dir.path(), move |event| {
            if let WatchEvent::Changed(paths) = event {
                sink.lock().unwrap().push(paths);
            }
        })
        .unwrap();

        std::fs::write(dir.path().join("trip").join("IMG_1.jpg"), "x").unwrap();
        std::fs::write(dir.path().join("trip").join("Thumbs.db"), "x").unwrap();

        let deadline = Instant::now() + Duration::from_secs(10);
        while reports.lock().unwrap().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let reports = reports.lock().unwrap();
        let paths = reports.first().cloned().flatten().unwrap_or_default();
        assert!(paths.contains(&"trip/IMG_1.jpg".to_string()), "{reports:?}");
        assert!(!paths.iter().any(|path| path.ends_with("Thumbs.db")));
    }
}
