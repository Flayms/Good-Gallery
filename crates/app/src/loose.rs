//! Files outside every library (spec 033): shown from a scratch index, a temporary index of the same schema that
//! holds just the file's folder, filled by the indexer's own scan code and read by a second [`Gallery`]. The viewer
//! runs on it unchanged; the user's index is never touched.

use crate::gallery::Gallery;
use db::libraries::{Library, LibraryStatus};
use db::rusqlite::Connection;
use db::writer::IndexWriter;
use domain::search::{GallerySearch, SortBy};
use eframe::egui;
use indexer::scan::{ScanDeps, ScanOptions, ScanScope, scan_root};
use media::metadata::MetadataSource;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Where scratch indexes go, and what reads their files' metadata.
pub struct LooseFiles {
    pub scratch: PathBuf,
    pub metadata: Arc<dyn MetadataSource>,
}

/// Ids in scratch indexes start here, far above any the user's index reaches, so the originals and textures the
/// viewer keeps by media id never mix up a file of both.
const FIRST_ID: i64 = 1 << 50;

/// The two steps of filling a scratch index: the opened file alone, so it shows after one read, then its folder.
const STEPS: usize = 2;

/// A file outside every library, shown from its scratch index.
pub struct Loose {
    /// The scratch index's gallery: the file's folder by name (REQ-033-02).
    pub gallery: Gallery,
    /// The scratch index's one library: the file's folder.
    pub libraries: Vec<Library>,
    /// The opened file.
    pub file: PathBuf,
    rel_path: String,
    reader: Arc<Mutex<Connection>>,
    path: PathBuf,
    cancel: Arc<AtomicBool>,
    filled: Receiver<()>,
    steps_done: usize,
    /// The viewer shows it.
    pub shown: bool,
}

impl Loose {
    /// Creates a scratch index for `file` in `files.scratch` and starts filling it on a thread of its own: the file
    /// first, then the rest of its folder (REQ-033-01, REQ-033-02). `settings` keeps the viewer's view choices.
    pub fn open(
        files: &LooseFiles,
        file: &Path,
        settings: Arc<Mutex<Connection>>,
        ctx: Option<egui::Context>,
    ) -> Result<Self, String> {
        let folder = file.parent().ok_or("it has no folder")?;
        let name = file.file_name().ok_or("it has no name")?.to_string_lossy().into_owned();
        std::fs::create_dir_all(&files.scratch).map_err(|error| error.to_string())?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = files.scratch.join(format!("{}-{nanos}.db", std::process::id()));
        let created = (|| -> db::Result<(Connection, Library)> {
            let conn = db::open(&path)?;
            conn.execute(
                "INSERT INTO sqlite_sequence (name, seq) VALUES ('media', ?1), ('library_roots', ?1), ('tags', ?1)",
                [FIRST_ID],
            )?;
            let label = "Not in a library";
            let library = db::libraries::insert(&conn, &folder.to_string_lossy(), label, LibraryStatus::Online)?;
            Ok((conn, library))
        })();
        let (conn, library) = match created {
            Ok(created) => created,
            Err(error) => {
                remove_later(path);
                return Err(error.to_string());
            }
        };
        let reader = match db::open(&path) {
            Ok(reader) => Arc::new(Mutex::new(reader)),
            Err(error) => {
                drop(conn);
                remove_later(path);
                return Err(error.to_string());
            }
        };
        let mut gallery = Gallery::new(reader.clone(), settings, ctx.clone());
        gallery.set_search(GallerySearch {
            library: Some(library.id),
            sort_by: SortBy::Name,
            descending: false,
            ..Default::default()
        });
        let cancel = Arc::new(AtomicBool::new(false));
        let (done, filled) = channel();
        let fill = Fill {
            writer: IndexWriter::new(conn),
            root: library.id,
            name: name.clone(),
            metadata: files.metadata.clone(),
            cancel: cancel.clone(),
            done,
            ctx,
            path: path.clone(),
        };
        std::thread::Builder::new()
            .name("scratch index".into())
            .spawn(move || fill.run())
            .map_err(|error| error.to_string())?;
        Ok(Self {
            gallery,
            libraries: vec![library],
            file: file.to_path_buf(),
            rel_path: name,
            reader,
            path,
            cancel,
            filled,
            steps_done: 0,
            shown: false,
        })
    }

    /// Takes in what the filling thread wrote: the gallery loads it again. Call every frame.
    pub fn poll(&mut self) {
        let mut filled = false;
        while self.filled.try_recv().is_ok() {
            self.steps_done += 1;
            filled = true;
        }
        if filled {
            self.gallery.reload();
        }
        self.gallery.poll();
    }

    /// Waits until the scratch index is filled and loaded (tests).
    pub fn wait_filled(&mut self) {
        let before = self.steps_done;
        while self.steps_done < STEPS {
            match self.filled.recv_timeout(Duration::from_secs(30)) {
                Ok(()) => self.steps_done += 1,
                Err(_) => break,
            }
        }
        if self.steps_done != before {
            self.gallery.reload();
        }
        self.gallery.wait_for_loads();
    }

    /// Whether the folder is still being read, or the gallery loaded.
    pub fn is_busy(&self) -> bool {
        self.steps_done < STEPS || self.gallery.is_loading()
    }

    /// The opened file's gallery position, once the scratch index and its gallery have it.
    pub fn position(&self) -> Option<usize> {
        if self.gallery.is_loading() {
            return None;
        }
        let id = {
            let conn = self.reader.lock().unwrap_or_else(|p| p.into_inner());
            db::media::id_of(&conn, self.libraries[0].id, &self.rel_path).ok()??
        };
        self.gallery.position_of(id)
    }
}

impl Drop for Loose {
    /// Stops the filling and deletes the scratch index once nothing has it open anymore (REQ-033-05).
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        remove_later(self.path.clone());
    }
}

/// What the filling thread needs.
struct Fill {
    writer: IndexWriter,
    root: i64,
    name: String,
    metadata: Arc<dyn MetadataSource>,
    cancel: Arc<AtomicBool>,
    done: Sender<()>,
    ctx: Option<egui::Context>,
    path: PathBuf,
}

impl Fill {
    fn run(mut self) {
        for scope in [ScanScope::Files(vec![self.name.clone()]), ScanScope::Folder] {
            if self.cancel.load(Ordering::Relaxed) {
                break;
            }
            let mut on_progress = |_| {};
            let options = ScanOptions::new(4, &self.cancel, &mut on_progress);
            let is_reachable = |_: &Path| true;
            let deps = ScanDeps {
                writer: &mut self.writer,
                metadata: self.metadata.as_ref(),
                is_reachable: &is_reachable,
            };
            if let Err(error) = scan_root(deps, self.root, &scope, options) {
                tracing::error!("can't read {} into its scratch index: {error}", self.path.display());
            }
            let _ = self.done.send(());
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint();
            }
        }
    }
}

/// The scratch index's files: the database and SQLite's write-ahead log and shared memory.
fn files_of(path: &Path) -> [PathBuf; 3] {
    let with = |suffix: &str| {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    };
    [path.to_path_buf(), with("-wal"), with("-shm")]
}

/// Deletes a scratch index in the background: its connections close shortly after the gallery and the filling
/// thread end, and Windows doesn't delete open files. What is still there after a minute goes at the next start.
fn remove_later(path: PathBuf) {
    let removed = std::thread::Builder::new()
        .name("scratch cleanup".into())
        .spawn(move || {
            for _ in 0..600 {
                let left = files_of(&path)
                    .iter()
                    .filter(|file| std::fs::remove_file(file).is_err() && file.exists())
                    .count();
                if left == 0 {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
    if let Err(error) = removed {
        tracing::warn!("can't start deleting a scratch index: {error}");
    }
}

/// Deletes the scratch indexes an earlier run left behind (REQ-033-05).
pub fn clear_scratch(scratch: &Path) {
    let Ok(entries) = std::fs::read_dir(scratch) else {
        return;
    };
    for entry in entries.flatten() {
        if let Err(error) = std::fs::remove_file(entry.path()) {
            tracing::warn!("can't delete {}: {error}", entry.path().display());
        }
    }
}
