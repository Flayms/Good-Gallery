//! Scans in the browser (REQ-019-03, REQ-019-10): one at a time, queued per library like the desktop's scan queue,
//! as an async task that yields to the UI between files. A scan lists the picked folder, reads the metadata of new and
//! changed files with `meta` and writes them in batches; files that vanished are removed once it completes.

use super::folders::{self, Source};
use super::{Slice, js};
use db::libraries::LibraryStatus;
use db::writer::{IndexWriter, IndexedFile};
use domain::media::{METADATA_VERSION, MediaKind, MediaMetadata, find_sidecar, is_ignored_dir, is_sidecar, media_kind};
use eframe::egui;
use indexer::{CurrentScan, IndexerEvent, IndexerStatus, ScanKind, ScanRequests, ScanResult};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use wasm_bindgen::JsCast as _;

/// Files written per transaction.
const BATCH: usize = 100;
/// Longest time read files wait to be written, so they show while a scan runs.
const FLUSH_MS: i64 = 1500;
/// Bytes read for one file's metadata at most.
const MAX_METADATA_BYTES: u64 = 32 << 20;

/// A file found in the folder.
struct Found {
    rel_path: String,
    file_name: String,
    file: web_sys::File,
}

#[derive(Default)]
struct Queue {
    waiting: Vec<i64>,
    running: Option<i64>,
    /// Libraries removed while their scan ran: it stops and writes nothing more.
    cancelled: HashSet<i64>,
    status: IndexerStatus,
}

/// The browser's scan queue. Its scans run on the page's event loop.
pub struct WebIndexer {
    queue: Mutex<Queue>,
    events: Sender<IndexerEvent>,
    ctx: egui::Context,
    on_written: Box<dyn Fn() + Send + Sync>,
}

impl WebIndexer {
    /// `on_written` runs after files were written (thumbnails wait for them).
    pub fn new(events: Sender<IndexerEvent>, ctx: egui::Context, on_written: Box<dyn Fn() + Send + Sync>) -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(Queue::default()),
            events,
            ctx,
            on_written,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn send(&self, event: IndexerEvent) {
        let _ = self.events.send(event);
        self.ctx.request_repaint();
    }

    fn report(&self, current: Option<CurrentScan>) {
        let status = {
            let mut queue = self.lock();
            queue.status = IndexerStatus {
                current,
                queue: queue.waiting.clone(),
            };
            queue.status.clone()
        };
        self.send(IndexerEvent::Status(status));
    }

    async fn run(self: Arc<Self>) {
        loop {
            let library = {
                let mut queue = self.lock();
                if queue.waiting.is_empty() {
                    queue.running = None;
                    break;
                }
                let library = queue.waiting.remove(0);
                queue.running = Some(library);
                queue.cancelled.remove(&library);
                library
            };
            let result = match scan(&self, library).await {
                Ok(result) => {
                    tracing::info!("scan of library {library} ended: {result:?}");
                    result
                }
                Err(error) => {
                    tracing::error!("scan of library {library} failed: {error}");
                    ScanResult::Failed(error)
                }
            };
            self.report(None);
            self.send(IndexerEvent::Done(library, result));
            (self.on_written)();
        }
        self.report(None);
    }
}

impl ScanRequests for WebIndexer {
    /// Every scan in the browser lists the whole folder: there are no watchers or folder dates to narrow it.
    fn request_scan(&self, library: i64, _kind: ScanKind) {
        let start = {
            let mut queue = self.lock();
            if !queue.waiting.contains(&library) {
                queue.waiting.push(library);
            }
            // Marks the queue as running until its task takes the first library.
            let idle = queue.running.is_none();
            queue.running.get_or_insert(-1);
            idle
        };
        if start {
            // `request_scan` gets `&self`; the queue runs on a handle of its own.
            super::spawn(self.handle().run());
        } else {
            self.report(self.lock().status.current);
        }
    }

    fn remove_library(&self, library: i64) {
        {
            let mut queue = self.lock();
            queue.waiting.retain(|id| *id != library);
            if queue.running == Some(library) {
                queue.cancelled.insert(library);
            }
        }
        super::spawn(async move {
            folders::forget(library).await;
            super::thumbs::forget_library(library).await;
        });
    }
}

thread_local! {
    static INDEXER: std::cell::RefCell<Option<Arc<WebIndexer>>> = const { std::cell::RefCell::new(None) };
}

impl WebIndexer {
    /// Keeps the page's indexer, so `&self` can hand its queue an owned handle.
    pub fn install(self: &Arc<Self>) {
        INDEXER.with_borrow_mut(|indexer| *indexer = Some(self.clone()));
    }

    fn handle(&self) -> Arc<Self> {
        INDEXER
            .with_borrow(|indexer| indexer.clone())
            .expect("the indexer is installed")
    }

    fn is_cancelled(&self, library: i64) -> bool {
        self.lock().cancelled.contains(&library)
    }
}

/// Lists the folder: media files, and sidecars by folder (lower-cased name → names).
async fn list(source: &Source) -> Result<(Vec<Found>, HashMap<String, (web_sys::File, i64)>), String> {
    let mut media = Vec::new();
    let mut sidecars = HashMap::new();
    let mut slice = Slice::new();
    let mut add = |rel_path: String, file: web_sys::File| {
        let file_name = rel_path.rsplit('/').next().unwrap_or_default().to_string();
        if is_sidecar(&file_name) {
            let mtime = file.last_modified() as i64;
            sidecars.insert(rel_path, (file, mtime));
        } else if media_kind(&file_name).is_some() {
            media.push(Found {
                rel_path,
                file_name,
                file,
            });
        }
    };
    match source {
        Source::Files(files) => {
            for (rel_path, file) in files.iter() {
                let mut parts: Vec<&str> = rel_path.split('/').collect();
                parts.pop();
                if parts.iter().any(|dir| is_ignored_dir(dir)) {
                    continue;
                }
                add(rel_path.clone(), file.clone());
            }
        }
        Source::Directory(root) => {
            let mut dirs = vec![(String::new(), root.clone())];
            while let Some((prefix, dir)) = dirs.pop() {
                let entries = match js::wait(js::list_directory(&dir)).await {
                    Ok(entries) => entries,
                    // A folder that can't be listed is skipped (REQ-002-11); the root must be readable.
                    Err(error) if !prefix.is_empty() => {
                        tracing::warn!("can't list {prefix}: {error}");
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                for entry in entries
                    .dyn_into::<js_sys::Array>()
                    .map_err(|_| "no folder listing")?
                    .iter()
                {
                    let Some(name) = super::at(&entry, 0).as_string() else {
                        continue;
                    };
                    let rel_path = if prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{prefix}/{name}")
                    };
                    if let Ok(file) = super::at(&entry, 1).dyn_into::<web_sys::File>() {
                        add(rel_path, file);
                    } else if !is_ignored_dir(&name) {
                        dirs.push((rel_path, super::at(&entry, 2)));
                    }
                }
                slice.yield_if_due().await;
            }
        }
    }
    media.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok((media, sidecars))
}

/// `start..end` of `file` as bytes.
async fn read_range(file: &web_sys::File, start: u64, end: u64) -> Result<Vec<u8>, String> {
    let blob = file
        .slice_with_f64_and_f64(start as f64, end as f64)
        .map_err(|e| js::message(&e))?;
    let buffer = js::wait(blob.array_buffer()).await?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

/// The tags of a file through `meta`, reading the parts it asks for.
async fn tags_of(file: &web_sys::File, file_name: &str) -> meta::rules::Tags {
    let size = file.size() as u64;
    let mut bytes = meta::Bytes::new(size);
    if let Ok(head) = read_range(file, 0, meta::FIRST_READ.min(size)).await {
        bytes.add(0, head);
    }
    for _ in 0..32 {
        match meta::read(file_name, &bytes) {
            Ok(tags) => return tags,
            Err(missing) => {
                if bytes.known() + missing.len > MAX_METADATA_BYTES {
                    break;
                }
                // Reads a little more than asked: the next part usually follows.
                let end = (missing.at + missing.len.max(64 << 10)).min(size);
                match read_range(file, missing.at, end).await {
                    Ok(data) => bytes.add(missing.at, data),
                    Err(_) => break,
                }
            }
        }
    }
    meta::rules::Tags::new()
}

async fn metadata(found: &Found, sidecar: Option<&web_sys::File>) -> MediaMetadata {
    let tags = tags_of(&found.file, &found.file_name).await;
    let sidecar_tags = match sidecar {
        Some(sidecar) => read_range(sidecar, 0, (sidecar.size() as u64).min(4 << 20))
            .await
            .ok()
            .map(|data| meta::read_sidecar(&data)),
        None => None,
    };
    // A panic in the rules costs this file's metadata only (REQ-011-10).
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        meta::rules::extract_metadata(&tags, sidecar_tags.as_ref())
    }))
    .unwrap_or_default()
}

async fn scan(indexer: &WebIndexer, library: i64) -> Result<ScanResult, String> {
    let mut writer = IndexWriter::new(super::index::open().map_err(|e| e.to_string())?);
    let Some(source) = folders::source(library) else {
        writer
            .set_root_status(library, LibraryStatus::Offline, None)
            .map_err(|e| e.to_string())?;
        return Ok(ScanResult::Offline);
    };
    let mut current = CurrentScan {
        library,
        kind: ScanKind::Full,
        scanned: 0,
        indexed: 0,
    };
    indexer.report(Some(current));
    let (found, sidecar_files) = match list(&source).await {
        Ok(listed) => listed,
        Err(error) => {
            tracing::warn!("can't list library {library}: {error}");
            writer
                .set_root_status(library, LibraryStatus::Offline, None)
                .map_err(|e| e.to_string())?;
            return Ok(ScanResult::Offline);
        }
    };
    // Sidecars by folder, as `find_sidecar` wants them.
    let mut sidecars: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();
    for rel_path in sidecar_files.keys() {
        let (dir, name) = rel_path.rsplit_once('/').unwrap_or(("", rel_path));
        sidecars
            .entry(dir.to_string())
            .or_default()
            .entry(name.to_lowercase())
            .or_default()
            .push(name.to_string());
    }
    let mut existing = writer.existing(library).map_err(|e| e.to_string())?;
    let mut batch = Vec::new();
    let mut flushed_at = super::now_ms();
    let mut slice = Slice::new();
    let mut folders_seen: HashMap<String, i64> = HashMap::new();
    for found in &found {
        if indexer.is_cancelled(library) {
            return Ok(ScanResult::Cancelled);
        }
        let dir = found.rel_path.rsplit_once('/').map_or("", |(dir, _)| dir);
        let mut parent = dir;
        loop {
            folders_seen.entry(parent.to_string()).or_insert(0);
            match parent.rsplit_once('/') {
                Some((up, _)) => parent = up,
                None if !parent.is_empty() => parent = "",
                None => break,
            }
        }
        let sidecar = sidecars
            .get(dir)
            .and_then(|names| find_sidecar(names, &found.file_name))
            .map(|name| {
                if dir.is_empty() {
                    name.to_string()
                } else {
                    format!("{dir}/{name}")
                }
            })
            .and_then(|path| sidecar_files.get(&path));
        let size = found.file.size() as i64;
        let mtime = found.file.last_modified() as i64;
        let sidecar_mtime = sidecar.map(|(_, mtime)| *mtime);
        let known = existing.remove(&found.rel_path);
        current.scanned += 1;
        let unchanged = known.as_ref().is_some_and(|known| {
            known.size == size
                && known.mtime == mtime
                && known.sidecar_mtime == sidecar_mtime
                && known.meta_version >= METADATA_VERSION
        });
        if !unchanged {
            let kind = media_kind(&found.file_name).unwrap_or(MediaKind::Image);
            batch.push(IndexedFile {
                rel_path: found.rel_path.clone(),
                file_name: found.file_name.clone(),
                kind,
                size,
                mtime,
                sidecar_mtime,
                metadata: metadata(found, sidecar.map(|(file, _)| file)).await,
            });
        }
        if batch.len() >= BATCH || (!batch.is_empty() && super::now_ms() - flushed_at >= FLUSH_MS) {
            current.indexed += batch.len() as u64;
            writer.write(library, &batch).map_err(|e| e.to_string())?;
            batch.clear();
            flushed_at = super::now_ms();
            indexer.send(IndexerEvent::Written(library));
            (indexer.on_written)();
            indexer.report(Some(current));
        } else if current.scanned.is_multiple_of(250) {
            indexer.report(Some(current));
        }
        slice.yield_if_due().await;
    }
    if indexer.is_cancelled(library) {
        return Ok(ScanResult::Cancelled);
    }
    if !batch.is_empty() {
        writer.write(library, &batch).map_err(|e| e.to_string())?;
        indexer.send(IndexerEvent::Written(library));
    }
    let vanished: Vec<i64> = existing.values().map(|entry| entry.id).collect();
    writer.remove(&vanished).map_err(|e| e.to_string())?;
    if found.is_empty() {
        folders_seen.insert(String::new(), 0);
    }
    writer
        .save_folders(library, &folders_seen, true)
        .map_err(|e| e.to_string())?;
    writer.prune_tags().map_err(|e| e.to_string())?;
    writer
        .set_root_status(library, LibraryStatus::Online, Some(super::now_ms()))
        .map_err(|e| e.to_string())?;
    Ok(ScanResult::Completed)
}
