//! M0b spike: libmpv in an eframe / wgpu window.
//!
//! mpv decodes (hardware decoding with copy-back by default) and renders each frame with its software
//! renderer into a CPU buffer on a render thread, which writes it into a `wgpu::Texture` (`Queue` is
//! `Send + Sync`). The UI thread only draws that texture and the controls. Audio, A/V sync and seeking are
//! mpv's.
//!
//! `cargo run --release -- --file <video>`; `--bench <name>` plays for `--seconds`, runs a seek test and
//! writes `results/<name>.json`.

mod mpv;

use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use eframe::wgpu;
use serde_json::json;
use std::collections::HashMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub struct Args(HashMap<String, String>);

impl Args {
    fn parse() -> Self {
        let mut map = HashMap::new();
        let mut it = std::env::args().skip(1);
        while let Some(key) = it.next() {
            if let Some(key) = key.strip_prefix("--") {
                map.insert(key.to_string(), it.next().unwrap_or_default());
            }
        }
        Self(map)
    }
    fn get<T: std::str::FromStr>(&self, key: &str, default: T) -> T {
        self.0
            .get(key)
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }
    fn str(&self, key: &str, default: &str) -> String {
        self.0
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }
}

/// wgpu needs `bytes_per_row` aligned to 256; mpv wants stride and pointer aligned to 64.
const ROW_ALIGN: usize = 256;

#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct Block([u8; 64]);

/// State shared between the UI thread, the render thread, the event thread and mpv's update callback.
struct Shared {
    wake: Mutex<bool>,
    wake_cv: Condvar,
    stop: AtomicBool,
    /// Render size wanted by the UI (physical pixels), packed `w << 16 | h`; 0 = the video's own size.
    target: AtomicU32,
    /// Display size of the video (after rotation), packed like `target`, set by the UI: the render thread
    /// must not call non-render libmpv functions.
    video_size: AtomicU32,
    /// Display-synced mode: the UI wakes the render thread once per presented frame.
    display_sync: bool,
    tick: AtomicBool,
    /// A texture the render thread created (new size), for the UI to register.
    new_texture: Mutex<Option<(wgpu::Texture, [u32; 2])>>,
    frames: AtomicU64,
    /// Per frame: render ms, alpha fix ms, write_texture ms.
    timings: Mutex<Vec<[f32; 3]>>,
    last_frame_at: Mutex<Option<Instant>>,
    restart_at: Mutex<Option<Instant>>,
    loaded_at: Mutex<Option<Instant>>,
    end_error: Mutex<Option<String>>,
    block: bool,
    /// Path to write the next rendered frame to (PPM), for visual checks.
    dump: Mutex<Option<String>>,
}

unsafe extern "C" fn on_update(data: *mut c_void) {
    let shared = unsafe { &*(data as *const Shared) };
    *shared.wake.lock().unwrap() = true;
    shared.wake_cv.notify_one();
}

fn render_thread(
    mpv: Arc<mpv::Mpv>,
    shared: Arc<Shared>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    ctx: egui::Context,
) {
    let sw = match mpv::SwRender::new(&mpv, on_update, Arc::as_ptr(&shared) as *mut c_void) {
        Ok(sw) => sw,
        Err(e) => {
            eprintln!("render context: {e}");
            return;
        }
    };
    let mut buf: Vec<Block> = Vec::new();
    let mut tex: Option<(wgpu::Texture, [u32; 2])> = None;
    while !shared.stop.load(Ordering::Relaxed) {
        {
            let mut wake = shared.wake.lock().unwrap();
            while !*wake && !shared.stop.load(Ordering::Relaxed) {
                wake = shared
                    .wake_cv
                    .wait_timeout(wake, Duration::from_millis(100))
                    .unwrap()
                    .0;
            }
            *wake = false;
        }
        if shared.display_sync {
            if !shared.tick.swap(false, Ordering::Relaxed) {
                continue;
            }
            sw.report_swap();
        }
        if sw.update() & mpv::RENDER_UPDATE_FRAME == 0 {
            continue;
        }
        let vs = shared.video_size.load(Ordering::Relaxed);
        let (dw, dh) = (vs >> 16, vs & 0xffff);
        if dw == 0 || dh == 0 {
            continue;
        }
        let t = shared.target.load(Ordering::Relaxed);
        let (w, h) = if t == 0 {
            (dw, dh)
        } else {
            // Never upscale on the CPU; the GPU scales the texture up for free.
            let (tw, th) = (t >> 16, t & 0xffff);
            if tw >= dw || th >= dh {
                (dw, dh)
            } else {
                (tw.max(2), th.max(2))
            }
        };
        let stride = (w as usize * 4).div_ceil(ROW_ALIGN) * ROW_ALIGN;
        let bytes = stride * h as usize;
        if buf.len() * 64 < bytes {
            buf = vec![Block([0; 64]); bytes.div_ceil(64)];
        }
        let created = tex.as_ref().map(|t| t.1) != Some([w, h]);
        if created {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("video"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            tex = Some((t.clone(), [w, h]));
            *shared.new_texture.lock().unwrap() = Some((t, [w, h]));
        }
        let ptr = buf.as_mut_ptr() as *mut u8;
        // Display-synced timing asks for a render every vsync; repeats of the shown frame need no work.
        if sw.next_frame_flags() & mpv::FRAME_INFO_REPEAT != 0 && !created {
            sw.render_with(w, h, stride, ptr, shared.block, true);
            continue;
        }
        let t0 = Instant::now();
        let rc = sw.render_with(w, h, stride, ptr, shared.block, false);
        if rc < 0 {
            eprintln!("render: {}", mpv.error(rc));
            continue;
        }
        let t1 = Instant::now();
        // "rgb0" leaves the fourth byte undefined; egui blends with alpha, so force it opaque.
        let px = unsafe { std::slice::from_raw_parts_mut(ptr as *mut u32, bytes / 4) };
        for p in px.iter_mut() {
            *p |= 0xff00_0000;
        }
        let t2 = Instant::now();
        let data = unsafe { std::slice::from_raw_parts(ptr, bytes) };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex.as_ref().unwrap().0,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride as u32),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let t3 = Instant::now();
        if let Some(path) = shared.dump.lock().unwrap().take() {
            let mut out = format!(
                "P6
{w} {h}
255
"
            )
            .into_bytes();
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let i = y * stride + x * 4;
                    out.extend_from_slice(&data[i..i + 3]);
                }
            }
            let _ = std::fs::write(path, out);
        }
        let ms = |a: Instant, b: Instant| (b - a).as_secs_f32() * 1000.0;
        shared
            .timings
            .lock()
            .unwrap()
            .push([ms(t0, t1), ms(t1, t2), ms(t2, t3)]);
        shared.frames.fetch_add(1, Ordering::Relaxed);
        *shared.last_frame_at.lock().unwrap() = Some(t3);
        ctx.request_repaint();
    }
    drop(sw);
}

fn event_thread(mpv: Arc<mpv::Mpv>, shared: Arc<Shared>) {
    while !shared.stop.load(Ordering::Relaxed) {
        let (id, _err, data) = mpv.wait_event(0.5);
        match id {
            mpv::EVENT_NONE => {}
            mpv::EVENT_SHUTDOWN => break,
            mpv::EVENT_PLAYBACK_RESTART => {
                *shared.restart_at.lock().unwrap() = Some(Instant::now())
            }
            mpv::EVENT_FILE_LOADED => *shared.loaded_at.lock().unwrap() = Some(Instant::now()),
            mpv::EVENT_END_FILE => {
                let e = unsafe { &*(data as *const mpv::EventEndFile) };
                // reason 4 = error
                if e.reason == 4 {
                    *shared.end_error.lock().unwrap() = Some(mpv.error(e.error));
                }
            }
            _ => {}
        }
    }
}

fn process_cpu_seconds() -> f64 {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let z = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut c, mut e, mut k, mut u) = (z, z, z, z);
    unsafe { GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) };
    let f = |t: FILETIME| ((t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64) as f64 / 1e7;
    f(k) + f(u)
}

fn pct(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f32 * p).round() as usize]
}

fn summary(mut v: Vec<f32>) -> serde_json::Value {
    v.sort_by(f32::total_cmp);
    let mean = v.iter().sum::<f32>() / v.len().max(1) as f32;
    json!({ "n": v.len(), "mean": mean, "p50": pct(&v, 0.5), "p95": pct(&v, 0.95), "p99": pct(&v, 0.99), "max": v.last().copied().unwrap_or(0.0) })
}

enum BenchPhase {
    Loading,
    Playing {
        from: Instant,
        cpu: f64,
        frames: u64,
        timings: usize,
    },
    Seeking {
        idx: usize,
        sent: Instant,
    },
    Done,
}

struct Bench {
    name: String,
    seconds: f32,
    phase: BenchPhase,
    intervals: Vec<f32>,
    play: Option<serde_json::Value>,
    seeks: Vec<serde_json::Value>,
    drops0: (i64, i64),
    /// No video: measures the UI frame intervals of an idle, continuously repainting window.
    idle: bool,
    /// Intervals between UI frames that showed a new video frame (what the viewer sees).
    shown: Vec<f32>,
    last_shown: Option<(u64, Instant)>,
}

/// Positions for the seek test (seconds), spread over a 30 s clip and not on keyframes.
const SEEKS: [f64; 8] = [17.3, 3.1, 25.9, 9.7, 28.2, 1.5, 21.4, 12.8];

#[derive(Default)]
struct Props {
    dw: i64,
    dh: i64,
    duration: f64,
    pos: f64,
    paused: bool,
    /// Clockwise display rotation in degrees (from the display matrix).
    rotate: i64,
    overlay: String,
}

struct VideoApp {
    props: Props,
    props_at: Option<Instant>,
    props_live: bool,
    ui_cpu: Vec<f32>,
    mpv: Arc<mpv::Mpv>,
    shared: Arc<Shared>,
    tex: Option<(egui::TextureId, [u32; 2])>,
    files: Vec<PathBuf>,
    current: usize,
    seek_drag: Option<f64>,
    volume: f64,
    render_size: String,
    bench: Option<Bench>,
    last_ui: Option<Instant>,
    cpu_sample: (Instant, f64, f32),
    dump: Option<(String, f32)>,
    /// `--screenshot <file.ppm> --screenshot-at <s>`: saves the window once playback passes that time, then closes.
    screenshot: Option<(String, f32)>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl VideoApp {
    fn new(cc: &eframe::CreationContext, args: &Args) -> Result<Self, String> {
        let dll = PathBuf::from(args.str("mpv", "vendor/mpv/libmpv-2.dll"));
        let api = mpv::Api::load(&dll)?;
        let mut options = vec![
            ("vo".to_string(), "libmpv".to_string()),
            ("hwdec".to_string(), args.str("hwdec", "auto-copy")),
            ("keep-open".to_string(), "yes".to_string()),
            ("terminal".to_string(), "no".to_string()),
            ("idle".to_string(), "yes".to_string()),
            ("volume".to_string(), args.str("volume", "60")),
        ];
        if args.str("sync", "audio") == "display" {
            options.push(("video-sync".into(), "display-resample".into()));
            options.push(("override-display-fps".into(), args.str("display-fps", "60")));
        }
        if let Some(extra) = args.0.get("mpv-opts") {
            for kv in extra.split(',') {
                if let Some((k, v)) = kv.split_once('=') {
                    options.push((k.to_string(), v.to_string()));
                }
            }
        }
        let mpv = Arc::new(mpv::Mpv::new(api, &options)?);
        let shared = Arc::new(Shared {
            wake: Mutex::new(false),
            wake_cv: Condvar::new(),
            stop: AtomicBool::new(false),
            target: AtomicU32::new(0),
            video_size: AtomicU32::new(0),
            display_sync: args.str("sync", "audio") == "display",
            tick: AtomicBool::new(false),
            new_texture: Mutex::new(None),
            frames: AtomicU64::new(0),
            timings: Mutex::new(Vec::new()),
            last_frame_at: Mutex::new(None),
            restart_at: Mutex::new(None),
            loaded_at: Mutex::new(None),
            end_error: Mutex::new(None),
            block: args.str("block", "on") == "on",
            dump: Mutex::new(None),
        });
        let rs = cc
            .wgpu_render_state
            .as_ref()
            .ok_or("needs the wgpu backend")?;
        let mut threads = Vec::new();
        {
            let (mpv, shared, device, queue, ctx) = (
                mpv.clone(),
                shared.clone(),
                rs.device.clone(),
                rs.queue.clone(),
                cc.egui_ctx.clone(),
            );
            threads.push(std::thread::spawn(move || {
                render_thread(mpv, shared, device, queue, ctx)
            }));
        }
        {
            let (mpv, shared) = (mpv.clone(), shared.clone());
            threads.push(std::thread::spawn(move || event_thread(mpv, shared)));
        }

        let dir = PathBuf::from(args.str(
            "dir",
            &format!(
                "{}/gg-spike/video",
                std::env::var("TEMP").unwrap_or_default()
            ),
        ));
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| {
                        matches!(
                            p.extension()
                                .and_then(|e| e.to_str())
                                .map(|e| e.to_ascii_lowercase())
                                .as_deref(),
                            Some("mp4" | "mov" | "mkv" | "webm" | "m4v" | "avi")
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        let file = args.0.get("file").map(PathBuf::from);
        let current = match &file {
            Some(f) => files.iter().position(|p| p == f).unwrap_or_else(|| {
                files.insert(0, f.clone());
                0
            }),
            None => 0,
        };
        let bench = args.0.get("bench").map(|name| Bench {
            name: name.clone(),
            seconds: args.get("seconds", 20.0),
            phase: BenchPhase::Loading,
            intervals: Vec::new(),
            play: None,
            seeks: Vec::new(),
            drops0: (0, 0),
            idle: args.str("idle", "off") == "on",
            shown: Vec::new(),
            last_shown: None,
        });
        let app = Self {
            mpv,
            shared,
            tex: None,
            files,
            current,
            seek_drag: None,
            volume: args.get("volume", 60.0),
            render_size: args.str("render-size", "display"),
            bench,
            last_ui: None,
            cpu_sample: (Instant::now(), process_cpu_seconds(), 0.0),
            props: Props::default(),
            props_at: None,
            props_live: args.str("props", "cached") == "live",
            ui_cpu: Vec::new(),
            screenshot: args
                .0
                .get("screenshot")
                .map(|p| (p.clone(), args.get("screenshot-at", 2.0))),
            dump: args
                .0
                .get("dump")
                .map(|p| (p.clone(), args.get("dump-at", 5.0))),
            threads,
        };
        if let Some(f) = app.files.get(app.current)
            && !app.bench.as_ref().is_some_and(|b| b.idle)
        {
            app.load(f);
        }
        Ok(app)
    }

    fn refresh_props(&mut self) {
        let m = &self.mpv;
        self.props = Props {
            dw: m.int("dwidth").unwrap_or(0),
            dh: m.int("dheight").unwrap_or(0),
            duration: m.double("duration").unwrap_or(0.0),
            pos: m.double("time-pos").unwrap_or(0.0),
            paused: m.flag("pause").unwrap_or(false),
            rotate: m.int("video-params/rotate").unwrap_or(0),
            overlay: format!(
                "{}  hwdec {}  {}  drops {} / {}
{}",
                m.string("video-codec").unwrap_or_default(),
                m.string("hwdec-current").unwrap_or_else(|| "no".into()),
                m.string("video-params/pixelformat").unwrap_or_default(),
                m.int("frame-drop-count").unwrap_or(0),
                m.int("decoder-frame-drop-count").unwrap_or(0),
                self.shared
                    .end_error
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_default(),
            ),
        };
        self.props_at = Some(Instant::now());
    }

    fn load(&self, path: &Path) {
        *self.shared.end_error.lock().unwrap() = None;
        *self.shared.loaded_at.lock().unwrap() = None;
        if let Err(e) = self.mpv.command(&["loadfile", &path.to_string_lossy()]) {
            *self.shared.end_error.lock().unwrap() = Some(e);
        }
        self.mpv.set_flag("pause", false);
    }

    fn seek(&self, t: f64, exact: bool) {
        let mode = if exact {
            "absolute+exact"
        } else {
            "absolute+keyframes"
        };
        let _ = self.mpv.command(&["seek", &format!("{t:.3}"), mode]);
    }

    fn mpv_stats(m: &mpv::Mpv) -> serde_json::Value {
        json!({
            "hwdec_current": m.string("hwdec-current"),
            "video_codec": m.string("video-codec"),
            "audio_codec": m.string("audio-codec-name"),
            "video_params": { "w": m.int("video-params/w"), "h": m.int("video-params/h"), "rotate": m.int("video-params/rotate"), "pixelformat": m.string("video-params/pixelformat") },
            "dwidth": m.int("dwidth"), "dheight": m.int("dheight"),
            "container_fps": m.double("container-fps"),
            "estimated_vf_fps": m.double("estimated-vf-fps"),
            "frame_drop_count": m.int("frame-drop-count"),
            "decoder_frame_drop_count": m.int("decoder-frame-drop-count"),
            "vo_delayed_frame_count": m.int("vo-delayed-frame-count"),
            "mistimed_frame_count": m.int("mistimed-frame-count"),
            "avsync": m.double("avsync"),
            "chapters": m.int("chapters"),
            "ao": m.string("current-ao"),
        })
    }

    fn bench_step(&mut self, ctx: &egui::Context) {
        let Some(b) = self.bench.as_mut() else { return };
        ctx.request_repaint();
        match b.phase {
            BenchPhase::Loading => {
                // Measure from the first rendered frame, not from file open.
                if b.idle || self.shared.frames.load(Ordering::Relaxed) > 5 {
                    b.phase = BenchPhase::Playing {
                        from: Instant::now(),
                        cpu: process_cpu_seconds(),
                        frames: self.shared.frames.load(Ordering::Relaxed),
                        timings: self.shared.timings.lock().unwrap().len(),
                    };
                    b.intervals.clear();
                    self.ui_cpu.clear();
                    b.drops0 = (
                        self.mpv.int("frame-drop-count").unwrap_or(0),
                        self.mpv.int("decoder-frame-drop-count").unwrap_or(0),
                    );
                }
            }
            BenchPhase::Playing {
                from,
                cpu,
                frames,
                timings,
            } => {
                let wall = from.elapsed().as_secs_f64();
                let eof = self.mpv.flag("eof-reached").unwrap_or(false);
                if wall >= b.seconds as f64 || eof {
                    let cpu = process_cpu_seconds() - cpu;
                    let cores = std::thread::available_parallelism()
                        .map(|n| n.get())
                        .unwrap_or(1) as f64;
                    let t = self.shared.timings.lock().unwrap()[timings..].to_vec();
                    let rendered = self.shared.frames.load(Ordering::Relaxed) - frames;
                    let fps = self.mpv.double("container-fps").unwrap_or(30.0);
                    b.play = Some(json!({
                        "wall_s": wall,
                        "frames_rendered": rendered,
                        "frames_expected": (wall * fps).round(),
                        "cpu_pct_of_one_core": cpu / wall * 100.0,
                        "cpu_pct_of_machine": cpu / wall / cores * 100.0,
                        "render_ms": summary(t.iter().map(|x| x[0]).collect()),
                        "alpha_fix_ms": summary(t.iter().map(|x| x[1]).collect()),
                        "write_texture_ms": summary(t.iter().map(|x| x[2]).collect()),
                                                "ui_intervals_over_20ms": b.intervals.iter().filter(|&&i| i > 20.0).count(),
                        "video_judder": b.shown.iter().filter(|&&i| !(25.0..=42.0).contains(&i)).count(),
                        "video_display_interval_ms": summary(std::mem::take(&mut b.shown)),
                        "ui_cpu_ms": summary(std::mem::take(&mut self.ui_cpu)),
                        "vo_drops_in_run": self.mpv.int("frame-drop-count").unwrap_or(0) - b.drops0.0,
                        "decoder_drops_in_run": self.mpv.int("decoder-frame-drop-count").unwrap_or(0) - b.drops0.1,
                        "ui_interval_ms": summary(std::mem::take(&mut b.intervals)),
                        "render_size": self.tex.map(|t| t.1),
                        "mpv": Self::mpv_stats(&self.mpv),
                    }));
                    if b.idle {
                        b.phase = BenchPhase::Done;
                        return;
                    }
                    b.phase = BenchPhase::Seeking {
                        idx: 0,
                        sent: Instant::now(),
                    };
                    *self.shared.restart_at.lock().unwrap() = None;
                    self.mpv.set_flag("pause", true);
                    let t = SEEKS[0];
                    self.seek(t, true);
                }
            }
            BenchPhase::Seeking { idx, sent } => {
                let restart = *self.shared.restart_at.lock().unwrap();
                let frame = *self.shared.last_frame_at.lock().unwrap();
                if let (Some(r), Some(f)) = (restart, frame)
                    && r >= sent
                    && f >= sent
                {
                    let pos = self.mpv.double("time-pos").unwrap_or(-1.0);
                    b.seeks.push(json!({
                        "target": SEEKS[idx],
                        "time_pos": pos,
                        "restart_ms": (r - sent).as_secs_f32() * 1000.0,
                        "first_frame_ms": (f - sent).as_secs_f32() * 1000.0,
                    }));
                    if idx + 1 < SEEKS.len() {
                        *self.shared.restart_at.lock().unwrap() = None;
                        b.phase = BenchPhase::Seeking {
                            idx: idx + 1,
                            sent: Instant::now(),
                        };
                        let t = SEEKS[idx + 1];
                        self.seek(t, true);
                    } else {
                        b.phase = BenchPhase::Done;
                    }
                } else if sent.elapsed() > Duration::from_secs(10) {
                    b.seeks
                        .push(json!({ "target": SEEKS[idx], "timeout": true }));
                    b.phase = BenchPhase::Done;
                }
            }
            BenchPhase::Done => {
                let b = self.bench.take().unwrap();
                let out = json!({
                    "name": b.name,
                    "file": self.files.get(self.current),
                    "block": self.shared.block,
                    "render_size_mode": self.render_size,
                    "play": b.play,
                    "seeks": b.seeks,
                    "error": *self.shared.end_error.lock().unwrap(),
                });
                std::fs::create_dir_all("results").ok();
                let path = format!("results/{}.json", b.name);
                std::fs::write(&path, serde_json::to_string_pretty(&out).unwrap()).unwrap();
                println!("{}", serde_json::to_string(&out).unwrap());
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

fn fmt_time(t: f64) -> String {
    let t = t.max(0.0);
    format!(
        "{:02}:{:02}.{:01}",
        (t / 60.0) as u32,
        (t % 60.0) as u32,
        ((t * 10.0) % 10.0) as u32
    )
}

impl eframe::App for VideoApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let now = Instant::now();
        if let Some(last) = self.last_ui
            && let Some(b) = self.bench.as_mut()
            && matches!(b.phase, BenchPhase::Playing { .. })
        {
            b.intervals.push((now - last).as_secs_f32() * 1000.0);
        }
        self.last_ui = Some(now);
        if self.shared.display_sync {
            self.shared.tick.store(true, Ordering::Relaxed);
            *self.shared.wake.lock().unwrap() = true;
            self.shared.wake_cv.notify_one();
            if !self.props.paused {
                ui.ctx().request_repaint();
            }
        }
        if let Some(b) = self.bench.as_mut()
            && matches!(b.phase, BenchPhase::Playing { .. })
        {
            let n = self.shared.frames.load(Ordering::Relaxed);
            if b.last_shown.is_none_or(|(m, _)| m != n) {
                if let Some((_, t)) = b.last_shown {
                    b.shown.push((now - t).as_secs_f32() * 1000.0);
                }
                b.last_shown = Some((n, now));
            }
        }
        let ctx = ui.ctx().clone();

        // A texture of a new size from the render thread replaces the registered one.
        if let Some((tex, size)) = self.shared.new_texture.lock().unwrap().take()
            && let Some(rs) = frame.wgpu_render_state()
        {
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            let mut renderer = rs.renderer.write();
            if let Some((old, _)) = self.tex.take() {
                renderer.free_texture(&old);
            }
            let id = renderer.register_native_texture(&rs.device, &view, wgpu::FilterMode::Linear);
            self.tex = Some((id, size));
        }

        let rect = ui.max_rect();
        ui.painter()
            .rect_filled(rect, 0.0, Color32::from_rgb(0x1f, 0x1f, 0x1f));
        let bar_h = 44.0;
        let video_area =
            Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.bottom() - bar_h));

        // mpv property reads take the core lock and can stall for milliseconds, so the UI reads them at most
        // every 100 ms (the time label needs no more) unless `--props live`.
        if self.props_live
            || self
                .props_at
                .is_none_or(|t| t.elapsed() > Duration::from_millis(100))
        {
            self.refresh_props();
        }
        let (dw, dh) = (self.props.dw, self.props.dh);
        if dw > 0 && dh > 0 {
            // mpv's software renderer ignores the rotation (display matrix), so the texture is unrotated
            // and the quad is drawn rotated: free on the GPU.
            let quarter = ((self.props.rotate.rem_euclid(360) + 45) / 90 % 4) as usize;
            let swap = quarter % 2 == 1;
            let aspect = if swap {
                dh as f32 / dw as f32
            } else {
                dw as f32 / dh as f32
            };
            let mut size = video_area.size();
            if size.x / size.y > aspect {
                size.x = size.y * aspect;
            } else {
                size.y = size.x / aspect;
            }
            let fitted = Rect::from_center_size(video_area.center(), size);
            let ppp = ctx.pixels_per_point();
            let (tw, th) = if swap {
                (size.y, size.x)
            } else {
                (size.x, size.y)
            };
            let target = if self.render_size == "native" {
                0
            } else {
                ((tw * ppp).round() as u32) << 16 | (th * ppp).round() as u32
            };
            self.shared.target.store(target, Ordering::Relaxed);
            self.shared
                .video_size
                .store((dw as u32) << 16 | dh as u32, Ordering::Relaxed);
            if let Some((id, _)) = self.tex {
                let corners = [
                    fitted.left_top(),
                    fitted.right_top(),
                    fitted.right_bottom(),
                    fitted.left_bottom(),
                ];
                let uvs = [
                    Pos2::new(0.0, 0.0),
                    Pos2::new(1.0, 0.0),
                    Pos2::new(1.0, 1.0),
                    Pos2::new(0.0, 1.0),
                ];
                let mut mesh = egui::Mesh::with_texture(id);
                for (i, &pos) in corners.iter().enumerate() {
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos,
                        uv: uvs[(i + 4 - quarter) % 4],
                        color: Color32::WHITE,
                    });
                }
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                ui.painter().add(egui::Shape::mesh(mesh));
            }
        }

        // Controls.
        let bar = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - bar_h), rect.max);
        let (duration, pos, paused) = (self.props.duration, self.props.pos, self.props.paused);
        let mut child =
            ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(12.0, 8.0))));
        child.horizontal_centered(|ui| {
            if ui.button(if paused { "Play" } else { "Pause" }).clicked() {
                self.mpv.set_flag("pause", !paused);
                self.props.paused = !paused;
            }
            ui.monospace(format!(
                "{} / {}",
                fmt_time(self.seek_drag.unwrap_or(pos)),
                fmt_time(duration)
            ));
            let mut v = self.seek_drag.unwrap_or(pos);
            ui.spacing_mut().slider_width = (ui.available_width() - 420.0).max(100.0);
            let r = ui.add(egui::Slider::new(&mut v, 0.0..=duration.max(0.1)).show_value(false));
            if r.dragged() {
                // Keyframe seeks while scrubbing, an exact one on release.
                if self.seek_drag.is_none_or(|d| (d - v).abs() > 0.05) {
                    self.seek(v, false);
                }
                self.seek_drag = Some(v);
            }
            if r.drag_stopped() || (r.clicked() && !r.dragged()) {
                self.seek(v, true);
                self.seek_drag = None;
            }
            ui.label("Vol");
            ui.spacing_mut().slider_width = 80.0;
            if ui
                .add(egui::Slider::new(&mut self.volume, 0.0..=100.0).show_value(false))
                .changed()
            {
                self.mpv.set_double("volume", self.volume);
            }
            let names: Vec<String> = self
                .files
                .iter()
                .map(|p| {
                    p.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            let mut sel = self.current;
            egui::ComboBox::from_id_salt("file")
                .width(200.0)
                .selected_text(names.get(sel).cloned().unwrap_or_default())
                .show_index(ui, &mut sel, names.len(), |i| names[i].clone());
            if sel != self.current {
                self.current = sel;
                self.load(&self.files[sel].clone());
            }
        });

        // Keyboard: Space play / pause, ← / → 5 s, ↑ / ↓ volume.
        let (space, left, right, up, down) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
            )
        });
        if space {
            self.mpv.set_flag("pause", !paused);
            self.props.paused = !paused;
        }
        if left || right {
            self.seek(
                (pos + if right { 5.0 } else { -5.0 }).clamp(0.0, duration),
                true,
            );
        }
        if up || down {
            self.volume = (self.volume + if up { 5.0 } else { -5.0 }).clamp(0.0, 100.0);
            self.mpv.set_double("volume", self.volume);
        }

        // Stats overlay.
        let (t0, c0, last) = self.cpu_sample;
        if t0.elapsed() > Duration::from_secs(1) {
            let c = process_cpu_seconds();
            self.cpu_sample = (
                Instant::now(),
                c,
                ((c - c0) / t0.elapsed().as_secs_f64() * 100.0) as f32,
            );
        }
        let recent: Vec<f32> = {
            let t = self.shared.timings.lock().unwrap();
            t.iter()
                .rev()
                .take(60)
                .map(|x| x[0] + x[1] + x[2])
                .collect()
        };
        let mean = recent.iter().sum::<f32>() / recent.len().max(1) as f32;
        let text = format!(
            "{}  render {:?}  frame {mean:.1} ms  CPU {:.0} % (1 core = 100)",
            self.props.overlay,
            self.tex.map(|t| t.1),
            self.cpu_sample.2,
        );
        let _ = last;
        ui.painter().text(
            rect.min + Vec2::new(10.0, 10.0),
            egui::Align2::LEFT_TOP,
            text,
            egui::FontId::monospace(12.0),
            Color32::from_gray(230),
        );

        if let Some((path, at)) = self.screenshot.clone()
            && pos >= at as f64
        {
            self.screenshot = None;
            let ctx2 = ctx.clone();
            ctx.request_screenshot(move |image: Arc<egui::ColorImage>| {
                let [w, h] = image.size;
                let mut out = format!(
                    "P6
{w} {h}
255
"
                )
                .into_bytes();
                for c in &image.pixels {
                    out.extend_from_slice(&c.to_array()[..3]);
                }
                let _ = std::fs::write(&path, out);
                ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
            });
        }
        if let Some((path, at)) = &self.dump
            && pos >= *at as f64
        {
            *self.shared.dump.lock().unwrap() = Some(path.clone());
            self.dump = None;
        }
        if matches!(
            self.bench.as_ref().map(|b| &b.phase),
            Some(BenchPhase::Playing { .. })
        ) {
            self.ui_cpu.push(now.elapsed().as_secs_f32() * 1000.0);
        }
        self.bench_step(&ctx);
        if !paused {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn on_exit(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.wake_cv.notify_all();
        self.mpv.wakeup();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

fn main() -> eframe::Result {
    let args = Args::parse();
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Spike · video")
            .with_inner_size([1280.0, 800.0])
            .with_maximized(args.str("maximized", "off") == "on"),
        ..Default::default()
    };
    if args.str("vsync", "on") == "off" {
        options.wgpu_options.surface.present_mode = wgpu::PresentMode::AutoNoVsync;
    }
    eframe::run_native(
        "spike-video",
        options,
        Box::new(move |cc| {
            Ok(Box::new(
                VideoApp::new(cc, &args).map_err(|e| e.to_string())?,
            ))
        }),
    )
}
