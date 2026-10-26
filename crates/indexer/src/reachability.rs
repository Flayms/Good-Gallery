//! Whether a folder can be reached, without hanging on an unreachable network share.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

/// How long a stat may take before a library counts as unreachable.
pub const TIMEOUT: Duration = Duration::from_secs(5);

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
}
