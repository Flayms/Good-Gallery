//! Good Gallery.
//!
//! Options: `--data-dir <folder>` (index and settings; default `%APPDATA%\good-gallery-rust`), `--screenshot
//! <file.png>` (saves a screenshot of the window after a few frames and exits), `--screenshot-after <seconds>` (waits
//! that long before the screenshot, e.g. for scans or thumbnails).

use anyhow::Context as _;
use app::{AppState, NativeFolderPicker};
use eframe::egui;
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
}

impl eframe::App for GalleryApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        app::ui(ui, &mut self.state);
        self.frames += 1;
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
    const USAGE: &str = "usage: app [--data-dir <folder>] [--screenshot <file.png> [--screenshot-after <seconds>]]";
    let mut args = std::env::args().skip(1);
    let mut data_dir = None;
    let mut screenshot = None;
    let mut screenshot_after = std::time::Duration::ZERO;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => data_dir = Some(args.next().context(USAGE)?.into()),
            "--screenshot" => screenshot = Some(args.next().context(USAGE)?.into()),
            "--screenshot-after" => {
                let seconds: f64 = args.next().context(USAGE)?.parse().context(USAGE)?;
                screenshot_after = std::time::Duration::try_from_secs_f64(seconds).context(USAGE)?;
            }
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
    })
}

/// Opens the index in the data folder (one connection for the UI, one for the indexer) and starts the indexer, or
/// says why it can't. `ctx` lets the indexer wake the UI.
fn open_state(data_dir: &Path, ctx: &egui::Context) -> AppState {
    let opened = std::fs::create_dir_all(data_dir)
        .with_context(|| format!("can't create {}", data_dir.display()))
        .and_then(|()| {
            let path = data_dir.join("gallery.db");
            let open = || db::open(&path).with_context(|| format!("can't open {}", path.display()));
            let (conn, writer) = (open()?, open()?);
            // Read-only: only after `open` has created and migrated the index.
            let reader = db::open_reader(&path).with_context(|| format!("can't open {}", path.display()))?;
            Ok((conn, writer, reader))
        });
    let (conn, writer, schedule_conn) = match opened {
        Ok(connections) => connections,
        Err(error) => {
            tracing::error!("{error:#}");
            return AppState::failed(format!("{error:#}"));
        }
    };
    let settings = db::settings::load(&conn).unwrap_or_default();
    let state = AppState::new(conn, Arc::new(indexer::NoScans), Arc::new(NativeFolderPicker));
    let Some(exiftool) = media::exiftool::find_exiftool() else {
        tracing::error!("exiftool not found: libraries can't be indexed");
        let mut state = state;
        state
            .toasts
            .error("exiftool not found, so libraries can't be indexed. Put exiftool.exe next to the app.");
        return state;
    };
    let concurrency = settings.io_concurrency as usize;
    let ctx = ctx.clone();
    let (indexer, events) = indexer::Indexer::start(indexer::IndexerDeps {
        writer: db::writer::IndexWriter::new(writer),
        metadata: Arc::new(media::exiftool::ExifTool::new(exiftool, concurrency)),
        is_reachable: Arc::new(|path: &Path| {
            indexer::reachability::is_reachable_directory(path, indexer::reachability::TIMEOUT)
        }),
        concurrency,
        notify: Box::new(move || ctx.request_repaint()),
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
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Good Gallery")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "good-gallery",
        native,
        Box::new(move |cc| {
            app::setup(&cc.egui_ctx);
            Ok(Box::new(GalleryApp {
                state: open_state(&options.data_dir, &cc.egui_ctx),
                screenshot: options.screenshot,
                screenshot_failed: failed,
                screenshot_at: std::time::Instant::now() + options.screenshot_after,
                screenshot_taken: false,
                frames: 0,
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
