//! Texture streaming: decode (and for the native path, upload) on a worker pool; the UI thread only registers
//! textures, within a time budget per frame. LRU eviction by byte budget.

use crossbeam_channel::{Receiver, Sender};
use eframe::egui::{self, ColorImage, TextureHandle, TextureId, TextureOptions};
use eframe::egui_wgpu::{self, wgpu};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decoder {
    Image,
    Libwebp,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Upload {
    /// `ctx.load_texture` on the UI thread: egui-managed, no mipmaps.
    Egui,
    /// Own `wgpu::Texture` with a CPU-built mip chain, created and written on the worker; the UI thread registers it.
    Native,
}

enum Payload {
    Pixels(Vec<u8>),
    Gpu(wgpu::Texture),
}

struct Decoded {
    file: u32,
    target: u32,
    width: u32,
    height: u32,
    bytes: usize,
    payload: Payload,
    /// Read + decode (+ mips + upload for the native path) on the worker.
    work_ms: f32,
    read_ms: f32,
}

#[derive(Default)]
struct Queue {
    /// Files wanted, highest priority first; replaced every frame so stale requests drop out.
    wanted: Vec<u32>,
    next: usize,
    in_flight: HashSet<u32>,
    stop: bool,
}

enum Tex {
    Egui(#[allow(dead_code)] TextureHandle),
    Native(TextureId, wgpu::Texture),
}

struct Entry {
    tex: Tex,
    target: u32,
    id: TextureId,
    bytes: usize,
    last_used: u64,
}

pub struct Stats {
    pub work_ms: Vec<f32>,
    pub read_ms: Vec<f32>,
    /// UI-thread time per registered texture.
    pub upload_ms: Vec<f32>,
}

pub struct Streamer {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    results: Receiver<Decoded>,
    cache: HashMap<u32, Entry>,
    bytes: usize,
    pub budget: usize,
    /// UI-thread time per frame spent registering textures.
    pub upload_budget_ms: f32,
    pub stats: Stats,
    frame: u64,
    /// Thumbnail width to decode at (400 or 800), set from the tile width.
    pub target: Arc<AtomicU32>,
    workers: Vec<std::thread::JoinHandle<()>>,
}

/// libwebp decode scaled down to `target` px wide (never up), RGBA into our own buffer.
/// Stands in for reading the 400 px thumbnail where `pickThumbWidth` would choose it.
pub fn decode_scaled(bytes: &[u8], target: u32) -> Option<(u32, u32, Vec<u8>)> {
    use libwebp_sys::*;
    unsafe {
        let mut config: WebPDecoderConfig = std::mem::zeroed();
        if !WebPInitDecoderConfig(&mut config)
            || WebPGetFeatures(bytes.as_ptr(), bytes.len(), &mut config.input)
                != VP8StatusCode::VP8_STATUS_OK
        {
            return None;
        }
        let (w, h) = (config.input.width as u32, config.input.height as u32);
        let (tw, th) = if target < w {
            (target, ((h * target + w / 2) / w).max(1))
        } else {
            (w, h)
        };
        if (tw, th) != (w, h) {
            config.options.use_scaling = 1;
            config.options.scaled_width = tw as i32;
            config.options.scaled_height = th as i32;
        }
        let mut out = vec![0u8; (tw * th * 4) as usize];
        config.output.colorspace = WEBP_CSP_MODE::MODE_RGBA;
        config.output.is_external_memory = 1;
        config.output.u.RGBA = WebPRGBABuffer {
            rgba: out.as_mut_ptr(),
            stride: (tw * 4) as i32,
            size: out.len(),
        };
        let status = WebPDecode(bytes.as_ptr(), bytes.len(), &mut config);
        WebPFreeDecBuffer(&mut config.output);
        (status == VP8StatusCode::VP8_STATUS_OK).then_some((tw, th, out))
    }
}

fn lower_thread_priority() {
    use windows_sys::Win32::System::Threading::*;
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

fn decode(bytes: &[u8], decoder: Decoder, target: u32) -> Option<(u32, u32, Vec<u8>)> {
    match decoder {
        Decoder::Libwebp => decode_scaled(bytes, target),
        Decoder::Image => {
            let img = image::load_from_memory_with_format(bytes, image::ImageFormat::WebP)
                .ok()?
                .to_rgba8();
            Some((img.width(), img.height(), img.into_raw()))
        }
    }
}

/// Box-filtered mip chain down to 16 px. Gamma-space averaging; good enough for the spike.
fn mip_chain(width: u32, height: u32, level0: Vec<u8>) -> Vec<Vec<u8>> {
    let mut levels = vec![level0];
    let (mut w, mut h) = (width as usize, height as usize);
    while w.min(h) >= 32 {
        let (nw, nh) = (w / 2, h / 2);
        let src = levels.last().unwrap();
        let mut dst = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            let r0 = &src[(2 * y) * w * 4..];
            let r1 = &src[(2 * y + 1) * w * 4..];
            let out = &mut dst[y * nw * 4..(y + 1) * nw * 4];
            for x in 0..nw {
                for c in 0..4 {
                    let i = 8 * x + c;
                    let sum = r0[i] as u32 + r0[i + 4] as u32 + r1[i] as u32 + r1[i + 4] as u32;
                    out[4 * x + c] = ((sum + 2) / 4) as u8;
                }
            }
        }
        levels.push(dst);
        (w, h) = (nw, nh);
    }
    levels
}

fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    levels: &[Vec<u8>],
) -> wgpu::Texture {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("thumb"),
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
                texture: &tex,
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
    tex
}

impl Streamer {
    pub fn new(
        paths: Arc<Vec<PathBuf>>,
        threads: usize,
        decoder: Decoder,
        upload: Upload,
        budget: usize,
        ctx: egui::Context,
        gpu: Option<(wgpu::Device, wgpu::Queue)>,
    ) -> Self {
        let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let (tx, results): (Sender<Decoded>, _) = crossbeam_channel::unbounded();
        let target = Arc::new(AtomicU32::new(800));
        let workers = (0..threads)
            .map(|_| {
                let queue = queue.clone();
                let tx = tx.clone();
                let paths = paths.clone();
                let ctx = ctx.clone();
                let gpu = gpu.clone();
                let target = target.clone();
                std::thread::spawn(move || {
                    lower_thread_priority();
                    loop {
                        let file = {
                            let (lock, cvar) = &*queue;
                            let mut q = lock.lock().unwrap();
                            loop {
                                if q.stop {
                                    return;
                                }
                                let mut found = None;
                                while q.next < q.wanted.len() {
                                    let f = q.wanted[q.next];
                                    q.next += 1;
                                    if q.in_flight.insert(f) {
                                        found = Some(f);
                                        break;
                                    }
                                }
                                match found {
                                    Some(f) => break f,
                                    None => q = cvar.wait(q).unwrap(),
                                }
                            }
                        };
                        let start = Instant::now();
                        let bytes = std::fs::read(&paths[file as usize]).unwrap_or_default();
                        let read_ms = start.elapsed().as_secs_f32() * 1000.0;
                        let want = target.load(Ordering::Relaxed);
                        if let Some((width, height, rgba)) = decode(&bytes, decoder, want) {
                            let (payload, bytes) = match (upload, &gpu) {
                                (Upload::Native, Some((device, gq))) => {
                                    let levels = mip_chain(width, height, rgba);
                                    let bytes = levels.iter().map(Vec::len).sum();
                                    (
                                        Payload::Gpu(upload_texture(
                                            device, gq, width, height, &levels,
                                        )),
                                        bytes,
                                    )
                                }
                                _ => {
                                    let bytes = rgba.len();
                                    (Payload::Pixels(rgba), bytes)
                                }
                            };
                            let work_ms = start.elapsed().as_secs_f32() * 1000.0;
                            let _ = tx.send(Decoded {
                                file,
                                target: want,
                                width,
                                height,
                                bytes,
                                payload,
                                work_ms,
                                read_ms,
                            });
                            ctx.request_repaint();
                        }
                        queue.0.lock().unwrap().in_flight.remove(&file);
                    }
                })
            })
            .collect();
        Self {
            queue,
            results,
            cache: HashMap::new(),
            bytes: 0,
            budget,
            upload_budget_ms: 3.0,
            stats: Stats {
                work_ms: Vec::new(),
                read_ms: Vec::new(),
                upload_ms: Vec::new(),
            },
            frame: 0,
            target,
            workers,
        }
    }

    /// Replaces the request queue; `wanted` is in priority order and may contain duplicates and cached files.
    pub fn request(&mut self, wanted: impl IntoIterator<Item = u32>) {
        let mut seen = HashSet::new();
        let (frame, target) = (self.frame, self.target.load(Ordering::Relaxed));
        let cache = &mut self.cache;
        let list: Vec<u32> = wanted
            .into_iter()
            .filter(|f| {
                if !seen.insert(*f) {
                    return false;
                }
                match cache.get_mut(f) {
                    // Prefetched textures count as used, so the LRU doesn't evict what was just requested.
                    Some(e) => {
                        e.last_used = frame;
                        e.target < target
                    }
                    None => true,
                }
            })
            .collect();
        let (lock, cvar) = &*self.queue;
        let mut q = lock.lock().unwrap();
        q.wanted = list;
        q.next = 0;
        cvar.notify_all();
    }

    /// Texture of `file` if loaded; marks it as used this frame.
    pub fn get(&mut self, file: u32) -> Option<TextureId> {
        let frame = self.frame;
        self.cache.get_mut(&file).map(|e| {
            e.last_used = frame;
            e.id
        })
    }

    pub fn texture_bytes(&self) -> usize {
        self.bytes
    }

    pub fn cached(&self) -> usize {
        self.cache.len()
    }

    /// View of the native texture of `file`, for custom passes (glass).
    pub fn view(&self, file: u32) -> Option<wgpu::TextureView> {
        match &self.cache.get(&file)?.tex {
            Tex::Native(_, t) => Some(t.create_view(&wgpu::TextureViewDescriptor::default())),
            Tex::Egui(_) => None,
        }
    }

    /// Registers finished thumbnails until the frame's upload budget is spent, then evicts down to the byte budget.
    pub fn begin_frame(
        &mut self,
        ctx: &egui::Context,
        render_state: Option<&egui_wgpu::RenderState>,
    ) {
        self.frame += 1;
        let frame_start = Instant::now();
        while frame_start.elapsed().as_secs_f32() * 1000.0 < self.upload_budget_ms {
            let Ok(d) = self.results.try_recv() else {
                break;
            };
            self.stats.work_ms.push(d.work_ms);
            self.stats.read_ms.push(d.read_ms);
            if self
                .cache
                .get(&d.file)
                .is_some_and(|e| e.target >= d.target)
            {
                if let Payload::Gpu(tex) = d.payload {
                    tex.destroy();
                }
                continue;
            }
            // Replaces a smaller version after zooming in.
            if let Some(old) = self.cache.remove(&d.file) {
                self.free(old, render_state);
            }
            let start = Instant::now();
            let (tex, id) = match d.payload {
                Payload::Pixels(rgba) => {
                    let img = ColorImage::from_rgba_unmultiplied(
                        [d.width as usize, d.height as usize],
                        &rgba,
                    );
                    let handle =
                        ctx.load_texture(format!("t{}", d.file), img, TextureOptions::LINEAR);
                    let id = handle.id();
                    (Tex::Egui(handle), id)
                }
                Payload::Gpu(tex) => {
                    let rs = render_state.expect("native upload needs wgpu");
                    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
                    let id = rs
                        .renderer
                        .write()
                        .register_native_texture_with_sampler_options(
                            &rs.device,
                            &view,
                            wgpu::SamplerDescriptor {
                                label: Some("thumb"),
                                mag_filter: wgpu::FilterMode::Linear,
                                min_filter: wgpu::FilterMode::Linear,
                                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                                ..Default::default()
                            },
                        );
                    (Tex::Native(id, tex), id)
                }
            };
            self.stats
                .upload_ms
                .push(start.elapsed().as_secs_f32() * 1000.0);
            self.bytes += d.bytes;
            self.cache.insert(
                d.file,
                Entry {
                    tex,
                    target: d.target,
                    id,
                    bytes: d.bytes,
                    last_used: self.frame,
                },
            );
        }
        if self.bytes > self.budget {
            self.evict(render_state);
        }
    }

    fn evict(&mut self, render_state: Option<&egui_wgpu::RenderState>) {
        let target = self.budget * 9 / 10;
        let mut old: Vec<(u64, u32)> = self
            .cache
            .iter()
            .filter(|(_, e)| e.last_used + 1 < self.frame)
            .map(|(f, e)| (e.last_used, *f))
            .collect();
        old.sort_unstable();
        for (_, file) in old {
            if self.bytes <= target {
                break;
            }
            let e = self.cache.remove(&file).unwrap();
            self.free(e, render_state);
        }
    }

    fn free(&mut self, e: Entry, render_state: Option<&egui_wgpu::RenderState>) {
        self.bytes -= e.bytes;
        if let Tex::Native(id, tex) = e.tex {
            if let Some(rs) = render_state {
                rs.renderer.write().free_texture(&id);
            }
            tex.destroy();
        }
    }
}

impl Drop for Streamer {
    fn drop(&mut self) {
        let (lock, cvar) = &*self.queue;
        lock.lock().unwrap().stop = true;
        cvar.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}
