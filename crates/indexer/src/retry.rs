//! Retries for file operations on network shares (spec 002, Assumptions): transient errors are retried 3 times with
//! exponential backoff; errors retrying won't fix fail at once.

use std::io::{self, ErrorKind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const RETRIES: u32 = 3;
const BASE_DELAY: Duration = Duration::from_millis(250);

/// Errors retrying won't fix: missing files, access denied, wrong type, invalid names (the reference's `ENOENT`,
/// `ENOTDIR`, `EISDIR`, `EACCES`, `EPERM`, `EINVAL`, `ENAMETOOLONG`). Anything else (busy, timeouts, connection resets,
/// unknown network errors) is treated as a transient hiccup.
pub fn is_transient(error: &io::Error) -> bool {
    !matches!(
        error.kind(),
        ErrorKind::NotFound
            | ErrorKind::PermissionDenied
            | ErrorKind::NotADirectory
            | ErrorKind::IsADirectory
            | ErrorKind::InvalidInput
            | ErrorKind::InvalidFilename
            | ErrorKind::InvalidData
    )
}

/// Runs `f`, retrying transient errors with 250 ms × 2ⁿ backoff. Gives up early when `cancel` is set.
pub fn with_retry<T>(cancel: &AtomicBool, mut f: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    with_retry_delay(cancel, BASE_DELAY, &mut f)
}

fn with_retry_delay<T>(cancel: &AtomicBool, base: Duration, f: &mut impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut attempt = 0;
    loop {
        match f() {
            Ok(value) => return Ok(value),
            Err(error) if attempt >= RETRIES || !is_transient(&error) || cancel.load(Ordering::Relaxed) => {
                return Err(error);
            }
            Err(_) => {
                std::thread::sleep(base * 2u32.pow(attempt));
                attempt += 1;
            }
        }
    }
}

/// Milliseconds since the epoch, truncated (as the reference's `Math.trunc(stats.mtimeMs)`).
pub fn mtime_ms(metadata: &std::fs::Metadata) -> i64 {
    match metadata.modified() {
        Ok(time) => match time.duration_since(std::time::UNIX_EPOCH) {
            Ok(after) => after.as_millis() as i64,
            Err(before) => -(before.duration().as_millis() as i64),
        },
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_transient_errors_but_not_permanent_ones() {
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = with_retry_delay(&cancel, Duration::ZERO, &mut || {
            calls += 1;
            if calls < 3 {
                Err(io::Error::from(ErrorKind::TimedOut))
            } else {
                Ok(calls)
            }
        });
        assert_eq!(result.unwrap(), 3);

        let mut calls = 0;
        let result: io::Result<()> = with_retry_delay(&cancel, Duration::ZERO, &mut || {
            calls += 1;
            Err(io::Error::from(ErrorKind::NotFound))
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn gives_up_after_three_retries() {
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result: io::Result<()> = with_retry_delay(&cancel, Duration::ZERO, &mut || {
            calls += 1;
            Err(io::Error::from(ErrorKind::ConnectionReset))
        });
        assert!(result.is_err());
        assert_eq!(calls, 4);
    }
}
