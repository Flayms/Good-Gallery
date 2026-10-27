//! The gallery (spec 006): the whole search result laid out up front, tile data loaded for the visible range, and
//! only the tiles near the viewport drawn, with their thumbnails streamed from the thumbnail cache.

use crate::icons::Icon;
use crate::textures::{TextureRequest, TextureStreamer};
use crate::{empty_state, format, theme};
use base64::Engine as _;
use db::gallery::{GalleryItem, GalleryLayout, GallerySearch, ThumbStatus};
use db::rusqlite::Connection;
use domain::layout::{LayoutOptions, TileLayout, TileView, tile_layout};
use domain::thumbs::{ThumbKey, pick_thumb_width};
use domain::zoom;
use eframe::egui::{self, Color32, Rect, Vec2, pos2, vec2};
use indexer::thumb_cache::ThumbCache;
use indexer::thumbnails::{ThumbnailReady, ThumbnailService};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Tile data is loaded in chunks of consecutive gallery positions.
const CHUNK: usize = 200;
/// Above this many remembered items, the memory of loaded ones is dropped.
const MAX_REMEMBERED: usize = 20_000;
/// Content drawn (and tile data loaded) above and below the viewport, in points.
const OVERSCAN: f32 = 600.0;
/// Thumbnails are requested this many screens ahead and behind (REQ-006-13).
const PREFETCH_AHEAD: f32 = 1.5;
const PREFETCH_BEHIND: f32 = 0.5;
const GAP: f32 = 8.0;
const PADDING: f32 = 12.0;
/// Placeholder textures made per frame; more wait for the next frame, so fast flings don't stall it.
const PLACEHOLDERS_PER_FRAME: usize = 48;
const MAX_PLACEHOLDERS: usize = 4_000;
/// Settings key of the zoom level, as the Electron version's `localStorage` key.
const COLUMNS_KEY: &str = "gallery.columns";
/// How long a new window width must stay before the gallery is laid out for it.
const RESIZE_SETTLE: std::time::Duration = std::time::Duration::from_millis(120);

enum Request {
    Layout {
        generation: u64,
        search: GallerySearch,
    },
    Items {
        generation: u64,
        chunk: usize,
        ids: Vec<i64>,
    },
}

enum Loaded {
    Layout {
        generation: u64,
        result: Result<GalleryLayout, String>,
    },
    Items {
        generation: u64,
        chunk: usize,
        result: Result<Vec<GalleryItem>, String>,
    },
}

/// Runs the gallery's queries on its own thread, so a 500k-row layout never stalls a frame.
struct Loader {
    requests: Sender<Request>,
    results: Receiver<Loaded>,
    /// Requests sent and not yet applied.
    pending: usize,
}

impl Loader {
    fn start(conn: Arc<Mutex<Connection>>, ctx: Option<egui::Context>) -> Self {
        let (requests, inbox) = channel::<Request>();
        let (outbox, results) = channel();
        std::thread::Builder::new()
            .name("gallery".into())
            .spawn(move || {
                while let Ok(request) = inbox.recv() {
                    let conn = conn.lock().unwrap_or_else(|p| p.into_inner());
                    let loaded = match request {
                        Request::Layout { generation, search } => Loaded::Layout {
                            generation,
                            result: db::gallery::layout(&conn, &search).map_err(|e| e.to_string()),
                        },
                        Request::Items { generation, chunk, ids } => Loaded::Items {
                            generation,
                            chunk,
                            result: db::gallery::items(&conn, &ids).map_err(|e| e.to_string()),
                        },
                    };
                    drop(conn);
                    if outbox.send(loaded).is_err() {
                        return;
                    }
                    if let Some(ctx) = &ctx {
                        ctx.request_repaint();
                    }
                }
            })
            .expect("can't start the gallery thread");
        Self {
            requests,
            results,
            pending: 0,
        }
    }

    fn send(&mut self, request: Request) {
        if self.requests.send(request).is_ok() {
            self.pending += 1;
        }
    }
}

/// The layout computed for one set of inputs; recomputed when any of them changes.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LayoutKey {
    /// Counts the layouts loaded, so a reload only lays out again once its data has arrived.
    data: u64,
    width: f32,
    columns: usize,
    view: TileView,
}

/// Thumbnail rendering and textures; absent in tests without a GPU.
pub struct Thumbnails {
    pub cache: ThumbCache,
    pub service: Option<Arc<ThumbnailService>>,
    pub ready: Receiver<ThumbnailReady>,
    pub streamer: TextureStreamer,
}

pub struct Gallery {
    /// Where view choices are stored (the UI's write connection).
    settings: Arc<Mutex<Connection>>,
    loader: Loader,
    search: GallerySearch,
    generation: u64,
    /// Whether the first layout was asked for (on first use, so a gallery replaced before it shows never queries).
    started: bool,
    data: Option<Arc<GalleryLayout>>,
    data_version: u64,
    error: Option<String>,
    items: HashMap<i64, GalleryItem>,
    /// Chunks of the current data asked for.
    chunks: HashSet<usize>,
    layout: Option<(LayoutKey, TileLayout)>,
    pub columns: usize,
    pub view: TileView,
    /// First visible item, by position and by media id: kept in view when the layout changes, also when a reload moved
    /// it to another position (REQ-006-07).
    anchor: usize,
    anchor_id: Option<i64>,
    /// A new width waiting to settle: a resize lays out the whole result only once it stopped.
    pending_width: Option<(f32, Instant)>,
    wheel: f32,
    placeholders: HashMap<i64, egui::TextureHandle>,
    thumbnails: Option<Thumbnails>,
    /// Media items whose thumbnail is being rendered on request.
    awaiting: HashSet<i64>,
    /// Media items whose thumbnail couldn't be rendered or stored.
    failed: HashSet<i64>,
    /// Where the tiles start on screen, below the toolbar.
    content_top: f32,
    /// The scroll offset of the last frame, and one to apply in the next.
    offset: f32,
    scroll_to: Option<f32>,
}

impl Gallery {
    /// The gallery reading from `reader` and storing view choices through `settings`; loads its layout when first
    /// used. `ctx` lets the loader wake the UI.
    pub fn new(reader: Arc<Mutex<Connection>>, settings: Arc<Mutex<Connection>>, ctx: Option<egui::Context>) -> Self {
        let columns = {
            let conn = settings.lock().unwrap_or_else(|p| p.into_inner());
            let stored = db::settings::value(&conn, COLUMNS_KEY).ok().flatten();
            zoom::clamp_columns(stored.and_then(|value| value.as_f64()))
        };
        Self {
            loader: Loader::start(reader, ctx),
            settings,
            search: GallerySearch::default(),
            generation: 0,
            started: false,
            data: None,
            data_version: 0,
            error: None,
            items: HashMap::new(),
            chunks: HashSet::new(),
            layout: None,
            columns,
            view: TileView::Masonry,
            anchor: 0,
            anchor_id: None,
            pending_width: None,
            wheel: 0.0,
            placeholders: HashMap::new(),
            thumbnails: None,
            awaiting: HashSet::new(),
            failed: HashSet::new(),
            content_top: 0.0,
            offset: 0.0,
            scroll_to: None,
        }
    }

    fn start(&mut self) {
        if !self.started {
            self.started = true;
            self.reload();
        }
    }

    /// Streams thumbnails from the cache and asks `service` to render missing ones.
    pub fn with_thumbnails(mut self, thumbnails: Thumbnails) -> Self {
        self.thumbnails = Some(thumbnails);
        self
    }

    /// Loads the layout again (new media items while indexing, REQ-006-11). The current one stays shown meanwhile.
    pub fn reload(&mut self) {
        if !self.started {
            return;
        }
        self.generation += 1;
        self.loader.send(Request::Layout {
            generation: self.generation,
            search: self.search,
        });
    }

    /// The number of media items in the current layout, once loaded.
    pub fn count(&self) -> Option<usize> {
        self.data.as_ref().map(|data| data.ids.len())
    }

    /// The first item starting below the toolbar, kept there when the layout changes (REQ-006-07).
    pub fn first_visible(&self) -> usize {
        self.anchor
    }

    /// Where the tiles start on screen, below the toolbar.
    pub fn content_top(&self) -> f32 {
        self.content_top
    }

    /// The scroll offset in points.
    pub fn offset(&self) -> f32 {
        self.offset
    }

    /// Scrolls to `offset` in the next frame (benchmarks).
    pub fn scroll_to(&mut self, offset: f32) {
        self.scroll_to = Some(offset);
    }

    /// GPU memory of the loaded thumbnails, in bytes.
    pub fn texture_bytes(&self) -> usize {
        self.thumbnails.as_ref().map_or(0, |t| t.streamer.texture_bytes())
    }

    pub fn is_loading(&self) -> bool {
        self.loader.pending > 0
    }

    /// Applies finished queries and answered thumbnail requests. Called every frame.
    pub fn poll(&mut self) {
        self.start();
        while let Ok(loaded) = self.loader.results.try_recv() {
            self.apply(loaded);
        }
        let Some(thumbnails) = &mut self.thumbnails else { return };
        while let Ok(ready) = thumbnails.ready.try_recv() {
            self.awaiting.remove(&ready.media);
            if ready.stored {
                thumbnails.streamer.retry(ready.media);
            } else {
                self.failed.insert(ready.media);
            }
        }
    }

    /// Waits for all queries sent so far and applies them (tests).
    pub fn wait_for_loads(&mut self) {
        self.start();
        while self.loader.pending > 0 {
            let Ok(loaded) = self.loader.results.recv() else { return };
            self.apply(loaded);
        }
    }

    fn apply(&mut self, loaded: Loaded) {
        self.loader.pending -= 1;
        match loaded {
            Loaded::Layout { generation, result } => {
                if generation != self.generation {
                    return;
                }
                match result {
                    Ok(data) => {
                        self.data = Some(Arc::new(data));
                        self.data_version += 1;
                        self.error = None;
                        // Positions moved: chunks are loaded again (remembered items stay shown meanwhile), and
                        // thumbnails found missing may have been rendered since.
                        self.chunks.clear();
                        if let Some(thumbnails) = &mut self.thumbnails {
                            thumbnails.streamer.retry_all();
                        }
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            Loaded::Items {
                generation,
                chunk,
                result,
            } => match result {
                Ok(items) => {
                    if self.items.len() > MAX_REMEMBERED {
                        self.items.clear();
                        self.placeholders.clear();
                        // Forgotten chunks are asked for again when in view.
                        self.chunks.clear();
                    }
                    for item in items {
                        self.items.insert(item.id, item);
                    }
                }
                Err(error) => {
                    tracing::error!("can't load gallery items: {error}");
                    // Asked again when the range is next in view.
                    if generation == self.data_version {
                        self.chunks.remove(&chunk);
                    }
                }
            },
        }
    }

    /// Zooms by `steps` (positive adds columns) and remembers the level (REQ-006-06).
    pub fn zoom(&mut self, steps: i32) {
        let columns = zoom::step_columns(self.columns, steps);
        if columns == self.columns {
            return;
        }
        self.columns = columns;
        let conn = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        if let Err(error) = db::settings::set_value(&conn, COLUMNS_KEY, &serde_json::Value::from(columns)) {
            tracing::warn!("can't store the zoom level: {error}");
        }
    }

    /// Ctrl + wheel over the gallery zooms instead of scrolling; one step per notch, small touchpad movements add up
    /// (REQ-006-06).
    fn handle_zoom_wheel(&mut self, ui: &egui::Ui, area: Rect) {
        let hovered = ui.rect_contains_pointer(area);
        let deltas: Vec<f32> = ui.input(|input| {
            input
                .raw
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::MouseWheel {
                        unit, delta, modifiers, ..
                    } if modifiers.ctrl => Some(match unit {
                        // Scrolling down (content moving up) zooms out. winit reports one line per notch (and
                        // several when a fast spin is coalesced); a page counts as one step.
                        egui::MouseWheelUnit::Point => -delta.y,
                        egui::MouseWheelUnit::Line => -delta.y * zoom::WHEEL_STEP,
                        egui::MouseWheelUnit::Page => -delta.y.signum() * zoom::WHEEL_STEP,
                    }),
                    _ => None,
                })
                .collect()
        });
        if !hovered {
            return;
        }
        for delta in deltas {
            let (steps, rest) = zoom::accumulate_wheel(self.wheel, delta);
            self.wheel = rest;
            if steps != 0 {
                self.zoom(steps);
            }
        }
    }

    /// The width to lay out for: a new width only once it has stayed the same for a moment, so dragging the window
    /// edge doesn't lay out the whole result every frame.
    fn settled_width(&mut self, ui: &egui::Ui, width: f32) -> f32 {
        let Some((key, _)) = &self.layout else { return width };
        if key.width == width {
            self.pending_width = None;
            return width;
        }
        let now = Instant::now();
        match self.pending_width {
            Some((pending, since)) if pending == width && now - since >= RESIZE_SETTLE => {
                self.pending_width = None;
                width
            }
            Some((pending, _)) if pending == width => {
                ui.ctx().request_repaint_after(RESIZE_SETTLE);
                key.width
            }
            _ => {
                self.pending_width = Some((width, now));
                ui.ctx().request_repaint_after(RESIZE_SETTLE);
                key.width
            }
        }
    }

    /// Lays out the tiles for the current data, width and zoom, only when one of them changed; whether it did.
    fn update_layout(&mut self, width: f32) -> Option<bool> {
        let data = self.data.as_ref()?;
        let key = LayoutKey {
            data: self.data_version,
            width,
            columns: self.columns,
            view: self.view,
        };
        let changed = self.layout.as_ref().is_none_or(|(old, _)| *old != key);
        if changed {
            let layout = tile_layout(&LayoutOptions {
                view: self.view,
                aspects: &data.aspects,
                groups: &[],
                width: f64::from(width),
                columns: self.columns,
                gap: f64::from(GAP),
                header_height: 0.0,
            });
            self.layout = Some((key, layout));
        }
        Some(changed)
    }

    /// Asks for the tile data of items `start..end` not loaded yet.
    fn load_range(&mut self, start: usize, end: usize) {
        let Some(data) = self.data.clone() else { return };
        if start >= end {
            return;
        }
        for chunk in start / CHUNK..=(end - 1) / CHUNK {
            // Once per layout: remembered items stay shown while a refreshed layout loads them again.
            if self.chunks.insert(chunk) {
                let ids = &data.ids[chunk * CHUNK..((chunk + 1) * CHUNK).min(data.ids.len())];
                self.loader.send(Request::Items {
                    generation: self.data_version,
                    chunk,
                    ids: ids.to_vec(),
                });
            }
        }
    }

    /// Shows the gallery in `ui`.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.poll();
        if let Some(thumbnails) = &mut self.thumbnails {
            thumbnails.streamer.begin_frame();
        }
        let area = ui.max_rect();
        self.handle_zoom_wheel(ui, area);
        if let Some(error) = &self.error
            && self.data.is_none()
        {
            empty_state::error(ui, "Couldn't load media", error);
            return;
        }
        let Some(count) = self.count() else {
            skeleton(ui, self.columns);
            return;
        };
        if count == 0 {
            empty_state::no_media(ui);
            return;
        }

        let inset_top = theme::CONTROL_HEIGHT + 2.0 * PADDING;
        let width = self.settled_width(ui, (area.width() - 2.0 * PADDING).floor().max(0.0));
        // Where the first visible item is now: a reload may have moved it.
        let anchor = self
            .anchor_id
            .and_then(|id| self.data.as_ref()?.ids.iter().position(|&other| other == id))
            .unwrap_or(self.anchor);
        self.content_top = area.top() + inset_top;
        let scroll_to = self.scroll_to.take();
        let Some(changed) = self.update_layout(width) else {
            return;
        };
        // Out of `self` for the frame: drawing needs `&mut self`, and a copy would cost megabytes per frame.
        let Some((key, layout)) = self.layout.take() else {
            return;
        };
        let total = layout.total_height as f32;
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("gallery")
            .auto_shrink(false)
            // Ctrl + wheel zooms (handled above), it never scrolls.
            .scroll_source(egui::scroll_area::ScrollSource::ALL);
        // The anchor goes right below the toolbar, where it was.
        if changed && anchor > 0 {
            scroll = scroll.vertical_scroll_offset(layout.start_of(&[], anchor.min(count - 1)) as f32);
        }
        if let Some(offset) = scroll_to {
            scroll = scroll.vertical_scroll_offset(offset);
        }
        scroll.show_viewport(ui, |ui, viewport| {
            self.offset = viewport.min.y;
            ui.set_min_height(total + inset_top + PADDING);
            let origin = ui.min_rect().min + vec2(PADDING, inset_top);
            // The first item starting below the toolbar.
            self.anchor = layout.first_item_from(f64::from(viewport.min.y));
            self.anchor_id = self.data.as_ref().and_then(|data| data.ids.get(self.anchor).copied());
            self.draw_tiles(ui, &layout, viewport, origin, inset_top);
        });
        self.layout = Some((key, layout));
    }

    fn draw_tiles(
        &mut self,
        ui: &mut egui::Ui,
        layout: &TileLayout,
        viewport: Rect,
        origin: egui::Pos2,
        inset_top: f32,
    ) {
        let Some(data) = self.data.clone() else { return };
        let view_top = f64::from(viewport.min.y - inset_top);
        let view_bottom = f64::from(viewport.max.y - inset_top);
        let (start, end) = layout.items_in_view(view_top - f64::from(OVERSCAN), view_bottom + f64::from(OVERSCAN));
        self.load_range(start, end);

        let pixels_per_point = ui.ctx().pixels_per_point();
        let radius = if self.columns >= 8 { 2 } else { 4 };
        let now = Instant::now();
        let mut fading = false;
        let mut made = 0;
        for index in start..end {
            let rect = tile_rect(layout, index, origin);
            if f64::from(rect.max.y - origin.y) < view_top - f64::from(OVERSCAN) {
                continue;
            }
            let id = data.ids[index];
            let item = self.items.get(&id);
            let response = ui.interact(rect, ui.make_persistent_id(("tile", id)), egui::Sense::click());
            if let Some(item) = item {
                let label = tile_label(item);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &label));
            }
            if !ui.is_rect_visible(rect) {
                continue;
            }
            let painter = ui.painter();
            let Some(item) = item else {
                painter.rect_filled(rect, radius, theme::RAISED);
                continue;
            };
            // Placeholder: the ThumbHash's blurred colors, else gray.
            let placeholder = match self.placeholders.get(&id) {
                Some(texture) => Some(texture.id()),
                None if made < PLACEHOLDERS_PER_FRAME => {
                    made += 1;
                    make_placeholder(&mut self.placeholders, ui.ctx(), item)
                }
                None => None,
            };
            match placeholder {
                Some(texture) => {
                    painter.add(
                        egui::epaint::RectShape::filled(rect, radius, Color32::WHITE).with_texture(texture, full_uv()),
                    );
                }
                None => {
                    painter.rect_filled(rect, radius, theme::RAISED);
                }
            }
            let failed = item.thumb_status == ThumbStatus::Error
                || self.failed.contains(&id)
                || self.thumbnails.as_ref().is_some_and(|t| t.streamer.is_broken(id));
            let loaded = self.thumbnails.as_mut().and_then(|t| t.streamer.get(id));
            if let Some(loaded) = loaded {
                let t = (now - loaded.at).as_secs_f32() / theme::DURATION_BASE;
                fading |= t < 1.0;
                let tint = Color32::WHITE.gamma_multiply(t.clamp(0.0, 1.0));
                let uv = cover_uv(loaded.size, rect.size());
                painter.add(egui::epaint::RectShape::filled(rect, radius, tint).with_texture(loaded.id, uv));
            } else if failed {
                let icon = Rect::from_center_size(rect.center(), Vec2::splat(24.0));
                Icon::ImageOff.image(24.0, theme::MIST).paint_at(ui, icon);
            }
            if item.kind == domain::media::MediaKind::Video {
                video_badge(ui, rect, item.duration);
            }
        }
        if fading {
            ui.ctx().request_repaint();
        }
        self.request_textures(layout, &data, viewport, inset_top, pixels_per_point);
    }

    /// Asks for the thumbnails of visible tiles first, then of those 1.5 screens ahead and half a screen behind
    /// (REQ-005-07, REQ-006-13), and for rendering of those the cache doesn't have.
    fn request_textures(
        &mut self,
        layout: &TileLayout,
        data: &GalleryLayout,
        viewport: Rect,
        inset_top: f32,
        pixels_per_point: f32,
    ) {
        let Some(thumbnails) = &mut self.thumbnails else { return };
        let height = f64::from(viewport.height());
        let top = f64::from(viewport.min.y - inset_top);
        let bottom = f64::from(viewport.max.y - inset_top);
        let ranges = [
            layout.items_in_view(top, bottom),
            layout.items_in_view(bottom, bottom + height * f64::from(PREFETCH_AHEAD)),
            layout.items_in_view(top - height * f64::from(PREFETCH_BEHIND), top),
        ];
        let mut wanted = Vec::new();
        for (start, end) in ranges {
            for index in start..end {
                let Some(item) = self.items.get(&data.ids[index]) else {
                    continue;
                };
                if item.thumb_status == ThumbStatus::Error || self.failed.contains(&item.id) {
                    continue;
                }
                let width = pick_thumb_width(layout.width[index] as f32, pixels_per_point);
                wanted.push(TextureRequest {
                    media: item.id,
                    path: thumbnails.cache.path(&key_of(item), width),
                    width,
                });
            }
        }
        thumbnails.streamer.request(wanted);
        for media in thumbnails.streamer.take_missing() {
            let Some(service) = &thumbnails.service else { continue };
            if self
                .items
                .get(&media)
                .is_some_and(|item| item.thumb_status != ThumbStatus::Error)
                && self.awaiting.insert(media)
            {
                service.request(media);
            }
        }
    }
}

/// The ThumbHash placeholder of an item as a small texture (REQ-005-06).
fn make_placeholder(
    placeholders: &mut HashMap<i64, egui::TextureHandle>,
    ctx: &egui::Context,
    item: &GalleryItem,
) -> Option<egui::TextureId> {
    let hash = base64::engine::general_purpose::STANDARD
        .decode(item.thumbhash.as_ref()?)
        .ok()?;
    let (w, h, rgba) = thumbhash::thumb_hash_to_rgba(&hash).ok()?;
    let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
    if placeholders.len() >= MAX_PLACEHOLDERS {
        placeholders.clear();
    }
    let texture = ctx.load_texture(format!("placeholder {}", item.id), image, egui::TextureOptions::LINEAR);
    let id = texture.id();
    placeholders.insert(item.id, texture);
    Some(id)
}

fn key_of(item: &GalleryItem) -> ThumbKey {
    ThumbKey {
        root_id: item.root_id,
        rel_path: item.rel_path.clone(),
        size: item.size,
        mtime: item.mtime,
    }
}

fn tile_rect(layout: &TileLayout, index: usize, origin: egui::Pos2) -> Rect {
    Rect::from_min_size(
        origin + vec2(layout.left[index] as f32, layout.top[index] as f32),
        vec2(layout.width[index] as f32, layout.height[index] as f32),
    )
}

fn full_uv() -> Rect {
    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
}

/// The part of a texture that covers a tile without distortion, centered (`object-fit: cover`).
pub fn cover_uv([width, height]: [u32; 2], tile: Vec2) -> Rect {
    let texture = width as f32 / height as f32;
    let target = tile.x / tile.y;
    if texture > target {
        let w = target / texture;
        Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
    } else {
        let h = texture / target;
        Rect::from_min_max(pos2(0.0, (1.0 - h) / 2.0), pos2(1.0, (1.0 + h) / 2.0))
    }
}

/// What a screen reader says for a tile: the file name, for videos with the duration.
fn tile_label(item: &GalleryItem) -> String {
    match (item.kind, item.duration) {
        (domain::media::MediaKind::Video, Some(duration)) => {
            format!("{}, video, {}", item.file_name, format::duration(duration))
        }
        (domain::media::MediaKind::Video, None) => format!("{}, video", item.file_name),
        _ => item.file_name.clone(),
    }
}

/// A play badge with the duration in the tile's bottom-right corner (REQ-006-09).
fn video_badge(ui: &egui::Ui, tile: Rect, duration: Option<f64>) {
    let text = duration.map(format::duration);
    let font = egui::FontId::new(theme::SIZE_CAPTION, theme::numbers());
    let galley = text.map(|text| ui.painter().layout_no_wrap(text, font, Color32::WHITE));
    let icon = 12.0;
    let text_width = galley.as_ref().map_or(0.0, |g| g.size().x + 4.0);
    let size = vec2(6.0 + icon + text_width + 6.0, 20.0);
    let rect = Rect::from_min_size(tile.max - size - vec2(6.0, 6.0), size);
    if rect.min.x < tile.min.x || rect.min.y < tile.min.y {
        return;
    }
    ui.painter()
        .rect_filled(rect, 4, Color32::from_rgba_unmultiplied(0, 0, 0, 153));
    let icon_rect = Rect::from_min_size(rect.min + vec2(6.0, (size.y - icon) / 2.0), Vec2::splat(icon));
    Icon::Play.image(icon, Color32::WHITE).paint_at(ui, icon_rect);
    if let Some(galley) = galley {
        let pos = pos2(icon_rect.max.x + 4.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, Color32::WHITE);
    }
}

/// Gray tiles while the layout loads (REQ-006-10).
fn skeleton(ui: &egui::Ui, columns: usize) {
    let area = ui.max_rect();
    let width = (area.width() - 2.0 * PADDING).max(0.0);
    let side = domain::layout::column_width(f64::from(width), columns, f64::from(GAP)) as f32;
    if side <= 0.0 {
        return;
    }
    let top = area.top() + theme::CONTROL_HEIGHT + 2.0 * PADDING;
    for row in 0.. {
        let y = top + row as f32 * (side + GAP);
        if y > area.bottom() {
            break;
        }
        for column in 0..columns {
            let min = pos2(area.left() + PADDING + column as f32 * (side + GAP), y);
            ui.painter()
                .rect_filled(Rect::from_min_size(min, Vec2::splat(side)), 4, theme::RAISED);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_textures_to_cover_the_tile_centered() {
        // A 2:1 texture in a square tile: the middle half of its width.
        let uv = cover_uv([200, 100], vec2(50.0, 50.0));
        assert_eq!(uv, Rect::from_min_max(pos2(0.25, 0.0), pos2(0.75, 1.0)));
        // A 1:2 texture in a square tile: the middle half of its height.
        let uv = cover_uv([100, 200], vec2(50.0, 50.0));
        assert_eq!(uv, Rect::from_min_max(pos2(0.0, 0.25), pos2(1.0, 0.75)));
        assert_eq!(cover_uv([100, 100], vec2(30.0, 30.0)), full_uv());
    }
}
