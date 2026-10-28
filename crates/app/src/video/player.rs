//! [`MpvPlayer`]: libmpv decodes (hardware decoding with copy-back), plays the audio and keeps A/V in sync. A render
//! thread renders each frame with mpv's software renderer at the size the video takes on screen and, like thumbnails
//! and originals, creates and fills the `wgpu::Texture` itself; the UI thread only draws it. Frames are timed to the
//! display (`video-sync=display-resample`): the UI wakes the render thread once per presented frame. Without a wgpu
//! render state (tests) frames go through egui's textures and are timed to mpv's clock.

use super::mpv::{Event, FrameBuffer, Mpv, SwRender};
use super::{Playback, PlaybackState, VideoFrame};
use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// mpv property reads take the core's lock and may stall for milliseconds: the UI reads them at most this often.
const STATE_INTERVAL: Duration = Duration::from_millis(100);
/// wgpu wants rows aligned to 256 bytes, mpv to 64.
const ROW_ALIGN: usize = 256;
/// Refresh rate assumed when Windows doesn't report one.
const DEFAULT_DISPLAY_FPS: f64 = 60.0;

/// Which `loadfile` frames belong to. Loads are counted on both sides: the UI counts what it asked for, the event
/// thread what mpv started, so a late "file loaded" of a video stepped away from doesn't let its frames through.
#[derive(Debug, Default)]
struct Loads {
    requested: u64,
    started: u64,
    /// Frames of the latest load may be shown.
    accept: bool,
    error: Option<String>,
}

/// A frame from the render thread for the UI thread.
enum Published {
    /// A texture of a new size (or for a new video); the render thread keeps writing frames into it.
    Texture(wgpu::Texture, [u32; 2]),
    /// A whole frame, without wgpu.
    Image(egui::ColorImage),
}

struct Shared {
    wake: Mutex<bool>,
    wake_cv: Condvar,
    stop: AtomicBool,
    /// Render size wanted by the UI, packed `w << 16 | h`; 0 = the video's size.
    target: AtomicU32,
    /// The video's decoded size, packed like `target`: the render thread may not ask mpv (render.h).
    video_size: AtomicU32,
    display_sync: bool,
    /// Set by the UI once per presented frame (display sync).
    tick: AtomicBool,
    loads: Mutex<Loads>,
    published: Mutex<Option<(u64, Published)>>,
    ctx: egui::Context,
}

impl Shared {
    fn wake(&self) {
        *lock(&self.wake) = true;
        self.wake_cv.notify_one();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn pack([width, height]: [u32; 2]) -> u32 {
    width.min(0xffff) << 16 | height.min(0xffff)
}

fn unpack(packed: u32) -> [u32; 2] {
    [packed >> 16, packed & 0xffff]
}

/// The texture the UI draws and what keeps it alive.
enum Shown {
    Gpu(#[expect(dead_code, reason = "held: the render thread writes into it")] wgpu::Texture),
    Egui(egui::TextureHandle),
}

pub struct MpvPlayer {
    mpv: Arc<Mpv>,
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
    render_state: Option<egui_wgpu::RenderState>,
    /// The load the UI waits for frames of.
    load: u64,
    loaded: bool,
    shown: Option<(VideoFrame, Shown)>,
    state: PlaybackState,
    state_at: Option<Instant>,
    /// The state was read since the load was accepted (its size is known from then on, if it has video).
    read_since_accept: bool,
}

impl MpvPlayer {
    /// Loads libmpv from `dll` and starts its threads. With `render_state` frames become wgpu textures timed to the
    /// display; without, egui textures. Without `audio`, nothing is played on the speakers (tests).
    pub fn new(
        dll: &Path,
        ctx: egui::Context,
        render_state: Option<egui_wgpu::RenderState>,
        audio: bool,
    ) -> Result<Self, String> {
        let display_sync = render_state.is_some();
        let fps = media::helpers::display_refresh_rate()
            .unwrap_or(DEFAULT_DISPLAY_FPS)
            .to_string();
        let mut options = vec![
            ("vo", "libmpv"),
            ("hwdec", "auto-copy"),
            ("keep-open", "yes"),
            ("idle", "yes"),
            ("terminal", "no"),
            ("config", "no"),
            ("input-default-bindings", "no"),
        ];
        if !audio {
            options.push(("ao", "null"));
        }
        if display_sync {
            options.push(("video-sync", "display-resample"));
            options.push(("override-display-fps", &fps));
        }
        let mpv = Arc::new(Mpv::new(dll, &options)?);
        let shared = Arc::new(Shared {
            wake: Mutex::new(false),
            wake_cv: Condvar::new(),
            stop: AtomicBool::new(false),
            target: AtomicU32::new(0),
            video_size: AtomicU32::new(0),
            display_sync,
            tick: AtomicBool::new(false),
            loads: Mutex::new(Loads::default()),
            published: Mutex::new(None),
            ctx,
        });
        let gpu = render_state
            .as_ref()
            .map(|render_state| (render_state.device.clone(), render_state.queue.clone()));
        let render = {
            let (mpv, shared) = (mpv.clone(), shared.clone());
            std::thread::Builder::new()
                .name("video render".into())
                .spawn(move || render_thread(&mpv, &shared, gpu))
                .map_err(|e| e.to_string())?
        };
        let events = {
            let (mpv, shared) = (mpv.clone(), shared.clone());
            std::thread::Builder::new()
                .name("video events".into())
                .spawn(move || event_thread(&mpv, &shared))
                .map_err(|e| e.to_string())?
        };
        Ok(Self {
            mpv,
            shared,
            threads: vec![render, events],
            render_state,
            load: 0,
            loaded: false,
            shown: None,
            state: PlaybackState::default(),
            state_at: None,
            read_since_accept: false,
        })
    }

    /// Drops the shown frame (a new video, or none).
    fn forget_frame(&mut self) {
        if let Some((frame, _)) = self.shown.take()
            && let Some(render_state) = &self.render_state
        {
            render_state.renderer.write().free_texture(&frame.id);
        }
    }

    fn refresh_state(&mut self) {
        let mpv = &self.mpv;
        let size = match (mpv.int("dwidth"), mpv.int("dheight")) {
            (Some(width), Some(height)) if width > 0 && height > 0 => Some([width as u32, height as u32]),
            _ => None,
        };
        self.shared.video_size.store(size.map_or(0, pack), Ordering::Relaxed);
        let error = lock(&self.shared.loads).error.clone();
        self.state = PlaybackState {
            paused: mpv.flag("pause").unwrap_or(false),
            position: mpv.double("time-pos").unwrap_or(0.0),
            duration: mpv.double("duration").unwrap_or(0.0),
            video_size: size,
            quarters: domain::video::quarter_turns(mpv.int("video-params/rotate").unwrap_or(0)),
            ended: mpv.flag("eof-reached").unwrap_or(false),
            volume: self.state.volume,
            muted: self.state.muted,
            error,
        };
        self.state_at = Some(Instant::now());
    }

    /// Takes a frame the render thread published for the current load.
    fn take_published(&mut self) {
        let Some((load, published)) = lock(&self.shared.published).take() else {
            return;
        };
        if load != self.load || !self.loaded {
            return;
        }
        match published {
            Published::Texture(texture, size) => {
                self.forget_frame();
                let Some(render_state) = &self.render_state else { return };
                let id = crate::textures::register(render_state, &texture);
                self.shown = Some((VideoFrame { id, size }, Shown::Gpu(texture)));
            }
            Published::Image(image) => {
                let size = [image.size[0] as u32, image.size[1] as u32];
                if let Some((frame, Shown::Egui(handle))) = &mut self.shown {
                    handle.set(image, egui::TextureOptions::LINEAR);
                    frame.size = size;
                } else {
                    let handle = self
                        .shared
                        .ctx
                        .load_texture("video", image, egui::TextureOptions::LINEAR);
                    self.shown = Some((VideoFrame { id: handle.id(), size }, Shown::Egui(handle)));
                }
            }
        }
    }
}

impl Playback for MpvPlayer {
    fn load(&mut self, file: &Path) {
        self.forget_frame();
        {
            let mut loads = lock(&self.shared.loads);
            loads.requested += 1;
            loads.accept = false;
            loads.error = None;
            self.load = loads.requested;
        }
        self.loaded = true;
        self.shared.video_size.store(0, Ordering::Relaxed);
        // `file` is absolute and given as one argument, so it can't be taken as an option (REQ-011-09).
        let path = file.to_string_lossy();
        if let Err(error) = self.mpv.command(&["loadfile", &path, "replace"]) {
            let mut loads = lock(&self.shared.loads);
            // No START_FILE follows a refused command.
            loads.requested -= 1;
            self.load = loads.requested;
            loads.error = Some(error);
        }
        self.mpv.set_flag("pause", false);
        let (volume, muted) = (self.state.volume, self.state.muted);
        self.state = PlaybackState {
            volume,
            muted,
            ..PlaybackState::default()
        };
        self.state_at = None;
        self.read_since_accept = false;
    }

    fn stop(&mut self) {
        if !self.loaded {
            return;
        }
        self.loaded = false;
        lock(&self.shared.loads).accept = false;
        self.forget_frame();
        let _ = self.mpv.command(&["stop"]);
        let (volume, muted) = (self.state.volume, self.state.muted);
        self.state = PlaybackState {
            volume,
            muted,
            ..PlaybackState::default()
        };
    }

    fn set_paused(&mut self, paused: bool) {
        self.mpv.set_flag("pause", paused);
        self.state.paused = paused;
    }

    fn seek(&mut self, position: f64, exact: bool) {
        let mode = if exact { "absolute+exact" } else { "absolute+keyframes" };
        if let Err(error) = self.mpv.command(&["seek", &format!("{position:.3}"), mode]) {
            tracing::debug!("mpv: can't seek: {error}");
        }
        self.state.position = position;
        self.state.ended = false;
    }

    fn set_volume(&mut self, volume: f64) {
        let volume = domain::video::clamp_volume(volume);
        self.mpv.set_double("volume", volume);
        self.state.volume = volume;
    }

    fn set_muted(&mut self, muted: bool) {
        self.mpv.set_flag("mute", muted);
        self.state.muted = muted;
    }

    fn state(&mut self) -> PlaybackState {
        let (accepted, error) = {
            let loads = lock(&self.shared.loads);
            (loads.accept, loads.error.clone())
        };
        // Until the file has loaded, mpv's properties may still be the previous video's.
        if !accepted {
            self.state.error = error;
            return self.state.clone();
        }
        // Right away once the file has loaded, so its size is known for the first frame; then every 100 ms.
        let stale = !self.read_since_accept || self.state_at.is_none_or(|at| at.elapsed() >= STATE_INTERVAL);
        if self.loaded && stale {
            self.refresh_state();
            self.read_since_accept = true;
        }
        self.state.clone()
    }

    fn frame(&mut self, render_size: Option<[u32; 2]>) -> Option<VideoFrame> {
        if !self.loaded {
            return None;
        }
        self.shared.target.store(render_size.map_or(0, pack), Ordering::Relaxed);
        self.take_published();
        if self.shared.display_sync {
            self.shared.tick.store(true, Ordering::Relaxed);
            self.shared.wake();
            // Every presented frame counts while it plays.
            if !self.state.paused && !self.state.ended {
                self.shared.ctx.request_repaint();
            }
        }
        self.shown.as_ref().map(|(frame, _)| *frame)
    }
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        self.forget_frame();
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.wake_cv.notify_all();
        self.mpv.wakeup();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// The frame buffer and the texture the frames go into.
#[derive(Default)]
struct Frames {
    buffer: FrameBuffer,
    /// The texture, its size and the load it is for.
    texture: Option<(wgpu::Texture, [u32; 2], u64)>,
}

fn render_thread(mpv: &Arc<Mpv>, shared: &Arc<Shared>, gpu: Option<(wgpu::Device, wgpu::Queue)>) {
    let waker = shared.clone();
    let render = match SwRender::new(mpv, Box::new(move || waker.wake())) {
        Ok(render) => render,
        Err(error) => {
            tracing::error!("mpv: no render context: {error}");
            return;
        }
    };
    let mut frames = Frames::default();
    while !shared.stop.load(Ordering::Relaxed) {
        {
            let mut wake = lock(&shared.wake);
            while !*wake && !shared.stop.load(Ordering::Relaxed) {
                wake = shared
                    .wake_cv
                    .wait_timeout(wake, Duration::from_millis(100))
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .0;
            }
            *wake = false;
        }
        if shared.display_sync {
            // Renders once per presented frame; a frame due while the UI sleeps (paused, seeking) asks for one.
            if !shared.tick.swap(false, Ordering::Relaxed) {
                shared.ctx.request_repaint();
                continue;
            }
            render.report_swap();
        }
        if !render.frame_due() {
            continue;
        }
        render_frame(&render, mpv, shared, gpu.as_ref(), &mut frames);
    }
    drop(render);
}

fn render_frame(
    render: &SwRender,
    mpv: &Mpv,
    shared: &Shared,
    gpu: Option<&(wgpu::Device, wgpu::Queue)>,
    frames: &mut Frames,
) {
    let (accept, load) = {
        let loads = lock(&shared.loads);
        (loads.accept, loads.requested)
    };
    let video = unpack(shared.video_size.load(Ordering::Relaxed));
    let target = shared.target.load(Ordering::Relaxed);
    let size = if target == 0 { video } else { unpack(target) };
    // Not shown (a video stepped away from, or its size not known yet): advance without drawing.
    let shown = accept && size[0] > 0 && size[1] > 0;
    let size = if shown { size } else { [2, 2] };
    let [width, height] = size;
    let stride = (width as usize * 4).div_ceil(ROW_ALIGN) * ROW_ALIGN;
    let pixels = frames.buffer.bytes_mut(stride * height as usize);
    let block = !shared.display_sync;
    let new_texture = gpu.is_some()
        && frames
            .texture
            .as_ref()
            .is_none_or(|(_, at, of)| *at != size || *of != load);
    // Display-synced timing asks for a render every presented frame; repeats of the shown frame need no work.
    if !shown || (shared.display_sync && !new_texture && render.next_is_repeat()) {
        let _ = render.render(size, stride, pixels, block, true);
        return;
    }
    if let Err(rc) = render.render(size, stride, pixels, block, false) {
        tracing::debug!("mpv: can't render: {}", mpv.error(rc));
        return;
    }
    // `rgb0` leaves the fourth byte undefined; egui blends with alpha.
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    match gpu {
        Some((device, queue)) => {
            if new_texture {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("video"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                frames.texture = Some((texture, size, load));
            }
            let Some((texture, _, _)) = &frames.texture else { return };
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride as u32),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            if new_texture {
                *lock(&shared.published) = Some((load, Published::Texture(texture.clone(), size)));
            }
        }
        None => {
            let row = width as usize * 4;
            let mut rgba = Vec::with_capacity(row * height as usize);
            for line in pixels.chunks_exact(stride) {
                rgba.extend_from_slice(&line[..row]);
            }
            let image = egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
            *lock(&shared.published) = Some((load, Published::Image(image)));
        }
    }
    shared.ctx.request_repaint();
}

fn event_thread(mpv: &Mpv, shared: &Shared) {
    while !shared.stop.load(Ordering::Relaxed) {
        match mpv.wait_event(0.5) {
            Event::None | Event::Other => continue,
            Event::Shutdown => break,
            Event::StartFile => lock(&shared.loads).started += 1,
            Event::FileLoaded => {
                let mut loads = lock(&shared.loads);
                loads.accept = loads.started == loads.requested;
            }
            Event::EndFile { error } => {
                let mut loads = lock(&shared.loads);
                if let Some(error) = error
                    && loads.started == loads.requested
                {
                    tracing::warn!("mpv: can't play a video: {error}");
                    loads.error = Some(error);
                }
            }
        }
        shared.ctx.request_repaint();
    }
}
