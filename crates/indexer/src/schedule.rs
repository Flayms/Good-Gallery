//! Scheduled scans: every library gets a quick scan at startup and whenever the quick check interval elapses (a full
//! scan while rows are behind `METADATA_VERSION`); offline libraries are checked again every minute, so a reconnect is
//! picked up quickly.

use crate::scan::ScanScope;
use db::rusqlite::Connection;
use domain::media::METADATA_VERSION;
use std::collections::HashSet;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How often offline libraries are checked for a reconnect.
pub const OFFLINE_RETRY: Duration = Duration::from_secs(60);

/// Where scheduled scans go (the indexer, or a fake in tests).
pub trait ScanTarget: Send + Sync {
    fn request(&self, library: i64, scope: ScanScope);
}

impl ScanTarget for crate::Indexer {
    fn request(&self, library: i64, scope: ScanScope) {
        crate::Indexer::request(self, library, scope);
    }
}

/// Quick scan of every library; a full scan for libraries with rows from an older metadata extractor, once, to pick
/// up the new fields (REQ-002-05).
pub fn scan_all(conn: &Connection, target: &dyn ScanTarget) -> db::Result<()> {
    let mut stmt = conn.prepare_cached("SELECT DISTINCT root_id FROM media WHERE meta_version < ?1")?;
    let outdated: HashSet<i64> = stmt
        .query_map([METADATA_VERSION], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    for library in db::libraries::list(conn)? {
        let scope = if outdated.contains(&library.id) {
            ScanScope::Full
        } else {
            ScanScope::Quick
        };
        target.request(library.id, scope);
    }
    Ok(())
}

/// Quick scans of offline libraries; the scan starts by checking reachability (REQ-003-10).
pub fn scan_offline(conn: &Connection, target: &dyn ScanTarget) -> db::Result<()> {
    for library in db::libraries::list(conn)? {
        if library.status == db::libraries::LibraryStatus::Offline {
            target.request(library.id, ScanScope::Quick);
        }
    }
    Ok(())
}

struct State {
    /// `None`: only at startup.
    interval: Option<Duration>,
    next_full: Option<Instant>,
    next_offline: Instant,
    stop: bool,
}

/// The schedule's thread. Dropping it stops the schedule.
pub struct Scheduler {
    shared: Arc<(Mutex<State>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

fn interval_of(minutes: u32) -> Option<Duration> {
    (minutes > 0).then(|| Duration::from_secs(u64::from(minutes) * 60))
}

impl Scheduler {
    /// Scans all libraries now, then every `interval_minutes` (`0`: only now), and offline ones every `offline_retry`.
    pub fn start(
        conn: Connection,
        target: Arc<dyn ScanTarget>,
        interval_minutes: u32,
        offline_retry: Duration,
    ) -> Self {
        let now = Instant::now();
        let interval = interval_of(interval_minutes);
        let shared = Arc::new((
            Mutex::new(State {
                interval,
                next_full: interval.map(|i| now + i),
                next_offline: now + offline_retry,
                stop: false,
            }),
            Condvar::new(),
        ));
        let thread_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("schedule".into())
            .spawn(move || {
                if let Err(error) = scan_all(&conn, target.as_ref()) {
                    tracing::error!("Schedule: {error}");
                }
                let (lock, wake) = &*thread_shared;
                let mut state = lock.lock().unwrap_or_else(|p| p.into_inner());
                loop {
                    if state.stop {
                        return;
                    }
                    let now = Instant::now();
                    let deadline = state
                        .next_full
                        .map_or(state.next_offline, |full| full.min(state.next_offline));
                    if deadline > now {
                        state = wake
                            .wait_timeout(state, deadline - now)
                            .unwrap_or_else(|p| p.into_inner())
                            .0;
                        continue;
                    }
                    let full_due = state.next_full.is_some_and(|full| full <= now);
                    let offline_due = state.next_offline <= now;
                    if full_due {
                        state.next_full = state.interval.map(|i| now + i);
                    }
                    if offline_due {
                        state.next_offline = now + offline_retry;
                    }
                    drop(state);
                    let result = if full_due {
                        scan_all(&conn, target.as_ref())
                    } else if offline_due {
                        scan_offline(&conn, target.as_ref())
                    } else {
                        Ok(())
                    };
                    if let Err(error) = result {
                        tracing::error!("Schedule: {error}");
                    }
                    state = lock.lock().unwrap_or_else(|p| p.into_inner());
                }
            })
            .expect("can't start the schedule thread");
        Self {
            shared,
            thread: Some(thread),
        }
    }

    /// Restarts the quick-scan timer with a new interval, without scanning right away (REQ-010-03).
    pub fn set_interval(&self, minutes: u32) {
        let (lock, wake) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|p| p.into_inner());
        state.interval = interval_of(minutes);
        state.next_full = state.interval.map(|i| Instant::now() + i);
        wake.notify_all();
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        let (lock, wake) = &*self.shared;
        lock.lock().unwrap_or_else(|p| p.into_inner()).stop = true;
        wake.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use db::libraries::{self, LibraryStatus};

    #[derive(Default)]
    struct Recorded(Mutex<Vec<(i64, ScanScope)>>);

    impl ScanTarget for Recorded {
        fn request(&self, library: i64, scope: ScanScope) {
            self.0.lock().unwrap().push((library, scope));
        }
    }

    impl Recorded {
        fn take(&self) -> Vec<(i64, ScanScope)> {
            std::mem::take(&mut *self.0.lock().unwrap())
        }
    }

    fn index() -> (tempfile::TempDir, std::path::PathBuf, i64, i64) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gallery.db");
        let conn = db::open(&path).unwrap();
        let online = libraries::insert(&conn, "D:\\a", "a", LibraryStatus::Online)
            .unwrap()
            .id;
        let offline = libraries::insert(&conn, "\\\\nas\\b", "b", LibraryStatus::Offline)
            .unwrap()
            .id;
        (dir, path, online, offline)
    }

    #[test]
    fn req_002_05_scans_libraries_with_outdated_rows_fully_and_others_quickly() {
        let (_dir, path, online, offline) = index();
        let conn = db::open(&path).unwrap();
        conn.execute(
            "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime, meta_version)
             VALUES (?1, 'x.jpg', 'x.jpg', 'image', 1, 1, 0)",
            [offline],
        )
        .unwrap();
        let target = Recorded::default();

        scan_all(&conn, &target).unwrap();

        assert_eq!(target.take(), [(online, ScanScope::Quick), (offline, ScanScope::Full)]);
    }

    #[test]
    fn req_003_09_req_003_10_scans_at_startup_and_offline_libraries_again_and_again() {
        let (_dir, path, online, offline) = index();
        let target = Arc::new(Recorded::default());

        let scheduler = Scheduler::start(db::open(&path).unwrap(), target.clone(), 0, Duration::from_millis(100));
        std::thread::sleep(Duration::from_millis(350));
        drop(scheduler);

        let requests = target.take();
        assert_eq!(requests[..2], [(online, ScanScope::Quick), (offline, ScanScope::Quick)]);
        let retries = requests[2..]
            .iter()
            .filter(|r| **r == (offline, ScanScope::Quick))
            .count();
        assert!(retries >= 2, "{requests:?}");
        assert!(!requests[2..].iter().any(|(library, _)| *library == online));
    }

    #[test]
    fn req_003_09_quick_checks_every_interval_and_a_new_interval_restarts_the_timer() {
        let (_dir, path, _, _) = index();
        let target = Arc::new(Recorded::default());
        let scheduler = Scheduler::start(db::open(&path).unwrap(), target.clone(), 0, Duration::from_secs(3600));
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(target.take().len(), 2, "startup scan of both libraries");

        // Only at startup: nothing more.
        std::thread::sleep(Duration::from_millis(200));
        assert!(target.take().is_empty());

        // A new interval doesn't scan right away.
        scheduler.set_interval(1);
        std::thread::sleep(Duration::from_millis(200));
        assert!(target.take().is_empty());
        assert_eq!(interval_of(1), Some(Duration::from_secs(60)));
        assert_eq!(interval_of(0), None);
    }
}
