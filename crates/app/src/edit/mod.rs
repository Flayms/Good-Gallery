//! Editing a photo in the viewer (spec 021). An [`EditSession`] loads the unedited original (the file itself, or the
//! original an earlier edit was rendered from), keeps the recipe with its undo history, draws the adjustments panel
//! and the preview: on the GPU through `edit.wgsl` while there is a wgpu renderer, else rendered on the CPU from a small
//! copy (tests). Saving happens in the app (`AppState::save_edit`), the viewer only asks for it.

mod gpu;
mod load;
mod panel;
#[cfg(test)]
mod tests;
mod tools;

use crate::originals::OriginalState;
use domain::crop::{Grip, Ratio};
use domain::edit::{Channel, Geometry, Histogram, History, Mask, Params, Part, Recipe, SavedEdit};
use domain::versions::Version;
use eframe::egui::{self, Rect};
use eframe::egui_wgpu::{self, wgpu};
pub use gpu::install;
use gpu::{PaintTexture, upload_mask};
use load::{find, read_small, resolve, spawn};
use panel::before_curves;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

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

/// What the save buttons say on hover (REQ-021-23).
const TIP_SAVE: &str = "Replaces the photo with the edited one. The original is kept as a backup, so you can change the edit or go back \
     to the original later.";
const TIP_WITHOUT_BACKUP: &str = "Replaces the photo with the edited one and deletes the original. Takes the least \
                                  space, but the edit can't be changed or undone afterwards.";

/// How a save treats the file and its original (REQ-021-09, REQ-021-10, REQ-021-22).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveMode {
    /// Replace the file, keeping the unedited original as a backup.
    Replace,
    /// Save the edited photo beside the file.
    Copy,
    /// Replace the file and keep no original.
    WithoutBackup,
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
    /// Replacing keeps a backup of the unedited original (REQ-021-22).
    pub keep_original: bool,
    /// The subject the preview showed, for a recipe with parts (REQ-022-05).
    pub subject: Option<Arc<Mask>>,
    /// Turns an original without an orientation of its own (REQ-004-12).
    pub sidecar_orientation: Option<u8>,
    /// The photo's versions so far (REQ-021-24).
    pub history: Vec<domain::versions::Version>,
    /// The table of the recipe's LUT, if this computer has it (REQ-043-03).
    pub lut: Option<Arc<domain::lut::Lut>>,
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
    /// Saving without a backup, every time (REQ-021-22).
    WithoutBackup(Option<Leave>),
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
    /// The lens blur's pyramid of `view`, built the first time a lens blur shows (REQ-023-05); freed with the session.
    lens: Option<wgpu::TextureView>,
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
    resolving: Option<Receiver<(String, Recipe, Vec<Version>)>>,
    /// The unedited original, once known; the panel shows from then on (REQ-021-19).
    source: Option<String>,
    history: History,
    /// The recipe shown: the history's current one, or one being dragged.
    draft: Recipe,
    /// The recipe the file was saved with: no unsaved changes while the draft equals it.
    saved: Recipe,
    /// The photo's versions, oldest first, ending with `saved`; none without an earlier edit or its original
    /// (REQ-021-24).
    versions: Vec<Version>,
    /// The version the pointer is over in _Versions_, shown instead of the draft (REQ-021-25).
    previewed: Option<Recipe>,
    base: Base,
    /// Reading the small copy back from the viewer's texture.
    reading: Option<Receiver<Result<image::RgbaImage, String>>>,
    open: [bool; 4],
    /// Whether the _Look_ section is open.
    look_open: bool,
    geometry_open: bool,
    /// The _Size_ section (REQ-029-09, REQ-021-21).
    size_open: bool,
    /// Whether the crop's frame is shown over the whole photo (REQ-029-02).
    cropping: bool,
    /// Whether _Straighten_ is being dragged (REQ-029-10).
    straightening: bool,
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
    /// The brush's radius and softness, in percent, and whether it erases (REQ-030-12).
    brush: (f32, f32, bool),
    /// Whether a stroke is being drawn on the photo.
    stroking: bool,
    /// What the preview samples of the recipe's paintings.
    paint_texture: Option<PaintTexture>,
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
    /// An adjustment's slider is held: the tint steps aside (REQ-030-16).
    adjusting: bool,
    pub dialog: Option<Dialog>,
    pub saving: bool,
    render_state: Option<egui_wgpu::RenderState>,
    ctx: Option<egui::Context>,
    /// The imported LUTs, to pick from and render with (spec 043).
    luts: Arc<crate::luts::Luts>,
    /// The shown LUT's table on the GPU, made when it was first shown.
    lut_texture: Option<(Arc<domain::lut::Lut>, wgpu::TextureView)>,
    /// _Import LUT…_ was clicked; the viewer passes it on (REQ-043-01).
    import_lut: bool,
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
            versions: Vec::new(),
            previewed: None,
            base: Base::Waiting,
            reading: None,
            open: [true, false, true, false],
            look_open: true,
            geometry_open: true,
            size_open: false,
            cropping: false,
            straightening: false,
            crop_ratio: Ratio::Free,
            crop_portrait: false,
            crop_drag: None,
            crop_area: None,
            gradient_drag: None,
            gradient_area: None,
            brush: (5.0, 50.0, false),
            stroking: false,
            paint_texture: None,
            channel: Channel::All,
            dragging_point: None,
            picking: false,
            placing_focus: false,
            histogram: None,
            part: Part::Photo,
            subject: Subject::Unknown,
            subject_wanted: false,
            show_selection: true,
            adjusting: false,
            dialog: None,
            saving: false,
            render_state,
            ctx: None,
            luts: Arc::default(),
            lut_texture: None,
            import_lut: false,
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

                            lens: None,
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
            let _ = tx.send(load::load(&target, &source, render_state.as_ref()));
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
            && let Ok((source, recipe, versions)) = rx.try_recv()
        {
            self.resolving = None;
            self.set_source(source, recipe);
            self.versions = versions;
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

    /// Whether the preview tints the chosen part's selection (REQ-030-07, REQ-022-04).
    pub fn tints_selection(&self) -> bool {
        self.show_selection
            && !self.adjusting
            && match self.part {
                Part::Photo => false,
                Part::Subject | Part::Background => matches!(self.subject, Subject::Found(..)),
                Part::Mask(_) => true,
            }
    }

    pub fn set_show_selection(&mut self, show: bool) {
        self.show_selection = show;
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
        let histogram = self.params(&key).histogram(samples);
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

    /// The imported LUTs to offer and render with (spec 043).
    pub fn with_luts(mut self, luts: Arc<crate::luts::Luts>) -> Self {
        self.luts = luts;
        self
    }

    /// Whether _Import LUT…_ was clicked since last asked (REQ-043-01).
    pub fn take_import_lut(&mut self) -> bool {
        std::mem::take(&mut self.import_lut)
    }

    /// The table of `recipe`'s LUT, if it has one and this computer has it (REQ-043-03).
    fn lut_table(&self, recipe: &Recipe) -> Option<Arc<domain::lut::Lut>> {
        recipe.lut.as_ref().and_then(|choice| self.luts.table(&choice.id))
    }

    /// The numbers the preview renders `recipe` with, with its LUT's table.
    fn params(&self, recipe: &Recipe) -> Params {
        Params::new(recipe).with_lut(self.lut_table(recipe))
    }

    /// The request to save the shown recipe.
    pub fn save_request(&self, mode: SaveMode) -> Option<SaveEdit> {
        Some(SaveEdit {
            media: self.target.media,
            root_id: self.target.root_id,
            rel_path: self.target.rel_path.clone(),
            source: self.source.clone()?,
            recipe: self.draft.clone(),
            copy: mode == SaveMode::Copy,
            keep_original: mode != SaveMode::WithoutBackup,
            subject: match &self.subject {
                Subject::Found(mask, _) if self.draft.needs_subject() => Some(mask.clone()),
                _ => None,
            },
            sidecar_orientation: self.target.sidecar_orientation,
            history: self.versions.clone(),
            lut: self.lut_table(&self.draft),
        })
    }

    /// _Save_ (over the original): asks first the first time, then saves and leaves as asked.
    pub fn save(&mut self, mode: SaveMode, then: Option<Leave>) -> Option<Outcome> {
        if self.saving || !self.is_loaded() {
            return None;
        }
        let ask = match mode {
            SaveMode::Replace if self.replaces_original() => Some(Dialog::Replace(then)),
            SaveMode::WithoutBackup => Some(Dialog::WithoutBackup(then)),
            _ => None,
        };
        if let Some(ask) = ask
            && self.dialog != Some(ask)
        {
            self.dialog = Some(ask);
            return None;
        }
        self.dialog = None;
        let request = self.save_request(mode)?;
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

    /// The size to fit into the media area while editing draws the photo: the output, or the photo as editing started
    /// while comparing (REQ-029-01, REQ-029-05). `None` until the original is there, while the viewer shows the file.
    pub fn shown_size(&self, compare: bool) -> Option<[f32; 2]> {
        if !matches!(self.base, Base::Loaded(_)) || self.waits_for_subject(compare) {
            return None;
        }
        let [width, height] = self.target.full_size;
        // The index doesn't know this photo's size: the viewer goes by the texture.
        if width == 0 || height == 0 {
            return None;
        }
        let (width, height) = self.sized_geometry(compare).output_size(width, height);
        Some([width as f32, height as f32])
    }

    /// The recipe drawn: while comparing the one this editing session started with, which is the saved edit or none
    /// (REQ-021-07, REQ-029-05, #247), else a version the pointer is over in _Versions_ (REQ-021-25), else the draft.
    fn shown_recipe(&self, compare: bool) -> &Recipe {
        if compare {
            &self.saved
        } else {
            self.previewed.as_ref().unwrap_or(&self.draft)
        }
    }

    /// The photo's versions, oldest first, ending with the one the file holds (REQ-021-24).
    pub fn versions(&self) -> &[Version] {
        &self.versions
    }

    /// The version the pointer is over in _Versions_ (REQ-021-25).
    pub fn previewed(&self) -> Option<&Recipe> {
        self.previewed.as_ref()
    }

    /// Whether the recipe shown has parts by the subject that isn't found yet: the preview would show the edit without
    /// them, so the viewer keeps showing the file meanwhile (REQ-022-05, #151).
    fn waits_for_subject(&self, compare: bool) -> bool {
        self.shown_recipe(compare).needs_subject() && matches!(self.subject, Subject::Unknown | Subject::Finding(_))
    }

    /// What is drawn of the photo: as [`Self::sized_geometry`], but all of it turned while _Straighten_ is held, around
    /// the crop at its size (REQ-029-12).
    fn shown_geometry(&self, compare: bool) -> Geometry {
        if self.straightening && !compare {
            self.draft.geometry().uncropped()
        } else {
            self.sized_geometry(compare)
        }
    }

    /// What the viewer fits: while comparing the output as editing started, while cropping all of it turned, else the
    /// output.
    fn sized_geometry(&self, compare: bool) -> Geometry {
        if compare {
            self.saved.geometry()
        } else if self.cropping {
            self.draft.geometry().uncropped()
        } else {
            // A version under the pointer shows with its own crop and turns (REQ-021-25).
            self.shown_recipe(false).geometry()
        }
    }
}
