//! The index in the browser (REQ-019-08). SQLite runs on the in-memory file system of `sqlite-wasm-rs`, where every
//! connection to [`INDEX`] shares one database, so `db` opens it as on the desktop. A snapshot of it is kept in the
//! page's storage: restored before the first connection opens, saved a moment after it changes and when the page is
//! hidden or closed.

use super::js;
use db::rusqlite::{Connection, MAIN_DB, backup::Backup};
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

/// The index's name on SQLite's in-memory file system.
pub const INDEX: &str = "gallery.db";
/// The snapshot in the page's storage.
const SNAPSHOT: &str = "index/gallery.db";
/// How often the index is checked for changes to save.
const SAVE_EVERY_MS: i32 = 3000;

/// Opens (and migrates) a connection to the index.
pub fn open() -> db::Result<Connection> {
    db::open(Path::new(INDEX))
}

/// Restores the snapshot of an earlier visit, if there is one. Call before [`open`].
pub async fn restore() -> Result<bool, String> {
    let Some(bytes) = js::bytes(&js::wait(js::opfs_read(SNAPSHOT)).await?) else {
        return Ok(false);
    };
    let mut snapshot = Connection::open_in_memory().map_err(|e| e.to_string())?;
    snapshot
        .deserialize_read_exact(MAIN_DB, bytes.as_slice(), bytes.len(), true)
        .map_err(|e| format!("the saved index is damaged: {e}"))?;
    let mut index = Connection::open(INDEX).map_err(|e| e.to_string())?;
    let backup = Backup::new(&snapshot, &mut index).map_err(|e| e.to_string())?;
    // All pages in one step: the copy is in memory, and a paused backup would need a thread to sleep on.
    backup.step(-1).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Saves the index whenever it changed, and when the page is hidden. Runs for the page's lifetime.
pub fn keep_saved() -> Result<(), String> {
    let saver = Rc::new(Saver {
        conn: RefCell::new(open().map_err(|e| e.to_string())?),
        version: Cell::new(None),
        saving: Cell::new(false),
    });
    let on_hidden = saver.clone();
    let closure = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
        let saver = on_hidden.clone();
        super::spawn(async move { saver.save_if_changed().await });
    });
    js::on_page_hidden(&closure);
    closure.forget();
    super::spawn(async move {
        loop {
            super::sleep(SAVE_EVERY_MS).await;
            saver.save_if_changed().await;
        }
    });
    Ok(())
}

struct Saver {
    conn: RefCell<Connection>,
    /// `PRAGMA data_version` at the last save: it changes when another connection commits.
    version: Cell<Option<i64>>,
    saving: Cell<bool>,
}

impl Saver {
    async fn save_if_changed(&self) {
        if self.saving.get() {
            return;
        }
        let version = self
            .conn
            .borrow()
            .query_row("PRAGMA data_version", [], |row| row.get::<_, i64>(0))
            .ok();
        if version.is_none() || version == self.version.get() {
            return;
        }
        let bytes = match self.conn.borrow_mut().serialize(MAIN_DB) {
            Ok(data) => js_sys::Uint8Array::from(&data[..]),
            Err(error) => {
                tracing::error!("can't save the index: {error}");
                return;
            }
        };
        self.saving.set(true);
        match js::wait(js::opfs_write(SNAPSHOT, &bytes)).await {
            Ok(_) => self.version.set(version),
            Err(error) => tracing::error!("can't save the index: {error}"),
        }
        self.saving.set(false);
    }
}
