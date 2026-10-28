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
use db::media::MediaDetail;
use db::rusqlite::Connection;
use db::tags::TagCount;
use domain::folders::{FolderNode, TagNode, build_folder_tree, build_tag_tree};
use domain::layout::{LayoutOptions, ListEntry, TileLayout, list_entry_at, list_index_of_item, tile_layout};
use domain::tags::TagCategory;
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
/// Settings keys of the view choices.
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
    Sidebar {
        library: Option<i64>,
    },
    Suggest {
        prefix: String,
        limit: usize,
    },
    Detail {
        id: i64,
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
    Sidebar(Result<SidebarData, String>),
    Suggest {
        prefix: String,
        result: Result<Vec<TagCount>, String>,
    },
    Detail {
        id: i64,
        result: Result<Option<MediaDetail>, String>,
    },
}

/// What a tile's, row's or the viewer's context menu asks for (REQ-009-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAction {
    ShowInExplorer,
    Information,
}

/// A media item's details as loaded: `Ok(None)` when it isn't indexed (any more).
pub type DetailResult = Result<Option<MediaDetail>, String>;

/// What the sidebar lists (spec 008), loaded with the gallery.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SidebarData {
    pub people: Vec<TagNode>,
    pub places: Vec<TagNode>,
    /// Media items per rating, highest first, `0` = unrated.
    pub ratings: Vec<(u8, u64)>,
    pub popular: Vec<TagCount>,
    /// The folder tree of the selected library.
    pub folders: Option<(i64, Vec<FolderNode>)>,
}

/// Most used tags outside both categories in the sidebar (REQ-008-05).
const POPULAR_TAGS: usize = 15;

fn load_sidebar(conn: &Connection, library: Option<i64>) -> db::Result<SidebarData> {
    let folders = match library {
        Some(library) => Some((library, build_folder_tree(&db::tags::library_folders(conn, library)?))),
        None => None,
    };
    Ok(SidebarData {
        people: build_tag_tree(&db::tags::category(conn, TagCategory::People)?),
        places: build_tag_tree(&db::tags::category(conn, TagCategory::Places)?),
        ratings: db::tags::rating_counts(conn)?,
        popular: db::tags::popular(conn, POPULAR_TAGS)?,
        folders,
    })
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
                        Request::Sidebar { library } => {
                            Loaded::Sidebar(load_sidebar(&conn, library).map_err(|e| e.to_string()))
                        }
                        Request::Suggest { prefix, limit } => Loaded::Suggest {
                            result: db::tags::autocomplete(&conn, &prefix, limit).map_err(|e| e.to_string()),
                            prefix,
                        },
                        Request::Detail { id } => Loaded::Detail {
                            id,
                            result: db::media::detail(&conn, id).map_err(|e| e.to_string()),
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
    sidebar: Option<SidebarData>,
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
    /// The item shown in the viewer: kept in view (REQ-016-06), its thumbnail wanted first (REQ-009-14).
    viewed: Option<(usize, i64)>,
    /// Positions shown in the viewer's filmstrip: their thumbnails are wanted after the viewer's own (REQ-015-05).
    strip: std::ops::Range<usize>,
    /// The media item whose details were asked for last, and what came back.
    detail: Option<(i64, Option<DetailResult>)>,
    /// A context menu's choice this frame.
    menu_action: Option<(usize, MediaAction)>,
    /// Where the tiles were drawn in the last frame: the content's origin and the visible area (REQ-013-12).
    tiles_frame: Option<(egui::Pos2, Rect)>,
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
            viewed: None,
            strip: 0..0,
            detail: None,
            menu_action: None,
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
        if !self.started {
            return;
        }
        self.generation += 1;
        self.loader.send(Request::Layout {
            generation: self.generation,
            search: self.search.clone(),
        });
        // Counts and new tags while indexing (REQ-006-11).
        self.reload_sidebar();
    }

    fn reload_sidebar(&mut self) {
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

    pub fn view(&self) -> GalleryView {
        self.view
    }

    pub fn group_by_day(&self) -> bool {
        self.group_by_day
    }

    pub fn tag_indicators(&self) -> &TagIndicatorChoice {
        &self.indicators
    }

    pub fn search(&self) -> &GallerySearch {
        &self.search
    }

    /// Shows another search, from the top (spec 007).
    pub fn set_search(&mut self, search: GallerySearch) {
        if search == self.search {
            return;
        }
        let library_changed = search.library != self.search.library;
        self.search = search;
        self.anchor = 0;
        self.anchor_id = None;
        self.scroll_to = Some(0.0);
        if !self.started {
            return;
        }
        self.generation += 1;
        self.loader.send(Request::Layout {
            generation: self.generation,
            search: self.search.clone(),
        });
        // The sidebar's lists don't depend on the filters, only its folder tree on the library.
        if library_changed {
            self.reload_sidebar();
        }
    }

    /// The sidebar's lists, once loaded.
    pub fn sidebar(&self) -> Option<&SidebarData> {
        self.sidebar.as_ref()
    }

    /// Asks for tag suggestions for `prefix` (REQ-007-04); answered in [`Self::suggestions`].
    pub fn request_suggestions(&mut self, prefix: &str, limit: usize) {
        self.start();
        self.loader.send(Request::Suggest {
            prefix: prefix.to_string(),
            limit,
        });
    }

    /// The latest suggestions and the prefix they are for.
    pub fn suggestions(&self) -> Option<(&str, &[TagCount])> {
        self.suggestions
            .as_ref()
            .map(|(prefix, tags)| (prefix.as_str(), tags.as_slice()))
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

    /// What a context menu asked for this frame, and for which gallery position (REQ-009-11).
    pub fn take_menu_action(&mut self) -> Option<(usize, MediaAction)> {
        self.menu_action.take()
    }

    /// The details of media item `id` (REQ-009-08), asked for on first use and again after each reload; `None` while
    /// loading.
    pub fn detail(&mut self, id: i64) -> Option<&DetailResult> {
        if self.detail.as_ref().is_none_or(|(asked, _)| *asked != id) {
            self.detail = Some((id, None));
            self.loader.send(Request::Detail { id });
        }
        self.detail.as_ref()?.1.as_ref()
    }

    /// Forgets the details asked for last: no info view shows them any more.
    pub fn forget_detail(&mut self) {
        self.detail = None;
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

    /// A stored view choice, e.g. whether the viewer shows its filmstrip.
    pub fn setting(&self, key: &str) -> Option<String> {
        let conn = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        db::settings::value(&conn, key)
            .ok()
            .flatten()
            .and_then(|value| value.as_str().map(String::from))
    }

    /// Stores a view choice.
    pub fn store_setting(&self, key: &str, value: &str) {
        self.store(key, value.into());
    }

    /// The item the viewer shows, or none once it closed: the gallery keeps it in view and wants its thumbnail first.
    pub fn set_viewed(&mut self, viewed: Option<(usize, i64)>) {
        self.viewed = viewed;
    }

    /// The loaded thumbnail of a media item, if any.
    pub fn thumbnail(&mut self, id: i64) -> Option<crate::textures::Loaded> {
        self.thumbnails.as_mut()?.streamer.get(id)
    }

    /// The ThumbHash placeholder of a loaded item (REQ-005-06).
    pub fn placeholder(&mut self, ctx: &egui::Context, id: i64) -> Option<egui::TextureId> {
        if let Some(texture) = self.placeholders.get(&id) {
            return Some(texture.id());
        }
        let item = self.items.get(&id)?;
        make_placeholder(&mut self.placeholders, ctx, item)
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
                        if let Some((id, _)) = self.detail {
                            self.loader.send(Request::Detail { id });
                        }
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            Loaded::Sidebar(result) => match result {
                Ok(data) => self.sidebar = Some(data),
                Err(error) => tracing::error!("can't load the sidebar: {error}"),
            },
            Loaded::Detail { id, result } => {
                if let Some((asked, loaded)) = &mut self.detail
                    && *asked == id
                {
                    if let Err(error) = &result {
                        tracing::error!("can't load the details of media item {id}: {error}");
                    }
                    *loaded = Some(result);
                }
            }
            Loaded::Suggest { prefix, result } => match result {
                Ok(tags) => self.suggestions = Some((prefix, tags)),
                Err(error) => tracing::error!("can't load tag suggestions: {error}"),
            },
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

    /// The scroll offset that brings item `index` into view below the toolbar, if it isn't (REQ-016-06).
    fn reveal(&self, arranged: &Arranged, index: usize, count: usize) -> Option<f32> {
        if index >= count {
            return None;
        }
        let (top, height) = if self.view == GalleryView::List {
            let entry = list_index_of_item(&arranged.groups, index);
            (list::ROWS.top(&arranged.groups, entry), list::ROWS.row)
        } else {
            (arranged.tiles.top[index], arranged.tiles.height[index])
        };
        let (top, bottom) = (top as f32, (top + height) as f32);
        let (view_top, view_bottom) = (self.offset, self.offset + self.view_height);
        // An item taller than the view counts as shown while it covers part of it; else it would be scrolled every frame.
        let visible = if bottom - top > self.view_height {
            top < view_bottom && bottom > view_top
        } else {
            top >= view_top && bottom <= view_bottom
        };
        (!visible).then_some(top)
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
            .scroll_bar_rect(scroll_bar_rect(area, list))
            // Ctrl + wheel zooms (handled above), it never scrolls.
            .scroll_source(egui::scroll_area::ScrollSource::ALL);
        // The anchor goes right below the toolbar, where it was.
        if changed && anchor > 0 {
            scroll = scroll.vertical_scroll_offset(self.start_of(&arranged, anchor.min(count - 1)) as f32);
        }
        if let Some(offset) = scroll_to {
            scroll = scroll.vertical_scroll_offset(offset);
        } else if let Some(top) = self.viewed.and_then(|(index, _)| self.reveal(&arranged, index, count)) {
            scroll = scroll.vertical_scroll_offset(top);
        }
        scroll.show_viewport(ui, |ui, viewport| {
            self.offset = viewport.min.y;
            self.view_height = viewport.height() - inset_top;
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
        // Under glass the placeholder is its plain color (the blur hides the difference), and only while the
        // thumbnail doesn't cover it yet (REQ-013-03).
        if loaded
            .as_ref()
            .is_none_or(|loaded| (frame.now - loaded.at).as_secs_f32() < theme::DURATION_BASE)
        {
            crate::glass::record_solid(ui.ctx(), rect, theme::RAISED, false);
        }
        if let Some(loaded) = loaded {
            let t = (frame.now - loaded.at).as_secs_f32() / theme::DURATION_BASE;
            frame.fading |= t < 1.0;
            let tint = Color32::WHITE.gamma_multiply(t.clamp(0.0, 1.0));
            let uv = cover_uv(loaded.size, rect.size());
            painter.add(egui::epaint::RectShape::filled(rect, radius, tint).with_texture(loaded.id, uv));
            crate::glass::record(
                ui.ctx(),
                crate::glass::BackdropItem {
                    rect,
                    uv,
                    view: Some(loaded.view),
                    color: tint,
                    chrome: false,
                },
            );
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
        // The viewer's preview first: the largest thumbnail, unless one is loaded already (REQ-009-14).
        if let Some((_, id)) = self.viewed
            && let Some(item) = self.items.get(&id)
            && item.thumb_status != ThumbStatus::Error
            && thumbnails.streamer.get(id).is_none()
        {
            requests.push(TextureRequest {
                media: id,
                path: thumbnails.cache.path(&key_of(item), domain::thumbs::MAX_THUMB_WIDTH),
                width: domain::thumbs::MAX_THUMB_WIDTH,
            });
        }
        let strip = self.strip.clone().map(|index| (index, domain::viewer::FILMSTRIP_THUMB));
        for (index, points) in strip.chain(wanted) {
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

/// _Show in Explorer_ and _Information_ in a context menu (REQ-009-11); what was chosen.
pub fn context_menu(ui: &mut egui::Ui) -> Option<MediaAction> {
    crate::glass::behind_menu(ui, |ui| {
        let mut action = None;
        if ui.button("Show in Explorer").clicked() {
            action = Some(MediaAction::ShowInExplorer);
        }
        if ui.button("Information").clicked() {
            action = Some(MediaAction::Information);
        }
        if action.is_some() {
            ui.close();
        }
        action
    })
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
        for button in crate::titlebar::button_rects(area) {
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
