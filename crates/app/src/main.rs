//! Good Gallery.
//!
//! Options: `--data-dir <folder>` (index, settings, thumbnails and logs; default `%APPDATA%\good-gallery` in the packaged
//! app, the Electron version's, and `%APPDATA%\good-gallery-rust` otherwise), `--screenshot
//! <file.png>` (saves a screenshot of the window after a few frames and exits), `--screenshot-after <seconds>` (waits
//! that long before the screenshot, e.g. for scans or thumbnails), `--bench-scroll <seconds>` (vsync off; scrolls the
//! gallery once to warm the files, then again while measuring frame times, prints them and exits; REQ-006-12), `--view
//! <position>` (opens the viewer at that gallery position once the gallery has loaded, e.g. for a screenshot; with
//! `--info` its info panel too), `--scroll
//! <points>` (scrolls the gallery that far once it has loaded, e.g. to show the glass toolbar over tiles).

// The packaged app (`cargo xtask dist` builds it with the `packaged` feature) opens no console window; the log goes to
// `logs\app.log` in the data folder.
#![cfg_attr(feature = "packaged", windows_subsystem = "windows")]

use anyhow::Context as _;
use app::{AppState, NativeFolderPicker};
use eframe::{egui, egui_wgpu};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Frames to render before a `--screenshot` is taken, so fonts, icons and layout have settled.
const SCREENSHOT_AFTER_FRAMES: u32 = 5;

struct Options {
    data_dir: PathBuf,
    screenshot: Option<PathBuf>,
    screenshot_after: std::time::Duration,
    bench_scroll: Option<f32>,
    view: Option<usize>,
    info: bool,
    scroll: Option<f32>,
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
    /// `--info`: the viewer opens with its info panel.
    info: bool,
    /// `--scroll`: how far to scroll the gallery once it has loaded.
    scroll: Option<f32>,
    /// Windows' animation and transparency effects, followed while the app runs.
    preferences: app::theme::OsPreferences,
}

impl eframe::App for GalleryApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.preferences.poll(ui.ctx());
        app::ui(ui, &mut self.state);
        self.frames += 1;
        if let Some(index) = self.view
            && let Some(gallery) = &self.state.gallery
            && let Some(mut viewer) = app::viewer::Viewer::open(gallery, index)
        {
            self.view = None;
            viewer.set_info_open(self.info);
            self.state.viewer = Some(viewer);
            ui.ctx().request_repaint();
        }
        if let Some(offset) = self.scroll
            && let Some(gallery) = &mut self.state.gallery
            && gallery.count().is_some()
        {
            self.scroll = None;
            gallery.scroll_to(offset);
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
    Ok(app::startup::default_data_dir(
        Path::new(&base),
        cfg!(feature = "packaged"),
    ))
}

fn parse_args() -> anyhow::Result<Options> {
    const USAGE: &str = "usage: app [--data-dir <folder>] [--screenshot <file.png> [--screenshot-after <seconds>]] \
                         [--bench-scroll <seconds>] [--view <position> [--info]] [--scroll <points>]";
    let mut args = std::env::args().skip(1);
    let mut data_dir = None;
    let mut screenshot = None;
    let mut screenshot_after = std::time::Duration::ZERO;
    let mut bench_scroll = None;
    let mut view = None;
    let mut info = false;
    let mut scroll = None;
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
            "--info" => info = true,
            "--scroll" => scroll = Some(args.next().context(USAGE)?.parse().context(USAGE)?),
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
        info,
        scroll,
    })
}

/// The running parts that take changed settings (spec 010).
struct RunningParts {
    thumbnails: Arc<indexer::thumbnails::ThumbnailService>,
    /// Without exiftool there is no indexer and no schedule.
    indexer: Option<(Arc<indexer::Indexer>, Arc<media::exiftool::ExifTool>)>,
    scheduler: Option<Arc<indexer::schedule::Scheduler>>,
}

impl app::SettingsTargets for RunningParts {
    fn set_quick_check_interval(&self, minutes: u32) {
        if let Some(scheduler) = &self.scheduler {
            scheduler.set_interval(minutes);
        }
    }

    fn set_io_concurrency(&self, reads: usize) {
        if let Some((indexer, exiftool)) = &self.indexer {
            // The next scan reads with the new number. exiftool's pool only grows now: shrinking it would throttle a
            // running scan, which keeps its value (REQ-010-04); extra idle processes cost nothing.
            indexer.set_concurrency(reads);
            exiftool.grow_max_processes(reads);
        }
        self.thumbnails.set_workers(reads);
    }

    fn set_cache_limit(&self, bytes: u64) {
        self.thumbnails.cache().set_max_bytes(bytes);
    }

    fn cache_usage(&self) -> Option<u64> {
        let cache = self.thumbnails.cache();
        cache.is_measured().then(|| cache.usage())
    }

    fn clear_cache(&self) -> u64 {
        let cache = self.thumbnails.cache();
        cache.clear();
        cache.usage()
    }
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
    let exiftool = media::exiftool::find_exiftool().map(|path| {
        tracing::info!("exiftool: {}", path.display());
        Arc::new(media::exiftool::ExifTool::new(path, concurrency))
    });
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
    let videos = videos(ctx, render_state.clone());
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
        .with_originals(originals)
        .with_videos(videos);
    let Some(exiftool) = exiftool else {
        tracing::error!("exiftool not found: libraries can't be indexed");
        let mut state = state.with_settings_targets(Arc::new(RunningParts {
            thumbnails,
            indexer: None,
            scheduler: None,
        }));
        state
            .toasts
            .error("exiftool not found, so libraries can't be indexed. Put exiftool.exe next to the app.");
        return state;
    };
    let ctx = ctx.clone();
    let rendering = thumbnails.clone();
    let (indexer, events) = indexer::Indexer::start(indexer::IndexerDeps {
        writer: db::writer::IndexWriter::new(writer),
        metadata: exiftool.clone(),
        is_reachable,
        concurrency,
        notify: Box::new(move || ctx.request_repaint()),
        on_written: Box::new(move || thumbnails.kick()),
    });
    let indexer = Arc::new(indexer);
    // Quick scans at startup and every quick check interval; offline libraries every minute.
    let scheduler = Arc::new(indexer::schedule::Scheduler::start(
        schedule_conn,
        indexer.clone(),
        settings.quick_check_interval_minutes,
        indexer::schedule::OFFLINE_RETRY,
    ));
    let parts = RunningParts {
        thumbnails: rendering,
        indexer: Some((indexer.clone(), exiftool)),
        scheduler: Some(scheduler.clone()),
    };
    state
        .with_indexer(indexer, events)
        .with_scheduler(scheduler)
        .with_settings_targets(Arc::new(parts))
}

/// The video player, made when the first video shows (libmpv is large); without libmpv, videos show their preview.
fn videos(ctx: &egui::Context, render_state: Option<egui_wgpu::RenderState>) -> app::video::Videos {
    let Some(dll) = media::helpers::find_mpv() else {
        tracing::error!("libmpv not found: videos can't play");
        return app::video::Videos::unavailable("libmpv-2.dll not found");
    };
    tracing::info!("libmpv: {}", dll.display());
    let ctx = ctx.clone();
    app::video::Videos::new(move || {
        let player = app::video::MpvPlayer::new(&dll, ctx, render_state, true)?;
        Ok(Box::new(player) as Box<dyn app::video::Playback>)
    })
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
    if let Some(path) = &ffmpeg {
        tracing::info!("ffmpeg: {}", path.display());
    } else {
        tracing::error!("ffmpeg not found: videos get no thumbnails");
        state
            .toasts
            .error("ffmpeg not found, so videos get no thumbnails. Put ffmpeg.exe next to the app.");
    }
    let previews: Arc<dyn media::thumbnail::PreviewSource> = match exiftool {
        Some(exiftool) => exiftool,
        None => Arc::new(media::thumbnail::NoPreviews),
    };
    let cache_bytes = app::state::gib_to_bytes(settings.thumb_cache_gib);
    indexer::thumbnails::ThumbnailService::start(indexer::thumbnails::ThumbnailDeps {
        conn,
        cache: indexer::thumb_cache::ThumbCache::open(data_dir.join("thumbs"), cache_bytes),
        render: Arc::new(media::thumbnail::Renderer::new(previews, ffmpeg)),
        is_reachable,
        workers: settings.io_concurrency as usize,
        on_ready,
    })
}

/// Logs to stderr and, when it can be opened, to `logs\app.log` in the data folder.
fn init_logging(data_dir: &Path) {
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let log = app::startup::open_log(data_dir);
    let (file, problem) = match log {
        Ok(file) => (Some(file), None),
        Err(error) => (None, Some(error)),
    };
    let file_layer = file.map(|file| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
    });
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(file_layer)
        .init();
    // The packaged app has no console: panics go to the log too.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        default_hook(info);
    }));
    match problem {
        None => tracing::info!("Good Gallery {} in {}", env!("CARGO_PKG_VERSION"), data_dir.display()),
        Some(error) => tracing::warn!("no log file in {}: {error}", data_dir.display()),
    }
}

/// Reports a failure; the packaged app has no console, so an interactive run also gets a message box.
fn report_failure(error: &anyhow::Error, interactive: bool) {
    tracing::error!("{error:#}");
    if interactive && cfg!(feature = "packaged") {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Good Gallery")
            .set_description(format!("Good Gallery couldn't start.\n\n{error:#}"))
            .show();
    }
}

fn main() -> std::process::ExitCode {
    let options = match parse_args() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("error: {error:#}");
            report_failure(&error, true);
            return std::process::ExitCode::FAILURE;
        }
    };
    init_logging(&options.data_dir);
    // Automated runs (screenshots, benchmarks) must not wait for someone to close a message box.
    let interactive = options.screenshot.is_none() && options.bench_scroll.is_none();
    match run(options) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            report_failure(&error, interactive);
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(options: Options) -> anyhow::Result<()> {
    // exiftool and ffmpeg must not outlive the app, however it ends.
    if let Err(error) = media::helpers::end_helpers_with_app() {
        tracing::warn!("helper processes may outlive the app: {error}");
    }
    let screenshot_failed = Arc::new(AtomicBool::new(false));
    let failed = screenshot_failed.clone();
    let mut native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Good Gallery")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 600.0])
            .with_icon(app::startup::window_icon()),
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
            if let Some(render_state) = &cc.wgpu_render_state {
                app::glass::install(&cc.egui_ctx, render_state);
            }
            app::titlebar::install(cc);
            Ok(Box::new(GalleryApp {
                state: open_state(&options.data_dir, &cc.egui_ctx, cc.wgpu_render_state.clone()),
                screenshot: options.screenshot,
                screenshot_failed: failed,
                screenshot_at: std::time::Instant::now() + options.screenshot_after,
                screenshot_taken: false,
                frames: 0,
                bench: options.bench_scroll.map(Bench::new),
                view: options.view,
                info: options.info,
                scroll: options.scroll,
                preferences: Default::default(),
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
