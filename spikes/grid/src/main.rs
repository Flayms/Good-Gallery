//! M0 spike: justified / masonry grid over 100k+ thumbnails, glass, font, title bar.
//! See docs/rewrite/milestones/M00-spikes.md.
//!
//! cargo run --release -- [--items 100000] [--view justified|masonry|grid] [--cols 5] [--vsync on|off]
//!   [--decoder image|libwebp] [--upload egui|native] [--threads N] [--budget-mb 512] [--upload-budget-ms 3]
//!   [--bench NAME --speed 1440 --seconds 20 --warm on|off] [--start-item N]
//!   [--look on|off] [--glass on|off] [--bar-passes 4] [--panel-passes 4] [--panel on|off]
//!   [--titlebar native|custom|hybrid] [--tnum on|off] [--hinting default|on|off] [--overlay on|off]
//!   [--screenshot NAME --screenshot-after 4] [--maximized on]

mod bench;
mod glass;
mod items;
mod layout;
mod look;
mod stream;
mod titlebar;

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Vec2};
use layout::{Layout, View};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use stream::{Decoder, Streamer, Upload};

pub const DARKROOM: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
pub const RAISED: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
const PADDING: f32 = 12.0;

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
    pub fn get<T: std::str::FromStr>(&self, key: &str, default: T) -> T {
        self.0
            .get(key)
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }
    pub fn str(&self, key: &str, default: &str) -> String {
        self.0
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }
}

struct GridApp {
    /// File index of each item; files repeat to reach the item count.
    item_file: Vec<u32>,
    aspects: Vec<f32>,
    view: View,
    columns: usize,
    layout: Layout,
    layout_key: (View, usize, u32),
    pub layout_ms: f32,
    scroll: f32,
    last_scroll: f32,
    streamer: Streamer,
    bench: Option<bench::Bench>,
    last_frame: Option<Instant>,
    paths: Arc<Vec<PathBuf>>,
    start_item: Option<usize>,
    look: bool,
    glass: bool,
    bar_passes: u32,
    panel_passes: u32,
    panel_open: bool,
    titlebar: titlebar::Variant,
    overlay: bool,
    screenshot: Option<String>,
    screenshot_after: f32,
    screenshot_requested: Option<Instant>,
    created: Instant,
    /// Extra scroll after `--start-item`, so tiles sit under the toolbar for screenshots.
    nudge: f32,
    /// Skip the chrome (sidebar, toolbar, popover), for the CSS reference render.
    bare: bool,
}

impl GridApp {
    fn new(cc: &eframe::CreationContext, args: &Args) -> Self {
        let dir = PathBuf::from(args.str(
            "thumbs",
            &format!("{}\\gg-spike\\thumbs", std::env::var("TEMP").unwrap()),
        ));
        let start = Instant::now();
        let thumbs = items::scan_cached(&dir);
        let scan_ms = start.elapsed().as_secs_f32() * 1000.0;
        assert!(!thumbs.is_empty(), "no thumbnails in {}", dir.display());
        let count = args.get("items", 100_000usize).max(1);
        let item_file: Vec<u32> = (0..count).map(|i| (i % thumbs.len()) as u32).collect();
        let aspects = item_file
            .iter()
            .map(|&f| {
                let t = &thumbs[f as usize];
                (t.height as f32 / t.width as f32).clamp(0.25, 4.0)
            })
            .collect();
        println!(
            "scanned {} thumbnails in {scan_ms:.0} ms, {count} items",
            thumbs.len()
        );

        let paths = Arc::new(thumbs.into_iter().map(|t| t.path).collect::<Vec<_>>());
        let decoder = match args.str("decoder", "libwebp").as_str() {
            "image" => Decoder::Image,
            _ => Decoder::Libwebp,
        };
        let upload = match args.str("upload", "native").as_str() {
            "egui" => Upload::Egui,
            _ => Upload::Native,
        };
        let threads = args.get("threads", 4);
        let mut streamer = Streamer::new(
            paths.clone(),
            threads,
            decoder,
            upload,
            args.get("budget-mb", 512usize) << 20,
            cc.egui_ctx.clone(),
            cc.wgpu_render_state
                .as_ref()
                .map(|rs| (rs.device.clone(), rs.queue.clone())),
        );
        streamer.upload_budget_ms = args.get("upload-budget-ms", 3.0);
        let view = match args.str("view", "justified").as_str() {
            "masonry" => View::Masonry,
            "grid" => View::Grid,
            _ => View::Justified,
        };
        let bench = args
            .0
            .contains_key("bench")
            .then(|| bench::Bench::new(args, scan_ms, threads));
        let look = args.str("look", if bench.is_some() { "off" } else { "on" }) == "on";
        let hinting = match args.str("hinting", "default").as_str() {
            "on" => Some(true),
            "off" => Some(false),
            _ => None,
        };
        look::install_fonts(&cc.egui_ctx, args.str("tnum", "on") == "on", hinting);
        if let Some(rs) = &cc.wgpu_render_state {
            glass::GlassResources::install(rs);
        }
        let variant = titlebar_variant(args);
        if let Some(hwnd) = titlebar::hwnd(cc) {
            match variant {
                titlebar::Variant::Native => titlebar::dark_caption(hwnd),
                titlebar::Variant::Hybrid => titlebar::install_hybrid(hwnd, cc.egui_ctx.clone()),
                titlebar::Variant::Custom => {}
            }
        }
        Self {
            item_file,
            aspects,
            view,
            columns: args.get("cols", 5),
            layout: Layout::default(),
            layout_key: (view, 0, 0),
            layout_ms: 0.0,
            scroll: 0.0,
            last_scroll: 0.0,
            streamer,
            bench,
            last_frame: None,
            paths,
            start_item: args
                .0
                .contains_key("start-item")
                .then(|| args.get("start-item", 0usize)),
            look,
            glass: args.str("glass", "on") == "on",
            bar_passes: args.get("bar-passes", 4),
            panel_passes: args.get("panel-passes", 4),
            panel_open: args.str("panel", "off") == "on",
            titlebar: variant,
            overlay: args.str("overlay", "on") == "on",
            screenshot: args.0.get("screenshot").cloned(),
            screenshot_after: args.get("screenshot-after", 4.0),
            screenshot_requested: None,
            created: Instant::now(),
            nudge: args.get("nudge", 0.0),
            bare: args.str("bare", "off") == "on",
        }
    }

    fn gap(&self) -> f32 {
        if self.columns >= 8 { 4.0 } else { 8.0 }
    }

    fn radius(&self) -> u8 {
        if self.columns >= 8 { 2 } else { 4 }
    }
}

impl eframe::App for GridApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let frame_start = Instant::now();
        let interval = self
            .last_frame
            .map(|t| frame_start.duration_since(t).as_secs_f32());
        self.last_frame = Some(frame_start);
        let ctx = ui.ctx().clone();
        self.streamer.begin_frame(&ctx, frame.wgpu_render_state());

        // Regions: sidebar, gallery column, toolbar over the top of the gallery (the gallery scrolls under it).
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, DARKROOM);
        let (gallery, toolbar_h) = if self.look {
            (
                Rect::from_min_max(
                    Pos2::new(rect.left() + look::SIDEBAR_WIDTH, rect.top()),
                    rect.max,
                ),
                look::TOOLBAR_HEIGHT,
            )
        } else {
            (rect, 0.0)
        };
        let top_inset = toolbar_h + PADDING;
        let content_width = gallery.width() - 2.0 * PADDING;
        let view_h = gallery.height();

        // Input: wheel scrolls, Ctrl + wheel (zoom) changes the column count, F12 takes a screenshot.
        let (wheel, zoom, keys, shot) = ctx.input(|i| {
            (
                i.smooth_scroll_delta.y,
                i.zoom_delta(),
                (
                    i.key_pressed(egui::Key::Minus),
                    i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals),
                ),
                i.key_pressed(egui::Key::F12),
            )
        });
        let old_cols = self.columns;
        if zoom > 1.0 || keys.1 {
            self.columns = (self.columns - 1).max(2);
        } else if zoom < 1.0 || keys.0 {
            self.columns = (self.columns + 1).min(12);
        }
        self.scroll -= wheel;
        self.handle_screenshots(&ctx, shot);

        // Layout, recomputed when width, view or zoom change. Keep the first visible item in place on zoom.
        let key = (self.view, self.columns, content_width as u32);
        if key != self.layout_key {
            // Layout coordinates start below the toolbar and padding.
            let anchor = layout::first_item_from(&self.layout, self.scroll - top_inset);
            let anchor_offset = self.layout.top.get(anchor).map(|t| t - self.scroll);
            let start = Instant::now();
            self.layout = layout::tile_layout(
                self.view,
                &self.aspects,
                content_width,
                self.columns,
                self.gap(),
            );
            self.layout_ms = start.elapsed().as_secs_f32() * 1000.0;
            if old_cols != self.columns
                && let Some(offset) = anchor_offset
            {
                self.scroll = self.layout.top[anchor] - offset;
            }
            if let Some(item) = self.start_item.take() {
                self.scroll = self.layout.top[item.min(self.item_file.len() - 1)] + self.nudge;
            }
            self.layout_key = key;
        }

        if let Some(bench) = &mut self.bench
            && bench.warm
            && bench.warming.is_none()
            && self.layout.total_height > 0.0
        {
            let (from, to) = layout::items_in_view(
                &self.layout,
                self.scroll - view_h,
                self.scroll + bench.distance() + 3.0 * view_h,
            );
            let mut files: Vec<u32> = self.item_file[from..to].to_vec();
            files.sort_unstable();
            files.dedup();
            let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
            bench.warming = Some(done.clone());
            let paths = self.paths.clone();
            std::thread::spawn(move || {
                use rayon::prelude::*;
                let start = Instant::now();
                files.par_iter().for_each(|&f| {
                    let _ = std::fs::read(&paths[f as usize]);
                });
                println!(
                    "warmed {} files in {:.1} s",
                    files.len(),
                    start.elapsed().as_secs_f32()
                );
                done.store(true, std::sync::atomic::Ordering::Relaxed);
            });
        }
        if let Some(bench) = &mut self.bench {
            bench.advance(&mut self.scroll, interval, &ctx);
        }
        let max_scroll = (self.layout.total_height + top_inset + PADDING - view_h).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max_scroll);
        let direction = if self.scroll >= self.last_scroll {
            1.0
        } else {
            -1.0
        };
        self.last_scroll = self.scroll;

        // Thumbnail width as `pickThumbWidth` would choose it, from the column width in pixels.
        let needed =
            layout::column_width(content_width, self.columns, self.gap()) * ctx.pixels_per_point();
        let thumb_width = if needed <= 400.0 { 400 } else { 800 };
        self.streamer
            .target
            .store(thumb_width, std::sync::atomic::Ordering::Relaxed);

        // Visible tiles (in layout coordinates), then a prefetch band of 1.5 screens ahead and 0.5 behind.
        let (top, bottom) = (self.scroll - top_inset, self.scroll - top_inset + view_h);
        let (vs, ve) = layout::items_in_view(&self.layout, top, bottom);
        let (ahead_top, ahead_bottom) = if direction > 0.0 {
            (bottom, bottom + 1.5 * view_h)
        } else {
            (top - 1.5 * view_h, top)
        };
        let (behind_top, behind_bottom) = if direction > 0.0 {
            (top - 0.5 * view_h, top)
        } else {
            (bottom, bottom + 0.5 * view_h)
        };
        let (as_, ae) = layout::items_in_view(&self.layout, ahead_top, ahead_bottom);
        let (bs, be) = layout::items_in_view(&self.layout, behind_top, behind_bottom);
        let ahead: Box<dyn Iterator<Item = usize>> = if direction > 0.0 {
            Box::new(as_..ae)
        } else {
            Box::new((as_..ae).rev())
        };
        let files = &self.item_file;
        self.streamer
            .request((vs..ve).chain(ahead).chain(bs..be).map(|i| files[i]));

        // Tiles.
        let origin = Pos2::new(
            gallery.left() + PADDING,
            gallery.top() + top_inset - self.scroll,
        );
        let layout = &self.layout;
        let tile_rect = move |i: usize| {
            Rect::from_min_size(
                origin + Vec2::new(layout.left[i], layout.top[i]),
                Vec2::new(layout.width[i], layout.height[i]),
            )
        };
        let painter = ui.painter().with_clip_rect(gallery);
        let radius = CornerRadius::same(self.radius());
        let (mut shown, mut placeholders) = (0usize, 0usize);
        for i in vs..ve {
            let r = tile_rect(i);
            if !r.intersects(gallery) {
                continue;
            }
            shown += 1;
            match self.streamer.get(self.item_file[i]) {
                Some(id) => {
                    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                    painter.add(
                        egui::epaint::RectShape::filled(r, radius, Color32::WHITE)
                            .with_texture(id, uv),
                    );
                }
                None => {
                    placeholders += 1;
                    painter.rect_filled(r, radius, RAISED);
                }
            }
        }

        // Scrollbar: thin pill, white at 24 %, its track starts below the toolbar.
        if max_scroll > 0.0 {
            let track = Rect::from_min_max(
                Pos2::new(gallery.right() - 8.0, gallery.top() + toolbar_h + 4.0),
                Pos2::new(gallery.right() - 2.0, gallery.bottom() - 4.0),
            );
            let thumb_h = (track.height() * view_h / (max_scroll + view_h)).max(24.0);
            let y = track.top() + (track.height() - thumb_h) * (self.scroll / max_scroll);
            let thumb = Rect::from_min_size(
                Pos2::new(track.left(), y),
                Vec2::new(track.width(), thumb_h),
            );
            ui.painter()
                .rect_filled(thumb, CornerRadius::same(3), Color32::from_white_alpha(61));
        }

        if self.look && self.bare {
            // Gallery only, for the CSS comparison; print where the chrome would go.
            let toolbar = Rect::from_min_size(
                gallery.min,
                Vec2::new(gallery.width(), look::TOOLBAR_HEIGHT),
            );
            println!("toolbar {:?}", toolbar);
        } else if self.look {
            let rects: Vec<(usize, Rect)> = (vs..ve).map(|i| (i, tile_rect(i))).collect();
            self.chrome(ui, frame, rect, gallery, &rects);
        }

        let cpu_ms = frame_start.elapsed().as_secs_f32() * 1000.0;
        if let Some(bench) = &mut self.bench {
            bench.record(
                interval,
                cpu_ms,
                shown,
                placeholders,
                &self.streamer,
                self.layout_ms,
                &ctx,
            );
        } else if self.overlay {
            let text = format!(
                "{} cols · {}–{} of {} · {:.1} ms cpu · {} textures, {} MB · layout {:.1} ms",
                self.columns,
                vs,
                ve,
                self.item_file.len(),
                cpu_ms,
                self.streamer.cached(),
                self.streamer.texture_bytes() >> 20,
                self.layout_ms
            );
            ui.painter().text(
                rect.right_bottom() + Vec2::new(-16.0, -8.0),
                egui::Align2::RIGHT_BOTTOM,
                text,
                egui::FontId::monospace(12.0),
                Color32::WHITE,
            );
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        DARKROOM.to_normalized_gamma_f32()
    }
}

impl GridApp {
    /// Sidebar, glass toolbar, optional glass popover and caption buttons. `tiles` are the visible tiles.
    fn chrome(
        &mut self,
        ui: &mut egui::Ui,
        frame: &eframe::Frame,
        rect: Rect,
        gallery: Rect,
        tiles: &[(usize, Rect)],
    ) {
        let ctx = ui.ctx().clone();
        look::sidebar(
            ui.painter(),
            Rect::from_min_max(rect.min, Pos2::new(gallery.left(), rect.bottom())),
        );
        let toolbar = Rect::from_min_size(
            gallery.min,
            Vec2::new(gallery.width(), look::TOOLBAR_HEIGHT),
        );
        let reserved = if self.titlebar == titlebar::Variant::Native {
            0.0
        } else {
            3.0 * titlebar::BUTTON_WIDTH
        };

        // Glass needs the native textures of the tiles below it.
        let glass_tiles = |region: Rect, passes: u32| -> Vec<glass::GlassTile> {
            let area = region.expand(4.0 * (1u32 << passes) as f32);
            tiles
                .iter()
                .filter(|(_, r)| r.intersects(area))
                .map(|&(i, r)| glass::GlassTile {
                    rect: r,
                    view: self.streamer.view(self.item_file[i]),
                })
                .collect()
        };
        let use_glass = self.glass && frame.wgpu_render_state().is_some();
        if use_glass {
            glass::Glass {
                slot: 0,
                region: toolbar,
                tiles: glass_tiles(toolbar, self.bar_passes),
                background: DARKROOM.to_normalized_gamma_f32(),
                placeholder: RAISED.to_normalized_gamma_f32(),
                passes: self.bar_passes,
                tint: [28.0 / 255.0, 28.0 / 255.0, 28.0 / 255.0, 0.72],
                saturate: 1.6,
                radius: 0.0,
            }
            .paint(ui.painter());
        } else {
            ui.painter().rect_filled(toolbar, 0.0, RAISED);
        }
        ui.painter().hline(
            toolbar.x_range(),
            toolbar.top() + 0.5,
            egui::Stroke::new(1.0, look::RIM),
        );
        let view_button = look::toolbar(ui.painter(), toolbar, reserved);
        if ui
            .interact(view_button, egui::Id::new("view"), egui::Sense::click())
            .clicked()
        {
            self.panel_open = !self.panel_open;
        }

        if self.panel_open {
            let panel = Rect::from_min_size(
                Pos2::new(view_button.right() - 240.0, toolbar.bottom() + 4.0),
                Vec2::new(240.0, 250.0),
            );
            if self.screenshot.is_some() && self.screenshot_requested.is_none() {
                println!("panel {panel:?}");
            }
            let shadow = egui::epaint::Shadow {
                offset: [0, 12],
                blur: 32,
                spread: 0,
                color: Color32::from_black_alpha(102),
            };
            ui.painter()
                .add(shadow.as_shape(panel, CornerRadius::same(12)));
            if use_glass {
                glass::Glass {
                    slot: 1,
                    region: panel,
                    tiles: glass_tiles(panel, self.panel_passes),
                    background: DARKROOM.to_normalized_gamma_f32(),
                    placeholder: RAISED.to_normalized_gamma_f32(),
                    passes: self.panel_passes,
                    tint: [36.0 / 255.0, 36.0 / 255.0, 36.0 / 255.0, 0.86],
                    saturate: 1.4,
                    radius: 12.0,
                }
                .paint(ui.painter());
            } else {
                ui.painter()
                    .rect_filled(panel, CornerRadius::same(12), RAISED);
            }
            ui.painter().rect_stroke(
                panel,
                CornerRadius::same(12),
                egui::Stroke::new(1.0, look::RIM),
                egui::StrokeKind::Outside,
            );
            look::panel(ui.painter(), panel);
        }

        if self.titlebar != titlebar::Variant::Native {
            let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            // Empty toolbar space drags the window: between the search field and Filters.
            let drag = Rect::from_min_max(
                Pos2::new(toolbar.left() + 640.0, toolbar.top()),
                Pos2::new(view_button.left() - 110.0, toolbar.bottom()),
            );
            if self.titlebar == titlebar::Variant::Custom {
                let resp = ui.interact(drag, egui::Id::new("drag"), egui::Sense::click_and_drag());
                if resp.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if resp.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
            }
            titlebar::caption_buttons(ui, toolbar, drag, self.titlebar, maximized);
        }
    }

    /// F12, or `--screenshot NAME` after `--screenshot-after` seconds: saves `results/<name>.png`.
    fn handle_screenshots(&mut self, ctx: &egui::Context, key: bool) {
        let auto = self.screenshot.is_some()
            && self.screenshot_requested.is_none()
            && self.created.elapsed().as_secs_f32() > self.screenshot_after;
        if key || auto {
            self.screenshot_requested = Some(Instant::now());
            let name = self
                .screenshot
                .clone()
                .unwrap_or_else(|| format!("shot-{}", self.created.elapsed().as_millis()));
            let close = self.screenshot.is_some();
            let ctx2 = ctx.clone();
            ctx.request_screenshot(move |image: Arc<egui::ColorImage>| {
                let [w, h] = image.size;
                let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                std::fs::create_dir_all("results").unwrap();
                let path = format!("results/{name}.png");
                image::save_buffer(
                    &path,
                    &rgba,
                    w as u32,
                    h as u32,
                    image::ExtendedColorType::Rgba8,
                )
                .unwrap();
                println!("saved {path}");
                if close {
                    ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        }
        if self.screenshot.is_some() {
            ctx.request_repaint();
        }
    }
}

/// `--probe N`: single-threaded read and decode timings for N files, no window.
fn probe(args: &Args) {
    let dir = PathBuf::from(format!(
        "{}\\gg-spike\\thumbs",
        std::env::var("TEMP").unwrap()
    ));
    let n: usize = args.get("probe", 500);
    let skip: usize = args.get("skip", 0);
    let mut paths = Vec::new();
    for a in std::fs::read_dir(&dir).unwrap().flatten() {
        for b in std::fs::read_dir(a.path()).unwrap().flatten() {
            for f in std::fs::read_dir(b.path()).unwrap().flatten() {
                paths.push(f.path());
            }
        }
        if paths.len() >= skip + n {
            break;
        }
    }
    let paths = &paths[skip.min(paths.len())..(skip + n).min(paths.len())];
    let t = Instant::now();
    let data: Vec<Vec<u8>> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
    let read = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    let t = Instant::now();
    let _: Vec<Vec<u8>> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
    let reread = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    let kb = data.iter().map(Vec::len).sum::<usize>() as f64 / n as f64 / 1024.0;
    let t = Instant::now();
    let px: usize = data
        .iter()
        .map(|d| {
            webp::Decoder::new(d)
                .decode()
                .map_or(0, |i| (i.width() * i.height()) as usize)
        })
        .sum();
    let libwebp = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    let t = Instant::now();
    for d in &data {
        let _ =
            image::load_from_memory_with_format(d, image::ImageFormat::WebP).map(|i| i.to_rgba8());
    }
    let image_rs = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    for target in [800u32, 400] {
        let t = Instant::now();
        for d in &data {
            let _ = stream::decode_scaled(d, target);
        }
        println!(
            "scaled {target}: {:.2} ms",
            t.elapsed().as_secs_f64() * 1000.0 / n as f64
        );
    }
    println!(
        "{n} files, {kb:.0} KB, {:.0} kpx avg: read {read:.2} ms, re-read {reread:.2} ms, libwebp {libwebp:.2} ms, image {image_rs:.2} ms",
        px as f64 / n as f64 / 1000.0
    );
}

fn titlebar_variant(args: &Args) -> titlebar::Variant {
    match args.str("titlebar", "hybrid").as_str() {
        "native" => titlebar::Variant::Native,
        "custom" => titlebar::Variant::Custom,
        _ => titlebar::Variant::Hybrid,
    }
}

fn main() -> eframe::Result {
    let args = Args::parse();
    if args.0.contains_key("probe") {
        probe(&args);
        return Ok(());
    }
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Spike · grid")
            .with_inner_size([1600.0, 960.0])
            .with_maximized(args.0.contains_key("bench") || args.str("maximized", "off") == "on")
            .with_decorations(titlebar_variant(&args) != titlebar::Variant::Custom),
        ..Default::default()
    };
    if args.str("vsync", "on") == "off" {
        options.wgpu_options.surface.present_mode = eframe::wgpu::PresentMode::AutoNoVsync;
    }
    eframe::run_native(
        "spike-grid",
        options,
        Box::new(move |cc| Ok(Box::new(GridApp::new(cc, &args)))),
    )
}
