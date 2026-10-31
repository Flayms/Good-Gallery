//! Whether a folder can be reached, without hanging on an unreachable network share.

use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long a stat may take before a library counts as unreachable.
pub const TIMEOUT: Duration = Duration::from_secs(5);

/// How long a library that was online may not answer before it counts as offline: a network share can take that long
/// to reconnect after the computer starts or wakes up (REQ-003-11).
pub const RECONNECT_GRACE: Duration = Duration::from_secs(30);

/// Runs `f` on a helper thread and gives up after `timeout`. The thread finishes in the background.
pub fn with_timeout<T: Send + 'static>(timeout: Duration, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(timeout).ok()
}

/// False instead of hanging when a network share doesn't answer within `timeout`.
pub fn is_reachable_directory(path: &Path, timeout: Duration) -> bool {
    let path = path.to_path_buf();
    with_timeout(timeout, move || std::fs::metadata(path).is_ok_and(|m| m.is_dir())).unwrap_or(false)
}

/// Probes again and again until `probe` says yes, `grace` has passed or `cancelled` says so, pausing a fifteenth of
/// `grace` (2 s of 30 s) after a failed probe.
pub fn reachable_within(grace: Duration, cancelled: impl Fn() -> bool, mut probe: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    let pause = grace / 15;
    loop {
        if probe() {
            return true;
        }
        if cancelled() || start.elapsed() + pause >= grace {
            return false;
        }
        std::thread::sleep(pause);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_are_reachable_files_and_missing_paths_are_not() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "").unwrap();

        assert!(is_reachable_directory(dir.path(), TIMEOUT));
        assert!(!is_reachable_directory(&file, TIMEOUT));
        assert!(!is_reachable_directory(&dir.path().join("missing"), TIMEOUT));
    }

    #[test]
    fn req_001_03_gives_up_after_the_timeout() {
        let slow = with_timeout(Duration::from_millis(20), || std::thread::sleep(Duration::from_secs(2)));

        assert_eq!(slow, None);
        assert_eq!(with_timeout(TIMEOUT, || 1), Some(1));
    }

    #[test]
    fn req_003_11_probes_until_the_folder_answers_or_the_grace_is_over() {
        let mut probes = 0;
        assert!(reachable_within(
            Duration::from_secs(10),
            || false,
            || {
                probes += 1;
                probes == 3
            }
        ));
        assert_eq!(probes, 3);

        let start = Instant::now();
        assert!(!reachable_within(Duration::from_millis(150), || false, || false));
        assert!(start.elapsed() >= Duration::from_millis(100), "{:?}", start.elapsed());

        let mut probes = 0;
        assert!(!reachable_within(
            Duration::from_secs(10),
            || true,
            || {
                probes += 1;
                false
            }
        ));
        assert_eq!(probes, 1, "a cancelled check stops probing");
    }
}
