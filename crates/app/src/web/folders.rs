//! Picked folders (REQ-019-02, REQ-019-09). A library in the browser is `/<folder name>`; what the page may read of it
//! is a directory handle (Chrome, Edge: kept in IndexedDB between visits) or the files of a folder upload (Firefox,
//! Safari: for this visit only). Both are held here by library, for scans, originals and videos.

use super::js;
use db::libraries::{Library, LibraryStatus};
use db::rusqlite::Connection;
use indexer::ScanRequests;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use wasm_bindgen::{JsCast as _, JsValue};

/// What the page may read of a library's folder.
#[derive(Clone)]
pub enum Source {
    /// A directory handle with read access.
    Directory(JsValue),
    /// The files of a folder upload, by `/`-separated path relative to the folder.
    Files(Rc<HashMap<String, web_sys::File>>),
}

#[derive(Default)]
struct Folders {
    /// Libraries the page can read now.
    readable: HashMap<i64, Source>,
    /// Directory handles of an earlier visit whose access the browser wants confirmed with a click.
    asleep: HashMap<i64, JsValue>,
    /// Library paths by id, to find the library of a file path.
    paths: HashMap<i64, String>,
}

thread_local! {
    static FOLDERS: RefCell<Folders> = RefCell::new(Folders::default());
}

/// The library path of a picked folder.
pub fn library_path(name: &str) -> String {
    format!("/{name}")
}

/// The library a folder picked now belongs to: `added` is false when it was already a library and the page reads it
/// again (REQ-019-02, REQ-019-09).
pub struct Picked {
    pub library: Library,
    pub added: bool,
}

fn lock(db: &Mutex<Connection>) -> std::sync::MutexGuard<'_, Connection> {
    db.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Opens the browser's folder picker; the picked folder becomes a library (or a known one is readable again) and is
/// scanned. `done` gets the library, `None` when the picker was cancelled, or why it failed.
pub fn pick_folder(
    db: Arc<Mutex<Connection>>,
    scans: Arc<dyn ScanRequests>,
    done: impl FnOnce(Result<Option<Picked>, String>) + 'static,
) {
    super::spawn(async move { done(pick(&db, scans.as_ref()).await) });
}

async fn pick(db: &Mutex<Connection>, scans: &dyn ScanRequests) -> Result<Option<Picked>, String> {
    let (name, source) = if js::can_keep_folders() {
        let dir = js::wait(js::pick_directory()).await?;
        if dir.is_null() {
            return Ok(None);
        }
        let name = super::get(&dir, "name").as_string().unwrap_or_default();
        (name, Source::Directory(dir))
    } else {
        let picked = js::wait(js::pick_files()).await?;
        if picked.is_null() {
            return Ok(None);
        }
        let name = super::get(&picked, "name").as_string().unwrap_or_default();
        let entries = super::get(&picked, "files");
        let entries = entries.dyn_ref::<js_sys::Array>().ok_or("the browser gave no files")?;
        let files = entries
            .iter()
            .filter_map(|entry| {
                let path = super::at(&entry, 0).as_string()?;
                let file = super::at(&entry, 1).dyn_into::<web_sys::File>().ok()?;
                Some((path, file))
            })
            .collect();
        (name, Source::Files(Rc::new(files)))
    };
    if name.is_empty() {
        return Err("The browser didn't name the folder.".into());
    }
    let path = library_path(&name);
    let known = {
        let conn = lock(db);
        db::libraries::find_by_path(&conn, &path)
            .map_err(|e| e.to_string())?
            .and_then(|id| db::libraries::get(&conn, id).ok().flatten())
    };
    let picked = match known {
        Some(library) => {
            readable(library.id, &library.path, source.clone());
            db::libraries::set_status(&lock(db), library.id, LibraryStatus::Online).map_err(|e| e.to_string())?;
            scans.request_scan(library.id, indexer::ScanKind::Full);
            Picked { library, added: false }
        }
        None => {
            let conn = lock(db);
            let library =
                db::libraries::insert(&conn, &path, &name, LibraryStatus::Online).map_err(|e| e.to_string())?;
            drop(conn);
            // Readable before the scan is queued.
            readable(library.id, &library.path, source.clone());
            scans.request_scan(library.id, indexer::ScanKind::Full);
            Picked { library, added: true }
        }
    };
    if let Source::Directory(dir) = &source
        && let Err(error) = js::wait(js::save_folder(&path, dir)).await
    {
        tracing::warn!("can't keep access to {path} for the next visit: {error}");
    }
    // The first library: ask the browser to keep what is indexed (REQ-019-08).
    let _ = js::wait(js::persist_storage()).await;
    Ok(Some(picked))
}

fn readable(library: i64, path: &str, source: Source) {
    FOLDERS.with_borrow_mut(|folders| {
        folders.asleep.remove(&library);
        folders.paths.insert(library, path.to_owned());
        folders.readable.insert(library, source);
    });
}

/// At the start of a visit: libraries whose folder the browser still lets the page read become readable and get a
/// full scan (REQ-019-10); the others are marked offline until the user gives access again (REQ-019-09). Returns
/// those offline ones.
pub async fn restore(libraries: &[Library], db: &Mutex<Connection>, scans: &dyn ScanRequests) -> Vec<i64> {
    let mut offline = Vec::new();
    for library in libraries {
        FOLDERS.with_borrow_mut(|folders| folders.paths.insert(library.id, library.path.clone()));
        let dir = match js::wait(js::load_folder(&library.path)).await {
            Ok(dir) if !dir.is_null() && !dir.is_undefined() => Some(dir),
            Ok(_) => None,
            Err(error) => {
                tracing::warn!("can't read the kept folders: {error}");
                None
            }
        };
        let granted = match &dir {
            Some(dir) => {
                js::wait(js::folder_permission(dir))
                    .await
                    .ok()
                    .and_then(|p| p.as_string())
                    == Some("granted".into())
            }
            None => false,
        };
        let status = if granted {
            LibraryStatus::Online
        } else {
            LibraryStatus::Offline
        };
        if let Err(error) = db::libraries::set_status(&lock(db), library.id, status) {
            tracing::error!("can't update {}: {error}", library.label);
        }
        match (dir, granted) {
            (Some(dir), true) => {
                readable(library.id, &library.path, Source::Directory(dir));
                scans.request_scan(library.id, indexer::ScanKind::Full);
            }
            (Some(dir), false) => {
                FOLDERS.with_borrow_mut(|folders| folders.asleep.insert(library.id, dir));
                offline.push(library.id);
            }
            (None, _) => offline.push(library.id),
        }
    }
    offline
}

/// What the page can read of `library` now, if anything.
pub fn source(library: i64) -> Option<Source> {
    FOLDERS.with_borrow(|folders| folders.readable.get(&library).cloned())
}

/// Forgets a removed library's folder, also for the next visit (REQ-019-11).
pub async fn forget(library: i64) {
    let path = FOLDERS.with_borrow_mut(|folders| {
        folders.readable.remove(&library);
        folders.asleep.remove(&library);
        folders.paths.remove(&library)
    });
    if let Some(path) = path
        && let Err(error) = js::wait(js::delete_folder(&path)).await
    {
        tracing::warn!("can't forget the folder of {path}: {error}");
    }
}

/// Why a file can't be read: the page has no access to its folder (anymore).
pub const NO_ACCESS: &str = "No access to this folder. Pick it again in Settings to show its files.";

/// The file of a media item. A folder kept from an earlier visit asks the browser for access first, which needs the
/// click that led here (REQ-019-09).
pub async fn file(library: i64, rel_path: &str) -> Result<web_sys::File, String> {
    if !domain::media::is_safe_rel_path(rel_path) {
        return Err("The file is outside its library.".into());
    }
    let source = match source(library) {
        Some(source) => source,
        None => wake(library).await.ok_or(NO_ACCESS)?,
    };
    match source {
        Source::Files(files) => files
            .get(rel_path)
            .cloned()
            .ok_or_else(|| "The file no longer exists.".into()),
        Source::Directory(dir) => {
            let file = js::wait(js::file_at(&dir, rel_path)).await?;
            file.dyn_into::<web_sys::File>()
                .map_err(|_| "The file no longer exists.".into())
        }
    }
}

/// Asks the browser to let the page read a folder kept from an earlier visit again.
async fn wake(library: i64) -> Option<Source> {
    let dir = FOLDERS.with_borrow(|folders| folders.asleep.get(&library).cloned())?;
    let granted = js::wait(js::request_folder_permission(&dir)).await.ok()?.as_bool() == Some(true);
    if !granted {
        return None;
    }
    let path = FOLDERS.with_borrow(|folders| folders.paths.get(&library).cloned())?;
    readable(library, &path, Source::Directory(dir));
    source(library)
}

/// The library and relative path of a media file's path (`/<folder>/<relative path>`).
pub fn locate(file: &Path) -> Option<(i64, String)> {
    let file = file.to_str()?;
    FOLDERS.with_borrow(|folders| {
        folders.paths.iter().find_map(|(id, root)| {
            let rel = file.strip_prefix(root.as_str())?.strip_prefix('/')?;
            Some((*id, rel.to_owned()))
        })
    })
}

/// The file at a media file's path (see [`locate`]).
pub async fn file_at(path: &Path) -> Result<web_sys::File, String> {
    let (library, rel_path) = locate(path).ok_or("The file's library isn't known.")?;
    file(library, &rel_path).await
}
