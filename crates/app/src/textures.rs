//! Thumbnail textures for the tiles (M0 spike, `docs/architecture.md`): worker threads read thumbnails from the
//! cache, decode them with libwebp, build the mip chain and create the `wgpu::Texture` themselves; the UI thread only
//! registers finished textures, within a time budget per frame. An LRU with a byte budget keeps GPU memory bounded
//! (REQ-006-13).

use eframe::egui::{self, TextureId};
use eframe::egui_wgpu::{self, wgpu};
use indexer::thumb_cache::ThumbCache;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Decode threads: more starve the UI thread and an integrated GPU (M0).
const WORKERS: usize = 4;
/// UI-thread time per frame for registering finished textures.
const REGISTER_BUDGET: Duration = Duration::from_millis(3);
/// GPU memory for thumbnails (REQ-006-13). At 12 columns the visible tiles and the prefetch band alone take ≈ 330 MB
/// (400 px thumbnails with mips).
pub const DEFAULT_BUDGET: usize = 512 << 20;

/// A thumbnail a tile wants: the cached file of the width it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureRequest {
    pub media: i64,
    pub path: PathBuf,
    pub width: u32,
}

/// What a worker made of a request.
enum Outcome {
    Loaded {
        texture: wgpu::Texture,
        size: [u32; 2],
        bytes: usize,
    },
    /// Not in the cache (not rendered yet, or evicted).
    Missing,
    /// In the cache, but unreadable.
    Broken,
}

struct Done {
    media: i64,
    width: u32,
    outcome: Outcome,
}

#[derive(Default)]
struct Queue {
    /// Highest priority first; replaced every frame, so stale requests drop out.
    wanted: Vec<TextureRequest>,
    next: usize,
    in_flight: HashSet<(i64, u32)>,
    stop: bool,
}

struct Entry {
    id: TextureId,
    texture: wgpu::Texture,
    width: u32,
    size: [u32; 2],
    bytes: usize,
    last_used: u64,
    loaded_at: Instant,
}

/// A loaded thumbnail.
#[derive(Debug, Clone, Copy)]
pub struct Loaded {
    pub id: TextureId,
    /// Pixels.
    pub size: [u32; 2],
    /// When it was registered, for the fade-in.
    pub at: Instant,
}

pub struct TextureStreamer {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    results: Receiver<Done>,
    render_state: egui_wgpu::RenderState,
    entries: HashMap<i64, Entry>,
    /// Media items whose thumbnail file isn't in the cache, and those just found so.
    missing: HashSet<i64>,
    newly_missing: Vec<i64>,
    /// Media items whose cached thumbnail can't be decoded.
    broken: HashSet<i64>,
    bytes: usize,
    budget: usize,
    frame: u64,
    workers: Vec<std::thread::JoinHandle<()>>,
}

impl TextureStreamer {
    pub fn new(render_state: egui_wgpu::RenderState, cache: ThumbCache, budget: usize, ctx: egui::Context) -> Self {
        let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let (tx, results) = channel();
        let workers = (0..WORKERS)
            .map(|i| {
                let (queue, tx, cache, ctx) = (queue.clone(), tx.clone(), cache.clone(), ctx.clone());
                let (device, gpu) = (render_state.device.clone(), render_state.queue.clone());
                std::thread::Builder::new()
                    .name(format!("textures {i}"))
                    .spawn(move || work(&queue, &tx, &cache, &device, &gpu, &ctx))
                    .expect("can't start a texture thread")
            })
            .collect();
        Self {
            queue,
            results,
            render_state,
            entries: HashMap::new(),
            missing: HashSet::new(),
            newly_missing: Vec::new(),
            broken: HashSet::new(),
            bytes: 0,
            budget,
            frame: 0,
            workers,
        }
    }

    /// Registers finished textures until the frame's budget is spent, then evicts down to the byte budget. Call once
    /// per frame before drawing.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        let start = Instant::now();
        while start.elapsed() < REGISTER_BUDGET {
            let Ok(done) = self.results.try_recv() else { break };
            match done.outcome {
                Outcome::Loaded { texture, size, bytes } => self.register(done.media, done.width, texture, size, bytes),
                Outcome::Missing => {
                    if self.missing.insert(done.media) {
                        self.newly_missing.push(done.media);
                    }
                }
                Outcome::Broken => {
                    self.broken.insert(done.media);
                }
            }
        }
        if self.bytes > self.budget {
            self.evict();
        }
    }

    fn register(&mut self, media: i64, width: u32, texture: wgpu::Texture, size: [u32; 2], bytes: usize) {
        if self.entries.get(&media).is_some_and(|entry| entry.width >= width) {
            texture.destroy();
            return;
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let id = self
            .render_state
            .renderer
            .write()
            .register_native_texture_with_sampler_options(
                &self.render_state.device,
                &view,
                wgpu::SamplerDescriptor {
                    label: Some("thumbnail"),
                    mag_filter: wgpu::FilterMode::Linear,
                    min_filter: wgpu::FilterMode::Linear,
                    mipmap_filter: wgpu::MipmapFilterMode::Linear,
                    ..Default::default()
                },
            );
        // A smaller version from before zooming in: the new one takes over without fading in again.
        let loaded_at = match self.entries.remove(&media) {
            Some(old) => {
                let at = old.loaded_at;
                self.free(old);
                at
            }
            None => Instant::now(),
        };
        self.bytes += bytes;
        self.entries.insert(
            media,
            Entry {
                id,
                texture,
                width,
                size,
                bytes,
                last_used: self.frame,
                loaded_at,
            },
        );
    }

    fn evict(&mut self) {
        let entries = self.entries.iter().map(|(media, e)| (*media, e.last_used, e.bytes));
        for media in eviction(entries, self.frame, self.bytes, self.budget) {
            if let Some(entry) = self.entries.remove(&media) {
                self.free(entry);
            }
        }
    }

    fn free(&mut self, entry: Entry) {
        self.bytes -= entry.bytes;
        self.render_state.renderer.write().free_texture(&entry.id);
        entry.texture.destroy();
    }

    /// Replaces the wanted thumbnails, in priority order (visible tiles, then ahead, then behind). Loaded ones count
    /// as used, so the LRU keeps what is about to scroll into view.
    pub fn request(&mut self, wanted: Vec<TextureRequest>) {
        let frame = self.frame;
        let mut seen = HashSet::new();
        let list: Vec<TextureRequest> = wanted
            .into_iter()
            .filter(|request| {
                if !seen.insert(request.media) {
                    return false;
                }
                // Loaded textures count as used even when a larger width can't be had.
                let wanted = match self.entries.get_mut(&request.media) {
                    Some(entry) => {
                        entry.last_used = frame;
                        entry.width < request.width
                    }
                    None => true,
                };
                wanted && !self.missing.contains(&request.media) && !self.broken.contains(&request.media)
            })
            .collect();
        let (lock, wake) = &*self.queue;
        let mut queue = lock.lock().unwrap_or_else(|p| p.into_inner());
        queue.wanted = list;
        queue.next = 0;
        wake.notify_all();
    }

    /// The loaded thumbnail of a media item; marks it as used this frame.
    pub fn get(&mut self, media: i64) -> Option<Loaded> {
        let frame = self.frame;
        self.entries.get_mut(&media).map(|entry| {
            entry.last_used = frame;
            Loaded {
                id: entry.id,
                size: entry.size,
                at: entry.loaded_at,
            }
        })
    }

    /// Whether the cached thumbnail exists but can't be decoded.
    pub fn is_broken(&self, media: i64) -> bool {
        self.broken.contains(&media)
    }

    /// Media items found missing from the cache since the last call: their thumbnails need rendering.
    pub fn take_missing(&mut self) -> Vec<i64> {
        std::mem::take(&mut self.newly_missing)
    }

    /// Looks for the item's thumbnail again (it was rendered meanwhile).
    pub fn retry(&mut self, media: i64) {
        self.missing.remove(&media);
        self.broken.remove(&media);
    }

    /// Forgets everything known about missing thumbnails, e.g. after the gallery reloaded.
    pub fn retry_all(&mut self) {
        self.missing.clear();
        self.newly_missing.clear();
        self.broken.clear();
    }

    pub fn texture_bytes(&self) -> usize {
        self.bytes
    }
}

impl Drop for TextureStreamer {
    fn drop(&mut self) {
        let (lock, wake) = &*self.queue;
        lock.lock().unwrap_or_else(|p| p.into_inner()).stop = true;
        wake.notify_all();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
        for (_, entry) in std::mem::take(&mut self.entries) {
            self.free(entry);
        }
    }
}

/// The textures to free, least recently used first, until `bytes` is at 90 % of `budget`. Textures used in this
/// or the last frame (visible, or requested as about to scroll into view) stay (REQ-006-13).
fn eviction(entries: impl Iterator<Item = (i64, u64, usize)>, frame: u64, mut bytes: usize, budget: usize) -> Vec<i64> {
    let target = budget / 10 * 9;
    let mut old: Vec<(u64, i64, usize)> = entries
        .filter(|&(_, last_used, _)| last_used + 1 < frame)
        .map(|(media, last_used, size)| (last_used, media, size))
        .collect();
    old.sort_unstable();
    let mut evicted = Vec::new();
    for (_, media, size) in old {
        if bytes <= target {
            break;
        }
        bytes -= size;
        evicted.push(media);
    }
    evicted
}

fn next_request(queue: &(Mutex<Queue>, Condvar)) -> Option<TextureRequest> {
    let (lock, wake) = queue;
    let mut queue = lock.lock().unwrap_or_else(|p| p.into_inner());
    loop {
        if queue.stop {
            return None;
        }
        while queue.next < queue.wanted.len() {
            let request = queue.wanted[queue.next].clone();
            queue.next += 1;
            if queue.in_flight.insert((request.media, request.width)) {
                return Some(request);
            }
        }
        queue = wake.wait(queue).unwrap_or_else(|p| p.into_inner());
    }
}

fn work(
    queue: &(Mutex<Queue>, Condvar),
    results: &Sender<Done>,
    cache: &ThumbCache,
    device: &wgpu::Device,
    gpu: &wgpu::Queue,
    ctx: &egui::Context,
) {
    media::helpers::lower_thread_priority();
    while let Some(request) = next_request(queue) {
        let outcome = match cache.read(&request.path) {
            Ok(Some(data)) => match decode(&data) {
                Some((size, rgba)) => {
                    let levels = mip_chain(size, rgba);
                    let bytes = levels.iter().map(Vec::len).sum();
                    Outcome::Loaded {
                        texture: upload(device, gpu, size, &levels),
                        size,
                        bytes,
                    }
                }
                None => Outcome::Broken,
            },
            Ok(None) => Outcome::Missing,
            Err(error) => {
                tracing::warn!("can't read thumbnail {}: {error}", request.path.display());
                Outcome::Broken
            }
        };
        let key = (request.media, request.width);
        let _ = results.send(Done {
            media: request.media,
            width: request.width,
            outcome,
        });
        ctx.request_repaint();
        queue.0.lock().unwrap_or_else(|p| p.into_inner()).in_flight.remove(&key);
    }
}

/// libwebp decode to RGBA (3× faster than the `image` crate's decoder, M0).
pub fn decode(data: &[u8]) -> Option<([u32; 2], Vec<u8>)> {
    let image = webp::Decoder::new(data).decode()?;
    let size = [image.width(), image.height()];
    if size[0] == 0 || size[1] == 0 {
        return None;
    }
    let rgba = if image.is_alpha() {
        image.to_vec()
    } else {
        image.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect()
    };
    Some((size, rgba))
}

/// Box-filtered mip chain down to 16 px, so tiles drawn smaller than their thumbnail don't alias.
pub fn mip_chain([width, height]: [u32; 2], level0: Vec<u8>) -> Vec<Vec<u8>> {
    let mut levels = vec![level0];
    let (mut w, mut h) = (width as usize, height as usize);
    while w.min(h) >= 32 {
        let (nw, nh) = (w / 2, h / 2);
        let src = levels.last().expect("level 0");
        let mut dst = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            let r0 = &src[(2 * y) * w * 4..];
            let r1 = &src[(2 * y + 1) * w * 4..];
            let out = &mut dst[y * nw * 4..(y + 1) * nw * 4];
            for x in 0..nw {
                for c in 0..4 {
                    let i = 8 * x + c;
                    let sum = u32::from(r0[i]) + u32::from(r0[i + 4]) + u32::from(r1[i]) + u32::from(r1[i + 4]);
                    out[4 * x + c] = ((sum + 2) / 4) as u8;
                }
            }
        }
        levels.push(dst);
        (w, h) = (nw, nh);
    }
    levels
}

fn upload(device: &wgpu::Device, queue: &wgpu::Queue, [width, height]: [u32; 2], levels: &[Vec<u8>]) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("thumbnail"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let (mut w, mut h) = (width, height);
    for (level, data) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        (w, h) = (w / 2, h / 2);
    }
    texture
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_webp_with_and_without_alpha() {
        let rgb = webp::Encoder::from_rgb(&[255, 0, 0].repeat(4 * 2), 4, 2).encode_lossless();
        let (size, pixels) = decode(&rgb).unwrap();
        assert_eq!(size, [4, 2]);
        assert_eq!(&pixels[..4], &[255, 0, 0, 255]);

        let rgba = webp::Encoder::from_rgba(&[0, 0, 255, 0].repeat(4 * 2), 4, 2).encode_lossless();
        let (_, pixels) = decode(&rgba).unwrap();
        assert_eq!(pixels[3], 0);
        assert!(decode(b"not webp").is_none());
    }

    #[test]
    fn req_006_13_frees_least_recently_shown_textures_down_to_90_percent_of_the_budget() {
        // (media, last used in frame, bytes); now frame 10, 100 bytes in use, budget 60.
        let entries = [(1, 3, 20), (2, 1, 20), (3, 9, 20), (4, 10, 20), (5, 5, 20)];
        assert_eq!(eviction(entries.into_iter(), 10, 100, 60), [2, 1, 5]);
    }

    #[test]
    fn req_006_13_keeps_textures_shown_or_requested_in_this_or_the_last_frame() {
        let entries = [(1, 9, 50), (2, 10, 50)];
        assert!(eviction(entries.into_iter(), 10, 100, 10).is_empty());
    }

    #[test]
    fn builds_mips_down_to_16_pixels() {
        let levels = mip_chain([64, 40], vec![200; 64 * 40 * 4]);
        let sizes: Vec<usize> = levels.iter().map(|level| level.len() / 4).collect();
        assert_eq!(sizes, [64 * 40, 32 * 20]);
        assert!(levels[1].iter().all(|&v| v == 200));
    }
}
