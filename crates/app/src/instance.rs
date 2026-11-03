//! One app per data folder (REQ-027-02). The first start holds a lock on `instance.lock` in the data folder for as
//! long as it runs; a later start finds it locked, leaves the file it was asked to open in the `open` folder there and
//! exits. The running app checks that folder a few times a second, shows the file and comes to the front. Plain files,
//! so there is no socket or pipe.

use std::fs::{self, File, TryLockError};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How often the running app looks for requests.
pub const POLL: Duration = Duration::from_millis(250);
const LOCK: &str = "instance.lock";
const INBOX: &str = "open";
/// A finished request; it is written under another name first, so a reader never sees half of one.
const EXTENSION: &str = "request";

/// What a start of the app is.
#[derive(Debug)]
pub enum Start {
    /// The only app on this data folder: it keeps the lock until it ends.
    First(Instance),
    /// Another app runs on this data folder and was handed the request.
    Handed,
}

/// The lock of the running app.
#[derive(Debug)]
pub struct Instance {
    _lock: File,
}

/// Something a later start asks the running app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Come to the front.
    Show,
    /// Come to the front and show this file (REQ-027-01).
    Open(PathBuf),
}

/// Takes the data folder's lock, or hands `request` to the app that holds it.
pub fn start(data_dir: &Path, request: &Request) -> io::Result<Start> {
    fs::create_dir_all(data_dir)?;
    let lock = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(data_dir.join(LOCK))?;
    match lock.try_lock() {
        Ok(()) => {
            // Requests left by starts that found the previous app still running as it ended.
            let _ = take_requests(data_dir);
            Ok(Start::First(Instance { _lock: lock }))
        }
        Err(TryLockError::WouldBlock) => {
            allow_foreground();
            hand(data_dir, request)?;
            Ok(Start::Handed)
        }
        Err(TryLockError::Error(error)) => Err(error),
    }
}

/// Windows lets only the foreground process bring a window to the front; this start, which the user just launched,
/// passes that right on to the running app.
#[cfg(windows)]
#[allow(unsafe_code)]
fn allow_foreground() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
    // SAFETY: no pointers; failing only means the window flashes in the taskbar instead.
    unsafe {
        AllowSetForegroundWindow(ASFW_ANY);
    }
}

#[cfg(not(windows))]
fn allow_foreground() {}

fn hand(data_dir: &Path, request: &Request) -> io::Result<()> {
    let inbox = data_dir.join(INBOX);
    fs::create_dir_all(&inbox)?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let name = format!("{nanos:032}-{}", std::process::id());
    let partial = inbox.join(format!("{name}.partial"));
    let mut file = File::create(&partial)?;
    if let Request::Open(path) = request {
        file.write_all(&path_bytes(path))?;
    }
    file.sync_all()?;
    drop(file);
    fs::rename(&partial, inbox.join(format!("{name}.{EXTENSION}")))
}

/// The requests handed to this app since the last call, oldest first; they are removed.
pub fn take_requests(data_dir: &Path) -> Vec<Request> {
    let Ok(entries) = fs::read_dir(data_dir.join(INBOX)) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == EXTENSION))
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|file| {
            let bytes = fs::read(&file).ok();
            let _ = fs::remove_file(&file);
            let bytes = bytes?;
            if bytes.is_empty() {
                return Some(Request::Show);
            }
            Some(Request::Open(path_from(bytes)))
        })
        .collect()
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn path_from(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt as _;
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

/// Windows paths are UTF-16; the few that aren't valid Unicode can't be handed over.
#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

#[cfg(not(unix))]
fn path_from(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_027_02_a_second_start_hands_its_file_to_the_first() {
        let data = tempfile::tempdir().unwrap();
        let first = start(data.path(), &Request::Show).unwrap();
        assert!(matches!(first, Start::First(_)));
        assert_eq!(take_requests(data.path()), []);

        let file = PathBuf::from("/photos/2021/beach ä.jpg");
        assert!(matches!(
            start(data.path(), &Request::Open(file.clone())).unwrap(),
            Start::Handed
        ));
        assert!(matches!(start(data.path(), &Request::Show).unwrap(), Start::Handed));
        assert_eq!(take_requests(data.path()), [Request::Open(file), Request::Show]);
        assert_eq!(take_requests(data.path()), [], "a request is taken once");

        // Once the first app ends, the next start is the first again.
        drop(first);
        assert!(matches!(start(data.path(), &Request::Show).unwrap(), Start::First(_)));
    }

    #[test]
    fn req_027_02_apps_on_different_data_folders_are_separate() {
        let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let _first = start(one.path(), &Request::Show).unwrap();
        assert!(matches!(start(two.path(), &Request::Show).unwrap(), Start::First(_)));
    }
}
