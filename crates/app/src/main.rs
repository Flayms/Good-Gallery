//! Good Gallery. `--screenshot <file.png>` saves a screenshot of the window after a few frames and exits.

use anyhow::Context as _;
use app::{AppState, theme};
use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Frames to render before a `--screenshot` is taken, so fonts and layout have settled.
const SCREENSHOT_AFTER_FRAMES: u32 = 3;

struct GalleryApp {
    state: AppState,
    screenshot: Option<PathBuf>,
    /// Set when `--screenshot` couldn't be saved, so the process exits with an error.
    screenshot_failed: Arc<AtomicBool>,
    frames: u32,
}

impl eframe::App for GalleryApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        app::ui(ui, &mut self.state);
        self.frames += 1;
        if let Some(path) = &self.screenshot {
            if self.frames == SCREENSHOT_AFTER_FRAMES {
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

fn save_png(path: &std::path::Path, image: &egui::ColorImage) -> anyhow::Result<()> {
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

fn parse_args() -> anyhow::Result<Option<PathBuf>> {
    let mut args = std::env::args().skip(1);
    let mut screenshot = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--screenshot" => screenshot = Some(args.next().context("--screenshot needs a file")?.into()),
            other => anyhow::bail!("unknown argument {other} (usage: app [--screenshot <file.png>])"),
        }
    }
    Ok(screenshot)
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let screenshot = parse_args()?;
    let screenshot_failed = Arc::new(AtomicBool::new(false));
    let failed = screenshot_failed.clone();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Good Gallery")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native(
        "good-gallery",
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(GalleryApp {
                state: AppState::default(),
                screenshot,
                screenshot_failed: failed,
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
