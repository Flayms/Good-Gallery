//! The gallery (spec 006): the whole search result laid out up front, tile data loaded for the visible range, and
//! only what is near the viewport drawn: tiles (`tiles`) or list rows (`list`), with thumbnails streamed from the
//! thumbnail cache.

mod list;
mod tiles;

use crate::icons::Icon;
use crate::textures::{TextureRequest, TextureStreamer};
use crate::{empty_state, theme};
use base64::Engine as _;
use db::gallery::{GalleryItem, GalleryLayout, GallerySearch, ThumbStatus};
use db::rusqlite::Connection;
use domain::layout::{LayoutOptions, ListEntry, TileLayout, list_entry_at, list_index_of_item, tile_layout};
use domain::thumbs::{ThumbKey, pick_thumb_width};
use domain::view::{GalleryView, TagIndicatorChoice, group_by_day_value, groups_shown, is_grouped_by_day};
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
const PADDING: f32 = 12.0;
/// From this zoom level on, tiles are small: narrower gaps and corners (`docs/design.md`).
const DENSE_COLUMNS: usize = 8;
/// Height of a date group header in the tile views, including the space above it.
const GROUP_HEADER_HEIGHT: f32 = 40.0;
/// Height of the toolbar floating over the top of the gallery (`toolbar`).
pub const TOOLBAR_HEIGHT: f32 = 48.0;
/// Placeholder textures made per frame; more wait for the next frame, so fast flings don't stall it.
const PLACEHOLDERS_PER_FRAME: usize = 48;
const MAX_PLACEHOLDERS: usize = 4_000;
/// Settings keys of the view choices, as the Electron version's `localStorage` keys.
const COLUMNS_KEY: &str = "gallery.columns";
const VIEW_KEY: &str = "gallery.view";
const GROUP_BY_DAY_KEY: &str = "gallery.groupByDay";
const TAG_INDICATORS_KEY: &str = "gallery.tagIndicators";
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
    view: GalleryView,
    grouped: bool,
}

/// What the current frame draws from: the tile positions (empty in the List view) and the shown groups' starts.
struct Arranged {
    key: LayoutKey,
    tiles: TileLayout,
    groups: Vec<usize>,
}

/// Thumbnail rendering and textures; absent in tests without a GPU.
pub struct Thumbnails {
    pub cache: ThumbCache,
    pub service: Option<Arc<ThumbnailService>>,
    pub ready: Receiver<ThumbnailReady>,
    pub streamer: TextureStreamer,
}

/// Per-frame drawing state shared by tiles and rows.
struct FrameState {
    now: Instant,
    /// Placeholder textures made this frame.
    made: usize,
    /// A thumbnail is fading in: draw again next frame.
    fading: bool,
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
    arranged: Option<Arranged>,
    pub columns: usize,
    view: GalleryView,
    group_by_day: bool,
    indicators: TagIndicatorChoice,
    /// First visible item, by position and by media id: kept in view when the layout changes, also when a reload moved
    /// it to another position (REQ-006-07).
    anchor: usize,
    anchor_id: Option<i64>,
    /// The data version the anchor's position was last looked up for.
    anchor_version: u64,
    /// Width of the List view's rows, for the column header above them.
    list_width: f32,
    /// A new width waiting to settle: a resize lays out the whole result only once it stopped.
    pending_width: Option<(f32, Instant)>,
    wheel: f32,
    placeholders: HashMap<i64, egui::TextureHandle>,
    thumbnails: Option<Thumbnails>,
    /// Media items whose thumbnail is being rendered on request.
    awaiting: HashSet<i64>,
    /// Media items whose thumbnail couldn't be rendered or stored.
    failed: HashSet<i64>,
    /// Where the content starts on screen, below the toolbar (and the List view's column header).
    content_top: f32,
    /// The scroll offset of the last frame, and one to apply in the next.
    offset: f32,
    scroll_to: Option<f32>,
}

impl Gallery {
    /// The gallery reading from `reader` and storing view choices through `settings`; loads its layout when first
    /// used. `ctx` lets the loader wake the UI.
    pub fn new(reader: Arc<Mutex<Connection>>, settings: Arc<Mutex<Connection>>, ctx: Option<egui::Context>) -> Self {
        let (columns, view, group_by_day, indicators) = {
            let conn = settings.lock().unwrap_or_else(|p| p.into_inner());
            let value = |key| db::settings::value(&conn, key).ok().flatten();
            let text = |key| value(key).and_then(|v| v.as_str().map(String::from));
            (
                zoom::clamp_columns(value(COLUMNS_KEY).and_then(|v| v.as_f64())),
                GalleryView::parse(text(VIEW_KEY).as_deref()),
                is_grouped_by_day(text(GROUP_BY_DAY_KEY).as_deref()),
                TagIndicatorChoice::parse(value(TAG_INDICATORS_KEY).as_ref()),
            )
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
            arranged: None,
            columns,
            view,
            group_by_day,
            indicators,
            anchor: 0,
            anchor_id: None,
            anchor_version: 0,
            list_width: 0.0,
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

    /// Streams thumbnails from the cache and asks `service` to render missing ones.
    pub fn with_thumbnails(mut self, thumbnails: Thumbnails) -> Self {
        self.thumbnails = Some(thumbnails);
        self
    }

    fn start(&mut self) {
        if !self.started {
            self.started = true;
            self.reload();
        }
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

    /// The first item at the top of the content, kept there when the layout changes (REQ-006-07).
    pub fn first_visible(&self) -> usize {
        self.anchor
    }

    /// Where the content starts on screen, below the toolbar.
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

    pub fn view(&self) -> GalleryView {
        self.view
    }

    pub fn group_by_day(&self) -> bool {
        self.group_by_day
    }

    pub fn tag_indicators(&self) -> &TagIndicatorChoice {
        &self.indicators
    }

    /// Whether the gallery is sorted by a date, so it can be grouped by day (REQ-016-04).
    pub fn is_date_sort(&self) -> bool {
        self.search.sort_by.is_date()
    }

    fn store(&self, key: &str, value: serde_json::Value) {
        let conn = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        if let Err(error) = db::settings::set_value(&conn, key, &value) {
            tracing::warn!("can't store {key}: {error}");
        }
    }

    /// Shows the gallery in another view and remembers it (REQ-006-01).
    pub fn set_view(&mut self, view: GalleryView) {
        if view != self.view {
            self.view = view;
            self.store(VIEW_KEY, view.key().into());
        }
    }

    /// Shows or hides the day groups and remembers it (REQ-016-03).
    pub fn set_group_by_day(&mut self, on: bool) {
        if on != self.group_by_day {
            self.group_by_day = on;
            self.store(GROUP_BY_DAY_KEY, group_by_day_value(on).into());
        }
    }

    /// Shows or hides tag indicators, or changes the picked ones, and remembers it (REQ-017-03).
    pub fn set_tag_indicators(&mut self, choice: TagIndicatorChoice) {
        if choice != self.indicators {
            self.store(TAG_INDICATORS_KEY, choice.to_value());
            self.indicators = choice;
        }
    }

    /// Zooms by `steps` (positive adds columns) and remembers the level (REQ-006-06).
    pub fn zoom(&mut self, steps: i32) {
        self.set_columns(zoom::step_columns(self.columns, steps));
    }

    pub fn set_columns(&mut self, columns: usize) {
        let columns = columns.clamp(zoom::MIN_COLUMNS, zoom::MAX_COLUMNS);
        if columns != self.columns {
            self.columns = columns;
            self.store(COLUMNS_KEY, columns.into());
        }
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
        if !hovered || self.view == GalleryView::List {
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
        let Some(arranged) = &self.arranged else { return width };
        let current = arranged.key.width;
        if current == width {
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
                current
            }
            _ => {
                self.pending_width = Some((width, now));
                ui.ctx().request_repaint_after(RESIZE_SETTLE);
                current
            }
        }
    }

    fn gap(&self) -> f32 {
        if self.columns >= DENSE_COLUMNS { 4.0 } else { 8.0 }
    }

    /// Lays out the current data for the width, zoom, view and grouping, only when one of them changed; whether it
    /// did.
    fn update_layout(&mut self, width: f32) -> Option<bool> {
        let data = self.data.as_ref()?;
        let grouped = groups_shown(self.group_by_day, self.is_date_sort());
        let key = LayoutKey {
            data: self.data_version,
            width,
            columns: self.columns,
            view: self.view,
            grouped,
        };
        if self.arranged.as_ref().is_some_and(|arranged| arranged.key == key) {
            return Some(false);
        }
        let groups: Vec<usize> = if grouped {
            data.groups.iter().map(|group| group.start).collect()
        } else {
            Vec::new()
        };
        let tiles = match self.view.tiles() {
            Some(view) => tile_layout(&LayoutOptions {
                view,
                aspects: &data.aspects,
                groups: &groups,
                width: f64::from(width),
                columns: self.columns,
                gap: f64::from(self.gap()),
                header_height: f64::from(GROUP_HEADER_HEIGHT),
            }),
            None => TileLayout::default(),
        };
        self.arranged = Some(Arranged { key, tiles, groups });
        Some(true)
    }

    /// Asks for the tile data of items `start..end` not loaded yet.
    fn load_range(&mut self, start: usize, end: usize) {
        let Some(data) = self.data.clone() else { return };
        let end = end.min(data.ids.len());
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

    /// Where item `index` starts in the content: its group's header if it is the group's first item.
    fn start_of(&self, arranged: &Arranged, index: usize) -> f64 {
        if self.view == GalleryView::List {
            let entry = list_index_of_item(&arranged.groups, index);
            let first_of_group = arranged.groups.binary_search(&index).is_ok();
            list::ROWS.top(&arranged.groups, if first_of_group { entry - 1 } else { entry })
        } else {
            arranged.tiles.start_of(&arranged.groups, index)
        }
    }

    /// The first item at height `y` of the content.
    fn item_at(&self, arranged: &Arranged, count: usize, y: f64) -> usize {
        if self.view == GalleryView::List {
            match list_entry_at(&arranged.groups, list::ROWS.entry_at(&arranged.groups, count, y)) {
                ListEntry::Group(group) => arranged.groups[group],
                ListEntry::Item(item) => item,
            }
        } else {
            arranged.tiles.first_item_from(y)
        }
    }

    /// Shows the gallery in `ui`, leaving room at the top for the toolbar.
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
        let list = self.view == GalleryView::List;
        let Some(count) = self.count() else {
            if list {
                list::skeleton(ui);
            } else {
                tiles::skeleton(ui, self.columns, self.gap());
            }
            return;
        };
        if count == 0 {
            empty_state::no_media(ui);
            return;
        }

        let inset_top = if list {
            TOOLBAR_HEIGHT + list::COLUMN_HEADER_HEIGHT + 4.0
        } else {
            TOOLBAR_HEIGHT + PADDING
        };
        let width = self.settled_width(ui, (area.width() - 2.0 * PADDING).floor().max(0.0));
        // Where the first visible item is now: a reload may have moved it. Looked up once per new layout.
        if self.anchor_version != self.data_version {
            self.anchor_version = self.data_version;
            if let Some(position) = self
                .anchor_id
                .and_then(|id| self.data.as_ref()?.ids.iter().position(|&other| other == id))
            {
                self.anchor = position;
            }
        }
        let anchor = self.anchor;
        self.content_top = area.top() + inset_top;
        let scroll_to = self.scroll_to.take();
        let Some(changed) = self.update_layout(width) else {
            return;
        };
        // Out of `self` for the frame: drawing needs `&mut self`, and a copy would cost megabytes per frame.
        let Some(arranged) = self.arranged.take() else { return };
        let total = if list {
            list::ROWS.total(&arranged.groups, count) as f32
        } else {
            arranged.tiles.total_height as f32
        };
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("gallery")
            .auto_shrink(false)
            // Ctrl + wheel zooms (handled above), it never scrolls.
            .scroll_source(egui::scroll_area::ScrollSource::ALL);
        // The anchor goes right below the toolbar, where it was.
        if changed && anchor > 0 {
            scroll = scroll.vertical_scroll_offset(self.start_of(&arranged, anchor.min(count - 1)) as f32);
        }
        if let Some(offset) = scroll_to {
            scroll = scroll.vertical_scroll_offset(offset);
        }
        scroll.show_viewport(ui, |ui, viewport| {
            self.offset = viewport.min.y;
            ui.set_min_height(total + inset_top + PADDING);
            self.anchor = self.item_at(&arranged, count, f64::from(viewport.min.y));
            self.anchor_id = self.data.as_ref().and_then(|data| data.ids.get(self.anchor).copied());
            if list {
                let origin = ui.min_rect().min + vec2(0.0, inset_top);
                self.draw_list(ui, &arranged, viewport, origin, inset_top);
            } else {
                let origin = ui.min_rect().min + vec2(PADDING, inset_top);
                self.draw_tiles(ui, &arranged, viewport, origin, inset_top);
            }
        });
        self.arranged = Some(arranged);
        if list {
            list::column_header(ui, area, self.list_width);
        }
    }

    /// Paints a media item's thumbnail into `rect`: its placeholder, the thumbnail fading in over it, or the
    /// "no image" icon when it can't be had (REQ-005-06, REQ-005-14).
    fn paint_thumbnail(&mut self, ui: &egui::Ui, rect: Rect, radius: u8, id: i64, frame: &mut FrameState) {
        let Some(item) = self.items.get(&id) else {
            ui.painter().rect_filled(rect, radius, theme::RAISED);
            return;
        };
        // Placeholder: the ThumbHash's blurred colors, else gray.
        let placeholder = match self.placeholders.get(&id) {
            Some(texture) => Some(texture.id()),
            None if frame.made < PLACEHOLDERS_PER_FRAME => {
                frame.made += 1;
                make_placeholder(&mut self.placeholders, ui.ctx(), item)
            }
            None => None,
        };
        let painter = ui.painter();
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
            let t = (frame.now - loaded.at).as_secs_f32() / theme::DURATION_BASE;
            frame.fading |= t < 1.0;
            let tint = Color32::WHITE.gamma_multiply(t.clamp(0.0, 1.0));
            let uv = cover_uv(loaded.size, rect.size());
            painter.add(egui::epaint::RectShape::filled(rect, radius, tint).with_texture(loaded.id, uv));
        } else if failed {
            let size = (rect.width().min(rect.height()) * 0.6).min(24.0);
            let icon = Rect::from_center_size(rect.center(), Vec2::splat(size));
            Icon::ImageOff.image(size, theme::MIST).paint_at(ui, icon);
        }
    }

    /// Asks for the thumbnails of `wanted` (item index and width in points), most important first, and for rendering
    /// of those the cache doesn't have (REQ-005-07, REQ-006-13).
    fn request_textures(&mut self, wanted: impl IntoIterator<Item = (usize, f32)>, pixels_per_point: f32) {
        let Some(data) = self.data.clone() else { return };
        let Some(thumbnails) = &mut self.thumbnails else { return };
        let mut requests = Vec::new();
        for (index, points) in wanted {
            let Some(item) = data.ids.get(index).and_then(|id| self.items.get(id)) else {
                continue;
            };
            if item.thumb_status == ThumbStatus::Error || self.failed.contains(&item.id) {
                continue;
            }
            let width = pick_thumb_width(points, pixels_per_point);
            requests.push(TextureRequest {
                media: item.id,
                path: thumbnails.cache.path(&key_of(item), width),
                width,
            });
        }
        thumbnails.streamer.request(requests);
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

/// Text in one line, cut with "…" to `max_width`.
fn one_line(ui: &egui::Ui, text: &str, font: egui::FontId, color: Color32, max_width: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping {
        max_width: max_width.max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
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
