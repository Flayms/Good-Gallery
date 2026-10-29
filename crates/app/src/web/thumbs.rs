//! Thumbnails in the browser (REQ-019-05): rendered by the browser's decoders and encoders with the desktop's sizes
//! and ThumbHash, stored in the page's storage under `thumbs/<library>/<ab>/<cd>/<sha1>.webp` (so removing a library
//! removes its thumbnails, REQ-019-11), size-capped with LRU eviction like the desktop cache (REQ-005-11).

use super::{folders, js};
use base64::Engine as _;
use db::rusqlite::Connection;
use db::thumbs::ThumbSource;
use domain::media::MediaKind;
use domain::thumbs::{
    MAX_THUMB_HEIGHT, MAX_THUMB_WIDTH, THUMB_WIDTHS, THUMBHASH_SIZE, ThumbKey, VIDEO_FRAME_POSITION, fit_inside,
};
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use wasm_bindgen::{JsCast as _, JsValue};

const DIR: &str = "thumbs";
/// Encoding quality, as the desktop's WebP quality 80.
const QUALITY: f64 = 0.8;
/// Evicting below the cap leaves headroom, so eviction doesn't run on every write.
const EVICT_TO: f64 = 0.9;
/// Renders at once: the browser decodes on its own threads, so two keep it busy.
const RENDERERS: usize = 2;
/// Media items asked for in one background batch.
const BATCH: usize = 50;

/// A rendered (or failed) thumbnail, as the desktop's service reports it.
pub struct ThumbnailReady {
    pub media: i64,
    /// The thumbnail files are stored. False if the item can't be rendered (or its folder can't be read).
    pub stored: bool,
}

#[derive(Default)]
struct CacheState {
    max_bytes: u64,
    /// Stored files: size and when last written or read.
    files: HashMap<String, (u64, f64)>,
    bytes: u64,
    measured: bool,
    evicting: bool,
}

/// The thumbnail cache in the page's storage. Cheap to clone; all clones share one cache.
#[derive(Clone)]
pub struct ThumbCache {
    state: Arc<Mutex<CacheState>>,
}

impl ThumbCache {
    /// Opens the cache and measures it in the background.
    pub fn open(max_bytes: u64) -> Self {
        let cache = Self {
            state: Arc::new(Mutex::new(CacheState {
                max_bytes,
                ..CacheState::default()
            })),
        };
        let measuring = cache.clone();
        super::spawn(async move {
            match js::wait(js::opfs_list(DIR)).await {
                Ok(list) => {
                    let mut state = measuring.lock();
                    for entry in list.dyn_into::<js_sys::Array>().map(|a| a.to_vec()).unwrap_or_default() {
                        let (Some(path), Some(size), Some(at)) = (
                            super::at(&entry, 0).as_string(),
                            super::at(&entry, 1).as_f64(),
                            super::at(&entry, 2).as_f64(),
                        ) else {
                            continue;
                        };
                        // Written while measuring: already counted.
                        if !state.files.contains_key(&format!("{DIR}/{path}")) {
                            state.bytes += size as u64;
                            state.files.insert(format!("{DIR}/{path}"), (size as u64, at));
                        }
                    }
                    state.measured = true;
                }
                Err(error) => tracing::warn!("can't measure the thumbnail cache: {error}"),
            }
            measuring.evict();
        });
        cache
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CacheState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Where the thumbnail of `key` at `width` is stored.
    pub fn path(&self, key: &ThumbKey, width: u32) -> PathBuf {
        let hash = domain::thumbs::thumb_hash(key, width);
        PathBuf::from(format!(
            "{DIR}/{}/{}/{}/{hash}.webp",
            key.root_id,
            &hash[0..2],
            &hash[2..4]
        ))
    }

    pub fn is_measured(&self) -> bool {
        self.lock().measured
    }

    pub fn usage(&self) -> u64 {
        self.lock().bytes
    }

    pub fn set_max_bytes(&self, max_bytes: u64) {
        self.lock().max_bytes = max_bytes;
        self.evict();
    }

    /// Notes a read, for the LRU.
    pub fn touch(&self, path: &str) {
        if let Some(entry) = self.lock().files.get_mut(path) {
            entry.1 = js_sys::Date::now();
        }
    }

    async fn write(&self, path: &str, data: &[u8]) -> Result<(), String> {
        js::wait(js::opfs_write(path, &js_sys::Uint8Array::from(data))).await?;
        let mut state = self.lock();
        let old = state
            .files
            .insert(path.to_string(), (data.len() as u64, js_sys::Date::now()));
        state.bytes = state.bytes - old.map_or(0, |(size, _)| size) + data.len() as u64;
        drop(state);
        self.evict();
        Ok(())
    }

    /// Removes the least recently used thumbnails down to 90 % of the cap, in the background.
    fn evict(&self) {
        let victims = {
            let mut state = self.lock();
            if state.evicting || !state.measured || state.bytes <= state.max_bytes {
                return;
            }
            state.evicting = true;
            let target = (state.max_bytes as f64 * EVICT_TO) as u64;
            let mut files: Vec<(f64, String, u64)> = state
                .files
                .iter()
                .map(|(path, (size, at))| (*at, path.clone(), *size))
                .collect();
            files.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut bytes = state.bytes;
            let mut victims = Vec::new();
            for (_, path, size) in files {
                if bytes <= target {
                    break;
                }
                bytes -= size;
                victims.push(path);
            }
            victims
        };
        let cache = self.clone();
        super::spawn(async move {
            for path in victims {
                if js::wait(js::opfs_remove(&path)).await.is_ok() {
                    let mut state = cache.lock();
                    if let Some((size, _)) = state.files.remove(&path) {
                        state.bytes -= size;
                    }
                }
            }
            cache.lock().evicting = false;
        });
    }

    /// Deletes every thumbnail (REQ-010-06).
    pub async fn clear(&self) -> u64 {
        if let Err(error) = js::wait(js::opfs_remove(DIR)).await {
            tracing::error!("can't clear the thumbnail cache: {error}");
        }
        let mut state = self.lock();
        state.files.clear();
        state.bytes = 0;
        0
    }

    fn forget(&self, library: i64) {
        let prefix = format!("{DIR}/{library}/");
        let mut state = self.lock();
        let gone: Vec<String> = state
            .files
            .keys()
            .filter(|path| path.starts_with(&prefix))
            .cloned()
            .collect();
        for path in gone {
            if let Some((size, _)) = state.files.remove(&path) {
                state.bytes -= size;
            }
        }
    }
}

thread_local! {
    static CACHE: std::cell::RefCell<Option<ThumbCache>> = const { std::cell::RefCell::new(None) };
}

/// Deletes a removed library's thumbnails (REQ-019-11).
pub async fn forget_library(library: i64) {
    if let Some(cache) = CACHE.with_borrow(Clone::clone) {
        cache.forget(library);
    }
    if let Err(error) = js::wait(js::opfs_remove(&format!("{DIR}/{library}"))).await {
        tracing::warn!("can't delete the thumbnails of library {library}: {error}");
    }
}

#[derive(Default)]
struct Queue {
    /// Asked for by tiles, newest last.
    requested: Vec<i64>,
    running: HashSet<i64>,
    /// Background work waiting, newest first.
    background: Vec<i64>,
    /// Items whose folder can't be read this visit: background work leaves them out.
    unreadable: HashSet<i64>,
    renderers: usize,
    /// No more pending items in the index until something new is written.
    idle: bool,
}

/// Renders thumbnails: requested ones first (newest request first), then every pending item, newest first
/// (REQ-005-08).
pub struct ThumbnailService {
    queue: Mutex<Queue>,
    cache: ThumbCache,
    ready: Sender<ThumbnailReady>,
    ctx: egui::Context,
}

impl ThumbnailService {
    pub fn start(cache: ThumbCache, ready: Sender<ThumbnailReady>, ctx: egui::Context) -> Arc<Self> {
        CACHE.with_borrow_mut(|kept| *kept = Some(cache.clone()));
        Arc::new(Self {
            queue: Mutex::new(Queue::default()),
            cache,
            ready,
            ctx,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn cache(&self) -> &ThumbCache {
        &self.cache
    }

    /// Renders `media` before background work.
    pub fn request(self: &Arc<Self>, media: i64) {
        {
            let mut queue = self.lock();
            queue.requested.retain(|id| *id != media);
            queue.requested.push(media);
        }
        self.wake();
    }

    /// New files were indexed: looks for pending items again.
    pub fn kick(self: &Arc<Self>) {
        let mut queue = self.lock();
        queue.idle = false;
        queue.unreadable.clear();
        drop(queue);
        self.wake();
    }

    fn wake(self: &Arc<Self>) {
        loop {
            {
                let mut queue = self.lock();
                if queue.renderers >= RENDERERS {
                    return;
                }
                queue.renderers += 1;
            }
            let service = self.clone();
            super::spawn(async move { service.render_loop().await });
        }
    }

    fn next(&self, conn: &Connection) -> Option<i64> {
        let mut queue = self.lock();
        while let Some(media) = queue.requested.pop() {
            if queue.running.insert(media) {
                return Some(media);
            }
        }
        loop {
            while let Some(media) = queue.background.pop() {
                if queue.running.insert(media) {
                    return Some(media);
                }
            }
            if queue.idle {
                return None;
            }
            let running: Vec<i64> = queue.running.iter().chain(&queue.unreadable).copied().collect();
            match db::thumbs::pending_thumbs(conn, BATCH, &running) {
                Ok(ids) if !ids.is_empty() => queue.background = ids.into_iter().rev().collect(),
                Ok(_) => {
                    queue.idle = true;
                    return None;
                }
                Err(error) => {
                    tracing::error!("can't find pending thumbnails: {error}");
                    queue.idle = true;
                    return None;
                }
            }
        }
    }

    async fn render_loop(self: Arc<Self>) {
        let conn = match super::index::open() {
            Ok(conn) => conn,
            Err(error) => {
                tracing::error!("thumbnails can't open the index: {error}");
                self.lock().renderers -= 1;
                return;
            }
        };
        while let Some(media) = self.next(&conn) {
            let stored = self.render(&conn, media).await;
            self.lock().running.remove(&media);
            let _ = self.ready.send(ThumbnailReady { media, stored });
            self.ctx.request_repaint();
        }
        self.lock().renderers -= 1;
    }

    async fn render(&self, conn: &Connection, media: i64) -> bool {
        let source = match db::thumbs::thumb_source(conn, media) {
            Ok(Some(source)) => source,
            Ok(None) => return false,
            Err(error) => {
                tracing::error!("can't read media item {media}: {error}");
                return false;
            }
        };
        let file = match folders::file(source.key.root_id, &source.key.rel_path).await {
            Ok(file) => file,
            // Not readable now: tried again when a tile asks for it or the folder is picked again (REQ-019-09).
            Err(_) => {
                self.lock().unreadable.insert(media);
                return false;
            }
        };
        match self.render_file(conn, &source, &file).await {
            Ok(hash) => {
                if let Err(error) = db::thumbs::set_thumbnail(conn, &source, Some(&hash)) {
                    tracing::error!("can't store the thumbnail of {}: {error}", source.key.rel_path);
                }
                true
            }
            Err(error) => {
                tracing::warn!("can't render a thumbnail of {}: {error}", source.key.rel_path);
                // A failed thumbnail isn't tried again until the file changes (REQ-005-09).
                let _ = db::thumbs::set_thumbnail(conn, &source, None);
                false
            }
        }
    }

    /// Renders both widths and returns the ThumbHash.
    async fn render_file(
        &self,
        conn: &Connection,
        source: &ThumbSource,
        file: &web_sys::File,
    ) -> Result<String, String> {
        let bitmap = match source.kind {
            MediaKind::Image => js::wait(js::decode_image(file)).await?,
            MediaKind::Video => {
                let frame = js::wait(js::video_frame(file, VIDEO_FRAME_POSITION)).await?;
                store_video_details(conn, source.id, &frame);
                super::at(&frame, 0)
            }
        };
        let result = self.encode(source, &bitmap).await;
        if let Ok(bitmap) = bitmap.dyn_into::<web_sys::ImageBitmap>() {
            bitmap.close();
        }
        result
    }

    async fn encode(&self, source: &ThumbSource, bitmap: &JsValue) -> Result<String, String> {
        let side = |name| super::get(bitmap, name).as_f64().unwrap_or(0.0) as u32;
        let (full_width, full_height) = (side("width"), side("height"));
        if full_width == 0 || full_height == 0 {
            return Err("empty image".into());
        }
        let (width, height) = fit_inside(full_width, full_height, MAX_THUMB_WIDTH, MAX_THUMB_HEIGHT, false);
        let (hash_width, hash_height) = fit_inside(width, height, THUMBHASH_SIZE, THUMBHASH_SIZE, true);
        let small = js::rgba_of(bitmap, hash_width, hash_height)
            .map_err(|e| js::message(&e))?
            .to_vec();
        let hash = thumbhash::rgba_to_thumb_hash(hash_width as usize, hash_height as usize, &small);
        for thumb_width in THUMB_WIDTHS {
            let (w, h) = if width > thumb_width {
                let h = ((f64::from(height) * f64::from(thumb_width) / f64::from(width)).round() as u32).max(1);
                (thumb_width, h)
            } else {
                (width, height)
            };
            let encoded = js::wait(js::encode_image(bitmap, w, h, QUALITY)).await?;
            let encoded = js::bytes(&encoded).ok_or("the browser encoded nothing")?;
            let path = self.cache.path(&source.key, thumb_width);
            self.cache.write(&path.to_string_lossy(), &encoded).await?;
        }
        Ok(base64::engine::general_purpose::STANDARD.encode(hash))
    }
}

/// Fills in a video's duration and size where its container didn't say (WebM, MKV): the browser knows them.
fn store_video_details(conn: &Connection, media: i64, frame: &JsValue) {
    let number = |i| super::at(frame, i).as_f64().filter(|n| n.is_finite() && *n > 0.0);
    let result = conn.execute(
        "UPDATE media SET duration = coalesce(duration, ?1), width = coalesce(width, ?2), height = coalesce(height, ?3)
         WHERE id = ?4",
        (
            number(1),
            number(2).map(|n| n as i64),
            number(3).map(|n| n as i64),
            media,
        ),
    );
    if let Err(error) = result {
        tracing::warn!("can't store the details of video {media}: {error}");
    }
}

/// The settings that apply in the browser: the thumbnail cache's limit, usage and clearing (REQ-019-13).
pub struct WebSettings {
    pub thumbnails: Arc<ThumbnailService>,
}

impl crate::SettingsTargets for WebSettings {
    fn set_quick_check_interval(&self, _minutes: u32) {}

    fn set_io_concurrency(&self, _reads: usize) {}

    fn set_cache_limit(&self, bytes: u64) {
        self.thumbnails.cache().set_max_bytes(bytes);
    }

    fn cache_usage(&self) -> Option<u64> {
        let cache = self.thumbnails.cache();
        cache.is_measured().then(|| cache.usage())
    }

    /// Starts clearing; the size shows as 0 at once, and stays so.
    fn clear_cache(&self) -> u64 {
        let cache = self.thumbnails.cache().clone();
        super::spawn(async move {
            cache.clear().await;
        });
        0
    }
}
