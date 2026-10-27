//! Good Gallery.
//!
//! Options: `--data-dir <folder>` (index and settings; default `%APPDATA%\good-gallery-rust`), `--screenshot
//! <file.png>` (saves a screenshot of the window after a few frames and exits), `--screenshot-after <seconds>` (waits
//! that long before the screenshot, e.g. for scans or thumbnails), `--bench-scroll <seconds>` (vsync off; scrolls the
//! gallery once to warm the files, then again while measuring frame times, prints them and exits; REQ-006-12), `--view
//! <position>` (opens the viewer at that gallery position once the gallery has loaded, e.g. for a screenshot).

use anyhow::Context as _;
use app::{AppState, NativeFolderPicker};
use eframe::{egui, egui_wgpu};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Frames to render before a `--screenshot` is taken, so fonts, icons and layout have settled.
const SCREENSHOT_AFTER_FRAMES: u32 = 5;

/// During the rewrite the app keeps its own data folder, so it never touches the index of the Electron version
/// (`%APPDATA%\good-gallery`). Sharing it is a packaging decision (M11).
const DATA_FOLDER: &str = "good-gallery-rust";

struct Options {
    data_dir: PathBuf,
    screenshot: Option<PathBuf>,
    screenshot_after: std::time::Duration,
    bench_scroll: Option<f32>,
    view: Option<usize>,
}

/// Scrolling speed of `--bench-scroll`, points per second (as in the M0 spike).
const BENCH_SPEED: f32 = 1440.0;

enum BenchPhase {
    /// Until the gallery has loaded, plus a moment for the first screen.
    Waiting(Option<std::time::Instant>),
    /// A first pass reads every thumbnail the run scrolls past, so Defender's first-read scan is out of the way.
    Warming(std::time::Instant),
    Measuring(std::time::Instant),
    Done,
}

/// `--bench-scroll`: frame intervals while the gallery scrolls at a constant speed, without vsync.
struct Bench {
    seconds: f32,
    phase: BenchPhase,
    last_frame: Option<std::time::Instant>,
    intervals: Vec<f32>,
    max_texture_bytes: usize,
}

impl Bench {
    fn new(seconds: f32) -> Self {
        Self {
            seconds,
            phase: BenchPhase::Waiting(None),
            last_frame: None,
            intervals: Vec::new(),
            max_texture_bytes: 0,
        }
    }

    /// Scrolls one frame further; true once the run is over.
    fn advance(&mut self, state: &mut AppState) -> bool {
        let now = std::time::Instant::now();
        let interval = self.last_frame.map(|last| (now - last).as_secs_f32());
        self.last_frame = Some(now);
        let Some(gallery) = state.gallery.as_mut() else {
            return true;
        };
        let step = BENCH_SPEED * interval.unwrap_or(0.0).min(0.1);
        match self.phase {
            BenchPhase::Waiting(None) if gallery.count().is_some() => self.phase = BenchPhase::Waiting(Some(now)),
            BenchPhase::Waiting(Some(since)) if (now - since).as_secs_f32() > 2.0 => {
                self.phase = BenchPhase::Warming(now);
            }
            BenchPhase::Warming(since) => {
                gallery.scroll_to(gallery.offset() + step);
                if (now - since).as_secs_f32() >= self.seconds {
                    gallery.scroll_to(0.0);
                    self.phase = BenchPhase::Measuring(now + std::time::Duration::from_secs(2));
                }
            }
            BenchPhase::Measuring(start) if now >= start => {
                gallery.scroll_to(gallery.offset() + step);
                if let Some(interval) = interval {
                    self.intervals.push(interval * 1000.0);
                }
                self.max_texture_bytes = self.max_texture_bytes.max(gallery.texture_bytes());
                if (now - start).as_secs_f32() >= self.seconds {
                    self.phase = BenchPhase::Done;
                }
            }
            BenchPhase::Done => return true,
            _ => {}
        }
        false
    }

    fn report(&self) {
        let mut sorted = self.intervals.clone();
        sorted.sort_by(f32::total_cmp);
        let at = |p: f32| {
            sorted
                .get(((sorted.len().max(1) - 1) as f32 * p).round() as usize)
                .copied()
                .unwrap_or(0.0)
        };
        println!(
            "bench-scroll: {} frames, interval p50 {:.1} ms, p95 {:.1} ms, p99 {:.1} ms, max {:.1} ms; thumbnail textures at most {} MB",
            sorted.len(),
            at(0.5),
            at(0.95),
            at(0.99),
            sorted.last().copied().unwrap_or(0.0),
            self.max_texture_bytes >> 20
        );
    }
}

struct GalleryApp {
    state: AppState,
    screenshot: Option<PathBuf>,
    /// Set when `--screenshot` couldn't be saved, so the process exits with an error.
    screenshot_failed: Arc<AtomicBool>,
    /// When the screenshot is due, at the earliest.
    screenshot_at: std::time::Instant,
    frames: u32,
    screenshot_taken: bool,
    bench: Option<Bench>,
    /// `--view`: the gallery position to open in the viewer once it has loaded.
    view: Option<usize>,
}

impl eframe::App for GalleryApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        app::ui(ui, &mut self.state);
        self.frames += 1;
        if let Some(index) = self.view
            && let Some(gallery) = &self.state.gallery
            && let Some(viewer) = app::viewer::Viewer::open(gallery, index)
        {
            self.view = None;
            self.state.viewer = Some(viewer);
            ui.ctx().request_repaint();
        }
        if let Some(bench) = &mut self.bench {
            if bench.advance(&mut self.state) {
                bench.report();
                self.bench = None;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ui.ctx().request_repaint();
        }
        if let Some(path) = &self.screenshot {
            if !self.screenshot_taken
                && self.frames >= SCREENSHOT_AFTER_FRAMES
                && std::time::Instant::now() >= self.screenshot_at
            {
                self.screenshot_taken = true;
                let (path, ctx, failed) = (path.clone(), ui.ctx().clone(), self.screenshot_failed.clone());
                ui.ctx().request_screenshot(move |image: Arc<egui::ColorImage>| {
                    match save_png(&path, &image) {
                        Ok(()) => tracing::info!("saved {}", path.display()),
                        Err(error) => {
                            tracing::error!("{error:#}");
                            failed.store(true, Ordering::Relaxed);
                        }
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                });
            }
            ui.ctx().request_repaint();
        }
    }
}

fn save_png(path: &Path, image: &egui::ColorImage) -> anyhow::Result<()> {
    let [width, height] = image.size;
    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    image::save_buffer(
        path,
        &rgba,
        width as u32,
        height as u32,
        image::ExtendedColorType::Rgba8,
    )
    .with_context(|| format!("can't save {}", path.display()))
}

fn default_data_dir() -> anyhow::Result<PathBuf> {
    let base = std::env::var_os("APPDATA").context("APPDATA isn't set; pass --data-dir")?;
    Ok(PathBuf::from(base).join(DATA_FOLDER))
}

fn parse_args() -> anyhow::Result<Options> {
    const USAGE: &str = "usage: app [--data-dir <folder>] [--screenshot <file.png> [--screenshot-after <seconds>]] \
                         [--bench-scroll <seconds>] [--view <position>]";
    let mut args = std::env::args().skip(1);
    let mut data_dir = None;
    let mut screenshot = None;
    let mut screenshot_after = std::time::Duration::ZERO;
    let mut bench_scroll = None;
    let mut view = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => data_dir = Some(args.next().context(USAGE)?.into()),
            "--screenshot" => screenshot = Some(args.next().context(USAGE)?.into()),
            "--screenshot-after" => {
                let seconds: f64 = args.next().context(USAGE)?.parse().context(USAGE)?;
                screenshot_after = std::time::Duration::try_from_secs_f64(seconds).context(USAGE)?;
            }
            "--bench-scroll" => {
                let seconds: f32 = args.next().context(USAGE)?.parse().context(USAGE)?;
                anyhow::ensure!(seconds > 0.0, USAGE);
                bench_scroll = Some(seconds);
            }
            "--view" => view = Some(args.next().context(USAGE)?.parse().context(USAGE)?),
            other => anyhow::bail!("unknown argument {other} ({USAGE})"),
        }
    }
    let data_dir = match data_dir {
        Some(dir) => dir,
        None => default_data_dir()?,
    };
    Ok(Options {
        data_dir,
        screenshot,
        screenshot_after,
        bench_scroll,
        view,
    })
}

/// Opens the index in the data folder (one connection for the UI, one for the indexer) and starts the indexer, or
/// says why it can't. `ctx` lets the indexer wake the UI.
/// Thumbnails are drawn as own wgpu textures, so the gallery streams them only with a wgpu `render_state`.
fn open_state(data_dir: &Path, ctx: &egui::Context, render_state: Option<egui_wgpu::RenderState>) -> AppState {
    let opened = std::fs::create_dir_all(data_dir)
        .with_context(|| format!("can't create {}", data_dir.display()))
        .and_then(|()| {
            let path = data_dir.join("gallery.db");
            let open = || db::open(&path).with_context(|| format!("can't open {}", path.display()));
            let (conn, writer, thumbs) = (open()?, open()?, open()?);
            // Read-only: only after `open` has created and migrated the index.
            let read = || db::open_reader(&path).with_context(|| format!("can't open {}", path.display()));
            Ok((conn, writer, thumbs, read()?, read()?))
        });
    let (conn, writer, thumbs_conn, schedule_conn, gallery_conn) = match opened {
        Ok(connections) => connections,
        Err(error) => {
            tracing::error!("{error:#}");
            return AppState::failed(format!("{error:#}"));
        }
    };
    let settings = db::settings::load(&conn).unwrap_or_default();
    let mut state = AppState::new(conn, Arc::new(indexer::NoScans), Arc::new(NativeFolderPicker));
    let concurrency = settings.io_concurrency as usize;
    let exiftool =
        media::exiftool::find_exiftool().map(|path| Arc::new(media::exiftool::ExifTool::new(path, concurrency)));
    let is_reachable =
        Arc::new(|path: &Path| indexer::reachability::is_reachable_directory(path, indexer::reachability::TIMEOUT));
    let (ready_tx, ready) = std::sync::mpsc::channel();
    let repaint = ctx.clone();
    let on_ready = Box::new(move |answer| {
        let _ = ready_tx.send(answer);
        repaint.request_repaint();
    });
    let thumbnails = Arc::new(start_thumbnails(
        data_dir,
        thumbs_conn,
        &settings,
        exiftool.clone(),
        is_reachable.clone(),
        on_ready,
        &mut state,
    ));
    // Media items indexed before (by this app or the Electron version) may still wait for thumbnails.
    thumbnails.kick();
    let settings_db = state.db().expect("opened above");
    let mut gallery = app::gallery::Gallery::new(
        Arc::new(std::sync::Mutex::new(gallery_conn)),
        settings_db,
        Some(ctx.clone()),
    );
    // Originals as wgpu textures made on worker threads, like thumbnails (REQ-009-04).
    let originals = app::originals::Originals::new(ctx.clone(), render_state.clone(), media::ffmpeg::find_ffmpeg());
    match render_state {
        Some(render_state) => {
            let cache = thumbnails.cache().clone();
            gallery = gallery.with_thumbnails(app::gallery::Thumbnails {
                streamer: app::textures::TextureStreamer::new(
                    render_state,
                    cache.clone(),
                    app::textures::DEFAULT_BUDGET,
                    ctx.clone(),
                ),
                cache,
                service: Some(thumbnails.clone()),
                ready,
            });
        }
        None => tracing::error!("no wgpu renderer: the gallery shows no thumbnails"),
    }
    let state = state
        .with_thumbnails(thumbnails.clone())
        .with_gallery(gallery)
        .with_originals(originals);
    let Some(exiftool) = exiftool else {
        tracing::error!("exiftool not found: libraries can't be indexed");
        let mut state = state;
        state
            .toasts
            .error("exiftool not found, so libraries can't be indexed. Put exiftool.exe next to the app.");
        return state;
    };
    let ctx = ctx.clone();
    let (indexer, events) = indexer::Indexer::start(indexer::IndexerDeps {
        writer: db::writer::IndexWriter::new(writer),
        metadata: exiftool,
        is_reachable,
        concurrency,
        notify: Box::new(move || ctx.request_repaint()),
        on_written: Box::new(move || thumbnails.kick()),
    });
    let indexer = Arc::new(indexer);
    // Quick scans at startup and every quick check interval; offline libraries every minute.
    let scheduler = indexer::schedule::Scheduler::start(
        schedule_conn,
        indexer.clone(),
        settings.quick_check_interval_minutes,
        indexer::schedule::OFFLINE_RETRY,
    );
    state.with_indexer(indexer, events).with_scheduler(scheduler)
}

/// The thumbnail cache in the data folder and the service that fills it. Without ffmpeg, videos and AVIF images get
/// no thumbnails; without exiftool, embedded JPEG previews aren't used.
fn start_thumbnails(
    data_dir: &Path,
    conn: db::rusqlite::Connection,
    settings: &db::settings::Settings,
    exiftool: Option<Arc<media::exiftool::ExifTool>>,
    is_reachable: Arc<dyn Fn(&Path) -> bool + Send + Sync>,
    on_ready: Box<dyn Fn(indexer::thumbnails::ThumbnailReady) + Send + Sync>,
    state: &mut AppState,
) -> indexer::thumbnails::ThumbnailService {
    let ffmpeg = media::ffmpeg::find_ffmpeg();
    if ffmpeg.is_none() {
        tracing::error!("ffmpeg not found: videos get no thumbnails");
        state
            .toasts
            .error("ffmpeg not found, so videos get no thumbnails. Put ffmpeg.exe next to the app.");
    }
    let previews: Arc<dyn media::thumbnail::PreviewSource> = match exiftool {
        Some(exiftool) => exiftool,
        None => Arc::new(media::thumbnail::NoPreviews),
    };
    let cache_bytes = (settings.thumb_cache_gib * 1024f64.powi(3)).round() as u64;
    indexer::thumbnails::ThumbnailService::start(indexer::thumbnails::ThumbnailDeps {
        conn,
        cache: indexer::thumb_cache::ThumbCache::open(data_dir.join("thumbs"), cache_bytes),
        render: Arc::new(media::thumbnail::Renderer::new(previews, ffmpeg)),
        is_reachable,
        workers: settings.io_concurrency as usize,
        on_ready,
    })
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    // exiftool (and later ffmpeg) must not outlive the app, however it ends.
    if let Err(error) = media::helpers::end_helpers_with_app() {
        tracing::warn!("helper processes may outlive the app: {error}");
    }
    let options = parse_args()?;
    let screenshot_failed = Arc::new(AtomicBool::new(false));
    let failed = screenshot_failed.clone();
    let mut native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Good Gallery")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    if options.bench_scroll.is_some() {
        // With vsync every interval is ≈ 16.7 ms; the budget is measured without it.
        native.wgpu_options.surface.present_mode = eframe::wgpu::PresentMode::AutoNoVsync;
    }
    eframe::run_native(
        "good-gallery",
        native,
        Box::new(move |cc| {
            app::setup(&cc.egui_ctx);
            Ok(Box::new(GalleryApp {
                state: open_state(&options.data_dir, &cc.egui_ctx, cc.wgpu_render_state.clone()),
                screenshot: options.screenshot,
                screenshot_failed: failed,
                screenshot_at: std::time::Instant::now() + options.screenshot_after,
                screenshot_taken: false,
                frames: 0,
                bench: options.bench_scroll.map(Bench::new),
                view: options.view,
            }))
        }),
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?;
    anyhow::ensure!(
        !screenshot_failed.load(Ordering::Relaxed),
        "the screenshot wasn't saved"
    );
    Ok(())
}
