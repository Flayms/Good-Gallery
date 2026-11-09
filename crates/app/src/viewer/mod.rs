//! The viewer (spec 009): one media item over the whole window, stepped through in the gallery's order. Photos load
//! their originals (`originals`) and show their thumbnail until then; they zoom and pan. The gallery keeps drawing
//! below the scrim and keeps the shown item in view. An info panel (REQ-009-08) and a filmstrip (spec 015) open beside
//! and below the media; the chrome takes a tint of the item's colors (REQ-013-11), and the photo grows from its tile
//! and back (REQ-013-12). Videos play in a [`Playback`](crate::video::Playback) with their controls along the bottom of
//! the video (REQ-009-06). A photo can be edited (spec 021): the adjustments panel takes the info panel's place and
//! the photo shows through the recipe (`edit`).

mod bar;
mod display;
mod editing;
mod face_regions;
mod filmstrip;
mod playback;

pub(crate) use bar::put_button;

use crate::edit::{self, EditSession, Leave};
use crate::gallery::{self, Gallery, MediaAction};
use crate::originals::{OriginalState, Originals};
use crate::video::controls::Control;
use crate::video::{PlaybackState, VideoFrame, Videos};
use crate::{info, theme};
use bar::editable;
use base64::Engine as _;
use db::gallery::GalleryItem;
use db::libraries::Library;
use db::media::file_in;
use display::{message, preload};
use domain::ambient::{self, Rgb};
use domain::faces;
use domain::media::MediaKind;
use domain::viewer::{self as zoom, FILMSTRIP_KEY, FILMSTRIP_THUMB, FIT, Strip, ZoomView};
use eframe::egui::{self, Color32, Key, Modifiers, Pos2, Rect, Vec2, pos2};
use face_regions::{draw_faces, draw_new_face, face_clicks};
use std::path::PathBuf;
use std::time::Duration;
use web_time::Instant;

/// The controls step aside after this long without pointer movement or key presses (REQ-013-10).
const IDLE: Duration = Duration::from_millis(2500);
const WHEEL_SENSITIVITY: f32 = 0.002;
/// Points per wheel line: one notch scrolls about 100 points in a browser.
const WHEEL_LINE: f32 = 100.0;
const KEY_ZOOM_STEP: f32 = 1.25;
/// Room around the media: for the arrows at the sides; above it the control bar, in the title bar's strip, and as much
/// room below (REQ-009-19).
const SIDE_SPACE: f32 = 64.0;
/// Room between the control bar or the filmstrip and the media.
const MEDIA_GAP: f32 = 12.0;
const BAR_HEIGHT: f32 = 48.0;
const NAV_SIZE: f32 = 40.0;
/// Distance of floating chrome from the window's edges.
const INSET: f32 = 12.0;
const STRIP: Strip = Strip {
    size: FILMSTRIP_THUMB,
    gap: 4.0,
    padding: 6.0,
};
const STRIP_HEIGHT: f32 = FILMSTRIP_THUMB + 12.0;
/// Room the filmstrip takes below the media and the control bar (REQ-015-01).
const STRIP_SPACE: f32 = STRIP_HEIGHT + INSET;

/// What the viewer asks the app for.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// It closed.
    Close,
    /// It closed to filter the gallery by this tag (REQ-009-09).
    FilterByTag(String),
    /// Explorer should show this file (REQ-009-10); the viewer stays.
    ShowInExplorer(PathBuf),
    /// Puts a photo on the clipboard, with its sidecar's orientation (REQ-009-25).
    Copy(PathBuf, Option<u8>),
    /// Save an edit (REQ-021-09, REQ-021-10); the viewer stays and hears back through [`Viewer::edit_saved`].
    SaveEdit(Box<edit::SaveEdit>),
    /// Ask to put the unedited original of this item back (REQ-021-12).
    Revert(crate::state::RevertTarget),
    /// Ask to remove this photo's earlier versions (REQ-021-26).
    ClearHistory(crate::state::ClearHistory),
    /// Rate an item (spec 024); the viewer stays.
    Rate(crate::state::Rate),
    /// Add or remove a tag of an item (spec 026); the viewer stays.
    Tag(crate::state::TagChange),
    /// What the user decided about a face (spec 032); the viewer stays.
    Face(crate::state::FaceAction),
    /// Add the folder of the file outside every library as a library (REQ-033-04); the viewer stays.
    AddFolder,
    /// Open the rename dialog for this item (REQ-009-21); the viewer stays.
    Rename(i64),
    /// Turn this photo a quarter by its orientation tag (REQ-009-22); the viewer stays.
    Turn(i64, domain::orientation::Turn),
    /// Ask before deleting this item (REQ-009-23); the viewer stays, and moves on once it is gone (REQ-009-24).
    Delete(i64),
    /// Open the date fix dialog for this item (REQ-038-01); the viewer stays.
    FixDates(i64),
    /// Let the user pick a LUT to import (REQ-043-01); the viewer stays.
    ImportLut,
}

/// What a face's popup is about.
#[derive(Clone)]
enum FaceTarget {
    /// A detected face without a name.
    Unknown(db::faces::ShownFace),
    /// A named region, with its name.
    Named { name: String, rect: [f32; 4] },
    /// A rectangle the user just drew.
    Drawn([f32; 4]),
}

/// The popup with what can be done with a face (REQ-032-09, REQ-032-12, REQ-032-13).
struct FacePopup {
    media: i64,
    target: FaceTarget,
    /// The name being typed.
    name: String,
}

/// What the keys and the mouse's back button asked for since the last frame.
#[derive(Debug, Default)]
struct Keys {
    close: bool,
    step: i32,
    zoom: Option<f32>,
    fit: bool,
    info: bool,
    filmstrip: bool,
    /// `p`: show or hide face regions (REQ-009-17).
    faces: bool,
    /// Space: play / pause the video.
    space: bool,
    /// ↑ / ↓: volume steps.
    volume: i32,
    /// `e`: start or stop editing.
    edit: bool,
    undo: bool,
    redo: bool,
    /// `1`–`5`: rate the item with that many stars (REQ-024-01).
    rate: Option<i64>,
    /// Delete: delete the item (REQ-009-23).
    delete: bool,
}

/// The video the viewer plays (REQ-009-06).
#[derive(Debug)]
struct Playing {
    /// Its media item.
    id: i64,
    /// Why it can't play at all (no player, or no file); its preview shows.
    unavailable: Option<String>,
    /// This frame's state and frame.
    state: PlaybackState,
    frame: Option<VideoFrame>,
    /// The seek slider's position while it is dragged.
    dragging: Option<f64>,
    /// Where the video controls were drawn in the last frame.
    bar: Option<Rect>,
    /// What the controls, a click on the video or a key asked for this frame.
    control: Option<Control>,
}

/// The chrome's colors: the glass tinted by the shown item (REQ-013-11).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chrome {
    pub bar: Color32,
    pub panel: Color32,
    tint: Option<Rgb>,
}

impl Chrome {
    fn new(tint: Option<Rgb>) -> Self {
        Self {
            bar: theme::glass_bar().fill(tint),
            panel: theme::glass_panel().fill(tint),
            tint,
        }
    }

    /// The bar or panel level as glass over the scrim's blur (`over`), tinted.
    fn backing(&self, glass: theme::Glass, over: Option<crate::glass::Painted>) -> crate::glass::Backing {
        crate::glass::Backing {
            glass,
            tint: self.tint,
            over,
        }
    }
}

/// The tint of an item with a ThumbHash: a dark, muted version of its average color.
fn tint_of(thumbhash: Option<&str>) -> Option<Rgb> {
    let hash = base64::engine::general_purpose::STANDARD.decode(thumbhash?).ok()?;
    let (r, g, b, _) = thumbhash::thumb_hash_to_average_rgba(&hash).ok()?;
    Some(ambient::ambient_tint(Rgb {
        r: r * 255.0,
        g: g * 255.0,
        b: b * 255.0,
    }))
}

/// The tile ↔ viewer transition (REQ-013-12): from when, and from or to which tile.
#[derive(Debug, Clone, Copy)]
struct Transition {
    /// egui's time when it started; `None` until the first frame.
    start: Option<f64>,
    /// The tile's rect; `None` fades instead.
    tile: Option<Rect>,
}

impl Transition {
    /// Progress 0 → 1, eased; 1 at once without motion.
    fn progress(&mut self, ctx: &egui::Context) -> f32 {
        if !theme::motion(ctx) {
            return 1.0;
        }
        let now = ctx.input(|input| input.time);
        let start = *self.start.get_or_insert(now);
        let t = ((now - start) as f32 / theme::DURATION_HERO).clamp(0.0, 1.0);
        if t < 1.0 {
            ctx.request_repaint();
        }
        // Ease out (cubic).
        1.0 - (1.0 - t).powi(3)
    }
}

pub struct Viewer {
    /// The imported LUTs, for editing (spec 043).
    luts: std::sync::Arc<crate::luts::Luts>,
    /// A text field had the keyboard at the end of the last frame (a new tag, REQ-026-01).
    typing: bool,
    /// The item is in a library. One outside every library, shown from a scratch index, has no rating, tags,
    /// editing, faces or filmstrip (REQ-033-03).
    indexed: bool,
    /// The shown item: its gallery position (out of the result, the place it had), and its id to find it again after a
    /// reload.
    index: usize,
    id: i64,
    /// The gallery result the position was checked against.
    version: u64,
    /// No longer in the index (REQ-009-12).
    missing: bool,
    /// No longer in the result, and shown all the same (REQ-009-18).
    left_result: bool,
    /// The shown item was deleted: once it leaves the result, the item taking its place shows (REQ-009-24).
    deleted: bool,
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
    info_open: bool,
    filmstrip: bool,
    /// Face regions are drawn over photos that have them (REQ-009-17).
    faces: bool,
    /// The filmstrip's scroll offset, and the position it was last centered on (REQ-015-03).
    strip_offset: f32,
    strip_centered: Option<usize>,
    /// Width of the filmstrip when it was last drawn: a new width centers it again.
    strip_width: f32,
    opening: Transition,
    /// Closing: the transition back, and what the app is told once it ends.
    closing: Option<(Transition, Action)>,
    /// Where the photo was drawn in the last frame.
    photo: Option<Rect>,
    /// The face regions drawn in the last frame, by name.
    faces_drawn: Vec<(String, Rect)>,
    face_popup: Option<FacePopup>,
    /// Drawing a new face (REQ-032-12): where the drag started, once it has.
    drawing_face: Option<Option<egui::Pos2>>,
    chrome: Chrome,
    /// The media item the chrome was tinted for.
    chrome_for: Option<i64>,
    /// The scrim, or the blur of it and the photo while the photo grows under the chrome, painted this frame: the
    /// chrome shows its blur (REQ-013-03).
    chrome_blur: Option<crate::glass::Painted>,
    playing: Option<Playing>,
    /// The last click was on the video or its controls: ← / → seek instead of stepping (REQ-009-02).
    video_focus: bool,
    /// Editing the shown photo (spec 021).
    edit: Option<EditSession>,
    /// What the session last asked to fit; a change fits the photo again.
    edited_size: Option<[f32; 2]>,
    /// The bar's button or the context menu asked to start or stop editing.
    toggle_edit: bool,
    /// Where to go once the save that was asked for on leaving has finished (REQ-021-08).
    after_save: Option<Leave>,
    leave_next: Option<Leave>,
    /// A saved copy to show once it is in the gallery (REQ-021-09).
    show_next: Option<i64>,
    /// _Compare_ is held in the panel (REQ-021-07).
    comparing: bool,
}

impl Viewer {
    /// Opens the viewer on gallery position `index` (media item `id`) of result `version`, growing from `tile` if it is
    /// on screen; the filmstrip shows if the user left it shown (REQ-015-02).
    pub fn new(index: usize, id: i64, version: u64, tile: Option<Rect>, filmstrip: bool) -> Self {
        Self {
            index,
            id,
            version,
            missing: false,
            left_result: false,
            deleted: false,
            view: FIT,
            instant: None,
            fade_from: None,
            last_activity: Instant::now(),
            close_at: None,
            keys: Keys::default(),
            luts: Default::default(),
            typing: false,
            indexed: true,
            info_open: false,
            filmstrip,
            faces: false,
            strip_offset: 0.0,
            strip_centered: None,
            strip_width: 0.0,
            opening: Transition { start: None, tile },
            closing: None,
            photo: None,
            faces_drawn: Vec::new(),
            face_popup: None,
            drawing_face: None,
            chrome: Chrome::new(None),
            chrome_blur: None,
            chrome_for: None,
            playing: None,
            video_focus: false,
            edit: None,
            edited_size: None,
            toggle_edit: false,
            after_save: None,
            leave_next: None,
            show_next: None,
            comparing: false,
        }
    }

    /// Opens the viewer on gallery position `index` as the user left the filmstrip, growing from its tile.
    pub fn open(gallery: &Gallery, index: usize) -> Option<Self> {
        let id = gallery.id_at(index)?;
        let filmstrip = zoom::is_filmstrip_shown(gallery.setting(FILMSTRIP_KEY).as_deref());
        let mut viewer = Self::new(index, id, gallery.data_version(), gallery.tile_rect(index), filmstrip);
        viewer.faces = faces::are_faces_shown(gallery.setting(faces::FACES_KEY).as_deref());
        Some(viewer)
    }

    /// Shows the item in its place from the first frame, without growing out of its tile or fading in (REQ-027-07).
    pub fn open_at_once(&mut self) {
        self.opening = Transition {
            start: Some(f64::NEG_INFINITY),
            tile: None,
        };
    }

    /// Opens the viewer on a file outside every library, from its scratch index's gallery (REQ-033-01, REQ-033-03).
    pub fn open_loose(gallery: &Gallery, index: usize) -> Option<Self> {
        let mut viewer = Self::open(gallery, index)?;
        viewer.indexed = false;
        viewer.filmstrip = false;
        viewer.faces = false;
        Some(viewer)
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    /// Media items `ids` were deleted: if the shown one is among them, the viewer moves on once the gallery no longer
    /// has it (REQ-009-24).
    pub fn deleted(&mut self, ids: &[i64]) {
        if ids.contains(&self.id) {
            self.deleted = true;
        }
    }

    pub fn view(&self) -> ZoomView {
        self.view
    }

    pub fn info_open(&self) -> bool {
        self.info_open
    }

    /// Opens or closes the info panel, as `i` does.
    pub fn set_info_open(&mut self, open: bool) {
        self.info_open = open;
    }

    /// Whether the shown item is in a library; one outside every library has no rating, tags, editing, faces or
    /// filmstrip (REQ-033-03).
    pub fn is_indexed(&self) -> bool {
        self.indexed
    }

    /// The chrome's colors in the last frame (REQ-013-11).
    pub fn chrome(&self) -> Chrome {
        self.chrome
    }

    /// Where the photo was drawn in the last frame.
    pub fn photo_rect(&self) -> Option<Rect> {
        self.photo
    }

    /// Whether it is closing (shrinking back into its tile or fading).
    pub fn is_closing(&self) -> bool {
        self.closing.is_some()
    }

    /// Shows media item `id` once the gallery has it (a saved copy, REQ-021-09).
    pub fn show_when_listed(&mut self, id: i64) {
        self.show_next = Some(id);
        // Checked against the next result, or the current one.
        self.version = self.version.wrapping_sub(1);
    }

    /// Whether the shown photo's original was loaded when it came up, so it shows without fading in (REQ-009-14).
    pub fn shows_original_at_once(&self) -> bool {
        self.instant == Some(true)
    }

    fn layer() -> egui::LayerId {
        egui::LayerId::new(egui::Order::Middle, egui::Id::NULL.with("viewer"))
    }

    /// Takes the viewer's keys before anything else sees them; call at the start of the frame.
    pub fn input(&mut self, ctx: &egui::Context) {
        // Focus stays in the viewer (its controls, the filmstrip's thumbnails) and what lies above it; nothing under it
        // takes focus or acts on Enter or Space. As the modal layer, Tab only moves focus among them (REQ-015-04).
        ctx.memory_mut(|memory| memory.set_modal_layer(Self::layer()));
        let focused = ctx.memory(|memory| memory.focused());
        let focus_inside = focused.and_then(|id| ctx.read_response(id)).is_some_and(|response| {
            response.layer_id == Self::layer()
                || ctx.memory(|memory| {
                    memory.top_modal_layer() == Some(Self::layer()) && memory.is_above_modal_layer(response.layer_id)
                })
        });
        if let Some(focused) = focused
            && !focus_inside
        {
            ctx.memory_mut(|memory| memory.surrender_focus(focused));
        }
        // Typing into a field (a new tag, REQ-026-01): its keys aren't the viewer's shortcuts, and Escape leaves the field.
        // Also the last frame's: egui takes the focus away before a frame with Escape starts.
        let typing = self.typing || ctx.egui_wants_keyboard_input();
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
            if typing {
                return;
            }
            // The mouse's back button and Alt + ← close instead of stepping back (REQ-009-02, REQ-009-03).
            if input.pointer.button_pressed(egui::PointerButton::Extra1)
                || input.consume_key(Modifiers::ALT, Key::ArrowLeft)
                || input.consume_key(Modifiers::NONE, Key::Escape)
            {
                self.keys.close = true;
            }
            input.consume_key(Modifiers::NONE, Key::Tab);
            input.consume_key(Modifiers::SHIFT, Key::Tab);
            if !focus_inside {
                // Space plays or pauses a video, unless a control has focus (which Space presses).
                if input.consume_key(Modifiers::NONE, Key::Space) {
                    self.keys.space = true;
                }
                for key in [Key::Enter, Key::Space] {
                    input.consume_key(Modifiers::NONE, key);
                    input.consume_key(Modifiers::SHIFT, key);
                }
            }
            if self.edit.is_some() {
                // Ctrl + Shift + Z first: Ctrl + Z also matches it.
                if input.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)
                    || input.consume_key(Modifiers::COMMAND, Key::Y)
                {
                    self.keys.redo = true;
                }
                if input.consume_key(Modifiers::COMMAND, Key::Z) {
                    self.keys.undo = true;
                }
            }
            if input.modifiers.ctrl || input.modifiers.alt || input.modifiers.command {
                return;
            }
            if input.consume_key(Modifiers::NONE, Key::E) {
                self.keys.edit = true;
            }
            if input.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                self.keys.step -= 1;
            }
            if input.consume_key(Modifiers::NONE, Key::ArrowRight) {
                self.keys.step += 1;
            }
            if input.consume_key(Modifiers::NONE, Key::ArrowUp) {
                self.keys.volume += 1;
            }
            if input.consume_key(Modifiers::NONE, Key::ArrowDown) {
                self.keys.volume -= 1;
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
            if input.consume_key(Modifiers::NONE, Key::I) {
                self.keys.info ^= true;
            }
            if input.consume_key(Modifiers::NONE, Key::F) {
                self.keys.filmstrip ^= true;
            }
            if input.consume_key(Modifiers::NONE, Key::P) {
                self.keys.faces ^= true;
            }
            // Not while editing, whose number fields take digits; never in the browser, which doesn't write files
            // (REQ-024-05).
            if self.edit.is_none() && !crate::WEB {
                let stars = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5];
                for (rating, key) in (1..).zip(stars) {
                    if input.consume_key(Modifiers::NONE, key) {
                        self.keys.rate = Some(rating);
                    }
                }
                if input.consume_key(Modifiers::NONE, Key::Delete) {
                    self.keys.delete = true;
                }
            }
        });
    }

    fn go_to(&mut self, index: usize, gallery: &Gallery) {
        let Some(id) = gallery.id_at(index) else { return };
        (self.index, self.id) = (index, id);
        self.missing = false;
        self.left_result = false;
        self.deleted = false;
        self.view = FIT;
        self.instant = None;
        self.fade_from = None;
        self.close_at = None;
        self.video_focus = false;
    }

    /// Finds the shown item again after the gallery reloaded (REQ-009-12).
    fn follow(&mut self, gallery: &Gallery) {
        if self.version == gallery.data_version() {
            return;
        }
        self.version = gallery.data_version();
        if let Some(next) = self.show_next
            && self.edit.is_none()
            && let Some(index) = gallery.position_of(next)
        {
            self.show_next = None;
            self.go_to(index, gallery);
            return;
        }
        if gallery.id_at(self.index) == Some(self.id) {
            (self.missing, self.left_result) = (false, false);
        } else if let Some(index) = gallery.position_of(self.id) {
            self.index = index;
            (self.missing, self.left_result) = (false, false);
        } else if self.deleted {
            // The item that took its place, or the one before it at the end; none left closes (REQ-009-24).
            match gallery.count().unwrap_or(0) {
                0 => self.close(gallery, Action::Close),
                count => self.go_to(self.index.min(count - 1), gallery),
            }
        } else {
            // Whether it is still indexed comes with `Gallery::item_outside` (REQ-009-18).
            self.left_result = true;
            self.index = self.index.min(gallery.count().unwrap_or(0));
        }
    }

    /// The gallery position a step of `by` leads to.
    fn step_target(&self, by: i32, count: usize) -> Option<usize> {
        zoom::step_target(self.index, count, by, self.left_result)
    }

    /// Starts closing: the photo shrinks back into its tile, or the viewer fades (REQ-013-12).
    fn close(&mut self, gallery: &Gallery, action: Action) {
        if self.closing.is_none() {
            let tile = if self.missing || self.left_result {
                None
            } else {
                gallery.tile_rect(self.index)
            };
            self.closing = Some((Transition { start: None, tile }, action));
        }
    }

    /// Draws the viewer over the window. Returns what the app should do: [`Action::Close`] and
    /// [`Action::FilterByTag`] once it has closed.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        gallery: &mut Gallery,
        originals: &mut Originals,
        libraries: &[Library],
        videos: &mut Videos,
        luts: &std::sync::Arc<crate::luts::Luts>,
    ) -> Option<Action> {
        self.luts = luts.clone();
        self.follow(gallery);
        let count = gallery.count().unwrap_or(0);
        let mut keys = std::mem::take(&mut self.keys);
        // What needs the index isn't there for a file outside every library (REQ-033-03).
        if !self.indexed {
            (keys.edit, keys.filmstrip, keys.faces, keys.rate, keys.delete) = (false, false, false, None, false);
        }
        // Escape first leaves what the faces are doing (REQ-032-09, REQ-032-12).
        if keys.close && (self.face_popup.is_some() || self.drawing_face.is_some()) {
            keys.close = false;
            self.face_popup = None;
            self.drawing_face = None;
        }
        self.track_video_focus(ctx);
        if let Some(session) = &mut self.edit {
            session.poll();
        }
        if let Some(leave) = self.leave_next.take() {
            self.apply_leave(leave, gallery);
        }
        if self.closing.is_none() && self.edit.is_some() {
            keys = self.edit_keys(keys, count, gallery);
        }
        if self.closing.is_none() {
            if keys.close || self.close_at.is_some_and(|at| Instant::now() >= at) {
                self.close(gallery, Action::Close);
            }
            let video = self.playing_shown().map(|playing| playing.state.clone());
            match &video {
                // While the video controls have focus, ← / → seek and ↑ / ↓ change the volume (REQ-009-02).
                Some(state) if self.video_focus && keys.step != 0 => {
                    let by = f64::from(keys.step.signum()) * domain::video::SEEK_STEP;
                    self.control_video(Control::Seek(
                        domain::video::seek_by(state.position, by, state.duration),
                        true,
                    ));
                }
                _ if keys.step != 0 => {
                    if let Some(target) = self.step_target(keys.step, count) {
                        self.go_to(target, gallery);
                    }
                }
                _ => {}
            }
            if let Some(state) = &video {
                if keys.space {
                    self.control_video(Control::PlayPause);
                }
                // From the volume set before muting; a step up unmutes.
                if keys.volume != 0 && self.video_focus {
                    self.control_video(Control::Volume(domain::video::step_volume(state.volume, keys.volume)));
                }
            }
            if keys.info {
                self.info_open = !self.info_open;
            }
            if keys.filmstrip {
                self.toggle_filmstrip(gallery);
            }
            if keys.faces {
                self.toggle_faces(gallery);
            }
        }
        // How far the viewer is open: grows in, shrinks back out.
        let (progress, tile) = match &mut self.closing {
            Some((transition, _)) => (1.0 - transition.progress(ctx), transition.tile),
            None => (self.opening.progress(ctx), self.opening.tile),
        };
        if let Some((_, action)) = &self.closing
            && progress <= 0.0
        {
            return Some(action.clone());
        }

        let item = if self.left_result {
            let item = gallery.item_outside(self.id).cloned();
            self.missing = gallery.is_outside_gone(self.id);
            item
        } else {
            gallery.forget_outside();
            gallery.item(self.index).cloned()
        };
        let outside = item.as_ref().filter(|_| self.left_result);
        preload(gallery, originals, libraries, outside, self.index, count);
        gallery.set_viewed((!self.missing).then_some((self.index, self.id)));
        let original = originals.get(self.id);
        if item.is_some() && self.instant.is_none() {
            self.instant = Some(matches!(original, OriginalState::Loaded(_)));
        }
        self.sync_video(ctx, item.as_ref(), videos, libraries);
        if self.chrome_for != Some(self.id) && item.is_some() {
            self.chrome = Chrome::new(tint_of(item.as_ref().and_then(|item| item.thumbhash.as_deref())));
            self.chrome_for = Some(self.id);
        }

        let mut action = None;
        if let (Some(item), Some(stars), None) = (&item, keys.rate, &self.closing) {
            action = Some(Action::Rate(crate::state::Rate::toggle(item, stars)));
        }
        if let (Some(item), true, None, false) = (&item, keys.delete, &self.closing, self.left_result) {
            action = Some(Action::Delete(item.id));
        }
        if let Some(item) = &item {
            self.sync_edit(ctx, item, libraries, originals, gallery, keys.edit);
            // Whether the photo has face regions, and where (REQ-009-17).
            if item.kind == MediaKind::Image {
                gallery.detail(item.id);
            }
        }

        let screen = ctx.content_rect();
        let info_open = self.info_open || self.edit.is_some();
        let reserved = [
            crate::titlebar::reserved_left_in(ctx, screen.left()),
            crate::titlebar::reserved_in(ctx, screen.right()),
        ];
        let layout = Layout::new(screen, info_open, self.filmstrip, reserved);
        egui::Area::new(Self::layer().id)
            .order(Self::layer().order)
            .fixed_pos(screen.min)
            .constrain(false)
            .show(ctx, |ui| {
                // egui's dark style too: the Area starts from the global one (REQ-020-05).
                theme::apply_style(ui);
                ui.set_min_size(screen.size());
                // Without a tile to grow from, the whole viewer fades in and out.
                if tile.is_none() {
                    ui.set_opacity(progress);
                }
                // The window behind, blurred and darkened; opaque without glass (REQ-013-14, REQ-013-06).
                let scrim = crate::glass::Surface::new(screen, 0.0, theme::SCRIM).opacity(progress);
                let scrim = crate::glass::paint(ui.painter(), scrim);
                // What the chrome blurs: the window under the scrim's darkening, then the photo (REQ-013-03).
                crate::glass::record_scrim(ui.ctx(), screen, progress);
                let backdrop = ui.interact(screen, ui.make_persistent_id("backdrop"), egui::Sense::click_and_drag());
                let label = item
                    .as_ref()
                    .map_or("Media viewer", |item| item.file_name.as_str())
                    .to_owned();
                backdrop.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Dialog, true, &label));
                let closing = self.closing.is_some();
                self.faces_drawn.clear();
                match &item {
                    Some(item) => {
                        let keys = if closing { Keys::default() } else { keys };
                        let transition = tile.filter(|_| progress < 1.0).map(|tile| (tile, progress));
                        let drawn = self.media(ui, &backdrop, &layout, item, gallery, &original, keys, transition);
                        self.photo = Some(drawn);
                        if self.faces && transition.is_none() && !closing && self.edit.is_none() {
                            let faces = gallery
                                .cached_detail(item.id)
                                .map_or(&[][..], |detail| &detail.faces)
                                .to_vec();
                            let area = if self.view.is_zoomed() {
                                layout.stage
                            } else {
                                layout.media
                            };
                            self.faces_drawn = draw_faces(ui, area, drawn, &faces);
                            let unknown = gallery.faces_on(item.id).to_vec();
                            if item.kind == MediaKind::Image && !crate::WEB {
                                if let Some(target) =
                                    face_clicks(ui, drawn, &faces, &unknown, self.drawing_face.is_some())
                                {
                                    self.face_popup = Some(FacePopup {
                                        media: item.id,
                                        name: match &target {
                                            FaceTarget::Named { name, .. } => name.clone(),
                                            _ => String::new(),
                                        },
                                        target,
                                    });
                                }
                                if let Some(rect) = draw_new_face(ui, drawn, &mut self.drawing_face) {
                                    self.face_popup = Some(FacePopup {
                                        media: item.id,
                                        name: String::new(),
                                        target: FaceTarget::Drawn(rect),
                                    });
                                }
                                let people = gallery.tag_paths();
                                if let Some(chosen) = self.face_popup_ui(ui, drawn, &people.people) {
                                    action = Some(Action::Face(chosen));
                                }
                            }
                        }
                    }
                    None if self.missing => {
                        message(ui, layout.media.center(), "Media not found");
                        self.backdrop_clicks(&backdrop);
                    }
                    None => {
                        self.backdrop_clicks(&backdrop);
                    }
                }
                // The context menu's choice (REQ-009-11); after the photo, so its glass blurs it (REQ-013-03).
                if !closing && let Some(item) = &item {
                    let editable = editable(item, libraries) && self.edit.is_none() && self.indexed;
                    let revert = gallery
                        .cached_detail(item.id)
                        .filter(|_| !crate::WEB && self.edit.is_none() && self.indexed)
                        .and_then(|detail| {
                            let edit = detail.edit.as_ref()?;
                            edit.replaced_original(&detail.rel_path)
                                .then(|| crate::state::RevertTarget::of(detail))
                        });
                    // Rename, rate and rotate an indexed item, not while it is edited (REQ-009-21).
                    let items = gallery::MenuItems {
                        edit: editable,
                        revert: revert.is_some(),
                        write: self.indexed && self.edit.is_none(),
                        ..gallery::MenuItems::for_item(item.kind, &item.file_name)
                    };
                    backdrop.context_menu(|ui| match gallery::context_menu_with(ui, items) {
                        Some(MediaAction::Copy) => {
                            action = file_in(libraries, item.root_id, &item.rel_path)
                                .map(|file| Action::Copy(file, item.sidecar_orientation));
                        }
                        Some(MediaAction::ShowInExplorer) => action = Self::explorer_action(item, libraries),
                        Some(MediaAction::Information) => self.info_open = true,
                        Some(MediaAction::Edit) => self.toggle_edit = true,
                        Some(MediaAction::Revert) => action = revert.clone().map(Action::Revert),
                        Some(MediaAction::Rename) => action = Some(Action::Rename(item.id)),
                        Some(MediaAction::Rate(rating)) => {
                            action = Some(Action::Rate(crate::state::Rate {
                                media: item.id,
                                root_id: item.root_id,
                                rel_path: item.rel_path.clone(),
                                kind: item.kind,
                                rating,
                            }));
                        }
                        Some(MediaAction::Turn(turn)) => action = Some(Action::Turn(item.id, turn)),
                        Some(MediaAction::Delete) => action = Some(Action::Delete(item.id)),
                        Some(MediaAction::FixDates) => action = Some(Action::FixDates(item.id)),
                        Some(MediaAction::Select) | None => {}
                    });
                }
                if closing {
                    return;
                }
                // Open, the photo stays in its area, clear of the chrome, which shows the scrim's blur; only while it
                // grows from its tile does the chrome cover it, and blur it (REQ-013-03).
                let growing = item.is_some() && tile.is_some() && progress < 1.0;
                self.chrome_blur = if growing {
                    crate::glass::blur_for_chrome(ui.painter(), screen).or(scrim)
                } else {
                    scrim
                };
                ui.scope(|ui| {
                    ui.multiply_opacity(progress);
                    if let Some(chosen) = self.controls(ui, &layout, item.as_ref(), count, gallery, libraries) {
                        action = Some(chosen);
                    }
                    if let Some(chosen) = self.edit_panel(ui, &layout, gallery) {
                        action = Some(chosen);
                    }
                });
            });
        if let Some(chosen) = self.edit_dialog(ctx, gallery) {
            action = Some(chosen);
        }
        if let Some(at) = self.close_at {
            ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
        }
        self.apply_video_control(videos);
        self.typing = ctx.egui_wants_keyboard_input();
        match action {
            Some(Action::ShowInExplorer(path)) => Some(Action::ShowInExplorer(path)),
            Some(Action::Copy(path, orientation)) => Some(Action::Copy(path, orientation)),
            Some(Action::SaveEdit(request)) => Some(Action::SaveEdit(request)),
            Some(Action::Revert(target)) => Some(Action::Revert(target)),
            Some(Action::ClearHistory(clear)) => Some(Action::ClearHistory(clear)),
            Some(Action::Rate(rate)) => Some(Action::Rate(rate)),
            Some(Action::Tag(change)) => Some(Action::Tag(change)),
            Some(Action::Face(face)) => Some(Action::Face(face)),
            Some(Action::AddFolder) => Some(Action::AddFolder),
            // Closing while editing asks about unsaved changes first (REQ-021-08).
            Some(Action::Close) if self.edit.is_some() => {
                self.leave_edit(Leave::Close, gallery);
                None
            }
            Some(other) => {
                self.close(gallery, other);
                // Without motion it closes at once.
                if !theme::motion(ctx) {
                    return self.closing.take().map(|(_, action)| action);
                }
                ctx.request_repaint();
                None
            }
            None => {
                if !theme::motion(ctx)
                    && let Some((_, action)) = self.closing.take()
                {
                    return Some(action);
                }
                None
            }
        }
    }

    fn explorer_action(item: &GalleryItem, libraries: &[Library]) -> Option<Action> {
        file_in(libraries, item.root_id, &item.rel_path).map(Action::ShowInExplorer)
    }

    /// Like [`Self::backdrop_clicks`], but only for clicks beside `media`; false for a single click on it.
    fn backdrop_clicks_beside(&mut self, backdrop: &egui::Response, media: Rect) -> bool {
        let on_media = backdrop.interact_pointer_pos().is_some_and(|pos| media.contains(pos));
        if on_media && backdrop.clicked() && !backdrop.double_clicked() {
            return false;
        }
        if !on_media {
            self.backdrop_clicks(backdrop);
        } else if backdrop.double_clicked() {
            self.close_at = None;
        }
        true
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

    /// The info panel at the right (REQ-009-08); a click on a tag filters the gallery by it (REQ-009-09).
    fn info_panel(&mut self, ui: &mut egui::Ui, rect: Rect, gallery: &mut Gallery) -> Option<Action> {
        self.chrome.backing(theme::glass_panel(), self.chrome_blur).paint(
            ui.painter(),
            rect,
            f32::from(theme::RADIUS_PANEL),
        );
        // Clicks and wheel turns on the panel stay in it.
        ui.interact(rect, ui.make_persistent_id("info panel"), egui::Sense::click());
        let mut clicked = None;
        let mut add_folder = false;
        let inner = rect.shrink(16.0);
        let mut panel = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        panel.set_clip_rect(inner);
        egui::ScrollArea::vertical()
            .id_salt("info panel")
            .auto_shrink(false)
            .show(&mut panel, |ui| {
                // A file outside every library: the way into the index; its _Library_ below says it is in none
                // (REQ-033-03, REQ-033-04).
                if !self.indexed {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                "Add its folder as a library to rate, tag and edit it, and to find it again.",
                            )
                            .color(theme::muted()),
                        )
                        .wrap(),
                    );
                    if ui.button("Add folder as library").clicked() {
                        add_folder = true;
                    }
                    ui.add_space(16.0);
                }
                let paths = gallery.tag_paths();
                clicked = info::show_loaded(ui, gallery.detail(self.id), &paths, self.indexed && !crate::WEB);
            });
        if add_folder {
            return Some(Action::AddFolder);
        }
        match clicked? {
            info::InfoAction::FilterByTag(tag) => Some(Action::FilterByTag(tag)),
            info::InfoAction::LookForFacesAgain => {
                Some(Action::Face(crate::state::FaceAction::LookAgain { media: self.id }))
            }
            info::InfoAction::Revert => gallery
                .cached_detail(self.id)
                .map(|detail| Action::Revert(crate::state::RevertTarget::of(detail))),
            info::InfoAction::ClearHistory => gallery
                .cached_detail(self.id)
                .and_then(crate::state::ClearHistory::of)
                .map(Action::ClearHistory),
            info::InfoAction::Rate(stars) => gallery
                .cached_detail(self.id)
                .map(|detail| Action::Rate(crate::state::Rate::toggle_detail(detail, stars))),
            info::InfoAction::AddTag(path) => gallery
                .cached_detail(self.id)
                .map(|detail| Action::Tag(crate::state::TagChange::of(detail, domain::tagging::Change::Add(path)))),
            info::InfoAction::RemoveTag(name) => gallery.cached_detail(self.id).map(|detail| {
                Action::Tag(crate::state::TagChange::of(
                    detail,
                    domain::tagging::Change::Remove(name),
                ))
            }),
        }
    }
}

/// Where everything goes in the window: the media area shrinks for the info panel and the filmstrip, so they never
/// cover it (REQ-009-08, REQ-015-01).
struct Layout {
    screen: Rect,
    media: Rect,
    /// Where a zoomed photo may spread: the window beside the info panel (REQ-009-20). Its middle is the media's.
    stage: Rect,
    info: Option<Rect>,
    strip: Option<Rect>,
    /// Right edge of the area for the media, the arrows, the bar and the filmstrip.
    right: f32,
    /// Room the window controls take at the left and the right of the title bar's strip.
    reserved: [f32; 2],
}

impl Layout {
    fn new(screen: Rect, info_open: bool, filmstrip: bool, reserved: [f32; 2]) -> Self {
        let info = info_open.then(|| {
            Rect::from_min_max(
                // Below the window controls (REQ-013-02).
                pos2(
                    // Never wider than the window (REQ-019-15).
                    (screen.right() - INSET - info::WIDTH).max(screen.left() + INSET),
                    screen.top() + crate::titlebar::HEIGHT + INSET,
                ),
                pos2(screen.right() - INSET, screen.bottom() - INSET),
            )
        });
        let right = info.map_or(screen.right(), |panel| panel.left());
        let below = if filmstrip { STRIP_SPACE } else { 0.0 };
        let strip = filmstrip.then(|| {
            Rect::from_min_max(
                pos2(screen.left() + INSET, screen.bottom() - INSET - STRIP_HEIGHT),
                pos2(right - INSET, screen.bottom() - INSET),
            )
        });
        // As much room below as above, so the media stays in the middle when the controls fade (REQ-009-19).
        let margin = (INSET + BAR_HEIGHT).max(below) + MEDIA_GAP;
        let media = Rect::from_min_max(
            pos2(screen.left() + SIDE_SPACE, screen.top() + margin),
            pos2(right - SIDE_SPACE, screen.bottom() - margin),
        );
        let stage = Rect::from_min_max(screen.min, pos2(right, screen.bottom()));
        Self {
            screen,
            media,
            stage,
            info,
            strip,
            right,
            reserved,
        }
    }

    /// Whether `pos` is over the info panel or the filmstrip.
    fn is_chrome(&self, pos: Pos2) -> bool {
        self.info.is_some_and(|r| r.contains(pos)) || self.strip.is_some_and(|r| r.contains(pos))
    }
}

#[cfg(test)]
mod tests {
    use super::bar::control_bar_rect;
    use super::*;

    const SCREEN: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1280.0, 800.0));

    /// REQ-009-19: as much room above the photo as below, with or without the filmstrip and the info panel, and the
    /// bar above it.
    #[test]
    fn req_009_19_centers_the_photo_with_the_bar_above_it() {
        for info in [false, true] {
            for filmstrip in [false, true] {
                let layout = Layout::new(SCREEN, info, filmstrip, [0.0, 0.0]);
                let above = layout.media.top() - SCREEN.top();
                let below = SCREEN.bottom() - layout.media.bottom();
                assert!(
                    (above - below).abs() < 0.5,
                    "info {info}, filmstrip {filmstrip}: {above} above, {below} below"
                );
                let bar = control_bar_rect(&layout, 400.0);
                assert!(bar.bottom() < layout.media.top(), "{bar:?} above {:?}", layout.media);
                assert!(
                    (bar.center().x - layout.media.center().x).abs() < 0.5,
                    "{bar:?} over the photo"
                );
                if let Some(strip) = layout.strip {
                    assert!(
                        strip.top() > layout.media.bottom(),
                        "{strip:?} below {:?}",
                        layout.media
                    );
                }
            }
        }
    }

    /// REQ-009-19: in a narrow window the bar moves aside or shrinks rather than covering the window controls.
    #[test]
    fn req_009_19_the_bar_keeps_clear_of_the_window_controls() {
        let narrow = Rect::from_min_max(pos2(0.0, 0.0), pos2(640.0, 600.0));
        for reserved in [[0.0, 138.0], [90.0, 0.0]] {
            let layout = Layout::new(narrow, false, false, reserved);
            for width in [300.0, 600.0] {
                let bar = control_bar_rect(&layout, width);
                assert!(
                    bar.left() >= reserved[0] && bar.right() <= 640.0 - reserved[1],
                    "{reserved:?}: {bar:?}"
                );
                assert!(bar.width() > 0.0);
            }
        }
    }
}
