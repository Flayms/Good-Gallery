//! The viewer (spec 009): one media item over the whole window, stepped through in the gallery's order. Photos load
//! their originals (`originals`) and show their thumbnail until then; they zoom and pan. The gallery keeps drawing
//! below the scrim and keeps the shown item in view. An info panel (REQ-009-08) and a filmstrip (spec 015) open beside
//! and below the media; the chrome takes a tint of the item's colors (REQ-013-11), and the photo grows from its tile
//! and back (REQ-013-12). Videos play in a [`Playback`](crate::video::Playback) with their controls along the bottom of
//! the video (REQ-009-06). A photo can be edited (spec 021): the adjustments panel takes the info panel's place and
//! the photo shows through the recipe (`edit`).

use crate::edit::{self, Dialog, EditSession, Leave, Outcome, SaveMode};
use crate::gallery::{self, Gallery, MediaAction};
use crate::icons::{self, Icon};
use crate::originals::{OriginalRequest, OriginalState, Originals};
use crate::video::controls::{self as video_controls, Control};
use crate::video::{PlaybackState, VideoFrame, Videos};
use crate::{format, info, theme};
use base64::Engine as _;
use db::gallery::GalleryItem;
use db::libraries::Library;
use db::media::file_in;
use domain::ambient::{self, Rgb};
use domain::faces;
use domain::media::MediaKind;
use domain::viewer::{self as zoom, FILMSTRIP_KEY, FILMSTRIP_THUMB, FIT, Size, Strip, ZoomView};
use eframe::egui::{self, Color32, CursorIcon, Key, Modifiers, Pos2, Rect, Vec2, pos2, vec2};
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

    pub fn filmstrip_shown(&self) -> bool {
        self.filmstrip
    }

    /// Whether the shown item is in a library; one outside every library has no rating, tags, editing, faces or
    /// filmstrip (REQ-033-03).
    pub fn is_indexed(&self) -> bool {
        self.indexed
    }

    pub fn faces_shown(&self) -> bool {
        self.faces
    }

    /// The face regions drawn in the last frame (REQ-009-17), by name, on screen.
    pub fn faces_drawn(&self) -> &[(String, Rect)] {
        &self.faces_drawn
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

    /// The shown video's playback state in the last frame, if it plays.
    pub fn video(&self) -> Option<&PlaybackState> {
        self.playing_shown().map(|playing| &playing.state)
    }

    /// Why the shown video can't play, if it can't.
    pub fn video_unavailable(&self) -> Option<&str> {
        self.playing
            .as_ref()
            .filter(|playing| playing.id == self.id)
            .and_then(|playing| playing.unavailable.as_deref())
    }

    /// Whether ← / → seek the video (the last click was on it or its controls) rather than step (REQ-009-02).
    pub fn video_focused(&self) -> bool {
        self.video_focus
    }

    /// The video being played, if it is the shown item and can play.
    fn playing_shown(&self) -> Option<&Playing> {
        self.playing
            .as_ref()
            .filter(|playing| playing.id == self.id && playing.unavailable.is_none() && !self.missing)
    }

    /// Asks the shown video for `control` (applied after the frame is drawn).
    fn control_video(&mut self, control: Control) {
        if let Some(playing) = self.playing.as_mut().filter(|playing| playing.unavailable.is_none()) {
            playing.control = Some(control);
        }
    }

    /// Starts editing the shown photo with the next frame, as `e` does.
    pub fn request_edit(&mut self) {
        self.toggle_edit = true;
    }

    /// The edit of the shown photo, while editing (spec 021).
    pub fn editing(&self) -> Option<&EditSession> {
        self.edit.as_ref()
    }

    pub fn editing_mut(&mut self) -> Option<&mut EditSession> {
        self.edit.as_mut()
    }

    /// The app finished the save it was asked for: editing ends (and leaves as asked), or stays open after a failure,
    /// which the app shows (REQ-021-16).
    pub fn edit_saved(&mut self, media: i64, saved: bool) {
        let Some(session) = self.edit.as_mut().filter(|session| session.media() == media) else {
            return;
        };
        session.saving = false;
        if saved {
            self.edit = None;
            self.leave_next = self.after_save.take();
        } else {
            self.after_save = None;
        }
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

    fn toggle_filmstrip(&mut self, gallery: &Gallery) {
        self.filmstrip = !self.filmstrip;
        self.strip_centered = None;
        gallery.store_setting(FILMSTRIP_KEY, if self.filmstrip { "shown" } else { "hidden" });
    }

    /// The popup of the face clicked or drawn: what it offers depends on whether it has a name.
    fn face_popup_ui(
        &mut self,
        ui: &egui::Ui,
        photo: Rect,
        people: &[Vec<String>],
    ) -> Option<crate::state::FaceAction> {
        use crate::people_page::{decision, naming, tips};
        use crate::state::FaceAction;
        let mut popup = self.face_popup.take()?;
        let rect = match &popup.target {
            FaceTarget::Unknown(face) => face.rect,
            FaceTarget::Named { rect, .. } | FaceTarget::Drawn(rect) => *rect,
        };
        let anchor = on_screen(photo, rect);
        let media = popup.media;
        let mut action = None;
        let mut close = false;
        egui::Area::new(ui.make_persistent_id("face popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(anchor.left_bottom() + vec2(0.0, FACE_LABEL_HEIGHT + 8.0))
            .show(ui.ctx(), |ui| {
                crate::glass::panel(ui, egui::Margin::same(10), |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    let id = ui.make_persistent_id("face popup name");
                    let field = crate::names::field(ui, id, &mut popup.name, "Who is this?", 200.0);
                    field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Who is this?"));
                    // A known person picked, or Enter on the field (REQ-032-34).
                    let picked = crate::names::suggestions(ui, id, &field, &popup.name, people);
                    let suggestion = match &popup.target {
                        FaceTarget::Unknown(face) => face.suggestion.clone(),
                        _ => None,
                    };
                    if let (FaceTarget::Unknown(face), Some(name)) = (&popup.target, &suggestion) {
                        ui.horizontal(|ui| {
                            if ui.button(format!("It's {name}")).clicked() {
                                action = Some(FaceAction::Name {
                                    name: name.clone(),
                                    places: vec![(media, face.rect)],
                                });
                            }
                            if ui.button("Not this person").clicked() {
                                action = Some(FaceAction::NotThisPerson {
                                    face: face.id,
                                    name: name.clone(),
                                });
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        let typed = crate::names::clean(&popup.name);
                        match &popup.target {
                            FaceTarget::Named { name, rect } => {
                                let renamed = typed.as_ref().is_some_and(|to| to != name);
                                let to = if naming(ui, "Rename", renamed) {
                                    typed.clone()
                                } else {
                                    picked.clone()
                                };
                                if let Some(to) = to.filter(|to| to != name) {
                                    action = Some(FaceAction::Rename {
                                        media,
                                        from: name.clone(),
                                        to,
                                        rect: *rect,
                                    });
                                }
                                if decision(ui, "Remove name", tips::REMOVE_NAME) {
                                    action = Some(FaceAction::Remove {
                                        media,
                                        name: name.clone(),
                                        rect: *rect,
                                    });
                                }
                            }
                            FaceTarget::Unknown(face) => {
                                let save = naming(ui, "Save", typed.is_some());
                                if let Some(name) = if save { typed.clone() } else { picked.clone() } {
                                    action = Some(FaceAction::Name {
                                        name,
                                        places: vec![(media, face.rect)],
                                    });
                                }
                                if decision(ui, "Ignore", tips::IGNORE) {
                                    action = Some(FaceAction::Ignore { faces: vec![face.id] });
                                }
                                if decision(ui, "Not a face", tips::NOT_A_FACE) {
                                    action = Some(FaceAction::NotAFace { faces: vec![face.id] });
                                }
                                if decision(ui, "Remove box", tips::REMOVE_BOX) {
                                    action = Some(FaceAction::Delete { face: face.id });
                                }
                            }
                            FaceTarget::Drawn(rect) => {
                                let save = naming(ui, "Save", typed.is_some());
                                if let Some(name) = if save { typed.clone() } else { picked.clone() } {
                                    action = Some(FaceAction::Name {
                                        name,
                                        places: vec![(media, *rect)],
                                    });
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                });
            });
        if action.is_none() && !close {
            self.face_popup = Some(popup);
        }
        action
    }

    /// Shows the face regions this time, without changing the remembered choice (REQ-032-26).
    pub fn show_faces(&mut self) {
        self.faces = true;
    }

    fn toggle_faces(&mut self, gallery: &Gallery) {
        self.faces = !self.faces;
        gallery.store_setting(faces::FACES_KEY, if self.faces { "shown" } else { "hidden" });
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
    ) -> Option<Action> {
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

    /// A press on the video or its controls gives them focus, a press anywhere else takes it (REQ-009-02).
    fn track_video_focus(&mut self, ctx: &egui::Context) {
        let Some(pos) = ctx.input(|input| {
            input
                .pointer
                .any_pressed()
                .then(|| input.pointer.press_origin())
                .flatten()
        }) else {
            return;
        };
        let photo = self.photo;
        self.video_focus = self.playing_shown().is_some_and(|playing| {
            playing.bar.is_some_and(|bar| bar.contains(pos)) || photo.is_some_and(|video| video.contains(pos))
        });
    }

    /// Plays the shown item if it is a video, and stops a video stepped away from, closed or gone (REQ-009-06). Then
    /// takes this frame's state and frame from the player.
    fn sync_video(
        &mut self,
        ctx: &egui::Context,
        item: Option<&GalleryItem>,
        videos: &mut Videos,
        libraries: &[Library],
    ) {
        let wanted = item.filter(|item| item.kind == MediaKind::Video && self.closing.is_none());
        if self.playing.as_ref().map(|playing| playing.id) != wanted.map(|item| item.id) {
            let was_playing = self.playing.take().is_some_and(|playing| playing.unavailable.is_none());
            self.video_focus = false;
            let mut unavailable = None;
            if let Some(item) = wanted {
                // The original file, from the library (REQ-009-04). Loading replaces a playing video without stopping
                // first, which would close and reopen mpv's audio and video outputs.
                unavailable = match (videos.player(), file_in(libraries, item.root_id, &item.rel_path)) {
                    (Ok(player), Some(path)) => {
                        player.load(&path);
                        None
                    }
                    (Ok(_), None) => Some("Its library is gone".to_owned()),
                    (Err(error), _) => Some(error),
                };
            }
            if was_playing && (wanted.is_none() || unavailable.is_some()) {
                videos.stop();
            }
            if let Some(item) = wanted {
                self.playing = Some(Playing {
                    id: item.id,
                    unavailable,
                    state: PlaybackState::default(),
                    frame: None,
                    dragging: None,
                    bar: None,
                    control: None,
                });
            }
        }
        let Some(playing) = self.playing.as_mut().filter(|playing| playing.unavailable.is_none()) else {
            return;
        };
        let Ok(player) = videos.player() else { return };
        playing.state = player.state();
        // Frames are rendered at the size the video took on screen in the last frame, unrotated.
        let render_size = self.photo.zip(playing.state.video_size).map(|(rect, size)| {
            let on_screen = rect.size() * ctx.pixels_per_point();
            let on_screen = if playing.state.quarters % 2 == 1 {
                [on_screen.y, on_screen.x]
            } else {
                [on_screen.x, on_screen.y]
            };
            domain::video::render_size(on_screen, size)
        });
        playing.frame = player.frame(render_size);
    }

    /// Applies what the video controls, a click on the video or a key asked for.
    fn apply_video_control(&mut self, videos: &mut Videos) {
        let Some(playing) = self.playing.as_mut() else { return };
        let Some(control) = playing.control.take() else { return };
        let Ok(player) = videos.player() else { return };
        let state = &playing.state;
        match control {
            // At the end, play starts over (as in a browser).
            Control::PlayPause if state.ended => {
                player.seek(0.0, true);
                player.set_paused(false);
            }
            Control::PlayPause => player.set_paused(!state.paused),
            Control::Seek(position, exact) => player.seek(position, exact),
            Control::Volume(volume) => {
                player.set_volume(volume);
                if state.muted && volume > 0.0 {
                    player.set_muted(false);
                }
            }
            Control::Muted(muted) => player.set_muted(muted),
        }
        playing.state = player.state();
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

    /// The photo (or a video's preview), fitted into the media area, zoomed and panned (REQ-009-05, REQ-009-14 …
    /// -16). During the tile ↔ viewer transition (`transition`: the tile and how far it is open) it is drawn between
    /// its tile and its place (REQ-013-12). Returns where it was drawn.
    #[expect(clippy::too_many_arguments, reason = "one frame's inputs of the media area")]
    fn media(
        &mut self,
        ui: &mut egui::Ui,
        backdrop: &egui::Response,
        layout: &Layout,
        item: &GalleryItem,
        gallery: &mut Gallery,
        original: &OriginalState,
        keys: Keys,
        transition: Option<(Rect, f32)>,
    ) -> Rect {
        let area = layout.media;
        let thumbnail = gallery.thumbnail(item.id);
        let loaded = match original {
            OriginalState::Loaded(loaded) => Some(loaded.clone()),
            _ => None,
        };
        let playing = self
            .playing_shown()
            .filter(|playing| playing.id == item.id)
            .map(|playing| (playing.state.video_size, playing.state.quarters, playing.frame));
        let video_size = playing.and_then(|(size, quarters, _)| Some(domain::video::shown_size(size?, quarters)));
        let natural = match (item.width, item.height, &loaded, &thumbnail) {
            // A video's own size once the player knows it (turned as it is shown), else its upright thumbnail's.
            _ if video_size.is_some() => {
                let [width, height] = video_size.unwrap_or_default();
                Size::new(width as f32, height as f32)
            }
            (_, _, _, Some(thumbnail)) if item.kind == MediaKind::Video => {
                Size::new(thumbnail.size[0] as f32, thumbnail.size[1] as f32)
            }
            (Some(width), Some(height), _, _) if width > 0 && height > 0 => Size::new(width as f32, height as f32),
            (_, _, Some(loaded), _) => Size::new(loaded.size[0] as f32, loaded.size[1] as f32),
            (_, _, _, Some(thumbnail)) => Size::new(thumbnail.size[0] as f32, thumbnail.size[1] as f32),
            _ => Size::new(area.width(), area.height()),
        };
        let editing = self.edit.as_ref().is_some_and(|session| session.media() == item.id);
        // While editing, the photo through the recipe; held `\\` or _Compare_ shows the original (REQ-021-02, REQ-021-07).
        let compare = self.comparing || ui.input(|input| input.key_down(Key::Backslash));
        // The turned, cropped photo fits as a whole, wherever it was zoomed to (REQ-029-01).
        let edited_size = self
            .edit
            .as_ref()
            .filter(|_| editing)
            .and_then(|session| session.shown_size(compare));
        let natural = edited_size.map_or(natural, |[width, height]| Size::new(width, height));
        if self.edited_size != edited_size {
            if self.edited_size.is_some() && edited_size.is_some() {
                self.view = FIT;
            }
            self.edited_size = edited_size;
        }
        let fitted = zoom::fit_size(natural, Size::new(area.width(), area.height()));
        // Zoomed, the photo spreads beyond the media area over the whole stage (REQ-009-20).
        let stage = layout.stage;
        let viewport = Size::new(stage.width(), stage.height());
        let photo = item.kind == MediaKind::Image;
        let picking = editing && self.edit.as_ref().is_some_and(EditSession::is_picking);
        let cropping = editing && !compare && self.edit.as_ref().is_some_and(EditSession::is_cropping);
        // The handles and painting need the whole photo (REQ-030-03, REQ-030-12).
        let gradient = editing && !compare && !cropping && self.edit.as_ref().is_some_and(EditSession::has_photo_tool);
        // The frame goes over the whole photo.
        if cropping || gradient {
            self.view = FIT;
        }

        // Not over the chrome: its own widgets take those clicks and wheel turns.
        let over_chrome = backdrop.hover_pos().is_some_and(|pos| layout.is_chrome(pos));
        if picking && !over_chrome {
            // The white balance picker takes the click on the photo (REQ-021-05).
            let target = photo_rect(area, fitted, self.view);
            if backdrop.clicked()
                && let Some(pos) = backdrop.interact_pointer_pos()
                && target.contains(pos)
                && let Some(session) = &mut self.edit
            {
                session.pick(pos2(
                    (pos.x - target.left()) / target.width(),
                    (pos.y - target.top()) / target.height(),
                ));
            }
        } else if cropping {
            let target = photo_rect(area, fitted, FIT);
            if let Some(session) = &mut self.edit {
                session.set_crop_area(target);
                // A drag goes on over the chrome.
                if !over_chrome || session.is_dragging_crop() {
                    session.crop_input(ui, backdrop, target);
                }
            }
            self.close_at = None;
        } else if gradient {
            let target = photo_rect(area, fitted, FIT);
            if let Some(session) = &mut self.edit {
                session.set_gradient_area(target);
                // A drag goes on over the chrome.
                if !over_chrome || session.is_using_photo_tool() {
                    session.photo_tool_input(ui, backdrop, target);
                }
            }
            self.close_at = None;
        } else if photo && !over_chrome {
            self.handle_zoom(ui, backdrop, area, stage, natural, fitted, viewport, keys);
            // A click beside the photo doesn't close while editing.
            if editing {
                self.close_at = None;
            }
        } else if playing.is_some() && !over_chrome {
            // A click on the video plays or pauses it, as in a browser; beside it, it closes (REQ-009-03).
            let video = photo_rect(area, fitted, FIT);
            if !self.backdrop_clicks_beside(backdrop, video) {
                self.control_video(Control::PlayPause);
            }
        } else if !over_chrome {
            self.backdrop_clicks(backdrop);
        }
        let view = self.view;
        let target = photo_rect(area, fitted, view);
        // During the transition the photo moves between its tile and its place, over the whole window.
        // Cropped like the tile at first, uncropped once open.
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let (rect, clip, uv) = match transition {
            Some((tile, progress)) => {
                let cover = gallery::cover_uv([natural.width as u32, natural.height as u32], tile.size());
                (
                    lerp_rect(tile, target, progress),
                    ui.clip_rect(),
                    lerp_rect(cover, full, progress),
                )
            }
            None => (target, stage, full),
        };
        if photo
            && !over_chrome
            && !cropping
            && let Some(pos) = backdrop.hover_pos()
        {
            let icon = if !target.intersect(stage).contains(pos) {
                CursorIcon::Default
            } else if picking {
                CursorIcon::Crosshair
            } else if !view.is_zoomed() {
                CursorIcon::ZoomIn
            } else if backdrop.dragged() {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            };
            ui.ctx().set_cursor_icon(icon);
        }

        let painter = ui.painter().with_clip_rect(clip);
        let fade = match loaded {
            Some(_) if self.instant == Some(true) => 1.0,
            Some(_) => {
                let now = ui.input(|input| input.time);
                let from = *self.fade_from.get_or_insert(now);
                ((now - from) as f32 / theme::DURATION_BASE).min(1.0)
            }
            None => 0.0,
        };
        // A video's frame replaces its preview once the first one has come (not while it grows or shrinks).
        let frame = playing
            .and_then(|(_, quarters, frame)| Some((frame?, quarters)))
            .filter(|_| transition.is_none());
        // The thumbnail stays under the original until it has faded in; the placeholder only without one.
        if fade < 1.0 && frame.is_none() {
            match thumbnail {
                Some(thumbnail) => {
                    painter.image(thumbnail.id, rect, uv, Color32::WHITE);
                    crate::glass::record_image(ui.ctx(), rect, uv, clip, &thumbnail.view, Color32::WHITE);
                }
                None => {
                    if let Some(placeholder) = gallery.placeholder(ui.ctx(), item.id) {
                        painter.image(placeholder, rect, uv, Color32::WHITE);
                    }
                }
            }
        }
        let edited = editing
            && transition.is_none()
            && self
                .edit
                .as_mut()
                .is_some_and(|session| session.paint(ui, rect, uv, clip, compare, 1.0));
        if edited {
            if cropping && let Some(session) = &self.edit {
                session.paint_crop(&painter, target);
            }
            if gradient && let Some(session) = &self.edit {
                session.paint_photo_tool(&painter, target);
            }
        } else if let Some(loaded) = &loaded {
            let tint = Color32::WHITE.gamma_multiply(fade);
            painter.image(loaded.id, rect, uv, tint);
            if let Some(view) = &loaded.view {
                crate::glass::record_image(ui.ctx(), rect, uv, clip, view, tint);
            }
            if fade < 1.0 {
                ui.ctx().request_repaint();
            }
        }
        if let Some((frame, quarters)) = frame {
            painter.add(rotated_image(frame.id, rect, quarters));
        }
        let at = pos2(target.center().x, target.bottom().min(area.bottom()) - 28.0);
        if editing && self.edit.as_ref().is_some_and(|session| session.failed().is_some()) {
            message(ui, at, "Can't load the original to edit.");
        } else if let OriginalState::Failed(_) = original {
            message(ui, at, "Can't load the original. Showing a preview.");
        }
        if item.kind == MediaKind::Video && self.video_unavailable().is_some() {
            message(ui, at, "Can't play videos here. Showing a preview.");
        } else if playing.is_some() && self.video().is_some_and(|state| state.error.is_some()) {
            message(ui, at, "Can't play this video. Showing a preview.");
        }
        rect
    }

    #[expect(clippy::too_many_arguments, reason = "one frame's inputs of the media area")]
    fn handle_zoom(
        &mut self,
        ui: &egui::Ui,
        backdrop: &egui::Response,
        area: Rect,
        stage: Rect,
        natural: Size,
        fitted: Size,
        viewport: Size,
        keys: Keys,
    ) {
        let content = fitted;
        let relative = |pos: Pos2| (pos.x - area.center().x, pos.y - area.center().y);
        // Only where it is drawn: around a zoomed photo's clipped edges the scrim shows, and a click there closes.
        let on_photo = |view: ZoomView, pos: Pos2| photo_rect(area, fitted, view).intersect(stage).contains(pos);
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
            let factor = zoom_factor(ui);
            if factor != 1.0
                && let Some(pos) = backdrop.hover_pos()
            {
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

    /// _Previous_ / _Next_, the control bar, the info panel and the filmstrip; the controls and the filmstrip fade out
    /// while idle (REQ-013-10, REQ-015-06). Returns what a control asked for.
    fn controls(
        &mut self,
        ui: &mut egui::Ui,
        layout: &Layout,
        item: Option<&GalleryItem>,
        count: usize,
        gallery: &mut Gallery,
        libraries: &[Library],
    ) -> Option<Action> {
        let ctx = ui.ctx().clone();
        let screen = layout.screen;
        let looks_for_faces = gallery.looks_for_faces();
        let has_faces = self.indexed
            && item.is_some_and(|item| {
                item.kind == MediaKind::Image
                    && (looks_for_faces
                        || gallery
                            .cached_detail(item.id)
                            .is_some_and(|detail| !detail.faces.is_empty()))
            });
        let mut text = bar_text(ui, screen, item, libraries, has_faces);
        text.add_face = has_faces && self.faces && !crate::WEB;
        text.editable &= self.indexed;
        text.filmstrip = self.indexed;
        let bar = control_bar_rect(layout, bar_width(&text));
        let previous = self.step_target(-1, count).map(|_| {
            Rect::from_center_size(
                pos2(screen.left() + INSET + NAV_SIZE / 2.0, layout.media.center().y),
                Vec2::splat(NAV_SIZE),
            )
        });
        let next = self.step_target(1, count).map(|_| {
            Rect::from_center_size(
                pos2(layout.right - INSET - NAV_SIZE / 2.0, layout.media.center().y),
                Vec2::splat(NAV_SIZE),
            )
        });
        // The video's controls along its bottom, while it plays here (not for a file that fails).
        let video_bar = self
            .playing_shown()
            .is_some_and(|playing| playing.state.error.is_none())
            .then_some(self.photo)
            .flatten()
            .map(|video| video_controls::bar_rect(video, layout.media));
        let paused = self
            .playing_shown()
            .is_some_and(|playing| playing.state.paused || playing.state.ended);
        let pointer = ctx.pointer_latest_pos();
        let hovering = pointer.is_some_and(|pos| {
            bar.contains(pos)
                || previous.is_some_and(|r| r.contains(pos))
                || next.is_some_and(|r| r.contains(pos))
                || layout.strip.is_some_and(|r| r.contains(pos))
                || video_bar.is_some_and(|r| r.contains(pos))
        });
        let idle = self.last_activity.elapsed() >= IDLE;
        // A paused video keeps them (REQ-013-10).
        let hidden = zoom::controls_hidden(idle, hovering, self.info_open || self.edit.is_some(), paused);
        if !idle {
            ctx.request_repaint_after(IDLE.saturating_sub(self.last_activity.elapsed()));
        }
        let shown = ctx.animate_bool_with_time(ui.make_persistent_id("controls"), !hidden, theme::DURATION_BASE);
        if hidden && pointer.is_some_and(|pos| screen.contains(pos)) {
            ctx.set_cursor_icon(CursorIcon::None);
        }

        let mut action = None;
        if self.info_open
            && self.edit.is_none()
            && let Some(panel) = layout.info
        {
            action = self.info_panel(ui, panel, gallery);
        }
        if let Some(playing) = &mut self.playing {
            playing.bar = video_bar.filter(|_| shown > 0.0);
        }
        if shown == 0.0 {
            return action;
        }
        let mut step = None;
        ui.scope(|ui| {
            ui.multiply_opacity(shown);
            let bar_backing = self.chrome.backing(theme::glass_bar(), self.chrome_blur);
            if let Some(rect) = video_bar
                && let Some(playing) = &mut self.playing
                && let Some(control) =
                    video_controls::show(ui, rect, &playing.state, &mut playing.dragging, bar_backing)
            {
                playing.control = Some(control);
            }
            for (rect, icon, label, by) in [
                (previous, Icon::ChevronLeft, "Previous", -1),
                (next, Icon::ChevronRight, "Next", 1),
            ] {
                let Some(rect) = rect else { continue };
                let tooltip = if by < 0 { "Previous (←)" } else { "Next (→)" };
                if round_button(ui, rect, icon, label, tooltip, bar_backing).clicked() {
                    step = self.step_target(by, count);
                }
            }
            if let Some(strip) = layout.strip
                && let Some(index) = self.filmstrip(ui, strip, count, gallery)
            {
                step = Some(index);
            }
            let editing = self.edit.is_some();
            let toggles = Toggles {
                info: self.info_open,
                filmstrip: self.filmstrip,
                faces: self.faces,
                editing,
            };
            match control_bar(ui, bar, text, bar_backing, toggles) {
                Some(BarButton::Edit) => self.toggle_edit = true,
                Some(BarButton::ShowInExplorer) => {
                    if let Some(item) = item {
                        action = Self::explorer_action(item, libraries);
                    }
                }
                Some(BarButton::Info) => self.info_open = !self.info_open,
                Some(BarButton::Filmstrip) => self.toggle_filmstrip(gallery),
                Some(BarButton::Faces) => self.toggle_faces(gallery),
                Some(BarButton::AddFace) => {
                    self.face_popup = None;
                    self.drawing_face = Some(None);
                }
                Some(BarButton::Close) => action = Some(Action::Close),
                None => {}
            }
        });
        if !self.filmstrip {
            gallery.set_strip(0..0);
        }
        if let Some(index) = step
            && index < count
            && (index != self.index || self.left_result)
        {
            if self.edit.is_some() {
                self.leave_edit(Leave::Step(index), gallery);
            } else {
                self.go_to(index, gallery);
            }
            ctx.request_repaint();
        }
        action
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

    /// Starts or stops editing the shown photo as asked (`e`, the bar's button, the context menu); loads the
    /// original once the item's details say which it is (REQ-021-01, REQ-021-11).
    fn sync_edit(
        &mut self,
        ctx: &egui::Context,
        item: &GalleryItem,
        libraries: &[Library],
        originals: &mut Originals,
        gallery: &mut Gallery,
        key: bool,
    ) {
        let toggle = key | std::mem::take(&mut self.toggle_edit);
        if self.edit.as_ref().is_some_and(|session| session.media() != item.id) {
            self.edit = None;
        }
        if toggle && self.closing.is_none() {
            if self.edit.is_some() {
                self.leave_edit(Leave::Exit, gallery);
            } else if editable(item, libraries)
                && let Some(library) = libraries.iter().find(|library| library.id == item.root_id)
            {
                let side = |side: Option<i64>| side.and_then(|side| u32::try_from(side).ok()).unwrap_or(0);
                let target = edit::Target {
                    media: item.id,
                    root_id: item.root_id,
                    rel_path: item.rel_path.clone(),
                    file_name: item.file_name.clone(),
                    library: PathBuf::from(&library.path),
                    full_size: [side(item.width), side(item.height)],
                    sidecar_orientation: item.sidecar_orientation,
                };
                self.edit = Some(EditSession::new(target, originals.render_state()));
            }
        }
        if let Some(session) = &mut self.edit
            && session.is_waiting()
        {
            match gallery.detail(item.id) {
                Some(Ok(Some(detail))) => session.start(detail.edit.clone(), ctx),
                Some(_) => session.start(None, ctx),
                None => {}
            }
        }
        // The original the viewer already shows, instead of reading the file again (REQ-021-19).
        if let Some(session) = &mut self.edit {
            session.use_shown(&originals.get(item.id), || originals.lend(item.id));
        }
    }

    /// The keys while editing (REQ-021-07, REQ-021-08): undo and redo; `Esc` stops editing, stepping asks first.
    /// Returns the keys left for the viewer.
    fn edit_keys(&mut self, keys: Keys, count: usize, gallery: &Gallery) -> Keys {
        let Some(session) = &mut self.edit else { return keys };
        if session.dialog.is_some() {
            if keys.close {
                session.dialog = None;
            }
            return Keys::default();
        }
        // Escape leaves the crop first.
        if keys.close && session.is_cropping() {
            session.set_cropping(false);
            return Keys::default();
        }
        if keys.undo {
            session.undo();
        }
        if keys.redo {
            session.redo();
        }
        if keys.close {
            self.leave_edit(Leave::Exit, gallery);
        }
        if keys.step != 0
            && let Some(target) = zoom::step_target(self.index, count, keys.step, self.left_result)
        {
            self.leave_edit(Leave::Step(target), gallery);
        }
        Keys {
            zoom: keys.zoom,
            fit: keys.fit,
            filmstrip: keys.filmstrip,
            edit: keys.edit,
            ..Keys::default()
        }
    }

    /// Leaves editing as asked, unless there are unsaved changes: then a dialog asks first (REQ-021-08).
    fn leave_edit(&mut self, leave: Leave, gallery: &Gallery) {
        if let Some(session) = &mut self.edit
            && let Some(Outcome::Leave(leave)) = session.leave(leave)
        {
            self.apply_leave(leave, gallery);
        }
    }

    fn apply_leave(&mut self, leave: Leave, gallery: &Gallery) {
        self.edit = None;
        self.comparing = false;
        match leave {
            Leave::Close => self.close(gallery, Action::Close),
            Leave::Step(index) => self.go_to(index, gallery),
            Leave::Exit => {}
        }
    }

    fn apply_outcome(&mut self, outcome: Option<Outcome>, gallery: &Gallery) -> Option<Action> {
        match outcome? {
            Outcome::Save(request, then) => {
                self.after_save = then;
                Some(Action::SaveEdit(request))
            }
            Outcome::Leave(leave) => {
                self.apply_leave(leave, gallery);
                None
            }
        }
    }

    /// The adjustments panel in the info panel's place (REQ-021-01).
    fn edit_panel(&mut self, ui: &mut egui::Ui, layout: &Layout, gallery: &Gallery) -> Option<Action> {
        let rect = layout.info?;
        let backing = self.chrome.backing(theme::glass_panel(), self.chrome_blur);
        let session = self.edit.as_mut()?;
        backing.paint(ui.painter(), rect, f32::from(theme::RADIUS_PANEL));
        // Clicks and wheel turns on the panel stay in it.
        ui.interact(rect, ui.make_persistent_id("edit panel"), egui::Sense::click());
        let (outcome, compare) = session.panel(ui, rect);
        self.comparing = compare;
        self.apply_outcome(outcome, gallery)
    }

    /// The dialogs of editing: unsaved changes (REQ-021-08), replacing the original (REQ-021-10).
    fn edit_dialog(&mut self, ctx: &egui::Context, gallery: &Gallery) -> Option<Action> {
        let session = self.edit.as_mut()?;
        let dialog = session.dialog?;
        let name = session.target.file_name.clone();
        let mut outcome = None;
        match dialog {
            Dialog::Leave(leave) => {
                let text = format!("Your changes to {name} aren't saved yet.");
                match crate::widgets::dialog(
                    ctx,
                    "edit-leave",
                    "Save your changes?",
                    &text,
                    &["Save", "Discard", "Cancel"],
                ) {
                    Some(0) => outcome = session.save(SaveMode::Replace, Some(leave)),
                    Some(1) => {
                        session.dialog = None;
                        outcome = Some(Outcome::Leave(leave));
                    }
                    Some(_) => session.dialog = None,
                    None => {}
                }
            }
            Dialog::Replace(then) => {
                let text = format!(
                    "The edited photo replaces {name}. The original is kept in the library's .good-gallery folder, and \
                     Revert to original brings it back."
                );
                match crate::widgets::dialog(
                    ctx,
                    "edit-replace",
                    "Replace the original?",
                    &text,
                    &["Replace", "Cancel"],
                ) {
                    Some(0) => outcome = session.save(SaveMode::Replace, then),
                    Some(_) => session.dialog = None,
                    None => {}
                }
            }
            Dialog::WithoutBackup(then) => {
                let text = format!(
                    "The edited photo replaces {name}, and its original is deleted. The edit can't be changed or \
                     undone afterwards."
                );
                match crate::widgets::dialog(
                    ctx,
                    "edit-without-backup",
                    "Save without a backup?",
                    &text,
                    &["Save without backup", "Cancel"],
                ) {
                    Some(0) => outcome = session.save(SaveMode::WithoutBackup, then),
                    Some(_) => session.dialog = None,
                    None => {}
                }
            }
        }
        self.apply_outcome(outcome, gallery)
    }

    /// The filmstrip (spec 015): thumbnails of the whole gallery in one row, the shown item highlighted and centered;
    /// the wheel scrolls it. Returns a clicked position.
    fn filmstrip(&mut self, ui: &mut egui::Ui, rect: Rect, count: usize, gallery: &mut Gallery) -> Option<usize> {
        let view = rect.width();
        if (self.strip_centered != Some(self.index) || self.strip_width != view) && !self.missing {
            self.strip_offset = STRIP.centered(self.index, count, view);
            self.strip_centered = Some(self.index);
        }
        self.strip_width = view;
        self.strip_offset = STRIP.clamp(self.strip_offset, count, view);
        let response = ui.interact(rect, ui.make_persistent_id("filmstrip"), egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Navigation, true, "Filmstrip"));
        // Over the thumbnails too, which lie on top of the strip.
        if ui.rect_contains_pointer(rect) {
            let delta = wheel(ui);
            // egui's deltas point the other way than a browser's.
            let sideways = -zoom::sideways_delta(delta.x, delta.y);
            if sideways != 0.0 {
                self.strip_offset = STRIP.clamp(self.strip_offset + sideways, count, view);
            }
        }
        self.chrome.backing(theme::glass_bar(), self.chrome_blur).paint(
            ui.painter(),
            rect,
            f32::from(theme::RADIUS_PANEL),
        );
        // A short row stays centered.
        let left = rect.left() + ((view - STRIP.width(count)) / 2.0).max(0.0) - self.strip_offset;
        let visible = STRIP.visible(count, self.strip_offset, view);
        gallery.set_strip(visible.clone());
        let painter = ui.painter().with_clip_rect(rect.shrink(1.0));
        let mut clicked = None;
        for index in visible {
            let thumb = Rect::from_min_size(
                pos2(left + STRIP.left(index), rect.top() + STRIP.padding),
                Vec2::splat(STRIP.size),
            );
            let Some(item) = gallery.item(index) else {
                painter.rect_filled(thumb, 2.0, theme::raised());
                continue;
            };
            let (id, video, name) = (item.id, item.kind == MediaKind::Video, item.file_name.clone());
            let current = index == self.index && !self.missing && !self.left_result;
            let response = ui.interact(
                thumb.intersect(rect),
                ui.make_persistent_id(("filmstrip item", id)),
                egui::Sense::click(),
            );
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, current, &name));
            let response = response.on_hover_text(&name);
            if response.clicked() {
                clicked = Some(index);
            }
            let tint = if current || response.hovered() {
                Color32::WHITE
            } else {
                Color32::WHITE.gamma_multiply(0.6)
            };
            match (gallery.thumbnail(id), gallery.placeholder(ui.ctx(), id)) {
                (Some(texture), _) => {
                    painter.add(
                        egui::epaint::RectShape::filled(thumb, 2, tint)
                            .with_texture(texture.id, gallery::cover_uv(texture.size, thumb.size())),
                    );
                }
                (None, Some(placeholder)) => {
                    painter.add(
                        egui::epaint::RectShape::filled(thumb, 2, tint)
                            .with_texture(placeholder, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))),
                    );
                }
                (None, None) => {
                    painter.rect_filled(thumb, 2.0, theme::raised());
                }
            }
            // The tile's duration badge doesn't fit; a play symbol still marks videos (REQ-015-07).
            if video {
                let badge = Rect::from_center_size(thumb.center(), Vec2::splat(24.0));
                painter.circle_filled(badge.center(), 12.0, Color32::from_black_alpha(153));
                let icon = Rect::from_center_size(badge.center() + vec2(1.0, 0.0), Vec2::splat(12.0));
                // Images paint unclipped: only while the symbol is inside the strip.
                if rect.contains_rect(icon) {
                    Icon::Play.image(12.0, Color32::WHITE).paint_at(ui, icon);
                }
                let marker = ui.interact(icon, ui.make_persistent_id(("video marker", id)), egui::Sense::hover());
                marker.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Video"));
            }
            if current {
                painter.rect_stroke(
                    thumb,
                    2,
                    egui::Stroke::new(2.0, theme::text()),
                    egui::StrokeKind::Inside,
                );
            }
        }
        clicked
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

/// A wheel event's movement in points (lines count 100).
fn wheel_points(unit: egui::MouseWheelUnit, delta: Vec2) -> Vec2 {
    match unit {
        egui::MouseWheelUnit::Point => delta,
        egui::MouseWheelUnit::Line => delta * WHEEL_LINE,
        egui::MouseWheelUnit::Page => vec2(delta.x.signum(), delta.y.signum()) * WHEEL_LINE,
    }
}

/// This frame's wheel movement in points, without Ctrl.
fn wheel(ui: &egui::Ui) -> Vec2 {
    ui.input(|input| {
        input
            .raw
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel {
                    unit, delta, modifiers, ..
                } if !modifiers.ctrl => Some(wheel_points(*unit, *delta)),
                _ => None,
            })
            .fold(Vec2::ZERO, |sum, delta| sum + delta)
    })
}

/// How much this frame's wheel, touchpad scrolling and pinching zoom a photo (REQ-009-05). Windows reports a pinch as
/// Ctrl + wheel, other systems as a zoom event. egui's delta is positive when scrolling up, a browser's deltaY then
/// negative: up zooms in.
fn zoom_factor(ui: &egui::Ui) -> f32 {
    ui.input(|input| {
        input.raw.events.iter().fold(1.0, |factor, event| match event {
            egui::Event::MouseWheel { unit, delta, .. } => {
                factor * (wheel_points(*unit, *delta).y * WHEEL_SENSITIVITY).exp()
            }
            egui::Event::Zoom(zoom) => factor * zoom,
            _ => factor,
        })
    })
}

/// Where the photo is drawn: fitted into `area`, scaled and moved by `view`; parts outside `area` are clipped.
fn photo_rect(area: Rect, fitted: Size, view: ZoomView) -> Rect {
    Rect::from_center_size(
        area.center() + vec2(view.x, view.y),
        vec2(fitted.width, fitted.height) * view.scale,
    )
}

/// `texture` drawn into `rect` turned clockwise by `quarters` quarter turns: mpv's software renderer leaves frames
/// unrotated, the GPU turns them for free.
fn rotated_image(texture: egui::TextureId, rect: Rect, quarters: u8) -> egui::Shape {
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    let uvs = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
    let mut mesh = egui::Mesh::with_texture(texture);
    for (i, pos) in corners.into_iter().enumerate() {
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: uvs[(i + 4 - usize::from(quarters % 4)) % 4],
            color: Color32::WHITE,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    egui::Shape::mesh(mesh)
}

fn lerp_rect(from: Rect, to: Rect, t: f32) -> Rect {
    Rect::from_min_max(from.min.lerp(to.min, t), from.max.lerp(to.max, t))
}

/// Asks for the originals of the shown photo and its neighbours, nearest first (REQ-009-13); first of all `outside`, the
/// shown item if it left the result (REQ-009-18).
fn preload(
    gallery: &mut Gallery,
    originals: &mut Originals,
    libraries: &[Library],
    outside: Option<&GalleryItem>,
    index: usize,
    count: usize,
) {
    let request = |item: &GalleryItem| {
        if item.kind != MediaKind::Image {
            return None;
        }
        Some(OriginalRequest {
            media: item.id,
            path: file_in(libraries, item.root_id, &item.rel_path)?,
            sidecar_orientation: item.sidecar_orientation,
        })
    };
    let mut wanted: Vec<_> = outside.and_then(request).into_iter().collect();
    for at in std::iter::once(index).chain(zoom::neighbours(count, index)) {
        wanted.extend(gallery.item(at).and_then(request));
    }
    originals.set(wanted);
}

/// Text in a glass pill centered at `at`.
fn message(ui: &mut egui::Ui, at: Pos2, text: &str) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.into(), egui::FontId::proportional(theme::SIZE_BODY), theme::text());
    let rect = Rect::from_center_size(at, galley.size() + vec2(32.0, 12.0));
    ui.painter()
        .rect_filled(rect, rect.height() / 2.0, Chrome::new(None).bar);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, theme::text());
    // Hover only: clicks still reach the photo and the scrim below.
    let response = ui.interact(rect, ui.make_persistent_id(("message", text)), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
}

/// An icon button that fills `rect` exactly. With the theme's padding it paints as wide as its icon and padding want,
/// past `rect` and over its neighbours, though its response says `rect` (REQ-013-18).
pub(crate) fn put_button(ui: &mut egui::Ui, rect: Rect, button: egui::Button<'_>) -> egui::Response {
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = Vec2::ZERO;
        ui.put(rect, button.min_size(rect.size()))
    })
    .inner
}

fn round_button(
    ui: &mut egui::Ui,
    rect: Rect,
    icon: Icon,
    label: &str,
    tooltip: &str,
    backing: crate::glass::Backing,
) -> egui::Response {
    backing.paint(ui.painter(), rect, NAV_SIZE / 2.0);
    let response = put_button(
        ui,
        rect,
        egui::Button::image(icon.image(20.0, theme::text())).corner_radius(NAV_SIZE / 2.0),
    )
    .on_hover_text(tooltip);
    let label = label.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    response
}

/// What the control bar shows: file name, date and rating of the item, and whether it can be edited.
struct BarText {
    name: std::sync::Arc<egui::Galley>,
    date: Option<std::sync::Arc<egui::Galley>>,
    stars: usize,
    editable: bool,
    /// The photo may have faces (REQ-009-17, REQ-032-09).
    faces: bool,
    /// Faces are on, so one can be drawn (REQ-032-12).
    add_face: bool,
    /// The filmstrip can be shown: not for a file outside every library (REQ-033-03).
    filmstrip: bool,
}

/// Whether the item is a photo that can be edited here: JPEG, PNG or WebP in an online library, not in the browser
/// (REQ-021-01).
fn editable(item: &GalleryItem, libraries: &[Library]) -> bool {
    !crate::WEB
        && item.kind == MediaKind::Image
        && domain::edit::is_editable(&item.file_name)
        && libraries
            .iter()
            .any(|library| library.id == item.root_id && library.status != db::libraries::LibraryStatus::Offline)
}

fn bar_text(ui: &egui::Ui, screen: Rect, item: Option<&GalleryItem>, libraries: &[Library], faces: bool) -> BarText {
    let name = item.map_or("", |item| item.file_name.as_str());
    let mut job = egui::text::LayoutJob::simple_singleline(
        name.to_owned(),
        egui::FontId::new(theme::SIZE_TITLE, theme::semibold()),
        theme::text(),
    );
    job.wrap = egui::text::TextWrapping {
        max_width: (screen.width() * 0.3).max(80.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let date = item.map(|item| {
        ui.painter().layout_no_wrap(
            format::medium_date_time(item.taken_at.unwrap_or(item.mtime)),
            egui::FontId::new(theme::SIZE_BODY, theme::numbers()),
            theme::text(),
        )
    });
    BarText {
        name: ui.painter().layout_job(job),
        date,
        stars: item.and_then(|item| item.rating).unwrap_or(0).clamp(0, 5) as usize,
        editable: item.is_some_and(|item| editable(item, libraries)),
        faces,
        add_face: false,
        filmstrip: true,
    }
}

const BAR_GAP: f32 = 12.0;
/// Between the control bar's buttons, so that two pressed ones stay two circles (REQ-013-18).
const BAR_BUTTON_GAP: f32 = theme::SPACE;

/// How wide `count` buttons of the control bar are.
fn bar_buttons_width(count: usize) -> f32 {
    count as f32 * (theme::CONTROL_HEIGHT + BAR_BUTTON_GAP) - BAR_BUTTON_GAP
}
const STAR_SIZE: f32 = 14.0;

/// The control bar's buttons, right of the separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarButton {
    Edit,
    ShowInExplorer,
    Info,
    Filmstrip,
    Faces,
    AddFace,
    Close,
}

const BAR_BUTTONS: [(BarButton, Icon, &str, &str); 7] = [
    (BarButton::Edit, Icon::SlidersHorizontal, "Edit", "Edit (e)"),
    (
        BarButton::ShowInExplorer,
        Icon::FolderOpen,
        crate::state::SHOW_IN_FOLDER,
        crate::state::SHOW_IN_FOLDER,
    ),
    (BarButton::Info, Icon::Info, "Info", "Info (i)"),
    (
        BarButton::Filmstrip,
        Icon::GalleryHorizontal,
        "Filmstrip",
        "Filmstrip (f)",
    ),
    (BarButton::Faces, Icon::ScanFace, "Faces", "Faces (p)"),
    (BarButton::AddFace, Icon::Plus, "Add face", "Draw a face on the photo"),
    (BarButton::Close, Icon::X, "Close", "Close (Esc)"),
];

/// The bar's buttons on this platform and for this item: the browser can't open a file manager (REQ-019-13), only
/// photos that can be edited have _Edit_ (REQ-021-01), only photos with face regions _Faces_ (REQ-009-17).
fn bar_buttons(
    editable: bool,
    faces: bool,
    add_face: bool,
    filmstrip: bool,
) -> Vec<(BarButton, Icon, &'static str, &'static str)> {
    BAR_BUTTONS
        .into_iter()
        .filter(|(button, ..)| match button {
            BarButton::Edit => editable,
            BarButton::Filmstrip => filmstrip,
            BarButton::Faces => faces,
            BarButton::AddFace => add_face,
            BarButton::ShowInExplorer => !crate::WEB,
            _ => true,
        })
        .collect()
}

/// The control bar's width for `text` and its buttons.
fn bar_width(text: &BarText) -> f32 {
    let mut width = 20.0 + text.name.size().x + BAR_GAP;
    if let Some(date) = &text.date {
        width += date.size().x + BAR_GAP;
    }
    if text.stars > 0 {
        width += text.stars as f32 * (STAR_SIZE + 2.0) + BAR_GAP;
    }
    // Separator, the buttons and the right padding.
    width += 1.0
        + BAR_GAP
        + bar_buttons_width(bar_buttons(text.editable, text.faces, text.add_face, text.filmstrip).len())
        + 6.0;
    width
}

/// Where the control bar of `width` goes: above the media, in the title bar's strip, over the media's middle where the
/// window controls leave room for it (REQ-009-19).
fn control_bar_rect(layout: &Layout, width: f32) -> Rect {
    let left = layout.screen.left() + layout.reserved[0] + INSET;
    let right = layout.right - layout.reserved[1] - INSET;
    let width = width.min(right - left).max(0.0);
    let middle = (layout.screen.left() + layout.right) / 2.0;
    let x = middle.clamp(left + width / 2.0, (right - width / 2.0).max(left + width / 2.0));
    Rect::from_center_size(
        pos2(x, layout.screen.top() + INSET + BAR_HEIGHT / 2.0),
        vec2(width, BAR_HEIGHT),
    )
}

/// Room for a name label under its face region.
const FACE_LABEL_HEIGHT: f32 = 20.0;

/// A box around each face region of the photo drawn at `photo`, with the person's name below it (above it at the
/// bottom of `area`), clipped to the media area (REQ-009-17). On photos, not glass: like badges on tiles, they keep a
/// fixed light-on-dark look in both appearances.
fn draw_faces(ui: &egui::Ui, area: Rect, photo: Rect, faces: &[faces::FaceRegion]) -> Vec<(String, Rect)> {
    let painter = ui.painter().with_clip_rect(area);
    let shadow = egui::Stroke::new(3.0, Color32::from_black_alpha(110));
    let line = egui::Stroke::new(1.5, Color32::WHITE);
    let font = egui::FontId::new(theme::SIZE_CAPTION, theme::semibold());
    let mut drawn = Vec::new();
    for face in faces {
        let rect = Rect::from_min_size(
            pos2(
                photo.left() + face.x * photo.width(),
                photo.top() + face.y * photo.height(),
            ),
            vec2(face.w * photo.width(), face.h * photo.height()),
        );
        painter.rect_stroke(rect, 4.0, shadow, egui::StrokeKind::Middle);
        painter.rect_stroke(rect, 4.0, line, egui::StrokeKind::Middle);
        let galley = painter.layout_no_wrap(face.name.clone(), font.clone(), Color32::WHITE);
        let size = vec2(galley.size().x + 12.0, FACE_LABEL_HEIGHT);
        let below = rect.bottom() + 4.0 + size.y <= area.bottom();
        let top = if below {
            rect.bottom() + 4.0
        } else {
            rect.top() - 4.0 - size.y
        };
        let label = Rect::from_min_size(pos2(rect.center().x - size.x / 2.0, top), size);
        painter.rect_filled(label, size.y / 2.0, Color32::from_black_alpha(153));
        painter.galley(label.center() - galley.size() / 2.0, galley, Color32::WHITE);
        drawn.push((face.name.clone(), rect));
    }
    drawn
}

/// A fraction-of-the-photo rectangle where the photo is drawn.
fn on_screen(photo: Rect, [x, y, w, h]: [f32; 4]) -> Rect {
    Rect::from_min_size(
        pos2(photo.left() + x * photo.width(), photo.top() + y * photo.height()),
        vec2(w * photo.width(), h * photo.height()),
    )
}

/// Draws the faces without a name, with their suggestions, and returns the face clicked, named or not (REQ-032-09). A
/// face is its box's own widget, so a click there doesn't reach the photo.
fn face_clicks(
    ui: &egui::Ui,
    photo: Rect,
    named: &[faces::FaceRegion],
    unknown: &[db::faces::ShownFace],
    drawing: bool,
) -> Option<FaceTarget> {
    if drawing {
        return None;
    }
    let mut clicked = None;
    for region in named {
        let rect = [region.x, region.y, region.w, region.h];
        let label = format!("Face of {}", region.name);
        let response = ui.interact(
            on_screen(photo, rect),
            ui.make_persistent_id(("named face", rect.map(f32::to_bits))),
            egui::Sense::click(),
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        if response.clicked() {
            clicked = Some(FaceTarget::Named {
                name: region.name.clone(),
                rect,
            });
        }
    }
    let painter = ui.painter();
    let font = egui::FontId::new(theme::SIZE_CAPTION, theme::semibold());
    for face in unknown {
        let area = on_screen(photo, face.rect);
        let shadow = egui::Stroke::new(3.0, Color32::from_black_alpha(110));
        let line = egui::Stroke::new(1.5, Color32::from_white_alpha(170));
        painter.rect_stroke(area, 4.0, shadow, egui::StrokeKind::Middle);
        painter.extend(egui::Shape::dashed_line(
            &[
                area.left_top(),
                area.right_top(),
                area.right_bottom(),
                area.left_bottom(),
                area.left_top(),
            ],
            line,
            6.0,
            4.0,
        ));
        let text = match &face.suggestion {
            Some(name) => format!("{name}?"),
            None => "Who is this?".to_owned(),
        };
        let galley = painter.layout_no_wrap(text.clone(), font.clone(), Color32::WHITE);
        let size = vec2(galley.size().x + 12.0, FACE_LABEL_HEIGHT);
        let label = Rect::from_min_size(pos2(area.center().x - size.x / 2.0, area.bottom() + 4.0), size);
        painter.rect_filled(label, size.y / 2.0, Color32::from_black_alpha(153));
        painter.galley(label.center() - galley.size() / 2.0, galley, Color32::WHITE);
        let response = ui.interact(
            area,
            ui.make_persistent_id(("unknown face", face.id)),
            egui::Sense::click(),
        );
        let spoken = format!("Unknown face: {text}");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &spoken));
        if response.clicked() {
            clicked = Some(FaceTarget::Unknown(face.clone()));
        }
    }
    clicked
}

/// Drawing a new face: a drag across the photo gives its rectangle, as fractions of the photo (REQ-032-12).
fn draw_new_face(ui: &egui::Ui, photo: Rect, drawing: &mut Option<Option<egui::Pos2>>) -> Option<[f32; 4]> {
    let start = (*drawing)?;
    let response = ui.interact(photo, ui.make_persistent_id("draw a face"), egui::Sense::drag());
    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    if response.drag_started() {
        *drawing = Some(response.interact_pointer_pos());
    }
    let start = (*drawing).flatten().or(start);
    let (Some(start), Some(now)) = (start, ui.ctx().pointer_latest_pos()) else {
        return None;
    };
    let area = Rect::from_two_pos(start, now).intersect(photo);
    ui.painter().rect_stroke(
        area,
        4.0,
        egui::Stroke::new(1.5, Color32::WHITE),
        egui::StrokeKind::Middle,
    );
    if !response.drag_stopped() {
        return None;
    }
    *drawing = None;
    (area.width() > 8.0 && area.height() > 8.0).then(|| {
        [
            (area.left() - photo.left()) / photo.width(),
            (area.top() - photo.top()) / photo.height(),
            area.width() / photo.width(),
            area.height() / photo.height(),
        ]
    })
}

/// Which of the bar's toggles are on.
#[derive(Clone, Copy)]
struct Toggles {
    info: bool,
    filmstrip: bool,
    faces: bool,
    editing: bool,
}

/// File name, date, rating and the buttons in a pill at the bottom (REQ-009-01); the button clicked, if any.
fn control_bar(
    ui: &mut egui::Ui,
    rect: Rect,
    text: BarText,
    backing: crate::glass::Backing,
    toggles: Toggles,
) -> Option<BarButton> {
    let Toggles {
        info: info_open,
        filmstrip,
        faces,
        editing,
    } = toggles;
    let buttons = bar_buttons(
        text.editable || editing,
        text.faces && !editing,
        text.add_face && !editing,
        text.filmstrip,
    );
    backing.paint(ui.painter(), rect, rect.height() / 2.0);
    // Clicks on the bar's background don't reach the scrim behind it.
    ui.interact(rect, ui.make_persistent_id("control bar"), egui::Sense::click());
    let y = rect.center().y;
    let buttons_width = bar_buttons_width(buttons.len()) + 6.0;
    let text_clip = Rect::from_min_max(rect.min, pos2(rect.right() - buttons_width - BAR_GAP, rect.bottom()));
    let painter = ui.painter().with_clip_rect(text_clip);
    let mut x = rect.left() + 20.0;
    let name_width = text.name.size().x;
    painter.galley(pos2(x, y - text.name.size().y / 2.0), text.name, theme::text());
    x += name_width + BAR_GAP;
    if let Some(date) = text.date {
        let width = date.size().x;
        painter.galley(pos2(x, y - date.size().y / 2.0), date, theme::text());
        x += width + BAR_GAP;
    }
    if text.stars > 0 {
        for _ in 0..text.stars {
            let star = Rect::from_min_size(pos2(x, y - STAR_SIZE / 2.0), Vec2::splat(STAR_SIZE));
            if text_clip.contains_rect(star) {
                icons::rating_star(true, STAR_SIZE).paint_at(ui, star);
            }
            x += STAR_SIZE + 2.0;
        }
    }
    let separator = rect.right() - buttons_width - BAR_GAP / 2.0;
    ui.painter()
        .vline(separator, (y - 10.0)..=(y + 10.0), egui::Stroke::new(1.0, theme::rim()));
    let mut clicked = None;
    for (i, &(button, icon, label, tooltip)) in buttons.iter().enumerate() {
        let at = Rect::from_min_size(
            pos2(
                rect.right() - 6.0 - bar_buttons_width(buttons.len() - i),
                y - theme::CONTROL_HEIGHT / 2.0,
            ),
            Vec2::splat(theme::CONTROL_HEIGHT),
        );
        let pressed = match button {
            BarButton::Edit => Some(editing),
            BarButton::Info => Some(info_open && !editing),
            BarButton::Filmstrip => Some(filmstrip),
            BarButton::Faces => Some(faces),
            _ => None,
        };
        let mut widget = egui::Button::image(icon.small())
            .frame(false)
            .corner_radius(theme::CONTROL_HEIGHT / 2.0);
        // A pressed toggle is filled; a fill turns the frame on.
        if pressed == Some(true) {
            widget = widget.fill(theme::selected()).stroke(egui::Stroke::NONE);
        }
        let response = put_button(ui, at, widget).on_hover_text(tooltip);
        response.widget_info(|| match pressed {
            Some(pressed) => egui::WidgetInfo::selected(egui::Role::Button, true, pressed, label),
            None => egui::WidgetInfo::labeled(egui::Role::Button, true, label),
        });
        if response.clicked() {
            clicked = Some(button);
        }
    }
    clicked
}

#[cfg(test)]
mod tests {
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
