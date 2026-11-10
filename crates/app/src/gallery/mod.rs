//! The gallery (spec 006): the whole search result laid out up front, tile data loaded for the visible range, and
//! only what is near the viewport drawn: tiles (`tiles`) or list rows (`list`), with thumbnails streamed from the
//! thumbnail cache.

pub mod album_editor;
pub use album_editor::AlbumEditor;
mod albums;
mod calendar_and_map;
mod details;
mod list;
mod loader;
mod menu;
mod options;
mod scrolling;
mod search;
mod selection;
mod thumbs;
mod tiles;
mod years;

pub use calendar_and_map::*;
pub use loader::*;
pub use menu::*;
pub use thumbs::*;

use crate::icons::Icon;
use crate::textures::{TextureRequest, TextureStreamer};
#[cfg(target_arch = "wasm32")]
use crate::web::thumbs::{ThumbCache, ThumbnailReady, ThumbnailService};
use crate::{empty_state, theme};
use base64::Engine as _;
use db::gallery::{GalleryItem, GalleryLayout, GallerySearch, ThumbStatus};
use db::media::MediaDetail;
use db::rusqlite::Connection;
use db::tags::TagCount;
use domain::autoscroll::Autoscroll;
use domain::folders::{FolderNode, TagNode, build_folder_tree, build_tag_tree};
use domain::layout::{LayoutOptions, ListEntry, TileLayout, list_entry_at, list_index_of_item, tile_layout};
use domain::media::MediaKind;
use domain::tags::TagCategory;
use domain::thumbs::{ThumbKey, pick_thumb_width};
use domain::view::{GalleryView, TagIndicatorChoice, group_by_day_value, groups_shown, is_grouped_by_day};
use domain::zoom;
use eframe::egui::{self, Color32, Rect, Vec2, pos2, vec2};
#[cfg(not(target_arch = "wasm32"))]
use indexer::thumb_cache::ThumbCache;
#[cfg(not(target_arch = "wasm32"))]
use indexer::thumbnails::{ThumbnailReady, ThumbnailService};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use web_time::Instant;

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
/// Settings keys of the view choices.
const COLUMNS_KEY: &str = "gallery.columns";
const VIEW_KEY: &str = "gallery.view";
const GROUP_BY_DAY_KEY: &str = "gallery.groupByDay";
const TAG_INDICATORS_KEY: &str = "gallery.tagIndicators";
/// How long a new window width must stay before the gallery is laid out for it.
const RESIZE_SETTLE: std::time::Duration = std::time::Duration::from_millis(120);

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
    sidebar: Option<SidebarData>,
    /// The sidebar's tags as paths (REQ-026-01).
    tag_paths: Arc<TagPaths>,
    suggestions: Option<(String, Vec<TagCount>)>,
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
    /// Height of the visible content below the toolbar in the last frame.
    view_height: f32,
    /// A tile or row clicked this frame: the viewer opens it.
    opened: Option<usize>,
    /// The selected items (spec 025).
    selection: domain::selection::Selection,
    /// A rating or tag asked for the selection this frame (REQ-025-07, REQ-026-06).
    selection_change: Option<crate::state::Many>,
    /// _Rename…_ was chosen for the selection (REQ-035-01).
    rename_asked: bool,
    /// Delete was pressed with a selection (REQ-009-23).
    delete_asked: bool,
    /// _Fix dates…_ was chosen for the selection (REQ-038-01).
    fix_dates_asked: bool,
    /// The item shown in the viewer: kept in view (REQ-016-06), its thumbnail wanted first (REQ-009-14).
    viewed: Option<(usize, i64)>,
    /// The item the viewer shows outside the result, and what the index says of it once known (REQ-009-18).
    outside: Option<(i64, Option<Option<GalleryItem>>)>,
    /// Positions shown in the viewer's filmstrip: their thumbnails are wanted after the viewer's own (REQ-015-05).
    strip: std::ops::Range<usize>,
    /// The media item whose details were asked for last, and what came back.
    detail: Option<(i64, Option<DetailResult>)>,
    /// The unknown faces on the item the viewer shows (REQ-032-09).
    faces: Option<(i64, Option<Vec<db::faces::ShownFace>>)>,
    /// The groups of unknown faces last loaded (REQ-032-15), still shown while they load again (REQ-032-31).
    face_groups: Option<FaceGroups>,
    /// Whether the groups were asked for since faces last changed.
    face_groups_asked: bool,
    /// The dialog naming a smart album, while open (REQ-034-01, REQ-034-05).
    album_dialog: Option<albums::AlbumDialog>,
    /// The libraries the smart album editor offers (REQ-039-02).
    album_libraries: Vec<(i64, String)>,
    /// A context menu's choice this frame.
    menu_action: Option<(usize, MediaAction)>,
    /// The item under the pointer in the last frame.
    hovered: Option<usize>,
    /// The items a Shift-click would add, shown brighter (REQ-025-14).
    range_preview: Option<std::ops::RangeInclusive<usize>>,
    /// Where the tiles were drawn in the last frame: the content's origin and the visible area (REQ-013-12).
    tiles_frame: Option<(egui::Pos2, Rect)>,
    /// Autoscroll with the middle mouse button, and where it started on screen (REQ-006-14).
    autoscroll: Autoscroll,
    /// Until when the years beside the scrollbar stay after the user scrolled (REQ-006-18), in egui's time.
    years_until: f64,
    /// Where the years and months were beside the scrollbar in the last frame, while shown: the pointer over it
    /// hovers no tile or row (REQ-006-22).
    year_lane: Option<Rect>,
    autoscroll_at: egui::Pos2,
    /// The click that stopped autoscroll: tiles and rows ignore it until every button is up again.
    swallow_click: bool,
    /// Photos content search's model hasn't looked at yet (REQ-036-06).
    content_left: u64,
    /// Where the date calendar is (REQ-041-01).
    calendar: Option<CalendarView>,
    /// What the calendar asked for last, and the answer it shows until a newer one comes (REQ-041-02).
    calendar_asked: Option<CalendarKey>,
    calendar_data: Option<(CalendarKey, CalendarData)>,
    /// The calendar's thumbnails, asked for before the tiles' in the next frame (REQ-041-02).
    calendar_wanted: Vec<(i64, ThumbKey)>,
    /// Whether this frame asked for thumbnails yet: without tiles drawn, the calendar asks itself.
    textures_asked: bool,
    /// What the map shows, kept while it is closed (REQ-044-02).
    pub map_view: domain::map::MapView,
    /// The search the map asked for last, and its located items once loaded (REQ-044-01).
    map_asked: Option<GallerySearch>,
    map_points: Option<Arc<Vec<(i64, f64, f64)>>>,
    /// The clusters of `map_points` at a zoom level, kept until either changes: grouping every photo each frame is slow.
    map_clusters: Option<MapClusters>,
    /// Thumbnail keys of photos the map showed, `None` for one without; each asked for once.
    map_keys: HashMap<i64, Option<ThumbKey>>,
    map_keys_asked: HashSet<i64>,
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
            sidebar: None,
            tag_paths: Default::default(),
            suggestions: None,
            wheel: 0.0,
            placeholders: HashMap::new(),
            thumbnails: None,
            awaiting: HashSet::new(),
            failed: HashSet::new(),
            content_top: 0.0,
            offset: 0.0,
            scroll_to: None,
            view_height: 0.0,
            opened: None,
            selection: Default::default(),
            selection_change: None,
            rename_asked: false,
            delete_asked: false,
            fix_dates_asked: false,
            viewed: None,
            outside: None,
            strip: 0..0,
            detail: None,
            faces: None,
            face_groups: None,
            face_groups_asked: false,
            album_dialog: None,
            album_libraries: Vec::new(),
            menu_action: None,
            hovered: None,
            range_preview: None,
            autoscroll: Autoscroll::Off,
            years_until: 0.0,
            year_lane: None,
            autoscroll_at: egui::Pos2::ZERO,
            swallow_click: false,
            content_left: 0,
            calendar: None,
            calendar_asked: None,
            calendar_data: None,
            calendar_wanted: Vec::new(),
            textures_asked: false,
            map_view: Default::default(),
            map_asked: None,
            map_points: None,
            map_clusters: None,
            map_keys: HashMap::new(),
            map_keys_asked: HashSet::new(),
            tiles_frame: None,
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
        // A file read again may have new regions, which name some of its faces.
        self.forget_faces();
        if !self.started {
            return;
        }
        self.generation += 1;
        self.loader.send(Request::Layout {
            generation: self.generation,
            search: self.search.clone(),
        });
        // New items may fall on the calendar's days, and on the map.
        self.calendar_asked = None;
        self.map_asked = None;
        // Counts and new tags while indexing (REQ-006-11).
        self.reload_sidebar();
    }

    pub(crate) fn reload_sidebar(&mut self) {
        self.loader.send(Request::Sidebar {
            library: self.search.library,
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

    /// The gallery position a click or Enter asked to open in the viewer this frame (REQ-009-01).
    pub fn take_opened(&mut self) -> Option<usize> {
        self.opened.take()
    }

    /// Changes whenever a new result arrived, so positions may have moved.
    pub fn data_version(&self) -> u64 {
        self.data_version
    }

    /// The media id at a gallery position.
    pub fn id_at(&self, index: usize) -> Option<i64> {
        self.data.as_ref()?.ids.get(index).copied()
    }

    /// Where a media item is in the current result, if it is in it.
    pub fn position_of(&self, id: i64) -> Option<usize> {
        self.data.as_ref()?.ids.iter().position(|&other| other == id)
    }

    /// The tile data of the item at `index`, loading it and its neighbours' if needed.
    pub fn item(&mut self, index: usize) -> Option<&GalleryItem> {
        let id = self.id_at(index)?;
        if !self.items.contains_key(&id) {
            self.load_range(
                index.saturating_sub(domain::viewer::PRELOAD_RADIUS),
                index + domain::viewer::PRELOAD_RADIUS + 1,
            );
        }
        self.items.get(&id)
    }

    /// The gallery positions the viewer's filmstrip shows (REQ-015-05).
    pub fn set_strip(&mut self, strip: std::ops::Range<usize>) {
        self.strip = strip;
    }

    /// Where the tile of item `index` is on screen, if it was drawn in the last frame and is at least partly visible
    /// (REQ-013-12). `None` in the List view.
    pub fn tile_rect(&self, index: usize) -> Option<Rect> {
        let (origin, visible) = self.tiles_frame?;
        let arranged = self.arranged.as_ref()?;
        if self.view == GalleryView::List || index >= arranged.tiles.top.len() {
            return None;
        }
        let rect = tiles::tile_rect(&arranged.tiles, index, origin);
        visible.intersects(rect).then_some(rect)
    }

    /// The item the viewer shows, or none once it closed: the gallery keeps it in view and wants its thumbnail first.
    pub fn set_viewed(&mut self, viewed: Option<(usize, i64)>) {
        self.viewed = viewed;
    }

    /// Waits for all queries sent so far and applies them (tests).
    pub fn wait_for_loads(&mut self) {
        self.start();
        while self.loader.pending > 0 {
            let Ok(loaded) = self.loader.results.recv() else { return };
            self.apply(loaded);
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

    /// Shows the gallery in `ui`, leaving room at the top for the toolbar.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.poll();
        self.begin_thumbnails();
        let area = ui.max_rect();
        self.handle_zoom_wheel(ui, area);
        if let Some(error) = &self.error
            && self.data.is_none()
        {
            empty_state::error(ui, "Couldn't load media", error);
            return;
        }
        let list = self.view == GalleryView::List;
        self.tiles_frame = None;
        let Some(count) = self.count() else {
            if list {
                list::skeleton(ui);
            } else {
                tiles::skeleton(ui, self.columns, self.gap());
            }
            return;
        };
        if count == 0 {
            match &self.search.content {
                Some(words) => empty_state::no_content_matches(ui, words, self.content_left),
                None => empty_state::no_media(ui),
            }
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
        let autoscroll = self.handle_autoscroll(ui, Rect::from_min_max(pos2(area.left(), self.content_top), area.max));
        let mut source = egui::scroll_area::ScrollSource::ALL;
        // Dragging with the middle button would scroll the other way.
        if self.autoscroll.is_on() {
            source.drag = egui::scroll_area::DragScroll::Never;
        }
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("gallery")
            .auto_shrink(false)
            .scroll_bar_rect(scroll_bar_rect(area, list))
            // Ctrl + wheel zooms (handled above), it never scrolls.
            .scroll_source(source);
        if autoscroll != 0.0 {
            // Within the content: past its end the scroll area would spring back once autoscroll stops.
            let end = (total + PADDING - self.view_height).max(0.0);
            scroll = scroll.vertical_scroll_offset((self.offset + autoscroll).clamp(0.0, end));
        }
        // The anchor goes right below the toolbar, where it was.
        if changed && anchor > 0 {
            scroll = scroll.vertical_scroll_offset(self.start_of(&arranged, anchor.min(count - 1)) as f32);
        }
        if let Some(offset) = scroll_to {
            scroll = scroll.vertical_scroll_offset(offset);
        } else if let Some(top) = self.viewed.and_then(|(index, _)| self.reveal(&arranged, index, count)) {
            scroll = scroll.vertical_scroll_offset(top);
        }
        let before = self.offset;
        // The user's wheel or touchpad, read before the scroll area takes it, not a jump of the app's (REQ-006-18).
        let wheel = ui.input(|input| input.smooth_scroll_delta().y != 0.0);
        let output = scroll.show_viewport(ui, |ui, viewport| {
            self.offset = viewport.min.y;
            self.view_height = viewport.height() - inset_top;
            ui.set_min_height(total + inset_top + PADDING);
            self.anchor = self.item_at(&arranged, count, f64::from(viewport.min.y));
            self.anchor_id = self.data.as_ref().and_then(|data| data.ids.get(self.anchor).copied());
            // Under the items, which take their own clicks: a click beside them clears the selection (REQ-025-04).
            let background = ui.interact(
                ui.clip_rect(),
                ui.make_persistent_id("gallery background"),
                self.tile_sense(),
            );
            if list {
                let origin = ui.min_rect().min + vec2(0.0, inset_top);
                self.draw_list(ui, &arranged, viewport, origin, inset_top);
            } else {
                let origin = ui.min_rect().min + vec2(PADDING, inset_top);
                self.draw_tiles(ui, &arranged, viewport, origin, inset_top);
            }
            let modifiers = ui.input(|input| input.modifiers);
            if background.clicked() && !modifiers.command && !modifiers.shift {
                self.selection.clear();
            }
        });
        let scrolled = wheel && !self.autoscroll.is_on() && (self.offset - before).abs() > 0.5;
        self.year_labels(
            ui,
            &arranged,
            scroll_bar_rect(area, list),
            output.content_size.y,
            output.inner_rect.height(),
            scrolled,
        );
        self.arranged = Some(arranged);
        if list {
            list::column_header(ui, area, self.list_width);
        }
    }

    /// Whether the pointer is over a tile or row's `response`, and not over the years beside the scrollbar above it
    /// (REQ-006-22).
    fn under_pointer(&self, ui: &egui::Ui, response: &egui::Response) -> bool {
        response.contains_pointer()
            && !self.year_lane.is_some_and(|lane| {
                ui.input(|input| input.pointer.latest_pos())
                    .is_some_and(|pos| lane.contains(pos))
            })
    }

    /// Whether tiles and rows react to the pointer: not while autoscroll is on or the click that stopped it lasts.
    fn tile_sense(&self) -> egui::Sense {
        if self.autoscroll.is_on() || self.swallow_click {
            egui::Sense::hover()
        } else {
            egui::Sense::click()
        }
    }
}

/// Where the gallery's scrollbar goes in `area`: below the toolbar (and the List view's column header), so it never
/// lies under the window controls (REQ-013-15).
pub fn scroll_bar_rect(area: Rect, list: bool) -> Rect {
    let top = if list {
        TOOLBAR_HEIGHT + list::COLUMN_HEADER_HEIGHT
    } else {
        TOOLBAR_HEIGHT
    };
    Rect::from_min_max(pos2(area.left(), area.top() + top), area.max)
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
    fn req_013_15_the_scrollbar_starts_below_the_toolbar() {
        let area = Rect::from_min_max(pos2(256.0, 0.0), pos2(1280.0, 800.0));
        let tiles = scroll_bar_rect(area, false);
        assert_eq!(
            (tiles.top(), tiles.bottom(), tiles.right()),
            (TOOLBAR_HEIGHT, 800.0, 1280.0)
        );
        let rows = scroll_bar_rect(area, true);
        assert_eq!(rows.top(), TOOLBAR_HEIGHT + list::COLUMN_HEADER_HEIGHT);
        // Below the window controls, whatever the window's width.
        for (_, button) in crate::titlebar::button_rects(area, &crate::titlebar::ButtonLayout::default()) {
            assert!(button.bottom() <= tiles.top());
        }
    }

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
