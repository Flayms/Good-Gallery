//! Editing a photo in the viewer (spec 021). An [`EditSession`] loads the unedited original (the file itself, or the
//! original an earlier edit was rendered from), keeps the recipe with its undo history, draws the adjustments panel
//! and the preview: on the GPU through `edit.wgsl` while there is a wgpu renderer, else rendered on the CPU from a small
//! copy (tests). Saving happens in the app (`AppState::save_edit`), the viewer only asks for it.

use crate::icons::Icon;
use crate::theme;
use domain::edit::{
    Adjustment, CURVE_SAMPLES, Channel, HISTOGRAM_BINS, Histogram, History, Params, Recipe, SavedEdit, Section,
};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2, pos2, vec2};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor, wgpu};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

/// The small copy for the white balance picker and the CPU preview.
#[cfg(not(target_arch = "wasm32"))]
const SMALL_SIDE: u32 = 512;
/// The picker averages this many pixels around the click, in both directions, in the small copy.
const PICK_RADIUS: i64 = 2;
/// About this many pixels of the small copy go into the histogram.
const HISTOGRAM_SAMPLES: u32 = 25_000;
const ROW_HEIGHT: f32 = 22.0;
const SLIDER_HEIGHT: f32 = 16.0;
const SCROLL_BAR_ROOM: f32 = 14.0;

/// The media item being edited and where its file is.
#[derive(Debug, Clone)]
pub struct Target {
    pub media: i64,
    pub root_id: i64,
    pub rel_path: String,
    pub file_name: String,
    /// The library's folder.
    pub library: PathBuf,
    /// The photo's displayed size at full resolution, which grain is measured in.
    pub full_size: [u32; 2],
}

/// What the viewer asks the app to save (REQ-021-09, REQ-021-10).
#[derive(Debug, Clone, PartialEq)]
pub struct SaveEdit {
    pub media: i64,
    pub root_id: i64,
    pub rel_path: String,
    /// The unedited original, relative to the library.
    pub source: String,
    pub recipe: Recipe,
    pub copy: bool,
}

/// Where the user wanted to go when editing had unsaved changes (REQ-021-08).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leave {
    /// Close the viewer.
    Close,
    /// Step to another gallery position.
    Step(usize),
    /// Just stop editing.
    Exit,
}

/// A dialog the session waits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    /// Unsaved changes: save, discard or stay.
    Leave(Leave),
    /// The first replacement of an original: confirm, then leave as asked.
    Replace(Option<Leave>),
}

/// What the panel or a dialog asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Save(Box<SaveEdit>, Option<Leave>),
    Leave(Leave),
}

struct Loaded {
    /// The original's texture as loaded (perhaps smaller than the file, within the GPU's limit). Not destroyed when
    /// editing ends: a frame's queued preview may still draw it (#69); wgpu frees it with its last view.
    view: Option<wgpu::TextureView>,
    small: image::RgbaImage,
    /// The CPU preview and the recipe it shows.
    cpu: Option<(Recipe, egui::TextureHandle)>,
}

enum Base {
    /// Waiting for the item's details (its saved edit).
    Waiting,
    Loading(Receiver<Result<(String, Recipe, Loaded), String>>),
    Loaded(Box<Loaded>),
    Failed(String),
}

pub struct EditSession {
    pub target: Target,
    /// The unedited original, once known.
    source: Option<String>,
    history: History,
    /// The recipe shown: the history's current one, or one being dragged.
    draft: Recipe,
    /// The recipe the file was saved with: no unsaved changes while the draft equals it.
    saved: Recipe,
    base: Base,
    open: [bool; 4],
    channel: Channel,
    dragging_point: Option<usize>,
    picking: bool,
    /// The histogram behind the curve and the steps before the curves it was counted with (REQ-021-18).
    histogram: Option<(Recipe, Histogram)>,
    pub dialog: Option<Dialog>,
    pub saving: bool,
    render_state: Option<egui_wgpu::RenderState>,
}

impl EditSession {
    /// Starts editing `target`; the original loads once [`Self::start`] has the item's saved edit.
    pub fn new(target: Target, render_state: Option<egui_wgpu::RenderState>) -> Self {
        Self {
            target,
            source: None,
            history: History::new(Recipe::default()),
            draft: Recipe::default(),
            saved: Recipe::default(),
            base: Base::Waiting,
            open: [true, false, true, false],
            channel: Channel::All,
            dragging_point: None,
            picking: false,
            histogram: None,
            dialog: None,
            saving: false,
            render_state,
        }
    }

    pub fn media(&self) -> i64 {
        self.target.media
    }

    /// Whether it still waits for the item's saved edit.
    pub fn is_waiting(&self) -> bool {
        matches!(self.base, Base::Waiting)
    }

    /// Loads the unedited original: the source of `edit` while it exists (REQ-021-11), else the file. Its recipe
    /// becomes the starting point.
    pub fn start(&mut self, edit: Option<SavedEdit>, ctx: &egui::Context) {
        if !self.is_waiting() {
            return;
        }
        let (tx, rx) = channel();
        let (target, render_state, ctx) = (self.target.clone(), self.render_state.clone(), ctx.clone());
        let job = move || {
            let result = load(&target, edit, render_state.as_ref());
            let _ = tx.send(result);
            ctx.request_repaint();
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::Builder::new()
            .name("edit original".into())
            .spawn(job)
            .expect("can't start a thread");
        #[cfg(target_arch = "wasm32")]
        job();
        self.base = Base::Loading(rx);
    }

    /// Applies a finished load. Call once per frame.
    pub fn poll(&mut self) {
        let Base::Loading(rx) = &self.base else { return };
        let Ok(result) = rx.try_recv() else { return };
        match result {
            Ok((source, recipe, loaded)) => {
                self.source = Some(source);
                self.history = History::new(recipe.clone());
                self.draft = recipe.clone();
                self.saved = recipe;
                self.base = Base::Loaded(Box::new(loaded));
            }
            Err(error) => {
                tracing::warn!("can't load the original to edit: {error}");
                self.base = Base::Failed(error);
            }
        }
    }

    /// Waits until the original has loaded or failed (tests).
    pub fn wait(&mut self) {
        if let Base::Loading(rx) = &self.base
            && let Ok(result) = rx.recv()
        {
            let (tx, rx) = channel();
            let _ = tx.send(result);
            self.base = Base::Loading(rx);
        }
        self.poll();
    }

    pub fn is_loaded(&self) -> bool {
        matches!(self.base, Base::Loaded(_))
    }

    pub fn failed(&self) -> Option<&str> {
        match &self.base {
            Base::Failed(error) => Some(error),
            _ => None,
        }
    }

    pub fn recipe(&self) -> &Recipe {
        &self.draft
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// Whether the shown recipe differs from the one the file was saved with.
    pub fn has_changes(&self) -> bool {
        self.draft != self.saved
    }

    /// Whether _Save_ would replace an original that has no backup yet, so it asks first (REQ-021-10).
    pub fn replaces_original(&self) -> bool {
        self.source.as_deref() == Some(self.target.rel_path.as_str())
    }

    /// The histogram behind the curve, once _Curves_ was shown (REQ-021-18).
    pub fn histogram(&self) -> Option<&Histogram> {
        self.histogram.as_ref().map(|(_, histogram)| histogram)
    }

    /// Counts the histogram again when a step before the curves changed.
    fn update_histogram(&mut self) {
        let Base::Loaded(loaded) = &self.base else { return };
        let key = before_curves(&self.draft);
        if self.histogram.as_ref().is_some_and(|(counted, _)| *counted == key) {
            return;
        }
        let small = &loaded.small;
        let pixels = u64::from(small.width()) * u64::from(small.height());
        let step = ((pixels as f64 / f64::from(HISTOGRAM_SAMPLES)).sqrt().ceil() as u32).max(1);
        let samples = small
            .enumerate_pixels()
            .filter(|(x, y, _)| x % step == 0 && y % step == 0)
            .map(|(_, _, pixel)| [pixel[0], pixel[1], pixel[2]].map(|c| f32::from(c) / 255.0));
        let histogram = Params::new(&key).histogram(samples);
        self.histogram = Some((key, histogram));
    }

    pub fn is_picking(&self) -> bool {
        self.picking
    }

    /// Sets the recipe and records it for undo.
    pub fn commit(&mut self, recipe: Recipe) {
        self.draft = recipe.clone();
        self.history.push(recipe);
    }

    pub fn undo(&mut self) {
        if self.history.undo() {
            self.draft = self.history.current().clone();
        }
    }

    pub fn redo(&mut self) {
        if self.history.redo() {
            self.draft = self.history.current().clone();
        }
    }

    /// The request to save the shown recipe.
    pub fn save_request(&self, copy: bool) -> Option<SaveEdit> {
        Some(SaveEdit {
            media: self.target.media,
            root_id: self.target.root_id,
            rel_path: self.target.rel_path.clone(),
            source: self.source.clone()?,
            recipe: self.draft.clone(),
            copy,
        })
    }

    /// _Save_ (over the original): asks first the first time, then saves and leaves as asked.
    pub fn save(&mut self, copy: bool, then: Option<Leave>) -> Option<Outcome> {
        if self.saving || !self.is_loaded() {
            return None;
        }
        if !copy && self.replaces_original() && self.dialog != Some(Dialog::Replace(then)) {
            self.dialog = Some(Dialog::Replace(then));
            return None;
        }
        self.dialog = None;
        let request = self.save_request(copy)?;
        self.saving = true;
        Some(Outcome::Save(Box::new(request), then))
    }

    /// Leaves as asked, after asking about unsaved changes (REQ-021-08).
    pub fn leave(&mut self, leave: Leave) -> Option<Outcome> {
        if self.saving {
            return None;
        }
        if self.has_changes() {
            self.dialog = Some(Dialog::Leave(leave));
            None
        } else {
            Some(Outcome::Leave(leave))
        }
    }

    /// Picks the white balance at `uv` of the photo (REQ-021-05).
    pub fn pick(&mut self, uv: Pos2) {
        self.picking = false;
        let Base::Loaded(loaded) = &self.base else { return };
        let small = &loaded.small;
        let (cx, cy) = (
            (uv.x.clamp(0.0, 1.0) * (small.width() - 1) as f32).round() as i64,
            (uv.y.clamp(0.0, 1.0) * (small.height() - 1) as f32).round() as i64,
        );
        let mut sum = [0.0f32; 3];
        let mut count = 0.0;
        for y in (cy - PICK_RADIUS)..=(cy + PICK_RADIUS) {
            for x in (cx - PICK_RADIUS)..=(cx + PICK_RADIUS) {
                if x < 0 || y < 0 || x >= i64::from(small.width()) || y >= i64::from(small.height()) {
                    continue;
                }
                let pixel = small.get_pixel(x as u32, y as u32);
                for (c, total) in sum.iter_mut().enumerate() {
                    *total += f32::from(pixel[c]) / 255.0;
                }
                count += 1.0;
            }
        }
        if count == 0.0 {
            return;
        }
        let (temperature, tint) = domain::edit::neutralize(sum.map(|c| c / count));
        let mut recipe = self.draft.clone();
        Adjustment::Temperature.set(&mut recipe, temperature.round());
        Adjustment::Tint.set(&mut recipe, tint.round());
        self.commit(recipe);
    }

    /// Draws the photo through the recipe into `rect` (texture coordinates `uv`), clipped to `clip`; the original
    /// while `compare` (REQ-021-02, REQ-021-07). False while there is nothing to draw yet.
    pub fn paint(&mut self, ui: &egui::Ui, rect: Rect, uv: Rect, clip: Rect, compare: bool, opacity: f32) -> bool {
        let Base::Loaded(loaded) = &mut self.base else {
            return false;
        };
        let ctx = ui.ctx();
        if let (Some(view), Some(render_state)) = (&loaded.view, &self.render_state)
            && render_state
                .renderer
                .read()
                .callback_resources
                .contains::<EditResources>()
        {
            let ppp = ctx.pixels_per_point();
            let shown = rect.intersect(clip);
            if shown.is_positive() {
                let flags = f32::from(u8::from(compare)) + if render_state.target_format.is_srgb() { 2.0 } else { 0.0 };
                let uniform = uniform(
                    &Params::new(&self.draft),
                    [rect.min.x * ppp, rect.min.y * ppp, rect.max.x * ppp, rect.max.y * ppp],
                    [uv.min.x, uv.min.y, uv.max.x, uv.max.y],
                    self.target.full_size.map(|side| side as f32),
                    opacity,
                    flags,
                );
                ui.painter()
                    .with_clip_rect(clip)
                    .add(egui_wgpu::Callback::new_paint_callback(
                        shown,
                        PreviewCallback {
                            view: view.clone(),
                            uniform,
                            group: std::sync::OnceLock::new(),
                        },
                    ));
            }
            return true;
        }
        // Without a wgpu renderer: the small copy, rendered on the CPU when the recipe changed.
        let recipe = if compare { Recipe::default() } else { self.draft.clone() };
        if loaded.cpu.as_ref().is_none_or(|(shown, _)| *shown != recipe) {
            let image = cpu_preview(&loaded.small, &recipe, self.target.full_size);
            match &mut loaded.cpu {
                Some((shown, handle)) => {
                    handle.set(image, egui::TextureOptions::LINEAR);
                    *shown = recipe;
                }
                None => {
                    let handle = ctx.load_texture("edit preview", image, egui::TextureOptions::LINEAR);
                    loaded.cpu = Some((recipe, handle));
                }
            }
        }
        if let Some((_, handle)) = &loaded.cpu {
            ui.painter()
                .with_clip_rect(clip)
                .image(handle.id(), rect, uv, Color32::WHITE.gamma_multiply(opacity));
        }
        true
    }

    /// The adjustments panel (REQ-021-03 … REQ-021-07): undo, redo, compare, the sections, and Reset all, Cancel,
    /// Save as copy and Save. Returns what was asked for, and whether _Compare_ is held.
    pub fn panel(&mut self, ui: &mut egui::Ui, rect: Rect) -> (Option<Outcome>, bool) {
        let mut outcome = None;
        let inner = rect.shrink(16.0);
        let footer_height = 2.0 * theme::CONTROL_HEIGHT + 8.0;
        let header = Rect::from_min_size(inner.min, vec2(inner.width(), theme::CONTROL_HEIGHT));
        let footer = Rect::from_min_max(pos2(inner.left(), inner.bottom() - footer_height), inner.max);
        let body = Rect::from_min_max(
            pos2(inner.left(), header.bottom() + 12.0),
            pos2(inner.right(), footer.top() - 12.0),
        );

        // Header: title, undo, redo, compare.
        let mut compare = false;
        let mut header_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(header)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        header_ui.label(egui::RichText::new("Edit").font(egui::FontId::new(theme::SIZE_TITLE, theme::semibold())));
        header_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let held = icon_button(
                ui,
                Icon::SquareSplitHorizontal,
                "Compare",
                "Hold to compare with the original (\\)",
                true,
            );
            compare = held.is_pointer_button_down_on();
            if icon_button(ui, Icon::Redo2, "Redo", "Redo (Ctrl+Shift+Z)", self.history.can_redo()).clicked() {
                self.redo();
            }
            if icon_button(ui, Icon::Undo2, "Undo", "Undo (Ctrl+Z)", self.history.can_undo()).clicked() {
                self.undo();
            }
        });

        let mut body_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(body)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        body_ui.set_clip_rect(body.intersect(ui.clip_rect()));
        match &self.base {
            Base::Waiting | Base::Loading(_) => {
                body_ui.spinner();
            }
            Base::Failed(error) => {
                body_ui.add(egui::Label::new(format!("Can't load the original: {error}")).wrap());
            }
            Base::Loaded(_) => {
                egui::ScrollArea::vertical()
                    .id_salt("edit panel")
                    .auto_shrink(false)
                    .show(&mut body_ui, |ui| self.sections(ui));
            }
        }

        // Footer: Reset all, Cancel / Save as copy / Save.
        let mut footer_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(footer)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ready = self.is_loaded() && !self.saving;
        footer_ui.horizontal(|ui| {
            let reset = ui.add_enabled(ready && !self.draft.is_neutral(), egui::Button::new("Reset all"));
            if reset.clicked() {
                self.commit(Recipe::default());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(!self.saving, egui::Button::new("Cancel")).clicked() {
                    outcome = self.leave(Leave::Exit);
                }
            });
        });
        footer_ui.add_space(8.0);
        footer_ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.saving { "Saving…" } else { "Save" };
                let save = egui::Button::new(
                    egui::RichText::new(label)
                        .color(theme::base())
                        .font(egui::FontId::new(theme::SIZE_BODY, theme::semibold())),
                )
                .fill(theme::text());
                let changed = self.has_changes();
                if ui.add_enabled(ready && changed, save).clicked() {
                    outcome = self.save(false, None);
                }
                if ui
                    .add_enabled(ready && !self.draft.is_neutral(), egui::Button::new("Save as copy"))
                    .clicked()
                {
                    outcome = self.save(true, None);
                }
            });
        });
        // Clicks and wheel turns on the panel stay in it.
        (outcome, compare)
    }

    fn sections(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        for (i, section) in Section::ALL.into_iter().enumerate() {
            let open = &mut self.open[i];
            let icon = if *open { Icon::ChevronDown } else { Icon::ChevronRight };
            let header = ui.add(
                egui::Button::image_and_text(
                    icon.image(14.0, theme::muted()),
                    egui::RichText::new(section.label()).font(egui::FontId::new(theme::SIZE_BODY, theme::semibold())),
                )
                .frame(false),
            );
            let label = section.label();
            let expanded = *open;
            let header = header.on_hover_text(if expanded { "Collapse" } else { "Expand" });
            header.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, expanded, label));
            if header.clicked() {
                *open = !*open;
            }
            if !self.open[i] {
                continue;
            }
            if section == Section::Curves {
                self.curves(ui);
            } else {
                if section == Section::Color {
                    let picking = self.picking;
                    let button =
                        egui::Button::image_and_text(Icon::Pipette.small(), "Pick a neutral spot").selected(picking);
                    let response = ui
                        .add(button)
                        .on_hover_text("Click a spot in the photo that should be grey");
                    if response.clicked() {
                        self.picking = !picking;
                    }
                }
                for adjustment in Adjustment::ALL.into_iter().filter(|a| a.section() == section) {
                    self.slider(ui, adjustment);
                }
            }
            ui.add_space(8.0);
        }
    }

    /// One adjustment: its name and value, and a slider. A double-click resets it (REQ-021-07).
    fn slider(&mut self, ui: &mut egui::Ui, adjustment: Adjustment) {
        // Clear of the scroll bar, which floats over the content.
        let width = ui.available_width() - SCROLL_BAR_ROOM;
        let (row, _) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT + SLIDER_HEIGHT), egui::Sense::hover());
        let value = adjustment.get(&self.draft);
        let text = if adjustment == Adjustment::Exposure {
            format!("{value:+.2}")
        } else if value == 0.0 || !matches!(adjustment.range().0, x if x < 0.0) {
            format!("{value:.0}")
        } else {
            format!("{value:+.0}")
        };
        let label_rect = Rect::from_min_size(row.min, vec2(width, ROW_HEIGHT));
        let painter = ui.painter();
        let font = egui::FontId::proportional(theme::SIZE_BODY);
        painter.text(
            label_rect.left_center(),
            egui::Align2::LEFT_CENTER,
            adjustment.label(),
            font,
            theme::text(),
        );
        painter.text(
            label_rect.right_center(),
            egui::Align2::RIGHT_CENTER,
            text,
            egui::FontId::new(theme::SIZE_BODY, theme::numbers()),
            theme::muted(),
        );
        let track = Rect::from_min_size(pos2(row.left(), label_rect.bottom()), vec2(width, SLIDER_HEIGHT));
        let (low, high) = adjustment.range();
        let step = if adjustment == Adjustment::Exposure { 0.05 } else { 1.0 };
        let mut current = f64::from(value);
        let response = crate::widgets::slider(
            ui,
            track,
            &mut current,
            f64::from(low)..=f64::from(high),
            step,
            adjustment.label(),
        );
        if response.double_clicked() {
            let mut recipe = self.draft.clone();
            adjustment.set(&mut recipe, adjustment.neutral());
            self.commit(recipe);
            return;
        }
        if response.changed() {
            adjustment.set(&mut self.draft, current as f32);
        }
        // One undo step per finished drag, click or key press.
        if response.drag_stopped() || response.clicked() || (response.changed() && !response.dragged()) {
            let recipe = self.draft.clone();
            self.commit(recipe);
        }
    }

    /// The tone curve editor (REQ-021-04): channel tabs, a square with the curve; a click adds a point, a drag moves
    /// it, a double-click on a point removes it.
    fn curves(&mut self, ui: &mut egui::Ui) {
        self.update_histogram();
        ui.horizontal(|ui| {
            for channel in Channel::ALL {
                let selected = self.channel == channel;
                let response = ui.add(egui::Button::selectable(selected, channel.label()));
                if response.clicked() {
                    self.channel = channel;
                }
            }
        });
        let side = (ui.available_width() - SCROLL_BAR_ROOM).min(256.0);
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), egui::Sense::click_and_drag());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Canvas, true, "Tone curve"));
        let to_screen = |[x, y]: [f32; 2]| pos2(rect.left() + x * rect.width(), rect.bottom() - y * rect.height());
        let from_screen = |pos: Pos2| {
            [
                ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
                ((rect.bottom() - pos.y) / rect.height()).clamp(0.0, 1.0),
            ]
        };
        let channel = self.channel;
        let near = |curve: &domain::edit::Curve, pos: Pos2| {
            curve
                .points
                .iter()
                .position(|&point| to_screen(point).distance(pos) <= 8.0)
        };
        if let Some(pos) = response.interact_pointer_pos() {
            let curve = self.draft.curves.get(channel).clone();
            if response.double_clicked() {
                if let Some(i) = near(&curve, pos) {
                    let mut recipe = self.draft.clone();
                    recipe.curves.get_mut(channel).remove_point(i);
                    self.commit(recipe);
                }
                self.dragging_point = None;
            } else if response.drag_started() || (response.is_pointer_button_down_on() && self.dragging_point.is_none())
            {
                self.dragging_point = near(&curve, pos).or_else(|| {
                    let [x, y] = from_screen(pos);
                    self.draft.curves.get_mut(channel).add_point(x, y)
                });
            }
            if let Some(i) = self.dragging_point
                && response.is_pointer_button_down_on()
            {
                let [x, y] = from_screen(pos);
                self.draft.curves.get_mut(channel).move_point(i, x, y);
            }
        }
        if (response.drag_stopped() || response.clicked()) && self.dragging_point.take().is_some() {
            let recipe = self.draft.clone();
            self.commit(recipe);
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, theme::field());
        let color = match channel {
            Channel::All => theme::text(),
            Channel::Red => Color32::from_rgb(240, 90, 90),
            Channel::Green => Color32::from_rgb(110, 210, 110),
            Channel::Blue => Color32::from_rgb(110, 150, 245),
        };
        if let Some(histogram) = self.histogram() {
            paint_histogram(&painter, rect, &histogram.get(channel), color.gamma_multiply(0.22));
        }
        for i in 1..4 {
            let t = i as f32 / 4.0;
            let stroke = egui::Stroke::new(1.0, theme::rim());
            painter.vline(rect.left() + t * rect.width(), rect.y_range(), stroke);
            painter.hline(rect.x_range(), rect.top() + t * rect.height(), stroke);
        }
        let curve = self.draft.curves.get(channel);
        let line: Vec<Pos2> = (0..=64)
            .map(|i| {
                let x = i as f32 / 64.0;
                to_screen([x, curve.eval(x)])
            })
            .collect();
        painter.add(egui::Shape::line(line, egui::Stroke::new(1.5, color)));
        for &point in &curve.points {
            painter.circle_filled(to_screen(point), 4.0, color);
        }
    }
}

/// The recipe's steps before the curves, which alone shape the histogram behind the curve.
fn before_curves(recipe: &Recipe) -> Recipe {
    let mut before = Recipe::default();
    for adjustment in Adjustment::ALL {
        if matches!(adjustment.section(), Section::Light)
            || matches!(adjustment, Adjustment::Temperature | Adjustment::Tint)
        {
            adjustment.set(&mut before, adjustment.get(recipe));
        }
    }
    before
}

/// `bins` as bars from the bottom of `rect`, scaled to the tallest bin apart from the two ends, so clipped blacks or
/// whites don't flatten the rest.
fn paint_histogram(painter: &egui::Painter, rect: Rect, bins: &[u32; HISTOGRAM_BINS], color: Color32) {
    let tallest = bins[1..HISTOGRAM_BINS - 1].iter().copied().max().unwrap_or(0).max(1) as f32;
    let width = rect.width() / HISTOGRAM_BINS as f32;
    let mut mesh = egui::Mesh::default();
    for (i, &count) in bins.iter().enumerate() {
        let height = (count as f32 / tallest).min(1.0) * rect.height();
        if height <= 0.0 {
            continue;
        }
        let left = rect.left() + i as f32 * width;
        let bar = Rect::from_min_max(pos2(left, rect.bottom() - height), pos2(left + width, rect.bottom()));
        mesh.add_colored_rect(bar, color);
    }
    painter.add(mesh);
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, label: &str, tooltip: &str, enabled: bool) -> egui::Response {
    let response = ui
        .add_enabled(
            enabled,
            egui::Button::image(icon.small())
                .frame(false)
                .corner_radius(theme::CONTROL_HEIGHT / 2.0)
                .min_size(Vec2::splat(theme::CONTROL_HEIGHT)),
        )
        .on_hover_text(tooltip);
    let label = label.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &label));
    response
}

/// Loads the unedited original of `target` (the edit's source while it exists) as a texture and a small copy.
#[cfg(not(target_arch = "wasm32"))]
fn load(
    target: &Target,
    edit: Option<SavedEdit>,
    render_state: Option<&egui_wgpu::RenderState>,
) -> Result<(String, Recipe, Loaded), String> {
    let file = |rel: &str| db::media::absolute_path(&target.library.to_string_lossy(), rel);
    let (source, recipe) = match edit {
        Some(edit) if file(&edit.source).is_some_and(|path| path.is_file()) => (edit.source.clone(), edit.recipe()),
        _ => (target.rel_path.clone(), Recipe::default()),
    };
    let path = file(&source).ok_or("the path leaves its library")?;
    let max_side = render_state.map_or(SMALL_SIDE, |render_state| {
        render_state.device.limits().max_texture_dimension_2d.min(8192)
    });
    let image = media::original::load_original(&path, None, max_side).map_err(|error| error.to_string())?;
    let small = if image.width().max(image.height()) > SMALL_SIDE {
        let (width, height) = domain::thumbs::fit_inside(image.width(), image.height(), SMALL_SIDE, SMALL_SIDE, false);
        image::imageops::resize(&image, width, height, image::imageops::FilterType::Triangle)
    } else {
        image.clone()
    };
    let view = render_state.map(|render_state| {
        let size = [image.width(), image.height()];
        let levels = crate::textures::mip_chain(size, image.into_raw());
        crate::textures::upload(&render_state.device, &render_state.queue, size, &levels)
            .create_view(&wgpu::TextureViewDescriptor::default())
    });
    Ok((source, recipe, Loaded { view, small, cpu: None }))
}

/// The browser version doesn't edit (REQ-021-01).
#[cfg(target_arch = "wasm32")]
fn load(
    _target: &Target,
    _edit: Option<SavedEdit>,
    _render_state: Option<&egui_wgpu::RenderState>,
) -> Result<(String, Recipe, Loaded), String> {
    Err("editing isn't available here".into())
}

/// `small` through `recipe`, on the CPU.
fn cpu_preview(small: &image::RgbaImage, recipe: &Recipe, full_size: [u32; 2]) -> egui::ColorImage {
    let params = Params::new(recipe);
    let (width, height) = small.dimensions();
    let size = full_size.map(|side| side.max(1) as f32);
    let mut rgba = Vec::with_capacity(small.as_raw().len());
    for (x, y, pixel) in small.enumerate_pixels() {
        let uv = [(x as f32 + 0.5) / width as f32, (y as f32 + 0.5) / height as f32];
        let rgb = [pixel[0], pixel[1], pixel[2]].map(|c| f32::from(c) / 255.0);
        let out = params.pixel(rgb, uv, size);
        rgba.extend(out.map(|c| (c * 255.0).round() as u8));
        rgba.push(pixel[3]);
    }
    egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba)
}

/// The shader's uniform: `edit.wgsl`'s `Edit`, in its order.
fn uniform(params: &Params, rect: [f32; 4], uv: [f32; 4], full_size: [f32; 2], opacity: f32, flags: f32) -> Vec<f32> {
    let mut floats = Vec::with_capacity(28 + CURVE_SAMPLES * 4);
    floats.extend_from_slice(&rect);
    floats.extend_from_slice(&uv);
    floats.extend_from_slice(&[full_size[0], full_size[1], opacity, flags]);
    let [r, g, b] = params.gains.map(|gain| gain * params.exposure);
    floats.extend_from_slice(&[r, g, b, f32::from(u8::from(params.curves.is_some()))]);
    floats.extend_from_slice(&[params.black, params.white, params.shadows, params.highlights]);
    floats.extend_from_slice(&[params.contrast, params.saturation, params.vibrance, params.brightness]);
    floats.extend_from_slice(&[params.vignette, params.vignette_start, params.grain, params.grain_cell]);
    for i in 0..CURVE_SAMPLES {
        let [r, g, b] = params.curves.as_ref().map_or([0.0; 3], |table| table[i]);
        floats.extend_from_slice(&[r, g, b, 0.0]);
    }
    floats
}

struct EditResources {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl EditResources {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("edit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("edit.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("edit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("edit"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("edit"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("edit"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
        }
    }

    fn bind(&self, device: &wgpu::Device, view: &wgpu::TextureView, uniform: &[f32]) -> wgpu::BindGroup {
        let buffer = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("edit"),
                contents: &uniform.iter().flat_map(|f| f.to_ne_bytes()).collect::<Vec<u8>>(),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("edit"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: buffer.as_entire_binding(),
                },
            ],
        })
    }
}

/// Installs the preview's pipeline into the egui renderer. Call once at startup.
pub fn install(render_state: &egui_wgpu::RenderState) {
    let resources = EditResources::new(&render_state.device, render_state.target_format);
    render_state.renderer.write().callback_resources.insert(resources);
}

struct PreviewCallback {
    view: wgpu::TextureView,
    uniform: Vec<f32>,
    /// Made in `prepare`; dropped with the frame, so the photo's texture isn't kept alive after editing ends.
    group: std::sync::OnceLock<wgpu::BindGroup>,
}

impl CallbackTrait for PreviewCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(res) = resources.get::<EditResources>() {
            let _ = self.group.set(res.bind(device, &self.view, &self.uniform));
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let Some(res) = resources.get::<EditResources>() else {
            return;
        };
        let Some(group) = self.group.get() else { return };
        pass.set_pipeline(&res.pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: u32 = 48;

    /// Colors across the range, with a ramp, saturated and skin-like patches.
    fn pattern() -> image::RgbaImage {
        image::RgbaImage::from_fn(SIZE, SIZE, |x, y| {
            let t = x as f32 / (SIZE - 1) as f32;
            let rgb = match y * 4 / SIZE {
                0 => [t, t, t],
                1 => [t, 0.3, 1.0 - t],
                2 => [0.85, 0.65 * t + 0.2, 0.5],
                _ => [0.1 + 0.8 * t, 0.9 - 0.5 * t, 0.2],
            };
            let [r, g, b] = rgb.map(|c| (c * 255.0).round() as u8);
            image::Rgba([r, g, b, 255])
        })
    }

    fn recipes() -> Vec<Recipe> {
        let mut all = Recipe::default();
        for (adjustment, value) in [
            (Adjustment::Exposure, 0.7),
            (Adjustment::Brightness, 35.0),
            (Adjustment::Contrast, 40.0),
            (Adjustment::Highlights, -50.0),
            (Adjustment::Shadows, 60.0),
            (Adjustment::Whites, 20.0),
            (Adjustment::Blacks, -30.0),
            (Adjustment::Temperature, 30.0),
            (Adjustment::Tint, -20.0),
            (Adjustment::Vibrance, 50.0),
            (Adjustment::Saturation, -20.0),
            (Adjustment::Vignette, -60.0),
            (Adjustment::Grain, 40.0),
        ] {
            adjustment.set(&mut all, value);
        }
        all.curves.all.add_point(0.3, 0.2);
        all.curves.blue.add_point(0.6, 0.7);
        let mut light = Recipe::default();
        Adjustment::Vignette.set(&mut light, 70.0);
        Adjustment::Contrast.set(&mut light, -50.0);
        Adjustment::Brightness.set(&mut light, -60.0);
        vec![Recipe::default(), all, light]
    }

    /// Renders `image` through `recipe` with the shader, at 1:1, and reads it back.
    fn render_gpu(render_state: &egui_wgpu::RenderState, image: &image::RgbaImage, recipe: &Recipe) -> Vec<u8> {
        let (device, queue) = (&render_state.device, &render_state.queue);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let resources = EditResources::new(device, format);
        let size = [SIZE, SIZE];
        let levels = crate::textures::mip_chain(size, image.as_raw().clone());
        let source = crate::textures::upload(device, queue, size, &levels);
        let view = source.create_view(&wgpu::TextureViewDescriptor::default());
        let side = SIZE as f32;
        let uniform = uniform(
            &Params::new(recipe),
            [0.0, 0.0, side, side],
            [0.0, 0.0, 1.0, 1.0],
            [side, side],
            1.0,
            0.0,
        );
        let group = resources.bind(device, &view, &uniform);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("edit test"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let row = (SIZE * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("edit test"),
            size: u64::from(row * SIZE),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("edit test"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&resources.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(SIZE),
                },
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| result.expect("mapped"));
        device.poll(wgpu::PollType::wait_indefinitely()).expect("polled");
        let data = slice.get_mapped_range().expect("mapped");
        let pixels = (0..SIZE as usize)
            .flat_map(|y| data[y * row as usize..y * row as usize + SIZE as usize * 4].to_vec())
            .collect();
        drop(data);
        buffer.unmap();
        pixels
    }

    /// REQ-021-02, REQ-021-06: the GPU preview shows what saving renders on the CPU (within two levels), grain and
    /// vignette included.
    #[test]
    fn req_021_02_req_021_06_the_gpu_preview_matches_the_cpu_render() {
        let render_state = egui_kittest::wgpu::create_render_state(
            egui_kittest::wgpu::default_wgpu_setup(),
            egui_wgpu::RendererOptions::default(),
        );
        let image = pattern();
        for recipe in recipes() {
            let gpu = render_gpu(&render_state, &image, &recipe);
            let cpu = cpu_preview(&image, &recipe, [SIZE, SIZE]);
            let mut worst = 0;
            for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
                for c in 0..3 {
                    let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
                    assert!(
                        diff <= 2,
                        "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?} with {recipe:?}"
                    );
                    worst = worst.max(diff);
                }
            }
            eprintln!("worst difference: {worst}");
        }
    }

    #[test]
    fn req_021_07_req_021_08_tracks_changes_undo_and_asks_before_leaving() {
        let target = Target {
            media: 1,
            root_id: 1,
            rel_path: "a.jpg".into(),
            file_name: "a.jpg".into(),
            library: PathBuf::from("/photos"),
            full_size: [10, 10],
        };
        let mut session = EditSession::new(target, None);
        session.base = Base::Loaded(Box::new(Loaded {
            view: None,
            small: image::RgbaImage::from_pixel(10, 10, image::Rgba([200, 150, 100, 255])),
            cpu: None,
        }));
        session.source = Some("a.jpg".into());
        assert_eq!(session.leave(Leave::Close), Some(Outcome::Leave(Leave::Close)));

        let mut recipe = Recipe::default();
        Adjustment::Exposure.set(&mut recipe, 1.0);
        session.commit(recipe);
        assert!(session.has_changes());
        assert_eq!(session.leave(Leave::Step(3)), None);
        assert_eq!(session.dialog, Some(Dialog::Leave(Leave::Step(3))));
        session.undo();
        assert!(!session.has_changes());
        session.redo();
        assert_eq!(session.recipe().exposure, 1.0);

        // The first save over the original asks; confirmed, it saves.
        session.dialog = None;
        assert_eq!(session.save(false, None), None);
        assert_eq!(session.dialog, Some(Dialog::Replace(None)));
        let Some(Outcome::Save(request, None)) = session.save(false, None) else {
            panic!("saves once confirmed")
        };
        assert_eq!((request.source.as_str(), request.copy), ("a.jpg", false));
        assert!(session.saving);
        // While saving, nothing else starts.
        assert_eq!(session.save(true, None), None);
        assert_eq!(session.leave(Leave::Exit), None);
    }

    /// REQ-021-08 (#69): _Discard_ ends editing after the frame's preview was queued; drawing it must still work.
    #[test]
    fn req_021_08_discarding_after_the_preview_was_queued_still_draws_it() {
        let render_state = egui_kittest::wgpu::create_render_state(
            egui_kittest::wgpu::default_wgpu_setup(),
            egui_wgpu::RendererOptions::default(),
        );
        let (device, queue) = (&render_state.device, &render_state.queue);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let size = [SIZE, SIZE];
        let texture = crate::textures::upload(
            device,
            queue,
            size,
            &crate::textures::mip_chain(size, pattern().into_raw()),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let target = Target {
            media: 1,
            root_id: 1,
            rel_path: "a.jpg".into(),
            file_name: "a.jpg".into(),
            library: PathBuf::from("/photos"),
            full_size: size,
        };
        let mut session = EditSession::new(target, Some(render_state.clone()));
        session.base = Base::Loaded(Box::new(Loaded {
            view: Some(view.clone()),
            small: pattern(),
            cpu: None,
        }));
        let callback = PreviewCallback {
            view,
            uniform: uniform(
                &Params::new(&Recipe::default()),
                [0.0, 0.0, SIZE as f32, SIZE as f32],
                [0.0, 0.0, 1.0, 1.0],
                [SIZE as f32; 2],
                1.0,
                0.0,
            ),
            group: std::sync::OnceLock::new(),
        };
        // Discard: the session ends, then the frame is drawn.
        drop(session);
        let mut resources = CallbackResources::default();
        resources.insert(EditResources::new(device, format));
        let screen = ScreenDescriptor {
            size_in_pixels: size,
            pixels_per_point: 1.0,
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        callback.prepare(device, queue, &screen, &mut encoder, &mut resources);
        let frame = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("edit test"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let frame_view = frame.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("edit test"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut pass = pass.forget_lifetime();
            let info = egui::PaintCallbackInfo {
                viewport: Rect::from_min_size(Pos2::ZERO, Vec2::splat(SIZE as f32)),
                clip_rect: Rect::from_min_size(Pos2::ZERO, Vec2::splat(SIZE as f32)),
                pixels_per_point: 1.0,
                screen_size_px: size,
            };
            callback.paint(info, &mut pass, &resources);
        }
        queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::wait_indefinitely()).expect("polled");
    }

    #[test]
    fn req_021_05_the_picker_neutralizes_the_picked_spot() {
        let target = Target {
            media: 1,
            root_id: 1,
            rel_path: "a.jpg".into(),
            file_name: "a.jpg".into(),
            library: PathBuf::from("/photos"),
            full_size: [10, 10],
        };
        let mut session = EditSession::new(target, None);
        let warm = image::RgbaImage::from_pixel(10, 10, image::Rgba([190, 160, 130, 255]));
        session.base = Base::Loaded(Box::new(Loaded {
            view: None,
            small: warm.clone(),
            cpu: None,
        }));
        session.picking = true;
        session.pick(pos2(0.5, 0.5));
        assert!(!session.is_picking());
        let out =
            Params::new(session.recipe()).pixel([190.0 / 255.0, 160.0 / 255.0, 130.0 / 255.0], [0.5; 2], [10.0; 2]);
        assert!(
            (out[0] - out[2]).abs() < 0.02 && (out[0] - out[1]).abs() < 0.02,
            "{out:?}"
        );
    }
}
