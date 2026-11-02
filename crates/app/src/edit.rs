//! Editing a photo in the viewer (spec 021). An [`EditSession`] loads the unedited original (the file itself, or the
//! original an earlier edit was rendered from), keeps the recipe with its undo history, draws the adjustments panel
//! and the preview: on the GPU through `edit.wgsl` while there is a wgpu renderer, else rendered on the CPU from a small
//! copy (tests). Saving happens in the app (`AppState::save_edit`), the viewer only asks for it.

use crate::icons::Icon;
use crate::originals::OriginalState;
use crate::theme;
use domain::crop::{self, Grip, Ratio};
use domain::edit::{
    Adjustment, BlurKind, CURVE_SAMPLES, Channel, Geometry, HISTOGRAM_BINS, Histogram, History, LocalMask,
    MAX_MASK_SHAPES, MAX_MASKS, Mask, MaskOp, MaskPart, MaskShape, Params, Part, Pyramid, Recipe, SavedEdit, Section,
    blur_lod,
};
use domain::look::{Look, LookChoice};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2, pos2, vec2};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor, wgpu};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

/// The small copy for the white balance picker and the CPU preview.
#[cfg(not(target_arch = "wasm32"))]
const SMALL_SIDE: u32 = 512;
/// The picker averages this many pixels around the click, in both directions, in the small copy.
const PICK_RADIUS: i64 = 2;
/// How far from the crop's frame a press still takes hold of it, in points.
const GRIP_REACH: f32 = 12.0;
/// How long the crop's corner marks are, in points.
const GRIP_LENGTH: f32 = 18.0;
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
    /// Turns a photo without an orientation of its own (REQ-004-12).
    pub sidecar_orientation: Option<u8>,
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
    /// The subject the preview showed, for a recipe with parts (REQ-022-05).
    pub subject: Option<Arc<Mask>>,
    /// Turns an original without an orientation of its own (REQ-004-12).
    pub sidecar_orientation: Option<u8>,
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
    /// The original's texture as loaded (perhaps smaller than the file, within the GPU's limit), or the viewer's
    /// (REQ-021-19). Not destroyed when editing ends: a frame's queued preview may still draw it (#69); wgpu frees it
    /// with its last view.
    view: Option<wgpu::TextureView>,
    /// Once read back from the viewer's texture, or decoded with it.
    small: Option<image::RgbaImage>,
    /// The CPU preview, and the recipe, the part of the photo and the flags it shows.
    cpu: Option<(Recipe, Geometry, u32, egui::TextureHandle)>,
}

/// The photo's subject (REQ-022-02).
enum Subject {
    /// Not looked for yet.
    Unknown,
    Finding(Receiver<Result<Mask, String>>),
    /// Found, with its texture for the preview.
    Found(Arc<Mask>, Option<wgpu::TextureView>),
    Failed(String),
}

/// The original's pixels.
enum Base {
    /// Not loading yet: the unedited original isn't known yet, or the viewer still loads it.
    Waiting,
    /// Decoding it from its file.
    Loading(Receiver<Result<Loaded, String>>),
    Loaded(Box<Loaded>),
    Failed(String),
}

pub struct EditSession {
    pub target: Target,
    /// Whether [`Self::start`] had the item's saved edit.
    started: bool,
    /// Finding out whether an earlier edit's source is still there (REQ-021-11).
    resolving: Option<Receiver<(String, Recipe)>>,
    /// The unedited original, once known; the panel shows from then on (REQ-021-19).
    source: Option<String>,
    history: History,
    /// The recipe shown: the history's current one, or one being dragged.
    draft: Recipe,
    /// The recipe the file was saved with: no unsaved changes while the draft equals it.
    saved: Recipe,
    base: Base,
    /// Reading the small copy back from the viewer's texture.
    reading: Option<Receiver<Result<image::RgbaImage, String>>>,
    open: [bool; 4],
    /// Whether the _Look_ section is open.
    look_open: bool,
    geometry_open: bool,
    masks_open: bool,
    /// Whether the crop's frame is shown over the whole photo (REQ-029-02).
    cropping: bool,
    crop_ratio: Ratio,
    /// Whether the crop's 4:3 or 16:9 is upright.
    crop_portrait: bool,
    /// The drag under way: what it took hold of and the frame when it began.
    crop_drag: Option<(Grip, [f32; 4])>,
    /// Where the whole photo was last drawn while cropping.
    crop_area: Option<Rect>,
    /// The gradient's handle being dragged: its second if true (REQ-030-03).
    gradient_drag: Option<bool>,
    /// Where the photo was last drawn with a gradient chosen.
    gradient_area: Option<Rect>,
    /// Which shape of the chosen mask the panel changes (REQ-030-10).
    mask_shape: usize,
    channel: Channel,
    dragging_point: Option<usize>,
    picking: bool,
    /// The next click on the photo places the blur's focus (REQ-023-03).
    placing_focus: bool,
    /// The histogram behind the curve and the steps before the curves it was counted with (REQ-021-18).
    histogram: Option<(Recipe, Histogram)>,
    /// The part the panel's light and color change (REQ-022-01).
    part: Part,
    subject: Subject,
    /// The subject is looked for once the small copy is there.
    subject_wanted: bool,
    /// Tint the chosen part in the preview (REQ-022-04).
    show_selection: bool,
    pub dialog: Option<Dialog>,
    pub saving: bool,
    render_state: Option<egui_wgpu::RenderState>,
    ctx: Option<egui::Context>,
}

impl EditSession {
    /// Starts editing `target`; the original loads once [`Self::start`] has the item's saved edit.
    pub fn new(target: Target, render_state: Option<egui_wgpu::RenderState>) -> Self {
        Self {
            target,
            started: false,
            resolving: None,
            source: None,
            history: History::new(Recipe::default()),
            draft: Recipe::default(),
            saved: Recipe::default(),
            base: Base::Waiting,
            reading: None,
            open: [true, false, true, false],
            look_open: true,
            geometry_open: true,
            masks_open: false,
            cropping: false,
            crop_ratio: Ratio::Free,
            crop_portrait: false,
            crop_drag: None,
            crop_area: None,
            gradient_drag: None,
            gradient_area: None,
            mask_shape: 0,
            channel: Channel::All,
            dragging_point: None,
            picking: false,
            placing_focus: false,
            histogram: None,
            part: Part::Photo,
            subject: Subject::Unknown,
            subject_wanted: false,
            show_selection: false,
            dialog: None,
            saving: false,
            render_state,
            ctx: None,
        }
    }

    pub fn media(&self) -> i64 {
        self.target.media
    }

    /// Whether it still waits for the item's saved edit.
    pub fn is_waiting(&self) -> bool {
        !self.started
    }

    /// Starts from the item's saved edit: its recipe, from the source of `edit` while it exists (REQ-021-11), else
    /// from the file. The panel shows once that is known, at once without an earlier edit (REQ-021-19); the pixels come
    /// through [`Self::use_shown`].
    pub fn start(&mut self, edit: Option<SavedEdit>, ctx: &egui::Context) {
        if self.started {
            return;
        }
        self.started = true;
        self.ctx = Some(ctx.clone());
        let Some(edit) = edit else {
            self.set_source(self.target.rel_path.clone(), Recipe::default());
            return;
        };
        // Whether the source is still there is a look at the library, perhaps over the network.
        let (tx, rx) = channel();
        let (target, ctx) = (self.target.clone(), ctx.clone());
        spawn("edit source", move || {
            let _ = tx.send(resolve(&target, &edit));
            ctx.request_repaint();
        });
        self.resolving = Some(rx);
    }

    fn set_source(&mut self, source: String, recipe: Recipe) {
        self.source = Some(source);
        // The item's size is the saved file's; the recipe's crop and turns start from the whole photo (REQ-029-07).
        let [width, height] = self.target.full_size;
        let (width, height) = recipe.geometry().original_size(width, height);
        self.target.full_size = [width, height];
        self.history = History::new(recipe.clone());
        self.draft = recipe.clone();
        self.saved = recipe;
        // Saved with parts or a background blur: they need the subject again (REQ-022-05).
        if self.draft.needs_subject() {
            self.find_subject();
        }
    }

    /// Takes the original the viewer shows (`shown`) when it is the unedited one, so the file isn't read again
    /// (REQ-021-19); waits while the viewer still loads it. Else loads the unedited original from its file. Call once
    /// per frame.
    pub fn use_shown(&mut self, shown: &OriginalState, lend: impl FnOnce() -> Option<wgpu::TextureView>) {
        let (Some(source), Base::Waiting) = (&self.source, &self.base) else {
            return;
        };
        if *source == self.target.rel_path {
            match shown {
                OriginalState::Loading => return,
                OriginalState::Loaded(_) if self.render_state.is_some() => {
                    if let Some(view) = lend() {
                        self.read_small(view.clone());
                        self.base = Base::Loaded(Box::new(Loaded {
                            view: Some(view),
                            small: None,
                            cpu: None,
                        }));
                        return;
                    }
                }
                _ => {}
            }
        }
        self.load_file(source.clone());
    }

    /// Decodes `source` from the library.
    fn load_file(&mut self, source: String) {
        let (tx, rx) = channel();
        let (target, render_state, ctx) = (self.target.clone(), self.render_state.clone(), self.ctx.clone());
        spawn("edit original", move || {
            let _ = tx.send(load(&target, &source, render_state.as_ref()));
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
        self.base = Base::Loading(rx);
    }

    /// Reads the small copy back from the viewer's texture, on a thread.
    fn read_small(&mut self, view: wgpu::TextureView) {
        let Some(render_state) = self.render_state.clone() else {
            return;
        };
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        spawn("edit small copy", move || {
            let _ = tx.send(read_small(&render_state, view.texture()));
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
        self.reading = Some(rx);
    }

    /// Applies what finished meanwhile: the unedited original, its pixels, its small copy, the subject. Call once per
    /// frame.
    pub fn poll(&mut self) {
        if let Subject::Finding(rx) = &self.subject
            && let Ok(result) = rx.try_recv()
        {
            self.subject = match result {
                Ok(mask) => {
                    let view = self
                        .render_state
                        .as_ref()
                        .map(|render_state| upload_mask(render_state, &mask));
                    Subject::Found(Arc::new(mask), view)
                }
                Err(error) => {
                    tracing::warn!("can't find the subject: {error}");
                    Subject::Failed(error)
                }
            };
        }
        if let Some(rx) = &self.resolving
            && let Ok((source, recipe)) = rx.try_recv()
        {
            self.resolving = None;
            self.set_source(source, recipe);
        }
        if let Base::Loading(rx) = &self.base
            && let Ok(result) = rx.try_recv()
        {
            match result {
                Ok(loaded) => {
                    self.base = Base::Loaded(Box::new(loaded));
                    self.small_ready();
                }
                Err(error) => {
                    tracing::warn!("can't load the original to edit: {error}");
                    self.base = Base::Failed(error);
                }
            }
        }
        if let Some(rx) = &self.reading
            && let Ok(result) = rx.try_recv()
        {
            self.reading = None;
            match (result, &mut self.base) {
                (Ok(small), Base::Loaded(loaded)) => {
                    loaded.small = Some(small);
                    self.small_ready();
                }
                (Ok(_), _) => {}
                (Err(error), _) => {
                    // Read from the file then.
                    tracing::warn!("can't read the shown original back: {error}");
                    if let Some(source) = self.source.clone() {
                        self.load_file(source);
                    }
                }
            }
        }
    }

    fn small_ready(&mut self) {
        if std::mem::take(&mut self.subject_wanted) {
            self.find_subject();
        }
    }

    /// Whether something is still on its way.
    fn pending(&self) -> bool {
        self.resolving.is_some()
            || self.reading.is_some()
            || matches!(self.base, Base::Loading(_))
            || matches!(self.subject, Subject::Finding(_))
    }

    /// Waits until what is on its way arrived: the unedited original, its pixels, and the subject if it is looked
    /// for (tests). Pixels the viewer still loads come with a later [`Self::use_shown`].
    pub fn wait(&mut self) {
        self.poll();
        while self.pending() {
            std::thread::sleep(std::time::Duration::from_millis(2));
            self.poll();
        }
    }

    /// Starts looking for the subject in the original, once (REQ-022-02).
    fn find_subject(&mut self) {
        let Subject::Unknown = &self.subject else { return };
        let Some(small) = self.small().cloned() else {
            self.subject_wanted = true;
            return;
        };
        let (tx, rx) = channel();
        spawn("find subject", move || {
            let _ = tx.send(find(&small));
        });
        self.subject = Subject::Finding(rx);
    }

    /// The small copy, once there.
    fn small(&self) -> Option<&image::RgbaImage> {
        match &self.base {
            Base::Loaded(loaded) => loaded.small.as_ref(),
            _ => None,
        }
    }

    /// The part the panel changes.
    pub fn part(&self) -> Part {
        self.part
    }

    /// Chooses the part the panel changes, and looks for the subject the first time it is needed (REQ-022-01).
    pub fn choose_part(&mut self, part: Part) {
        if part != self.part {
            self.show_selection = false;
            self.mask_shape = 0;
        }
        self.part = part;
        if matches!(part, Part::Subject | Part::Background) {
            self.find_subject();
        }
    }

    /// The subject, once found.
    pub fn subject(&self) -> Option<&Mask> {
        match &self.subject {
            Subject::Found(mask, _) => Some(mask),
            _ => None,
        }
    }

    /// Whether looking for the subject failed.
    pub fn subject_failed(&self) -> Option<&str> {
        match &self.subject {
            Subject::Failed(error) => Some(error),
            _ => None,
        }
    }

    pub fn set_show_selection(&mut self, show: bool) {
        self.show_selection = show && self.part != Part::Photo;
    }

    /// Whether the original and its small copy are there.
    pub fn is_loaded(&self) -> bool {
        self.small().is_some()
    }

    /// Whether the unedited original and the recipe to start from are known, so the panel shows (REQ-021-19).
    pub fn is_ready(&self) -> bool {
        self.source.is_some()
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
        let Some(small) = self.small() else { return };
        let key = before_curves(&self.draft);
        if self.histogram.as_ref().is_some_and(|(counted, _)| *counted == key) {
            return;
        }
        let pixels = u64::from(small.width()) * u64::from(small.height());
        let step = ((pixels as f64 / f64::from(HISTOGRAM_SAMPLES)).sqrt().ceil() as u32).max(1);
        let samples = small
            .enumerate_pixels()
            .filter(|(x, y, _)| x % step == 0 && y % step == 0)
            .map(|(_, _, pixel)| [pixel[0], pixel[1], pixel[2]].map(|c| f32::from(c) / 255.0));
        let histogram = Params::new(&key).histogram(samples);
        self.histogram = Some((key, histogram));
    }

    /// Whether the next click on the photo picks a neutral spot or places the blur's focus.
    pub fn is_picking(&self) -> bool {
        self.picking || self.placing_focus
    }

    /// Sets the recipe and records it for undo.
    pub fn commit(&mut self, mut recipe: Recipe) {
        recipe.tidy();
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
            subject: match &self.subject {
                Subject::Found(mask, _) if self.draft.needs_subject() => Some(mask.clone()),
                _ => None,
            },
            sidecar_orientation: self.target.sidecar_orientation,
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

    /// Asks for the next [`Self::pick`] to place the blur's focus (REQ-023-03).
    pub fn place_focus(&mut self) {
        self.placing_focus = true;
    }

    /// The size to fit into the media area while editing draws the photo: the output, or the whole unedited photo while
    /// comparing (REQ-029-01, REQ-029-05). `None` until the original is there, while the viewer shows the file.
    pub fn shown_size(&self, compare: bool) -> Option<[f32; 2]> {
        if !matches!(self.base, Base::Loaded(_)) {
            return None;
        }
        let [width, height] = self.target.full_size;
        // The index doesn't know this photo's size: the viewer goes by the texture.
        if width == 0 || height == 0 {
            return None;
        }
        let (width, height) = self.shown_geometry(compare).output_size(width, height);
        Some([width as f32, height as f32])
    }

    /// What is drawn of the photo: while comparing all of it unturned, while cropping all of it turned, else the output.
    fn shown_geometry(&self, compare: bool) -> Geometry {
        if compare {
            Geometry::new(None, 0)
        } else if self.cropping {
            Geometry::new(None, self.draft.quarter_turns)
        } else {
            self.draft.geometry()
        }
    }

    pub fn is_cropping(&self) -> bool {
        self.cropping
    }

    /// Where the whole photo was last drawn while cropping.
    pub fn crop_area(&self) -> Option<Rect> {
        self.crop_area
    }

    pub fn set_crop_area(&mut self, area: Rect) {
        self.crop_area = Some(area);
    }

    pub fn is_dragging_crop(&self) -> bool {
        self.crop_drag.is_some()
    }

    /// Shows the crop's frame over the whole photo, or leaves it (REQ-029-02).
    pub fn set_cropping(&mut self, cropping: bool) {
        self.cropping = cropping;
        self.crop_drag = None;
    }

    /// The whole photo's size as it is turned on screen.
    fn turned_size(&self) -> [f32; 2] {
        let [width, height] = self.target.full_size;
        let (width, height) = Geometry::new(None, self.draft.quarter_turns).output_size(width, height);
        [width as f32, height as f32]
    }

    fn crop_frame(&self) -> [f32; 4] {
        self.draft.geometry().frame()
    }

    fn with_frame(&self, frame: [f32; 4]) -> Recipe {
        let mut recipe = self.draft.clone();
        recipe.crop = Geometry::crop_of_frame(frame, recipe.quarter_turns);
        recipe
    }

    /// Picks a shape: the largest frame of it inside the current one (REQ-029-03).
    fn set_crop_ratio(&mut self, ratio: Ratio) {
        self.crop_ratio = ratio;
        let (frame, size) = (self.crop_frame(), self.turned_size());
        self.crop_portrait = crop::is_portrait(frame, size);
        if let Some(value) = ratio.value(self.crop_portrait, size) {
            self.commit(self.with_frame(crop::fit_ratio(frame, value, size)));
        }
    }

    /// Turns the 4:3 or 16:9 to its other form (REQ-029-03).
    fn swap_crop_ratio(&mut self) {
        self.crop_portrait = !self.crop_portrait;
        self.commit(self.with_frame(crop::swap_sides(self.crop_frame(), self.turned_size())));
    }

    /// Takes drags on the crop's frame, over the whole photo at `area` (REQ-029-02).
    pub fn crop_input(&mut self, ui: &egui::Ui, backdrop: &egui::Response, area: Rect) {
        let fraction = |pos: Pos2| {
            [
                (pos.x - area.left()) / area.width(),
                (pos.y - area.top()) / area.height(),
            ]
        };
        let tolerance = [GRIP_REACH / area.width(), GRIP_REACH / area.height()];
        let frame = self.crop_frame();
        let hover = backdrop
            .hover_pos()
            .and_then(|pos| crop::grip_at(frame, fraction(pos), tolerance));
        if let Some(grip) = self.crop_drag.map(|(grip, _)| grip).or(hover) {
            ui.ctx().set_cursor_icon(match grip {
                Grip::Move => egui::CursorIcon::Grab,
                Grip::Left | Grip::Right => egui::CursorIcon::ResizeHorizontal,
                Grip::Top | Grip::Bottom => egui::CursorIcon::ResizeVertical,
                Grip::TopLeft | Grip::BottomRight => egui::CursorIcon::ResizeNwSe,
                Grip::TopRight | Grip::BottomLeft => egui::CursorIcon::ResizeNeSw,
            });
        }
        let origin = ui.input(|input| input.pointer.press_origin());
        if backdrop.drag_started()
            && let Some(origin) = origin
        {
            self.crop_drag = crop::grip_at(frame, fraction(origin), tolerance).map(|grip| (grip, frame));
        }
        if let (Some((grip, start)), Some(origin), Some(pos)) =
            (self.crop_drag, origin, backdrop.interact_pointer_pos())
        {
            let delta = [(pos.x - origin.x) / area.width(), (pos.y - origin.y) / area.height()];
            let size = self.turned_size();
            let ratio = self.crop_ratio.value(self.crop_portrait, size);
            self.draft = self.with_frame(crop::drag(start, grip, delta, ratio, size));
            ui.ctx().request_repaint();
        }
        // One undo step per drag.
        if backdrop.drag_stopped() && self.crop_drag.take().is_some() {
            self.commit(self.draft.clone());
        }
    }

    /// The chosen mask's gradient, if it is one: its handles.
    fn gradient(&self) -> Option<([f32; 2], [f32; 2])> {
        let Part::Mask(i) = self.part else { return None };
        match self.draft.masks.get(i)?.shape_at(self.mask_shape) {
            MaskShape::Linear { from, to } => Some((from, to)),
            _ => None,
        }
    }

    /// Whether the chosen mask is a gradient, whose handles need the whole photo (REQ-030-03).
    pub fn is_on_gradient(&self) -> bool {
        self.gradient().is_some()
    }

    pub fn is_dragging_gradient(&self) -> bool {
        self.gradient_drag.is_some()
    }

    /// Where the photo was last drawn with a gradient chosen.
    pub fn gradient_area(&self) -> Option<Rect> {
        self.gradient_area
    }

    pub fn set_gradient_area(&mut self, area: Rect) {
        self.gradient_area = Some(area);
    }

    /// A point of the original on the photo at `area`.
    fn on_photo(&self, point: [f32; 2], area: Rect) -> Pos2 {
        let [u, v] = self.shown_geometry(false).to_output(point);
        pos2(area.left() + u * area.width(), area.top() + v * area.height())
    }

    /// Takes drags on the chosen gradient's handles, on the photo at `area` (REQ-030-03).
    pub fn gradient_input(&mut self, ui: &egui::Ui, backdrop: &egui::Response, area: Rect) {
        let Some((from, to)) = self.gradient() else { return };
        let (first, second) = (self.on_photo(from, area), self.on_photo(to, area));
        let grabs = |pos: Pos2| -> Option<bool> {
            let (near_first, near_second) = (pos.distance(first), pos.distance(second));
            (near_first.min(near_second) <= GRIP_REACH).then_some(near_second < near_first)
        };
        if self.gradient_drag.is_some() || backdrop.hover_pos().and_then(grabs).is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let origin = ui.input(|input| input.pointer.press_origin());
        if backdrop.drag_started()
            && let Some(origin) = origin
        {
            self.gradient_drag = grabs(origin);
        }
        if let (Some(second), Some(pos), Part::Mask(i)) =
            (self.gradient_drag, backdrop.interact_pointer_pos(), self.part)
        {
            let [u, v] = self.shown_geometry(false).to_original([
                (pos.x - area.left()) / area.width(),
                (pos.y - area.top()) / area.height(),
            ]);
            let point = [u.clamp(-1.0, 2.0), v.clamp(-1.0, 2.0)];
            let chosen = self.mask_shape;
            if let Some(MaskShape::Linear { from, to }) = self.draft.masks.get_mut(i).map(|mask| mask.shape_mut(chosen))
            {
                *(if second { to } else { from }) = point;
            }
            ui.ctx().request_repaint();
        }
        // One undo step per drag.
        if backdrop.drag_stopped() && self.gradient_drag.take().is_some() {
            self.commit(self.draft.clone());
        }
    }

    /// The chosen gradient over the photo at `area`: its two handles, each with a line across the photo.
    pub fn paint_gradient(&self, painter: &egui::Painter, area: Rect) {
        let Some((from, to)) = self.gradient() else { return };
        let (first, second) = (self.on_photo(from, area), self.on_photo(to, area));
        let painter = painter.with_clip_rect(area);
        let dark = egui::Stroke::new(3.0, Color32::from_black_alpha(120));
        let white = egui::Stroke::new(1.0, Color32::WHITE);
        let across = (second - first).normalized().rot90() * (area.width() + area.height());
        for stroke in [dark, white] {
            painter.line_segment([first, second], stroke);
            for handle in [first, second] {
                painter.line_segment([handle - across, handle + across], stroke);
            }
        }
        painter.circle(first, 6.0, Color32::WHITE, dark);
        painter.circle(
            second,
            6.0,
            Color32::from_black_alpha(160),
            egui::Stroke::new(1.5, Color32::WHITE),
        );
    }

    /// The crop's frame over the whole photo at `area`: darkened outside, thirds and grips inside (REQ-029-02).
    pub fn paint_crop(&self, painter: &egui::Painter, area: Rect) {
        let [x, y, w, h] = self.crop_frame();
        let frame = Rect::from_min_size(
            pos2(area.left() + x * area.width(), area.top() + y * area.height()),
            Vec2::new(w * area.width(), h * area.height()),
        );
        let dark = Color32::from_black_alpha(150);
        for outside in [
            Rect::from_min_max(area.min, pos2(area.right(), frame.top())),
            Rect::from_min_max(pos2(area.left(), frame.bottom()), area.max),
            Rect::from_min_max(pos2(area.left(), frame.top()), pos2(frame.left(), frame.bottom())),
            Rect::from_min_max(pos2(frame.right(), frame.top()), pos2(area.right(), frame.bottom())),
        ] {
            painter.rect_filled(outside, 0.0, dark);
        }
        let thin = egui::Stroke::new(1.0, Color32::from_white_alpha(120));
        for third in [1.0, 2.0] {
            let across = frame.left() + frame.width() * third / 3.0;
            let down = frame.top() + frame.height() * third / 3.0;
            painter.line_segment([pos2(across, frame.top()), pos2(across, frame.bottom())], thin);
            painter.line_segment([pos2(frame.left(), down), pos2(frame.right(), down)], thin);
        }
        painter.rect_stroke(
            frame,
            0.0,
            egui::Stroke::new(1.0, Color32::WHITE),
            egui::StrokeKind::Outside,
        );
        let grip = egui::Stroke::new(3.0, Color32::WHITE);
        let long = GRIP_LENGTH.min(frame.width() / 2.0).min(frame.height() / 2.0);
        for (corner, sign_x, sign_y) in [
            (frame.left_top(), 1.0, 1.0),
            (frame.right_top(), -1.0, 1.0),
            (frame.left_bottom(), 1.0, -1.0),
            (frame.right_bottom(), -1.0, -1.0),
        ] {
            painter.line_segment([corner, corner + Vec2::new(sign_x * long, 0.0)], grip);
            painter.line_segment([corner, corner + Vec2::new(0.0, sign_y * long)], grip);
        }
    }

    /// Turns the photo by `quarters` quarter turns clockwise (REQ-029-01).
    fn rotate(&mut self, quarters: u8) {
        if quarters % 2 == 1 {
            self.crop_portrait = !self.crop_portrait;
        }
        let mut recipe = self.draft.clone();
        recipe.quarter_turns = (recipe.quarter_turns + quarters) % 4;
        self.commit(recipe);
    }

    /// Picks the white balance at `uv` of the photo (REQ-021-05), or places the blur's focus there (REQ-023-03).
    pub fn pick(&mut self, uv: Pos2) {
        // `uv` is on the output as shown; the photo's own coordinates stay on the content (REQ-029-04).
        let [x, y] = self
            .shown_geometry(false)
            .to_original([uv.x.clamp(0.0, 1.0), uv.y.clamp(0.0, 1.0)]);
        let uv = pos2(x, y);
        if std::mem::take(&mut self.placing_focus) {
            let mut recipe = self.draft.clone();
            recipe.blur_focus = [uv.x.clamp(0.0, 1.0), uv.y.clamp(0.0, 1.0)];
            self.commit(recipe);
            return;
        }
        self.picking = false;
        let Some(small) = self.small() else { return };
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
        let geometry = self.shown_geometry(compare);
        let Base::Loaded(loaded) = &mut self.base else {
            return false;
        };
        let ctx = ui.ctx();
        let (mask, mask_view) = match &self.subject {
            Subject::Found(mask, view) => (Some(mask.as_ref()), view.clone()),
            _ => (None, None),
        };
        // Parts by the mask (4); the selection tinted (8), the background's (16) (REQ-022-03, REQ-022-04).
        let mut flags = u32::from(compare);
        // The chosen mask tinted, as index + 1 above the other flags (REQ-030-07).
        if let (Part::Mask(i), true) = (self.part, self.show_selection) {
            flags |= (i as u32 + 1) << 8;
        }
        if mask.is_some() {
            flags |= 4;
            if self.show_selection && matches!(self.part, Part::Subject | Part::Background) {
                flags |= 8 | if self.part == Part::Background { 16 } else { 0 };
            }
        }
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
                let flags = flags | if render_state.target_format.is_srgb() { 2 } else { 0 };
                let uniform = uniform(
                    &Params::new(&self.draft),
                    geometry,
                    [rect.min.x * ppp, rect.min.y * ppp, rect.max.x * ppp, rect.max.y * ppp],
                    [uv.min.x, uv.min.y, uv.max.x, uv.max.y],
                    self.target.full_size.map(|side| side as f32),
                    opacity,
                    flags as f32,
                );
                ui.painter()
                    .with_clip_rect(clip)
                    .add(egui_wgpu::Callback::new_paint_callback(
                        shown,
                        PreviewCallback {
                            view: view.clone(),
                            mask: mask_view,
                            uniform,
                            group: std::sync::OnceLock::new(),
                        },
                    ));
            }
            return true;
        }
        // Without a wgpu renderer: the small copy, rendered on the CPU when the recipe changed.
        let Some(small) = &loaded.small else { return false };
        let recipe = if compare { Recipe::default() } else { self.draft.clone() };
        if loaded
            .cpu
            .as_ref()
            .is_none_or(|(shown, shown_geometry, shown_flags, _)| {
                *shown != recipe || *shown_geometry != geometry || *shown_flags != flags
            })
        {
            let image = cpu_preview(small, &recipe, geometry, self.target.full_size, mask, flags);
            match &mut loaded.cpu {
                Some((shown, shown_geometry, shown_flags, handle)) => {
                    handle.set(image, egui::TextureOptions::LINEAR);
                    *shown = recipe;
                    *shown_geometry = geometry;
                    *shown_flags = flags;
                }
                None => {
                    let handle = ctx.load_texture("edit preview", image, egui::TextureOptions::LINEAR);
                    loaded.cpu = Some((recipe, geometry, flags, handle));
                }
            }
        }
        if let Some((_, _, _, handle)) = &loaded.cpu {
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
        // The sliders show once the recipe is known, while the original may still load (REQ-021-19).
        match &self.base {
            Base::Failed(error) => {
                body_ui.add(egui::Label::new(format!("Can't load the original: {error}")).wrap());
            }
            _ if !self.is_ready() => {
                body_ui.spinner();
            }
            _ => {
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
        if !self.parts(ui) {
            return;
        }
        // Crop and rotate belong to the whole photo (spec 029).
        if self.part == Part::Photo {
            section_header(ui, "Crop and rotate", &mut self.geometry_open);
            if self.geometry_open {
                ui.horizontal(|ui| {
                    if icon_button(ui, Icon::RotateCcw, "Rotate left", "Rotate left", true).clicked() {
                        self.rotate(3);
                    }
                    if icon_button(ui, Icon::RotateCw, "Rotate right", "Rotate right", true).clicked() {
                        self.rotate(1);
                    }
                    let cropping = self.cropping;
                    let button = egui::Button::image_and_text(Icon::Crop.small(), "Crop").selected(cropping);
                    let response = ui.add(button);
                    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, cropping, "Crop"));
                    if response.clicked() {
                        self.set_cropping(!cropping);
                    }
                });
                if self.cropping {
                    self.crop_shapes(ui);
                }
                ui.add_space(8.0);
            }
        }
        // A film look, under all other adjustments; the subject and background have none (REQ-028-01).
        if self.part == Part::Photo {
            section_header(ui, "Look", &mut self.look_open);
            if self.look_open {
                self.looks(ui);
                ui.add_space(8.0);
            }
        }
        for (i, section) in Section::ALL.into_iter().enumerate() {
            // The subject and the background have light and color only (REQ-022-01).
            if self.part != Part::Photo && matches!(section, Section::Curves | Section::Effects) {
                continue;
            }
            section_header(ui, section.label(), &mut self.open[i]);
            if !self.open[i] {
                continue;
            }
            if section == Section::Curves {
                self.curves(ui);
            } else {
                if section == Section::Color && self.part == Part::Photo {
                    let picking = self.picking;
                    let button =
                        egui::Button::image_and_text(Icon::Pipette.small(), "Pick a neutral spot").selected(picking);
                    // It picks from the small copy.
                    let response = ui
                        .add_enabled(self.small().is_some(), button)
                        .on_hover_text("Click a spot in the photo that should be grey");
                    if response.clicked() {
                        self.picking = !picking;
                    }
                }
                for adjustment in Adjustment::ALL.into_iter().filter(|a| a.section() == section) {
                    if adjustment == Adjustment::Blur {
                        self.blur_kind(ui);
                    }
                    let kind = self.draft.blur_kind;
                    let shown = match adjustment {
                        Adjustment::Blur => kind != BlurKind::None,
                        Adjustment::BlurSize | Adjustment::BlurFeather => kind.has_focus(),
                        _ => true,
                    };
                    if shown {
                        self.slider(ui, adjustment);
                    }
                }
            }
            ui.add_space(8.0);
        }
        // The subject and the background have their own switch; masks go with the photo.
        if matches!(self.part, Part::Photo | Part::Mask(_)) {
            section_header(ui, "Masks", &mut self.masks_open);
            if self.masks_open {
                self.add_mask(ui);
            }
            ui.add_space(8.0);
        }
    }

    /// The switch between the photo, its subject and its background, and the state of finding the subject
    /// (REQ-022-01, REQ-022-02, REQ-022-04). False while the chosen part's sliders can't be shown yet.
    fn parts(&mut self, ui: &mut egui::Ui) -> bool {
        ui.horizontal(|ui| {
            for part in Part::ALL {
                let response = ui.add(egui::Button::selectable(self.part == part, part.label()));
                if response.clicked() {
                    self.choose_part(part);
                }
            }
        });
        ui.add_space(4.0);
        self.masks(ui);
        if self.part == Part::Photo {
            ui.add_space(4.0);
            return true;
        }
        if let Part::Mask(i) = self.part {
            self.mask_controls(ui, i);
            ui.add_space(8.0);
            return true;
        }
        let ready = match &self.subject {
            Subject::Unknown | Subject::Finding(_) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Finding the subject…");
                });
                false
            }
            Subject::Failed(_) => {
                ui.add(egui::Label::new("No subject found in this photo. You can still edit the whole photo.").wrap());
                false
            }
            Subject::Found(..) => {
                let shown = self.show_selection;
                let button = egui::Button::new("Show selection").selected(shown);
                let response = ui.add(button).on_hover_text("Tint the part these adjustments change");
                let label = "Show selection";
                response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, shown, label));
                if response.clicked() {
                    self.set_show_selection(!shown);
                }
                true
            }
        };
        ui.add_space(8.0);
        ready
    }

    /// The photo's masks to choose from, once it has any (REQ-030-01).
    fn masks(&mut self, ui: &mut egui::Ui) {
        // Undo may have taken the chosen mask away.
        if matches!(self.part, Part::Mask(i) if i >= self.draft.masks.len()) {
            self.choose_part(Part::Photo);
        }
        if self.draft.masks.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for i in 0..self.draft.masks.len() {
                let label = format!("{} {}", self.draft.masks[i].shape.label(), i + 1);
                if ui
                    .add(egui::Button::selectable(self.part == Part::Mask(i), label))
                    .clicked()
                {
                    self.choose_part(Part::Mask(i));
                }
            }
        });
        ui.add_space(4.0);
    }

    /// One button per kind of mask (REQ-030-01).
    fn add_mask(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let room = self.draft.masks.len() < MAX_MASKS;
            for (label, shape) in [
                ("Add gradient", MaskShape::LINEAR),
                ("Add luminance", MaskShape::LUMINANCE),
                ("Add color", MaskShape::COLOR),
            ] {
                if ui.add_enabled(room, egui::Button::new(label)).clicked() {
                    let mut recipe = self.draft.clone();
                    recipe.masks.push(LocalMask::new(shape));
                    let chosen = Part::Mask(recipe.masks.len() - 1);
                    self.commit(recipe);
                    self.choose_part(chosen);
                }
            }
        });
    }

    /// What the chosen mask is made of and selects, and turning it around, showing or removing it
    /// (REQ-030-03 … REQ-030-07, REQ-030-10, REQ-030-11).
    fn mask_controls(&mut self, ui: &mut egui::Ui, i: usize) {
        let Some(mask) = self.draft.masks.get(i) else { return };
        let (count, invert) = (mask.shapes(), mask.invert);
        // Undo may have taken the chosen shape away.
        self.mask_shape = self.mask_shape.min(count - 1);
        let chosen = self.mask_shape;
        let shape = mask.shape_at(chosen);
        if count > 1 {
            ui.horizontal_wrapped(|ui| {
                for j in 0..count {
                    let label = format!("{}. {}", j + 1, self.draft.masks[i].shape_at(j).label());
                    if ui.add(egui::Button::selectable(chosen == j, label)).clicked() {
                        self.mask_shape = j;
                    }
                }
            });
            ui.add_space(4.0);
        }
        if let Some(op) = chosen.checked_sub(1).map(|j| self.draft.masks[i].more[j].op) {
            ui.horizontal_wrapped(|ui| {
                for choice in MaskOp::ALL {
                    if ui.add(egui::Button::selectable(op == choice, choice.label())).clicked() {
                        let mut recipe = self.draft.clone();
                        recipe.masks[i].more[chosen - 1].op = choice;
                        self.commit(recipe);
                    }
                }
                if ui.button("Remove shape").clicked() {
                    let mut recipe = self.draft.clone();
                    recipe.masks[i].more.remove(chosen - 1);
                    self.commit(recipe);
                    self.mask_shape = chosen - 1;
                }
            });
            ui.add_space(4.0);
        }
        if matches!(shape, MaskShape::Linear { .. }) {
            let hint = "Drag the two handles on the photo: full at the white one, none at the dark one.";
            ui.add(egui::Label::new(hint).wrap());
            ui.add_space(4.0);
        }
        if self.draft.masks[i].uses_subject() {
            match &self.subject {
                Subject::Found(..) => {}
                Subject::Failed(_) => {
                    ui.add(egui::Label::new("No subject found in this photo, so that shape selects nothing.").wrap());
                }
                Subject::Unknown | Subject::Finding(_) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Finding the subject…");
                    });
                }
            }
        }
        for (slider, (label, value, range, unit)) in shape.sliders().into_iter().enumerate() {
            let mut current = f64::from(value);
            let response = slider_row(ui, label, &format!("{value:.0}{unit}"), &mut current, range, 1.0);
            if response.changed() {
                self.draft.masks[i].shape_mut(chosen).set_slider(slider, current as f32);
            }
            if response.drag_stopped() || response.clicked() || (response.changed() && !response.dragged()) {
                self.commit(self.draft.clone());
            }
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for (label, shape) in [
                ("+ Gradient", MaskShape::LINEAR),
                ("+ Luminance", MaskShape::LUMINANCE),
                ("+ Color", MaskShape::COLOR),
                ("+ Subject", MaskShape::Subject),
                ("+ Background", MaskShape::Background),
            ] {
                let room = count < MAX_MASK_SHAPES;
                if ui.add_enabled(room, egui::Button::new(label)).clicked() {
                    let mut recipe = self.draft.clone();
                    recipe.masks[i].more.push(MaskPart {
                        op: MaskOp::Intersect,
                        shape,
                    });
                    self.commit(recipe);
                    self.mask_shape = count;
                    if shape.uses_subject() {
                        self.find_subject();
                    }
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let response = ui.add(egui::Button::new("Invert").selected(invert));
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, invert, "Invert"));
            if response.clicked() {
                let mut recipe = self.draft.clone();
                recipe.masks[i].invert = !invert;
                self.commit(recipe);
            }
            let shown = self.show_selection;
            let response = ui
                .add(egui::Button::new("Show selection").selected(shown))
                .on_hover_text("Tint the part these adjustments change");
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, shown, "Show selection"));
            if response.clicked() {
                self.set_show_selection(!shown);
            }
            if ui.button("Remove mask").clicked() {
                let mut recipe = self.draft.clone();
                recipe.masks.remove(i);
                self.commit(recipe);
                self.choose_part(Part::Photo);
            }
        });
    }

    /// The crop's shapes (REQ-029-03), and leaving it.
    fn crop_shapes(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for ratio in Ratio::ALL {
                let response = ui.add(egui::Button::selectable(self.crop_ratio == ratio, ratio.label()));
                if response.clicked() {
                    self.set_crop_ratio(ratio);
                }
            }
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.crop_ratio.can_swap(), egui::Button::new("Swap"))
                .clicked()
            {
                self.swap_crop_ratio();
            }
            if ui.button("Done").on_hover_text("Done cropping (Esc)").clicked() {
                self.set_cropping(false);
            }
        });
    }

    /// The kind of blur, and placing its focus (REQ-023-01, REQ-023-03). _Background_ looks for the subject.
    fn blur_kind(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Blur").color(theme::muted()));
        ui.horizontal_wrapped(|ui| {
            for kind in BlurKind::ALL {
                let response = ui.add(egui::Button::selectable(self.draft.blur_kind == kind, kind.label()));
                if response.clicked() && self.draft.blur_kind != kind {
                    let mut recipe = self.draft.clone();
                    recipe.blur_kind = kind;
                    // A kind without a strength would show nothing.
                    if kind != BlurKind::None && recipe.blur == 0.0 {
                        recipe.blur = 50.0;
                    }
                    self.commit(recipe);
                    self.placing_focus = false;
                    if kind == BlurKind::Background {
                        self.find_subject();
                    }
                }
            }
        });
        match (&self.subject, self.draft.blur_kind) {
            (Subject::Unknown | Subject::Finding(_), BlurKind::Background) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Finding the subject…");
                });
            }
            (Subject::Failed(_), BlurKind::Background) => {
                ui.add(egui::Label::new("No subject found in this photo, so nothing is blurred.").wrap());
            }
            (_, kind) if kind.has_focus() => {
                let placing = self.placing_focus;
                let button = egui::Button::image_and_text(Icon::Crosshair.small(), "Place focus").selected(placing);
                if ui
                    .add(button)
                    .on_hover_text("Click the spot in the photo that should stay sharp")
                    .clicked()
                {
                    self.placing_focus = !placing;
                    self.picking = false;
                }
            }
            _ => {}
        }
    }

    /// The looks to pick from, and the chosen one's strength and grain (REQ-028-01 … REQ-028-03).
    fn looks(&mut self, ui: &mut egui::Ui) {
        let chosen = self.draft.look;
        ui.horizontal_wrapped(|ui| {
            let none = ui.add(egui::Button::selectable(chosen.is_none(), "None"));
            if none.clicked() && chosen.is_some() {
                let mut recipe = self.draft.clone();
                recipe.look = None;
                self.commit(recipe);
            }
            for look in Look::ALL {
                let selected = chosen.is_some_and(|choice| choice.look == look);
                if ui.add(egui::Button::selectable(selected, look.label())).clicked() && !selected {
                    let mut recipe = self.draft.clone();
                    // Another look keeps the strength and grain set for the last one.
                    recipe.look =
                        Some(chosen.map_or_else(|| LookChoice::new(look), |choice| LookChoice { look, ..choice }));
                    self.commit(recipe);
                }
            }
        });
        if chosen.is_some() {
            ui.add_space(4.0);
            self.look_slider(ui, "Strength", LookChoice::STRENGTH, |choice| &mut choice.strength);
            self.look_slider(ui, "Grain", LookChoice::GRAIN, |choice| &mut choice.grain);
        }
    }

    /// _Strength_ or _Grain_ of the chosen look, in percent; a double-click goes back to 100 %.
    fn look_slider(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        (low, high): (f32, f32),
        value: fn(&mut LookChoice) -> &mut f32,
    ) {
        let Some(mut choice) = self.draft.look else { return };
        let mut current = f64::from(*value(&mut choice));
        let text = format!("{current:.0} %");
        let response = slider_row(ui, label, &text, &mut current, (low, high), 1.0);
        if response.double_clicked() {
            *value(&mut choice) = 100.0;
            let mut recipe = self.draft.clone();
            recipe.look = Some(choice);
            self.commit(recipe);
            return;
        }
        if response.changed() {
            *value(&mut choice) = current as f32;
            self.draft.look = Some(choice);
        }
        if response.drag_stopped() || response.clicked() || (response.changed() && !response.dragged()) {
            let recipe = self.draft.clone();
            self.commit(recipe);
        }
    }

    /// One adjustment: its name and value, and a slider. A double-click resets it (REQ-021-07).
    fn slider(&mut self, ui: &mut egui::Ui, adjustment: Adjustment) {
        let part = self.part;
        let value = self
            .draft
            .part(part)
            .map_or(adjustment.neutral(), |recipe| adjustment.get(recipe));
        let text = if adjustment == Adjustment::Exposure {
            format!("{value:+.2}")
        } else if adjustment == Adjustment::Hue {
            if value == 0.0 {
                "0°".to_owned()
            } else {
                format!("{value:+.0}°")
            }
        } else if value == 0.0 || !matches!(adjustment.range().0, x if x < 0.0) {
            format!("{value:.0}")
        } else {
            format!("{value:+.0}")
        };
        let step = if adjustment == Adjustment::Exposure { 0.05 } else { 1.0 };
        let mut current = f64::from(value);
        let response = slider_row(ui, adjustment.label(), &text, &mut current, adjustment.range(), step);
        if response.double_clicked() {
            let mut recipe = self.draft.clone();
            adjustment.set(recipe.part_mut(part), adjustment.neutral());
            self.commit(recipe);
            return;
        }
        if response.changed() {
            adjustment.set(self.draft.part_mut(part), current as f32);
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

/// A collapsible section's header; a click opens or closes it.
fn section_header(ui: &mut egui::Ui, label: &str, open: &mut bool) {
    let icon = if *open { Icon::ChevronDown } else { Icon::ChevronRight };
    let header = ui.add(
        egui::Button::image_and_text(
            icon.image(14.0, theme::muted()),
            egui::RichText::new(label).font(egui::FontId::new(theme::SIZE_BODY, theme::semibold())),
        )
        .frame(false),
    );
    let expanded = *open;
    let header = header.on_hover_text(if expanded { "Collapse" } else { "Expand" });
    header.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, expanded, label));
    if header.clicked() {
        *open = !*open;
    }
}

/// A slider's row: its name and `text` above, the slider below, as wide as the panel allows.
fn slider_row(
    ui: &mut egui::Ui,
    label: &str,
    text: &str,
    value: &mut f64,
    (low, high): (f32, f32),
    step: f64,
) -> egui::Response {
    // Clear of the scroll bar, which floats over the content.
    let width = ui.available_width() - SCROLL_BAR_ROOM;
    let (row, _) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT + SLIDER_HEIGHT), egui::Sense::hover());
    let label_rect = Rect::from_min_size(row.min, vec2(width, ROW_HEIGHT));
    let painter = ui.painter();
    painter.text(
        label_rect.left_center(),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(theme::SIZE_BODY),
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
    crate::widgets::slider(ui, track, value, f64::from(low)..=f64::from(high), step, label)
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

/// Runs `job` on a thread of its own (in the browser, at once).
fn spawn(name: &str, job: impl FnOnce() + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::Builder::new()
        .name(name.into())
        .spawn(job)
        .expect("can't start a thread");
    #[cfg(target_arch = "wasm32")]
    {
        let _ = name;
        job();
    }
}

/// The unedited original of `target` and the recipe to start from: the source of `edit` and its recipe while the
/// source exists (REQ-021-11), else the file itself, unedited.
fn resolve(target: &Target, edit: &SavedEdit) -> (String, Recipe) {
    let exists =
        db::media::absolute_path(&target.library.to_string_lossy(), &edit.source).is_some_and(|path| path.is_file());
    if exists {
        (edit.source.clone(), edit.recipe())
    } else {
        (target.rel_path.clone(), Recipe::default())
    }
}

/// Loads `source`, the unedited original of `target`, as a texture and a small copy.
#[cfg(not(target_arch = "wasm32"))]
fn load(target: &Target, source: &str, render_state: Option<&egui_wgpu::RenderState>) -> Result<Loaded, String> {
    let path =
        db::media::absolute_path(&target.library.to_string_lossy(), source).ok_or("the path leaves its library")?;
    let max_side = render_state.map_or(SMALL_SIDE, |render_state| {
        render_state.device.limits().max_texture_dimension_2d.min(8192)
    });
    let image = media::original::load_original(&path, None, max_side, target.sidecar_orientation)
        .map_err(|error| error.to_string())?;
    let small = fit_small(&image);
    let view = render_state.map(|render_state| {
        let size = [image.width(), image.height()];
        let levels = crate::textures::mip_chain(size, image.into_raw());
        crate::textures::upload(&render_state.device, &render_state.queue, size, &levels)
            .create_view(&wgpu::TextureViewDescriptor::default())
    });
    Ok(Loaded {
        view,
        small: Some(small),
        cpu: None,
    })
}

/// The browser version doesn't edit (REQ-021-01).
#[cfg(target_arch = "wasm32")]
fn read_small(_render_state: &egui_wgpu::RenderState, _texture: &wgpu::Texture) -> Result<image::RgbaImage, String> {
    Err("editing isn't available here".into())
}

/// The browser version doesn't edit (REQ-021-01).
#[cfg(target_arch = "wasm32")]
fn load(_target: &Target, _source: &str, _render_state: Option<&egui_wgpu::RenderState>) -> Result<Loaded, String> {
    Err("editing isn't available here".into())
}

/// `image` fit into the small copy's size.
#[cfg(not(target_arch = "wasm32"))]
fn fit_small(image: &image::RgbaImage) -> image::RgbaImage {
    if image.width().max(image.height()) > SMALL_SIDE {
        let (width, height) = domain::thumbs::fit_inside(image.width(), image.height(), SMALL_SIDE, SMALL_SIDE, false);
        image::imageops::resize(image, width, height, image::imageops::FilterType::Triangle)
    } else {
        image.clone()
    }
}

/// The small copy of `texture`'s photo: its smallest mip level still at least as long as the small copy, read back
/// and fit into it (REQ-021-19).
#[cfg(not(target_arch = "wasm32"))]
fn read_small(render_state: &egui_wgpu::RenderState, texture: &wgpu::Texture) -> Result<image::RgbaImage, String> {
    let size = texture.size();
    let (mut level, mut width, mut height) = (0, size.width, size.height);
    while level + 1 < texture.mip_level_count() && (width / 2).max(height / 2) >= SMALL_SIDE {
        (level, width, height) = (level + 1, (width / 2).max(1), (height / 2).max(1));
    }
    let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let device = &render_state.device;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("edit small copy"),
        size: u64::from(row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: level,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    render_state.queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (tx, rx) = channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| error.to_string())?;
    rx.recv()
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    let data = slice.get_mapped_range().map_err(|error| error.to_string())?;
    let pixels = (0..height as usize)
        .flat_map(|y| {
            data[y * row as usize..y * row as usize + width as usize * 4]
                .iter()
                .copied()
        })
        .collect();
    drop(data);
    buffer.unmap();
    let image = image::RgbaImage::from_raw(width, height, pixels).ok_or("the read back copy has the wrong size")?;
    Ok(fit_small(&image))
}

/// `small` through `recipe`, on the CPU, with the shader's `flags` (but 2: the output is encoded).
fn cpu_preview(
    small: &image::RgbaImage,
    recipe: &Recipe,
    geometry: Geometry,
    full_size: [u32; 2],
    mask: Option<&Mask>,
    flags: u32,
) -> egui::ColorImage {
    let params = Params::new(recipe);
    let mask_given = mask.is_some();
    let (width, height) = small.dimensions();
    let size = full_size.map(|side| side.max(1) as f32);
    // A background blur waits for the subject (REQ-023-02).
    let blurs = params.blur > 0.0 && flags & 1 == 0 && (params.blur_kind != BlurKind::Background || flags & 4 != 0);
    let glows = params.halation > 0.0 && flags & 1 == 0;
    let pyramid = (blurs || glows).then(|| {
        let pixels = small.pixels().map(|p| p.0.map(|c| f32::from(c) / 255.0)).collect();
        Pyramid::new(width, height, pixels)
    });
    let mut rgba = Vec::with_capacity(small.as_raw().len());
    for (x, y, pixel) in small.enumerate_pixels() {
        let uv = [(x as f32 + 0.5) / width as f32, (y as f32 + 0.5) / height as f32];
        let subject = mask.map_or(0.0, |mask| mask.sample(uv));
        let mut rgba_in = pixel.0.map(|c| f32::from(c) / 255.0);
        let mut glow = [0.0; 3];
        if let Some(pyramid) = &pyramid {
            // σ in pixels of the small copy.
            let scale = width as f32 / size[0];
            if glows {
                let lod = blur_lod(Params::halation_sigma(size) * scale, pyramid.levels());
                let around = pyramid.blurred(rgba_in, uv, lod);
                glow = params.halation_glow([rgba_in[0], rgba_in[1], rgba_in[2]], [around[0], around[1], around[2]]);
            }
            if blurs {
                let sigma = params.blur_sigma(uv, size, subject) * scale;
                rgba_in = pyramid.blurred(rgba_in, uv, blur_lod(sigma, pyramid.levels()));
            }
        }
        let rgb = [rgba_in[0], rgba_in[1], rgba_in[2]];
        let mut out = match mask {
            _ if flags & 1 != 0 => rgb,
            Some(_) if flags & 4 != 0 => params.pixel_glowing(rgb, glow, uv, size, Some(subject)),
            _ => params.pixel_glowing(rgb, glow, uv, size, None),
        };
        if flags & 8 != 0 {
            let selected = if flags & 16 != 0 { 1.0 - subject } else { subject };
            out = tint(out, selected);
        }
        if let Some(mask) = (flags >> 8).checked_sub(1).and_then(|i| params.masks.get(i as usize)) {
            let found = (flags & 4 != 0 && mask_given).then_some(subject);
            out = tint(out, mask.weight(rgb, uv, size, found));
        }
        rgba.extend(out.map(|c| (c * 255.0).round() as u8));
        rgba.push(pixel[3]);
    }
    let (out_width, out_height) = geometry.output_size(width, height);
    let mut output = Vec::with_capacity(out_width as usize * out_height as usize * 4);
    for y in 0..out_height {
        for x in 0..out_width {
            let [u, v] = geometry.to_original([
                (x as f32 + 0.5) / out_width as f32,
                (y as f32 + 0.5) / out_height as f32,
            ]);
            let from_x = ((u * width as f32) as u32).min(width - 1) as usize;
            let from_y = ((v * height as f32) as u32).min(height - 1) as usize;
            let at = (from_y * width as usize + from_x) * 4;
            output.extend_from_slice(&rgba[at..at + 4]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([out_width as usize, out_height as usize], &output)
}

/// The selection's tint (REQ-022-04): towards red by how much a pixel is `selected`, as `edit.wgsl` does.
fn tint(rgb: [f32; 3], selected: f32) -> [f32; 3] {
    let red = [1.0, 0.25, 0.25];
    std::array::from_fn(|c| rgb[c] + (red[c] - rgb[c]) * 0.45 * selected)
}

/// One part's light and color in the shader's order: gains times exposure and `on`, tone, color.
fn part_floats(params: &Params, on: f32) -> [f32; 16] {
    let [r, g, b] = params.gains.map(|gain| gain * params.exposure);
    let (sin, cos) = params.hue.sin_cos();
    [
        r,
        g,
        b,
        on,
        params.black,
        params.white,
        params.shadows,
        params.highlights,
        params.contrast,
        params.saturation,
        params.vibrance,
        params.brightness,
        cos,
        sin,
        0.0,
        0.0,
    ]
}

/// The shader's uniform: `edit.wgsl`'s `Edit`, in its order.
fn uniform(
    params: &Params,
    geometry: Geometry,
    rect: [f32; 4],
    uv: [f32; 4],
    full_size: [f32; 2],
    opacity: f32,
    flags: f32,
) -> Vec<f32> {
    let mut floats = Vec::with_capacity(116 + (20 + 8 * MAX_MASK_SHAPES) * MAX_MASKS + CURVE_SAMPLES * 4);
    floats.extend_from_slice(&rect);
    floats.extend_from_slice(&uv);
    floats.extend_from_slice(&[full_size[0], full_size[1], opacity, flags]);
    floats.extend_from_slice(&part_floats(params, f32::from(u8::from(params.curves.is_some()))));
    floats.extend_from_slice(&[params.vignette, params.vignette_start, params.grain, params.grain_cell]);
    for part in [&params.subject, &params.background] {
        match part {
            Some(part) => floats.extend_from_slice(&part_floats(part, 1.0)),
            None => floats.extend_from_slice(&[0.0; 16]),
        }
    }
    let kind = BlurKind::ALL
        .iter()
        .position(|kind| *kind == params.blur_kind)
        .unwrap_or(0) as f32;
    floats.extend_from_slice(&[params.blur, kind, params.blur_size, params.blur_feather]);
    floats.extend_from_slice(&[
        params.blur_focus[0],
        params.blur_focus[1],
        f32::from(geometry.quarter_turns()),
        0.0,
    ]);
    floats.extend_from_slice(&geometry.crop());
    floats.extend_from_slice(&[
        f32::from(u8::from(params.grain_midtones)),
        params.grain_color,
        f32::from(u8::from(params.hue_nodes.is_some())),
        params.halation,
    ]);
    for i in 0..domain::look::HUE_NODES {
        let [turn, chroma, light] = params.hue_nodes.map_or([0.0, 1.0, 0.0], |nodes| nodes[i]);
        floats.extend_from_slice(&[turn, chroma, light, 0.0]);
    }
    floats.extend_from_slice(&[params.masks.len() as f32, 0.0, 0.0, 0.0]);
    for i in 0..MAX_MASKS {
        match params.masks.get(i) {
            Some(mask) => {
                floats.extend_from_slice(&part_floats(&mask.params, 1.0));
                let shapes = 1 + mask.more.len();
                floats.extend_from_slice(&[f32::from(u8::from(mask.invert)), shapes as f32, 0.0, 0.0]);
                let parts = std::iter::once((MaskOp::Add, mask.shape))
                    .chain(mask.more.iter().map(|part| (part.op, part.shape)));
                for (op, shape) in parts {
                    let [kind, a, b, c, d] = shape.numbers();
                    floats.extend_from_slice(&[kind, op.number(), a, b, c, d, 0.0, 0.0]);
                }
                floats.extend(std::iter::repeat_n(0.0, 8 * (MAX_MASK_SHAPES - shapes)));
            }
            None => floats.extend(std::iter::repeat_n(0.0, 20 + 8 * MAX_MASK_SHAPES)),
        }
    }
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
    /// The mask while there is no subject: nothing is.
    no_subject: wgpu::TextureView,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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
        let no_subject = mask_texture(device, 1, 1).create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            pipeline,
            layout,
            sampler,
            no_subject,
        }
    }

    fn bind(
        &self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        mask: Option<&wgpu::TextureView>,
        uniform: &[f32],
    ) -> wgpu::BindGroup {
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
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(mask.unwrap_or(&self.no_subject)),
                },
            ],
        })
    }
}

/// A one-channel texture for a subject's mask, zeroed.
fn mask_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("subject"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// The subject's mask as a texture the preview samples like the photo.
fn upload_mask(render_state: &egui_wgpu::RenderState, mask: &Mask) -> wgpu::TextureView {
    let (width, height) = (mask.width(), mask.height());
    let texture = mask_texture(&render_state.device, width, height);
    let bytes: Vec<u8> = mask
        .values()
        .iter()
        .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    render_state.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Looks for the subject of `photo` (REQ-022-02).
#[cfg(not(target_arch = "wasm32"))]
fn find(photo: &image::RgbaImage) -> Result<Mask, String> {
    media::subject::find(photo)
}

#[cfg(target_arch = "wasm32")]
fn find(_photo: &image::RgbaImage) -> Result<Mask, String> {
    Err("not available here".into())
}

/// Installs the preview's pipeline into the egui renderer. Call once at startup.
pub fn install(render_state: &egui_wgpu::RenderState) {
    let resources = EditResources::new(&render_state.device, render_state.target_format);
    render_state.renderer.write().callback_resources.insert(resources);
}

struct PreviewCallback {
    view: wgpu::TextureView,
    /// The subject's mask, if found.
    mask: Option<wgpu::TextureView>,
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
            let _ = self
                .group
                .set(res.bind(device, &self.view, self.mask.as_ref(), &self.uniform));
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

    /// A wgpu device, one test at a time: devices on parallel test threads crash the test process (lavapipe, WARP).
    fn render_state() -> (std::sync::MutexGuard<'static, ()>, egui_wgpu::RenderState) {
        static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = GPU.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let render_state = egui_kittest::wgpu::create_render_state(
            egui_kittest::wgpu::default_wgpu_setup(),
            egui_wgpu::RendererOptions::default(),
        );
        (guard, render_state)
    }

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
            (Adjustment::Hue, 40.0),
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
        // The hue alone, half way round (REQ-021-20).
        let mut hue = Recipe::default();
        Adjustment::Hue.set(&mut hue, -180.0);
        vec![Recipe::default(), all, light, hue]
    }

    /// Renders `image` through `recipe` with the shader, at 1:1, and reads it back.
    fn render_gpu(
        render_state: &egui_wgpu::RenderState,
        image: &image::RgbaImage,
        recipe: &Recipe,
        mask: Option<&Mask>,
        flags: u32,
    ) -> Vec<u8> {
        let (device, queue) = (&render_state.device, &render_state.queue);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let resources = EditResources::new(device, format);
        let (width, height) = image.dimensions();
        let size = [width, height];
        let levels = crate::textures::mip_chain(size, image.as_raw().clone());
        let source = crate::textures::upload(device, queue, size, &levels);
        let view = source.create_view(&wgpu::TextureViewDescriptor::default());

        let (output_width, output_height) = recipe.geometry().output_size(width, height);
        let uniform = uniform(
            &Params::new(recipe),
            recipe.geometry(),
            [0.0, 0.0, output_width as f32, output_height as f32],
            [0.0, 0.0, 1.0, 1.0],
            [width as f32, height as f32],
            1.0,
            flags as f32,
        );
        let mask = mask.map(|mask| upload_mask(render_state, mask));
        let group = resources.bind(device, &view, mask.as_ref(), &uniform);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("edit test"),
            size: wgpu::Extent3d {
                width: output_width,
                height: output_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let (width, height) = (output_width, output_height);
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let row = (width * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("edit test"),
            size: u64::from(row * height),
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
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| result.expect("mapped"));
        device.poll(wgpu::PollType::wait_indefinitely()).expect("polled");
        let data = slice.get_mapped_range().expect("mapped");
        let pixels = (0..height as usize)
            .flat_map(|y| data[y * row as usize..y * row as usize + width as usize * 4].to_vec())
            .collect();
        drop(data);
        buffer.unmap();
        pixels
    }

    /// REQ-021-02, REQ-021-06, REQ-021-20, REQ-022-03, REQ-022-04: the GPU preview shows what saving renders on the CPU
    /// (within two levels), grain, vignette, the hue, the subject's and the background's own adjustments and the
    /// selection's tint included.
    #[test]
    fn req_021_02_req_021_06_the_gpu_preview_matches_the_cpu_render() {
        let (_gpu, render_state) = render_state();
        let image = pattern();
        // The subject grows from left to right, in steps the GPU's 8-bit mask holds exactly.
        let values = (0..SIZE * SIZE).map(|i| ((i % SIZE) * 5) as f32 / 255.0).collect();
        let mask = Mask::new(SIZE, SIZE, values).unwrap();
        let mut parts = recipes()[1].clone();
        Adjustment::Exposure.set(parts.part_mut(Part::Subject), 1.2);
        Adjustment::Saturation.set(parts.part_mut(Part::Subject), 40.0);
        Adjustment::Shadows.set(parts.part_mut(Part::Background), -50.0);
        Adjustment::Temperature.set(parts.part_mut(Part::Background), -60.0);
        Adjustment::Hue.set(parts.part_mut(Part::Subject), 90.0);
        let mut cases: Vec<(Recipe, Option<&Mask>, u32)> =
            recipes().into_iter().map(|recipe| (recipe, None, 0)).collect();
        for flags in [4, 4 | 8, 4 | 8 | 16] {
            cases.push((parts.clone(), Some(&mask), flags));
        }
        for (recipe, mask, flags) in cases {
            let gpu = render_gpu(&render_state, &image, &recipe, mask, flags);
            let cpu = cpu_preview(&image, &recipe, recipe.geometry(), [SIZE, SIZE], mask, flags);
            let mut worst = 0;
            for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
                for c in 0..3 {
                    let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
                    assert!(
                        diff <= 2,
                        "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?} with {recipe:?}, flags {flags}"
                    );
                    worst = worst.max(diff);
                }
            }
            eprintln!("worst difference: {worst}");
        }
    }

    /// REQ-029-01, REQ-029-04: every turn and a crop preview as saving renders them, with the vignette on the crop.
    #[test]
    fn req_029_04_the_gpu_preview_of_crop_and_turns_matches_the_cpu_render() {
        let (_gpu, render_state) = render_state();
        let image = pattern();
        for (crop, turns) in [
            (None, 1),
            (None, 2),
            (None, 3),
            (Some([0.0, 0.5, 0.5, 0.5]), 0),
            (Some([0.5, 0.0, 0.5, 0.5]), 1),
        ] {
            let mut recipe = recipes()[1].clone();
            Adjustment::Vignette.set(&mut recipe, -60.0);
            recipe.crop = crop;
            recipe.quarter_turns = turns;
            let gpu = render_gpu(&render_state, &image, &recipe, None, 0);
            let cpu = cpu_preview(&image, &recipe, recipe.geometry(), [SIZE, SIZE], None, 0);
            let (width, height) = recipe.geometry().output_size(SIZE, SIZE);
            assert_eq!(cpu.size, [width as usize, height as usize], "{crop:?} {turns}");
            assert_eq!(gpu.len(), cpu.pixels.len() * 4, "{crop:?} {turns}");
            for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
                for c in 0..3 {
                    let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
                    assert!(
                        diff <= 3,
                        "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?}, {crop:?} {turns}"
                    );
                }
            }
        }
    }

    /// REQ-028-05: a film look previews as saving renders it (within two levels), at any strength and grain, with
    /// other adjustments and curves on top.
    #[test]
    fn req_028_05_the_gpu_preview_of_a_look_matches_the_cpu_render() {
        let (_gpu, render_state) = render_state();
        let image = pattern();
        for (i, look) in domain::look::Look::ALL.into_iter().enumerate() {
            let mut recipe = recipes()[i % recipes().len()].clone();
            recipe.look = Some(domain::look::LookChoice {
                look,
                strength: [100.0, 70.0, 35.0][i % 3],
                grain: [100.0, 200.0, 0.0][i % 3],
            });
            let gpu = render_gpu(&render_state, &image, &recipe, None, 0);
            let cpu = cpu_preview(&image, &recipe, recipe.geometry(), [SIZE, SIZE], None, 0);
            for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
                for c in 0..3 {
                    let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
                    assert!(
                        diff <= 2,
                        "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?} with {recipe:?}"
                    );
                }
            }
        }
    }

    /// REQ-023-02, REQ-023-03, REQ-023-04: the preview blurs as saving does (within three levels), deep into the
    /// pyramid, around a focus and behind a subject.
    #[test]
    fn req_023_04_the_gpu_blur_matches_the_cpu_blur() {
        let (_gpu, render_state) = render_state();
        let (width, height) = (512, 384);
        // Hard edges: a checker of 16 px with a color ramp.
        let image = image::RgbaImage::from_fn(width, height, |x, y| {
            let on = (x / 16 + y / 16) % 2 == 0;
            let t = (x * 255 / width) as u8;
            if on {
                image::Rgba([t, 200, 40, 255])
            } else {
                image::Rgba([20, 60, 255 - t, 255])
            }
        });
        let values = (0..width * height)
            .map(|i| if i % width < width / 2 { 1.0 } else { 0.0 })
            .collect();
        // The subject is the left half, in a mask as large as the photo.
        let mask = Mask::new(width, height, values).unwrap();
        let blur = |kind: BlurKind, focus: [f32; 2], size: f32| Recipe {
            blur_kind: kind,
            blur_focus: focus,
            blur: 100.0,
            blur_size: size,
            ..Recipe::default()
        };
        let mut lit = blur(BlurKind::Radial, [0.3, 0.6], 10.0);
        Adjustment::Exposure.set(&mut lit, 0.5);
        let cases = [
            (blur(BlurKind::TiltShift, [0.5, 0.3], 20.0), None, 0),
            (lit, None, 0),
            (blur(BlurKind::Background, [0.5, 0.5], 30.0), Some(&mask), 4),
        ];
        for (recipe, mask, flags) in cases {
            let gpu = render_gpu(&render_state, &image, &recipe, mask, flags);
            let cpu = cpu_preview(&image, &recipe, recipe.geometry(), [width, height], mask, flags);
            let mut worst = 0;
            for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
                for c in 0..3 {
                    let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
                    assert!(
                        diff <= 3,
                        "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?} with {recipe:?}"
                    );
                    worst = worst.max(diff);
                }
            }
            eprintln!("worst difference: {worst}");
            // And it does blur: many pixels of the checker change a lot.
            let changed = gpu
                .chunks_exact(4)
                .zip(image.pixels())
                .filter(|(gpu, source)| (i32::from(gpu[1]) - i32::from(source[1])).abs() > 40)
                .count();
            assert!(
                changed > (width * height / 10) as usize,
                "{changed} changed with {recipe:?}"
            );
        }
    }

    /// REQ-028-05, REQ-028-08: the preview's halation matches the saved render, on a photo large enough for it to reach
    /// beyond a pixel, and it does glow red beside a light.
    #[test]
    fn req_028_08_the_gpu_halation_matches_the_cpu_render() {
        let (_gpu, render_state) = render_state();
        let (width, height) = (512, 384);
        let image = image::RgbaImage::from_fn(width, height, |x, _| {
            let v = if (250..262).contains(&x) { 255 } else { 25 };
            image::Rgba([v, v, v, 255])
        });
        let recipe = Recipe {
            look: Some(domain::look::LookChoice {
                grain: 0.0,
                ..domain::look::LookChoice::new(domain::look::Look::Gold200)
            }),
            ..Recipe::default()
        };
        let gpu = render_gpu(&render_state, &image, &recipe, None, 0);
        let cpu = cpu_preview(&image, &recipe, recipe.geometry(), [width, height], None, 0);
        for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
            for c in 0..3 {
                let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
                assert!(diff <= 3, "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?}");
            }
        }
        let red = |x: usize| i32::from(gpu[(height as usize / 2 * width as usize + x) * 4]);
        assert!(
            red(248) > red(100) + 8,
            "beside the light {} vs far {}",
            red(248),
            red(100)
        );
    }

    /// REQ-030-07, REQ-030-08: every kind of mask, and its tint, previews as saving renders it (within two levels).
    #[test]
    fn req_030_07_req_030_08_the_gpu_preview_of_masks_matches_the_cpu_render() {
        let (_gpu, render_state) = render_state();
        let image = pattern();
        let mask = |shape: MaskShape, invert: bool, adjustment: Adjustment, value: f32| {
            let mut mask = LocalMask::new(shape);
            mask.invert = invert;
            adjustment.set(&mut mask.recipe, value);
            mask
        };
        let linear = MaskShape::Linear {
            from: [0.2, 0.1],
            to: [0.7, 0.8],
        };
        let luminance = MaskShape::Luminance {
            low: 30.0,
            high: 70.0,
            feather: 10.0,
        };
        let color = MaskShape::Color {
            hue: 30.0,
            range: 60.0,
            feather: 30.0,
        };
        let recipe = Recipe {
            masks: vec![
                mask(linear, false, Adjustment::Exposure, 1.0),
                mask(luminance, false, Adjustment::Temperature, 60.0),
                mask(color, false, Adjustment::Saturation, -80.0),
                mask(linear, true, Adjustment::Contrast, 40.0),
            ],
            ..Recipe::default()
        };
        let plain = render_gpu(&render_state, &image, &Recipe::default(), None, 0);
        for flags in [0, 1 << 8, 2 << 8, 3 << 8, 4 << 8] {
            let gpu = render_gpu(&render_state, &image, &recipe, None, flags);
            let cpu = cpu_preview(&image, &recipe, recipe.geometry(), [SIZE, SIZE], None, flags);
            for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
                for c in 0..3 {
                    let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
                    assert!(
                        diff <= 2,
                        "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?} with flags {flags}"
                    );
                }
            }
            assert_ne!(gpu, plain, "the masks change the photo");
        }
        // Cropped and turned, a gradient stays on its part of the photo.
        let mut turned = recipe;
        turned.quarter_turns = 1;
        turned.crop = Some([0.5, 0.0, 0.5, 0.5]);
        let gpu = render_gpu(&render_state, &image, &turned, None, 0);
        let cpu = cpu_preview(&image, &turned, turned.geometry(), [SIZE, SIZE], None, 0);
        for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
            for c in 0..3 {
                let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
                assert!(diff <= 3, "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?}");
            }
        }
    }

    /// REQ-030-10, REQ-030-11: combined shapes preview as saving renders them, with the subject and without one yet.
    #[test]
    fn req_030_10_req_030_11_the_gpu_preview_of_combined_masks_matches_the_cpu_render() {
        let (_gpu, render_state) = render_state();
        let image = pattern();
        let values = (0..SIZE * SIZE)
            .map(|i| if i % SIZE < SIZE / 2 { 1.0 } else { 0.0 })
            .collect();
        let subject = Mask::new(SIZE, SIZE, values).unwrap();
        let linear = MaskShape::Linear {
            from: [0.0, 0.0],
            to: [1.0, 1.0],
        };
        let bright = MaskShape::Luminance {
            low: 30.0,
            high: 90.0,
            feather: 10.0,
        };
        let mask = |shape: MaskShape, more: &[(MaskOp, MaskShape)], adjustment: Adjustment, value: f32| {
            let mut mask = LocalMask::new(shape);
            mask.more = more.iter().map(|&(op, shape)| MaskPart { op, shape }).collect();
            adjustment.set(&mut mask.recipe, value);
            mask
        };
        let recipe = Recipe {
            masks: vec![
                mask(
                    linear,
                    &[
                        (MaskOp::Intersect, bright),
                        (MaskOp::Add, MaskShape::COLOR),
                        (MaskOp::Subtract, MaskShape::Subject),
                    ],
                    Adjustment::Exposure,
                    1.0,
                ),
                mask(
                    MaskShape::Background,
                    &[(MaskOp::Intersect, bright)],
                    Adjustment::Saturation,
                    -80.0,
                ),
            ],
            ..Recipe::default()
        };
        // With the subject, its tint, and without a subject yet.
        for (mask, flags) in [
            (Some(&subject), 4),
            (Some(&subject), 4 | (1 << 8)),
            (Some(&subject), 4 | (2 << 8)),
            (None, 0),
            (None, 2 << 8),
        ] {
            let gpu = render_gpu(&render_state, &image, &recipe, mask, flags);
            let cpu = cpu_preview(&image, &recipe, recipe.geometry(), [SIZE, SIZE], mask, flags);
            for (i, (gpu, cpu)) in gpu.chunks_exact(4).zip(cpu.pixels.iter()).enumerate() {
                for c in 0..3 {
                    let diff = (i32::from(gpu[c]) - i32::from(cpu.to_array()[c])).abs();
                    assert!(
                        diff <= 2,
                        "pixel {i} channel {c}: GPU {gpu:?}, CPU {cpu:?} with flags {flags}"
                    );
                }
            }
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
            sidecar_orientation: None,
        };
        let mut session = EditSession::new(target, None);
        session.base = Base::Loaded(Box::new(Loaded {
            view: None,
            small: Some(image::RgbaImage::from_pixel(10, 10, image::Rgba([200, 150, 100, 255]))),
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
        let (_gpu, render_state) = render_state();
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
            sidecar_orientation: None,
        };
        let mut session = EditSession::new(target, Some(render_state.clone()));
        session.base = Base::Loaded(Box::new(Loaded {
            view: Some(view.clone()),
            small: Some(pattern()),
            cpu: None,
        }));
        let callback = PreviewCallback {
            view,
            mask: None,
            uniform: uniform(
                &Params::new(&Recipe::default()),
                Geometry::new(None, 0),
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
            sidecar_orientation: None,
        };
        let mut session = EditSession::new(target, None);
        let warm = image::RgbaImage::from_pixel(10, 10, image::Rgba([190, 160, 130, 255]));
        session.base = Base::Loaded(Box::new(Loaded {
            view: None,
            small: Some(warm.clone()),
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

    /// A target whose file isn't there, so a test sees when it would be read.
    fn missing_target(full_size: [u32; 2]) -> Target {
        Target {
            media: 1,
            root_id: 1,
            rel_path: "missing.jpg".into(),
            file_name: "missing.jpg".into(),
            library: std::env::temp_dir().join("good-gallery-no-library"),
            full_size,
            sidecar_orientation: None,
        }
    }

    /// REQ-021-19: without an earlier edit, the panel shows its sliders as soon as editing starts, before any pixels
    /// arrived.
    #[test]
    fn req_021_19_the_panel_shows_its_sliders_at_once() {
        use egui_kittest::kittest::Queryable as _;
        let mut session = EditSession::new(missing_target([120, 80]), None);
        session.start(None, &egui::Context::default());
        assert!(session.is_ready());
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(320.0, 900.0))
            .with_context_setup(crate::setup)
            .build_ui_state(
                |ui, session: &mut EditSession| {
                    session.panel(ui, ui.max_rect());
                },
                session,
            );
        harness.run();
        assert!(!harness.state().is_loaded(), "no pixels yet");
        harness.get_by_label("Exposure");
        harness.get_by_label("Saturation");
    }

    /// REQ-021-19: editing takes the texture the viewer shows and reads its small copy back from it, without reading
    /// the file (which isn't there here).
    #[test]
    fn req_021_19_uses_the_shown_original_instead_of_reading_the_file() {
        let (_gpu, render_state) = render_state();
        let size = [1200, 800];
        let image = image::RgbaImage::from_fn(size[0], size[1], |x, y| {
            image::Rgba([(x * 255 / size[0]) as u8, (y * 255 / size[1]) as u8, 120, 255])
        });
        let texture = crate::textures::upload(
            &render_state.device,
            &render_state.queue,
            size,
            &crate::textures::mip_chain(size, image.clone().into_raw()),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let shown = OriginalState::Loaded(crate::originals::Loaded {
            id: egui::TextureId::default(),
            size,
            view: Some(view.clone()),
        });

        let mut session = EditSession::new(missing_target(size), Some(render_state.clone()));
        session.start(None, &egui::Context::default());
        // Still loading in the viewer: editing waits for it instead of reading the file itself.
        session.use_shown(&OriginalState::Loading, || panic!("nothing to lend yet"));
        session.wait();
        assert!(session.failed().is_none() && !session.is_loaded());

        session.use_shown(&shown, || Some(view.clone()));
        session.wait();
        assert_eq!(session.failed(), None);
        assert!(session.is_loaded());
        let small = session.small().unwrap();
        let expected = fit_small(&image);
        assert_eq!(small.dimensions(), expected.dimensions());
        for (got, want) in small.pixels().zip(expected.pixels()) {
            for c in 0..4 {
                assert!(got[c].abs_diff(want[c]) <= 3, "{got:?} vs {want:?}");
            }
        }
    }
}
