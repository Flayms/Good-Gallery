//! The viewer (spec 009): one media item over the whole window, stepped through in the gallery's order. Photos load
//! their originals (`originals`) and show their thumbnail until then; they zoom and pan. The gallery keeps drawing
//! below the scrim and keeps the shown item in view.

use crate::format;
use crate::gallery::Gallery;
use crate::icons::Icon;
use crate::originals::{OriginalRequest, OriginalState, Originals};
use crate::theme;
use db::gallery::GalleryItem;
use db::libraries::Library;
use domain::media::MediaKind;
use domain::viewer::{self as zoom, FIT, Size, ZoomView};
use eframe::egui::{self, Color32, CursorIcon, Key, Modifiers, Pos2, Rect, Vec2, pos2, vec2};
use std::path::Path;
use std::time::{Duration, Instant};

/// The controls step aside after this long without pointer movement or key presses (REQ-013-10).
const IDLE: Duration = Duration::from_millis(2500);
const WHEEL_SENSITIVITY: f32 = 0.002;
/// Points per wheel line: one notch scrolls about 100 points in a browser.
const WHEEL_LINE: f32 = 100.0;
const KEY_ZOOM_STEP: f32 = 1.25;
/// Room around the media: for the arrows at the sides, the title bar strip at the top, the control bar below.
const SIDE_SPACE: f32 = 64.0;
const TOP_SPACE: f32 = 48.0;
const CONTROLS_SPACE: f32 = 80.0;
const BAR_HEIGHT: f32 = 48.0;
const NAV_SIZE: f32 = 40.0;
/// The window behind the viewer, darkened. The design's 0.5 needs the blur of M10 (REQ-013-14).
const SCRIM: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 204);
/// `GLASS_BAR` without its blur (M10).
const GLASS_BAR: Color32 = Color32::from_rgba_premultiplied(20, 20, 20, 184);

/// What the keys and the mouse's back button asked for since the last frame.
#[derive(Debug, Default)]
struct Keys {
    close: bool,
    step: i32,
    zoom: Option<f32>,
    fit: bool,
}

pub struct Viewer {
    /// The shown item: its gallery position, and its id to find it again after a reload.
    index: usize,
    id: i64,
    /// The gallery result the position was checked against.
    version: u64,
    /// No longer in the result (REQ-009-12).
    missing: bool,
    view: ZoomView,
    /// The original was loaded when the item came up: it shows without a fade (REQ-009-14). `None` until the first
    /// frame of the item.
    instant: Option<bool>,
    /// egui's time when the original was first drawn, for its fade-in.
    fade_from: Option<f64>,
    last_activity: Instant,
    /// A scrim click waiting to close the viewer, unless a second click comes first.
    close_at: Option<Instant>,
    keys: Keys,
}

impl Viewer {
    pub fn new(index: usize, id: i64, version: u64) -> Self {
        Self {
            index,
            id,
            version,
            missing: false,
            view: FIT,
            instant: None,
            fade_from: None,
            last_activity: Instant::now(),
            close_at: None,
            keys: Keys::default(),
        }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn view(&self) -> ZoomView {
        self.view
    }

    /// Whether the shown photo's original was loaded when it came up, so it shows without fading in (REQ-009-14).
    pub fn shows_original_at_once(&self) -> bool {
        self.instant == Some(true)
    }

    /// Takes the viewer's keys before anything else sees them; call at the start of the frame.
    pub fn input(&mut self, ctx: &egui::Context) {
        ctx.memory_mut(|memory| {
            if let Some(focused) = memory.focused() {
                memory.surrender_focus(focused);
            }
        });
        ctx.input_mut(|input| {
            let active = input.pointer.delta() != Vec2::ZERO
                || input.pointer.any_pressed()
                || input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Key { pressed: true, .. }));
            if active {
                self.last_activity = Instant::now();
            }
            // The mouse's back button and Alt + ← close instead of stepping back (REQ-009-02, REQ-009-03).
            if input.pointer.button_pressed(egui::PointerButton::Extra1)
                || input.consume_key(Modifiers::ALT, Key::ArrowLeft)
                || input.consume_key(Modifiers::NONE, Key::Escape)
            {
                self.keys.close = true;
            }
            // Nothing under the viewer may take focus or act on Enter or Space.
            for key in [Key::Tab, Key::Enter, Key::Space] {
                input.consume_key(Modifiers::NONE, key);
                input.consume_key(Modifiers::SHIFT, key);
            }
            if input.modifiers.ctrl || input.modifiers.alt || input.modifiers.command {
                return;
            }
            if input.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                self.keys.step -= 1;
            }
            if input.consume_key(Modifiers::NONE, Key::ArrowRight) {
                self.keys.step += 1;
            }
            if input.consume_key(Modifiers::NONE, Key::Plus) || input.consume_key(Modifiers::NONE, Key::Equals) {
                self.keys.zoom = Some(self.keys.zoom.unwrap_or(1.0) * KEY_ZOOM_STEP);
            }
            if input.consume_key(Modifiers::NONE, Key::Minus) {
                self.keys.zoom = Some(self.keys.zoom.unwrap_or(1.0) / KEY_ZOOM_STEP);
            }
            if input.consume_key(Modifiers::NONE, Key::Num0) {
                self.keys.fit = true;
            }
        });
    }

    fn go_to(&mut self, index: usize, gallery: &Gallery) {
        let Some(id) = gallery.id_at(index) else { return };
        (self.index, self.id) = (index, id);
        self.missing = false;
        self.view = FIT;
        self.instant = None;
        self.fade_from = None;
        self.close_at = None;
    }

    /// Finds the shown item again after the gallery reloaded (REQ-009-12).
    fn follow(&mut self, gallery: &Gallery) {
        if self.version == gallery.data_version() {
            return;
        }
        self.version = gallery.data_version();
        if gallery.id_at(self.index) == Some(self.id) {
            self.missing = false;
        } else if let Some(index) = gallery.position_of(self.id) {
            (self.index, self.missing) = (index, false);
        } else {
            self.missing = true;
        }
    }

    /// Draws the viewer over the window; false once it closed.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        gallery: &mut Gallery,
        originals: &mut Originals,
        libraries: &[Library],
    ) -> bool {
        self.follow(gallery);
        let count = gallery.count().unwrap_or(0);
        let keys = std::mem::take(&mut self.keys);
        if keys.close {
            return false;
        }
        if keys.step != 0 {
            let target = self.index as i64 + i64::from(keys.step.signum());
            if target >= 0 && (target as usize) < count {
                self.go_to(target as usize, gallery);
            }
        }
        if self.close_at.is_some_and(|at| Instant::now() >= at) {
            return false;
        }

        preload(gallery, originals, libraries, self.index, count);
        gallery.set_viewed((!self.missing).then_some((self.index, self.id)));
        let item = if self.missing {
            None
        } else {
            gallery.item(self.index).cloned()
        };
        let original = originals.get(self.id);
        if item.is_some() && self.instant.is_none() {
            self.instant = Some(matches!(original, OriginalState::Loaded(_)));
        }

        let screen = ctx.content_rect();
        let mut open = true;
        egui::Area::new(egui::Id::NULL.with("viewer"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .constrain(false)
            .show(ctx, |ui| {
                ui.set_min_size(screen.size());
                ui.painter().rect_filled(screen, 0.0, SCRIM);
                let media = Rect::from_min_max(
                    screen.min + vec2(SIDE_SPACE, TOP_SPACE),
                    screen.max - vec2(SIDE_SPACE, CONTROLS_SPACE),
                );
                let backdrop = ui.interact(screen, ui.make_persistent_id("backdrop"), egui::Sense::click_and_drag());
                let label = item
                    .as_ref()
                    .map_or("Media viewer", |item| item.file_name.as_str())
                    .to_owned();
                backdrop.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Dialog, true, &label));
                match &item {
                    Some(item) => {
                        self.media(ui, &backdrop, media, item, gallery, &original, keys);
                    }
                    None if self.missing => {
                        message(ui, media.center(), "Media not found");
                        self.backdrop_clicks(&backdrop);
                    }
                    None => {
                        self.backdrop_clicks(&backdrop);
                    }
                }
                if !self.controls(ui, screen, item.as_ref(), count) {
                    open = false;
                }
            });
        if let Some(at) = self.close_at {
            ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
        }
        open
    }

    /// A click on the scrim closes after a moment; a double-click cancels it (REQ-009-03).
    fn backdrop_clicks(&mut self, backdrop: &egui::Response) -> bool {
        if backdrop.double_clicked() {
            self.close_at = None;
            return true;
        }
        if backdrop.clicked() {
            self.close_later(&backdrop.ctx);
        }
        false
    }

    /// Closes once egui's double-click window has passed without a second click (which would zoom instead).
    fn close_later(&mut self, ctx: &egui::Context) {
        let delay = ctx.options(|options| options.input_options.max_double_click_delay);
        self.close_at = Some(Instant::now() + Duration::from_secs_f64(delay));
    }

    /// The photo (or a video's preview), fitted into `area`, zoomed and panned (REQ-009-05, REQ-009-14 … -16).
    #[expect(clippy::too_many_arguments, reason = "one frame's inputs of the media area")]
    fn media(
        &mut self,
        ui: &mut egui::Ui,
        backdrop: &egui::Response,
        area: Rect,
        item: &GalleryItem,
        gallery: &mut Gallery,
        original: &OriginalState,
        keys: Keys,
    ) {
        let thumbnail = gallery.thumbnail(item.id);
        let loaded = match original {
            OriginalState::Loaded(loaded) => Some(*loaded),
            _ => None,
        };
        let natural = match (item.width, item.height, loaded, thumbnail) {
            (Some(width), Some(height), _, _) if width > 0 && height > 0 => Size::new(width as f32, height as f32),
            (_, _, Some(loaded), _) => Size::new(loaded.size[0] as f32, loaded.size[1] as f32),
            (_, _, _, Some(thumbnail)) => Size::new(thumbnail.size[0] as f32, thumbnail.size[1] as f32),
            _ => Size::new(area.width(), area.height()),
        };
        let viewport = Size::new(area.width(), area.height());
        let fitted = zoom::fit_size(natural, viewport);
        let photo = item.kind == MediaKind::Image;

        if photo {
            self.handle_zoom(ui, backdrop, area, natural, fitted, viewport, keys);
        } else {
            self.backdrop_clicks(backdrop);
        }
        let view = self.view;
        let rect = photo_rect(area, fitted, view);
        if photo && let Some(pos) = backdrop.hover_pos() {
            let icon = if !rect.intersect(area).contains(pos) {
                CursorIcon::Default
            } else if !view.is_zoomed() {
                CursorIcon::ZoomIn
            } else if backdrop.dragged() {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            };
            ui.ctx().set_cursor_icon(icon);
        }

        let painter = ui.painter().with_clip_rect(area);
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let fade = match loaded {
            Some(_) if self.instant == Some(true) => 1.0,
            Some(_) => {
                let now = ui.input(|input| input.time);
                let from = *self.fade_from.get_or_insert(now);
                ((now - from) as f32 / theme::DURATION_BASE).min(1.0)
            }
            None => 0.0,
        };
        // The thumbnail stays under the original until it has faded in; the placeholder only without one.
        if fade < 1.0 {
            match thumbnail {
                Some(thumbnail) => {
                    painter.image(thumbnail.id, rect, uv, Color32::WHITE);
                }
                None => {
                    if let Some(placeholder) = gallery.placeholder(ui.ctx(), item.id) {
                        painter.image(placeholder, rect, uv, Color32::WHITE);
                    }
                }
            }
        }
        if let Some(loaded) = loaded {
            painter.image(loaded.id, rect, uv, Color32::WHITE.gamma_multiply(fade));
            if fade < 1.0 {
                ui.ctx().request_repaint();
            }
        }
        if let OriginalState::Failed(_) = original {
            let at = pos2(rect.center().x, rect.bottom().min(area.bottom()) - 28.0);
            message(ui, at, "Can't load the original. Showing a preview.");
        }
    }

    #[expect(clippy::too_many_arguments, reason = "one frame's inputs of the media area")]
    fn handle_zoom(
        &mut self,
        ui: &egui::Ui,
        backdrop: &egui::Response,
        area: Rect,
        natural: Size,
        fitted: Size,
        viewport: Size,
        keys: Keys,
    ) {
        let content = fitted;
        let relative = |pos: Pos2| (pos.x - area.center().x, pos.y - area.center().y);
        // Only where it is drawn: around a zoomed photo's clipped edges the scrim shows, and a click there closes.
        let on_photo = |view: ZoomView, pos: Pos2| photo_rect(area, fitted, view).intersect(area).contains(pos);
        let toggle = |view: ZoomView, pos: Pos2| {
            let scale = zoom::toggle_scale(view, natural.width, fitted.width);
            zoom::zoom_to(view, scale, relative(pos), content, viewport)
        };

        if keys.fit {
            self.view = FIT;
        }
        if let Some(factor) = keys.zoom {
            self.view = zoom::zoom_to(self.view, self.view.scale * factor, (0.0, 0.0), content, viewport);
        }
        if backdrop.hovered() {
            let delta: f32 = ui.input(|input| {
                input
                    .raw
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::MouseWheel {
                            unit, delta, modifiers, ..
                        } if !modifiers.ctrl => Some(match unit {
                            egui::MouseWheelUnit::Point => delta.y,
                            egui::MouseWheelUnit::Line => delta.y * WHEEL_LINE,
                            egui::MouseWheelUnit::Page => delta.y.signum() * WHEEL_LINE,
                        }),
                        _ => None,
                    })
                    .sum()
            });
            // egui's delta is positive when scrolling up, a browser's deltaY then negative: up zooms in.
            if delta != 0.0
                && let Some(pos) = backdrop.hover_pos()
            {
                let factor = (delta * WHEEL_SENSITIVITY).exp();
                self.view = zoom::zoom_to(self.view, self.view.scale * factor, relative(pos), content, viewport);
            }
        }
        if backdrop.dragged() && self.view.is_zoomed() {
            let delta = backdrop.drag_delta();
            let moved = ZoomView {
                x: self.view.x + delta.x,
                y: self.view.y + delta.y,
                ..self.view
            };
            self.view = zoom::clamp_view(moved, content, viewport);
        }
        let Some(pos) = backdrop.interact_pointer_pos() else {
            return;
        };
        if backdrop.double_clicked() {
            self.close_at = None;
            // On the photo, the first click already switched (REQ-009-16).
            if !on_photo(self.view, pos) {
                self.view = toggle(self.view, pos);
            }
        } else if backdrop.clicked() {
            if on_photo(self.view, pos) {
                self.view = toggle(self.view, pos);
            } else {
                self.close_later(&backdrop.ctx);
            }
        }
    }

    /// _Previous_ / _Next_ and the control bar, faded out while idle (REQ-013-10); false when _Close_ was clicked.
    fn controls(&mut self, ui: &mut egui::Ui, screen: Rect, item: Option<&GalleryItem>, count: usize) -> bool {
        let ctx = ui.ctx().clone();
        let text = bar_text(ui, screen, item);
        let bar = control_bar_rect(screen, &text);
        let previous = (self.index > 0).then(|| {
            Rect::from_center_size(
                pos2(screen.left() + 12.0 + NAV_SIZE / 2.0, screen.center().y),
                Vec2::splat(NAV_SIZE),
            )
        });
        let next = (self.index + 1 < count).then(|| {
            Rect::from_center_size(
                pos2(screen.right() - 12.0 - NAV_SIZE / 2.0, screen.center().y),
                Vec2::splat(NAV_SIZE),
            )
        });
        let pointer = ctx.pointer_latest_pos();
        let hovering = pointer.is_some_and(|pos| {
            bar.contains(pos) || previous.is_some_and(|r| r.contains(pos)) || next.is_some_and(|r| r.contains(pos))
        });
        let idle = self.last_activity.elapsed() >= IDLE;
        let hidden = zoom::controls_hidden(idle, hovering, false, false);
        if !idle {
            ctx.request_repaint_after(IDLE.saturating_sub(self.last_activity.elapsed()));
        }
        let shown = ctx.animate_bool_with_time(ui.make_persistent_id("controls"), !hidden, theme::DURATION_BASE);
        if hidden && pointer.is_some_and(|pos| screen.contains(pos)) {
            ctx.set_cursor_icon(CursorIcon::None);
        }
        if shown == 0.0 {
            return true;
        }

        let mut open = true;
        let mut step = 0;
        ui.scope(|ui| {
            ui.set_opacity(shown);
            for (rect, icon, label, by) in [
                (previous, Icon::ChevronLeft, "Previous", -1),
                (next, Icon::ChevronRight, "Next", 1),
            ] {
                let Some(rect) = rect else { continue };
                let tooltip = if by < 0 { "Previous (←)" } else { "Next (→)" };
                if round_button(ui, rect, icon, label, tooltip).clicked() {
                    step = by;
                }
            }
            if control_bar(ui, bar, text) {
                open = false;
            }
        });
        if step != 0 {
            self.keys.step = step;
            ctx.request_repaint();
        }
        open
    }
}

/// Where the photo is drawn: fitted into `area`, scaled and moved by `view`; parts outside `area` are clipped.
fn photo_rect(area: Rect, fitted: Size, view: ZoomView) -> Rect {
    Rect::from_center_size(
        area.center() + vec2(view.x, view.y),
        vec2(fitted.width, fitted.height) * view.scale,
    )
}

/// Asks for the originals of the shown photo and its neighbours, nearest first (REQ-009-13).
fn preload(gallery: &mut Gallery, originals: &mut Originals, libraries: &[Library], index: usize, count: usize) {
    let mut wanted = Vec::new();
    for at in std::iter::once(index).chain(zoom::neighbours(count, index)) {
        let Some(item) = gallery.item(at) else { continue };
        if item.kind != MediaKind::Image {
            continue;
        }
        let Some(library) = libraries.iter().find(|library| library.id == item.root_id) else {
            continue;
        };
        wanted.push(OriginalRequest {
            media: item.id,
            path: Path::new(&library.path).join(&item.rel_path),
        });
    }
    originals.set(wanted);
}

/// Text in a glass pill centered at `at`.
fn message(ui: &mut egui::Ui, at: Pos2, text: &str) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.into(), egui::FontId::proportional(theme::SIZE_BODY), theme::PAPER);
    let rect = Rect::from_center_size(at, galley.size() + vec2(32.0, 12.0));
    ui.painter().rect_filled(rect, rect.height() / 2.0, GLASS_BAR);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, theme::PAPER);
    // Hover only: clicks still reach the photo and the scrim below.
    let response = ui.interact(rect, ui.make_persistent_id(("message", text)), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
}

fn round_button(ui: &mut egui::Ui, rect: Rect, icon: Icon, label: &str, tooltip: &str) -> egui::Response {
    let response = ui
        .put(
            rect,
            egui::Button::image(icon.image(20.0, theme::PAPER))
                .fill(GLASS_BAR)
                .stroke(egui::Stroke::new(1.0, theme::RIM))
                .corner_radius(NAV_SIZE / 2.0),
        )
        .on_hover_text(tooltip);
    let label = label.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    response
}

/// What the control bar shows: file name, date and rating of the item.
struct BarText {
    name: std::sync::Arc<egui::Galley>,
    date: Option<std::sync::Arc<egui::Galley>>,
    stars: usize,
}

fn bar_text(ui: &egui::Ui, screen: Rect, item: Option<&GalleryItem>) -> BarText {
    let name = item.map_or("", |item| item.file_name.as_str());
    let mut job = egui::text::LayoutJob::simple_singleline(
        name.to_owned(),
        egui::FontId::new(theme::SIZE_TITLE, theme::semibold()),
        theme::PAPER,
    );
    job.wrap = egui::text::TextWrapping {
        max_width: (screen.width() * 0.4).max(80.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let date = item.map(|item| {
        ui.painter().layout_no_wrap(
            format::medium_date_time(item.taken_at.unwrap_or(item.mtime)),
            egui::FontId::new(theme::SIZE_BODY, theme::numbers()),
            theme::PAPER,
        )
    });
    BarText {
        name: ui.painter().layout_job(job),
        date,
        stars: item.and_then(|item| item.rating).unwrap_or(0).clamp(0, 5) as usize,
    }
}

const BAR_GAP: f32 = 12.0;
const STAR_SIZE: f32 = 14.0;

fn control_bar_rect(screen: Rect, text: &BarText) -> Rect {
    let mut width = 20.0 + text.name.size().x + BAR_GAP;
    if let Some(date) = &text.date {
        width += date.size().x + BAR_GAP;
    }
    if text.stars > 0 {
        width += text.stars as f32 * (STAR_SIZE + 2.0) + BAR_GAP;
    }
    // Separator, _Close_ and the right padding.
    width += 1.0 + BAR_GAP + theme::CONTROL_HEIGHT + 6.0;
    let width = width.min(screen.width() - 32.0);
    Rect::from_center_size(
        pos2(screen.center().x, screen.bottom() - 16.0 - BAR_HEIGHT / 2.0),
        vec2(width, BAR_HEIGHT),
    )
}

/// File name, date, rating and _Close_ in a pill at the bottom (REQ-009-01); true when _Close_ was clicked.
fn control_bar(ui: &mut egui::Ui, rect: Rect, text: BarText) -> bool {
    let painter = ui.painter();
    painter.rect(
        rect,
        rect.height() / 2.0,
        GLASS_BAR,
        egui::Stroke::new(1.0, theme::RIM),
        egui::StrokeKind::Inside,
    );
    // Clicks on the bar's background don't reach the scrim behind it.
    ui.interact(rect, ui.make_persistent_id("control bar"), egui::Sense::click());
    let y = rect.center().y;
    let mut x = rect.left() + 20.0;
    let name_width = text.name.size().x;
    painter.galley(pos2(x, y - text.name.size().y / 2.0), text.name, theme::PAPER);
    x += name_width + BAR_GAP;
    if let Some(date) = text.date {
        let width = date.size().x;
        painter.galley(pos2(x, y - date.size().y / 2.0), date, theme::PAPER);
        x += width + BAR_GAP;
    }
    if text.stars > 0 {
        for _ in 0..text.stars {
            let star = Rect::from_min_size(pos2(x, y - STAR_SIZE / 2.0), Vec2::splat(STAR_SIZE));
            Icon::Star.image(STAR_SIZE, theme::STAR).paint_at(ui, star);
            x += STAR_SIZE + 2.0;
        }
        x += BAR_GAP - 2.0;
    }
    ui.painter()
        .vline(x, (y - 10.0)..=(y + 10.0), egui::Stroke::new(1.0, theme::RIM));
    let close = Rect::from_min_size(
        pos2(
            rect.right() - 6.0 - theme::CONTROL_HEIGHT,
            y - theme::CONTROL_HEIGHT / 2.0,
        ),
        Vec2::splat(theme::CONTROL_HEIGHT),
    );
    let response = ui
        .put(
            close,
            egui::Button::image(Icon::X.small())
                .frame(false)
                .corner_radius(theme::CONTROL_HEIGHT / 2.0),
        )
        .on_hover_text("Close (Esc)");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Close"));
    response.clicked()
}
