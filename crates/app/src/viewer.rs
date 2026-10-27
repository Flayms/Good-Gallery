//! The viewer (spec 009): one media item over the whole window, stepped through in the gallery's order. Photos load
//! their originals (`originals`) and show their thumbnail until then; they zoom and pan. The gallery keeps drawing
//! below the scrim and keeps the shown item in view. An info panel (REQ-009-08) and a filmstrip (spec 015) open beside
//! and below the media; the chrome takes a tint of the item's colors (REQ-013-11), and the photo grows from its tile
//! and back (REQ-013-12). Videos play in a [`Playback`](crate::video::Playback) with their controls along the bottom of
//! the video (REQ-009-06).

use crate::gallery::{self, Gallery, MediaAction};
use crate::icons::Icon;
use crate::originals::{OriginalRequest, OriginalState, Originals};
use crate::video::controls::{self as video_controls, Control};
use crate::video::{PlaybackState, VideoFrame, Videos};
use crate::{format, info, theme};
use base64::Engine as _;
use db::gallery::GalleryItem;
use db::libraries::Library;
use db::media::file_in;
use domain::ambient::{self, Rgb};
use domain::media::MediaKind;
use domain::viewer::{self as zoom, FILMSTRIP_KEY, FILMSTRIP_THUMB, FIT, Size, Strip, ZoomView};
use eframe::egui::{self, Color32, CursorIcon, Key, Modifiers, Pos2, Rect, Vec2, pos2, vec2};
use std::path::PathBuf;
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
/// The window behind the viewer, darkened. The design's 0.5 needs the blur of M10 (REQ-013-14).
const SCRIM: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 204);
/// `GLASS_BAR` and `GLASS_PANEL` without their blur (M10), before the item's tint.
const GLASS_BAR: Rgb = Rgb {
    r: 28.0,
    g: 28.0,
    b: 28.0,
};
const GLASS_BAR_ALPHA: f32 = 0.72;
const GLASS_PANEL: Rgb = Rgb {
    r: 36.0,
    g: 36.0,
    b: 36.0,
};
const GLASS_PANEL_ALPHA: f32 = 0.86;

/// What the viewer asks the app for.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// It closed.
    Close,
    /// It closed to filter the gallery by this tag (REQ-009-09).
    FilterByTag(String),
    /// Explorer should show this file (REQ-009-10); the viewer stays.
    ShowInExplorer(PathBuf),
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
    /// Space: play / pause the video.
    space: bool,
    /// ↑ / ↓: volume steps.
    volume: i32,
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
}

impl Chrome {
    fn new(tint: Option<Rgb>) -> Self {
        let glass = |base: Rgb, alpha: f32| {
            let Rgb { r, g, b } = match tint {
                Some(tint) => ambient::tint_glass(base, tint, ambient::AMOUNT),
                None => base,
            };
            Color32::from_rgba_unmultiplied(r as u8, g as u8, b as u8, (alpha * 255.0).round() as u8)
        };
        Self {
            bar: glass(GLASS_BAR, GLASS_BAR_ALPHA),
            panel: glass(GLASS_PANEL, GLASS_PANEL_ALPHA),
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
    info_open: bool,
    filmstrip: bool,
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
    chrome: Chrome,
    /// The media item the chrome was tinted for.
    chrome_for: Option<i64>,
    playing: Option<Playing>,
    /// The last click was on the video or its controls: ← / → seek instead of stepping (REQ-009-02).
    video_focus: bool,
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
            view: FIT,
            instant: None,
            fade_from: None,
            last_activity: Instant::now(),
            close_at: None,
            keys: Keys::default(),
            info_open: false,
            filmstrip,
            strip_offset: 0.0,
            strip_centered: None,
            strip_width: 0.0,
            opening: Transition { start: None, tile },
            closing: None,
            photo: None,
            chrome: Chrome::new(None),
            chrome_for: None,
            playing: None,
            video_focus: false,
        }
    }

    /// Opens the viewer on gallery position `index` as the user left the filmstrip, growing from its tile.
    pub fn open(gallery: &Gallery, index: usize) -> Option<Self> {
        let id = gallery.id_at(index)?;
        let filmstrip = zoom::is_filmstrip_shown(gallery.setting(FILMSTRIP_KEY).as_deref());
        Some(Self::new(
            index,
            id,
            gallery.data_version(),
            gallery.tile_rect(index),
            filmstrip,
        ))
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

    pub fn info_open(&self) -> bool {
        self.info_open
    }

    pub fn filmstrip_shown(&self) -> bool {
        self.filmstrip
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

    /// Whether the shown photo's original was loaded when it came up, so it shows without fading in (REQ-009-14).
    pub fn shows_original_at_once(&self) -> bool {
        self.instant == Some(true)
    }

    fn layer() -> egui::LayerId {
        egui::LayerId::new(egui::Order::Middle, egui::Id::NULL.with("viewer"))
    }

    /// Takes the viewer's keys before anything else sees them; call at the start of the frame.
    pub fn input(&mut self, ctx: &egui::Context) {
        // Focus stays in the viewer (the filmstrip's thumbnails); nothing under it takes focus or acts on Enter or
        // Space.
        let focused = ctx.memory(|memory| memory.focused());
        let focus_inside = focused
            .and_then(|id| ctx.read_response(id))
            .is_some_and(|response| response.layer_id == Self::layer());
        if let Some(focused) = focused
            && !focus_inside
        {
            ctx.memory_mut(|memory| memory.surrender_focus(focused));
        }
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
            if input.modifiers.ctrl || input.modifiers.alt || input.modifiers.command {
                return;
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
        self.video_focus = false;
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

    /// Starts closing: the photo shrinks back into its tile, or the viewer fades (REQ-013-12).
    fn close(&mut self, gallery: &Gallery, action: Action) {
        if self.closing.is_none() {
            let tile = if self.missing {
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
        let keys = std::mem::take(&mut self.keys);
        self.track_video_focus(ctx);
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
                    let target = self.index as i64 + i64::from(keys.step.signum());
                    if target >= 0 && (target as usize) < count {
                        self.go_to(target as usize, gallery);
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
        self.sync_video(ctx, item.as_ref(), videos, libraries);
        if self.chrome_for != Some(self.id) && item.is_some() {
            self.chrome = Chrome::new(tint_of(item.as_ref().and_then(|item| item.thumbhash.as_deref())));
            self.chrome_for = Some(self.id);
        }

        let screen = ctx.content_rect();
        let layout = Layout::new(screen, self.info_open, self.filmstrip);
        let mut action = None;
        egui::Area::new(Self::layer().id)
            .order(Self::layer().order)
            .fixed_pos(screen.min)
            .constrain(false)
            .show(ctx, |ui| {
                ui.set_min_size(screen.size());
                // Without a tile to grow from, the whole viewer fades in and out.
                if tile.is_none() {
                    ui.set_opacity(progress);
                }
                ui.painter().rect_filled(screen, 0.0, SCRIM.gamma_multiply(progress));
                let backdrop = ui.interact(screen, ui.make_persistent_id("backdrop"), egui::Sense::click_and_drag());
                let label = item
                    .as_ref()
                    .map_or("Media viewer", |item| item.file_name.as_str())
                    .to_owned();
                backdrop.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Dialog, true, &label));
                let closing = self.closing.is_some();
                // The context menu's choice (REQ-009-11).
                if !closing && let Some(item) = &item {
                    backdrop.context_menu(|ui| match gallery::context_menu(ui) {
                        Some(MediaAction::ShowInExplorer) => action = Self::explorer_action(item, libraries),
                        Some(MediaAction::Information) => self.info_open = true,
                        None => {}
                    });
                }
                match &item {
                    Some(item) => {
                        let keys = if closing { Keys::default() } else { keys };
                        let transition = tile.filter(|_| progress < 1.0).map(|tile| (tile, progress));
                        let drawn = self.media(ui, &backdrop, &layout, item, gallery, &original, keys, transition);
                        self.photo = Some(drawn);
                    }
                    None if self.missing => {
                        message(ui, layout.media.center(), "Media not found");
                        self.backdrop_clicks(&backdrop);
                    }
                    None => {
                        self.backdrop_clicks(&backdrop);
                    }
                }
                if closing {
                    return;
                }
                ui.scope(|ui| {
                    ui.multiply_opacity(progress);
                    if let Some(chosen) = self.controls(ui, &layout, item.as_ref(), count, gallery, libraries) {
                        action = Some(chosen);
                    }
                });
            });
        if let Some(at) = self.close_at {
            ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
        }
        self.apply_video_control(videos);
        match action {
            Some(Action::ShowInExplorer(path)) => Some(Action::ShowInExplorer(path)),
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
            OriginalState::Loaded(loaded) => Some(*loaded),
            _ => None,
        };
        let playing = self
            .playing_shown()
            .filter(|playing| playing.id == item.id)
            .map(|playing| (playing.state.video_size, playing.state.quarters, playing.frame));
        let video_size = playing.and_then(|(size, quarters, _)| Some(domain::video::shown_size(size?, quarters)));
        let natural = match (item.width, item.height, loaded, thumbnail) {
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
        let viewport = Size::new(area.width(), area.height());
        let fitted = zoom::fit_size(natural, viewport);
        let photo = item.kind == MediaKind::Image;

        // Not over the chrome: its own widgets take those clicks and wheel turns.
        let over_chrome = backdrop.hover_pos().is_some_and(|pos| layout.is_chrome(pos));
        if photo && !over_chrome {
            self.handle_zoom(ui, backdrop, area, natural, fitted, viewport, keys);
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
            None => (target, area, full),
        };
        if photo
            && !over_chrome
            && let Some(pos) = backdrop.hover_pos()
        {
            let icon = if !target.intersect(area).contains(pos) {
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
        if let Some((frame, quarters)) = frame {
            painter.add(rotated_image(frame.id, rect, quarters));
        }
        let at = pos2(target.center().x, target.bottom().min(area.bottom()) - 28.0);
        if let OriginalState::Failed(_) = original {
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
            let delta = wheel(ui).y;
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
        let text = bar_text(ui, screen, item);
        let bar = control_bar_rect(layout, &text);
        let previous = (self.index > 0).then(|| {
            Rect::from_center_size(
                pos2(screen.left() + INSET + NAV_SIZE / 2.0, layout.media.center().y),
                Vec2::splat(NAV_SIZE),
            )
        });
        let next = (self.index + 1 < count).then(|| {
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
        let hidden = zoom::controls_hidden(idle, hovering, self.info_open, paused);
        if !idle {
            ctx.request_repaint_after(IDLE.saturating_sub(self.last_activity.elapsed()));
        }
        let shown = ctx.animate_bool_with_time(ui.make_persistent_id("controls"), !hidden, theme::DURATION_BASE);
        if hidden && pointer.is_some_and(|pos| screen.contains(pos)) {
            ctx.set_cursor_icon(CursorIcon::None);
        }

        let mut action = None;
        if self.info_open
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
            let chrome = self.chrome;
            if let Some(rect) = video_bar
                && let Some(playing) = &mut self.playing
                && let Some(control) = video_controls::show(ui, rect, &playing.state, &mut playing.dragging, chrome.bar)
            {
                playing.control = Some(control);
            }
            for (rect, icon, label, by) in [
                (previous, Icon::ChevronLeft, "Previous", -1),
                (next, Icon::ChevronRight, "Next", 1),
            ] {
                let Some(rect) = rect else { continue };
                let tooltip = if by < 0 { "Previous (←)" } else { "Next (→)" };
                if round_button(ui, rect, icon, label, tooltip, chrome.bar).clicked() {
                    step = Some((self.index as i64 + by).max(0) as usize);
                }
            }
            if let Some(strip) = layout.strip
                && let Some(index) = self.filmstrip(ui, strip, count, gallery)
            {
                step = Some(index);
            }
            match control_bar(ui, bar, text, chrome.bar, self.info_open, self.filmstrip) {
                Some(BarButton::ShowInExplorer) => {
                    if let Some(item) = item {
                        action = Self::explorer_action(item, libraries);
                    }
                }
                Some(BarButton::Info) => self.info_open = !self.info_open,
                Some(BarButton::Filmstrip) => self.toggle_filmstrip(gallery),
                Some(BarButton::Close) => action = Some(Action::Close),
                None => {}
            }
        });
        if !self.filmstrip {
            gallery.set_strip(0..0);
        }
        if let Some(index) = step
            && index < count
            && index != self.index
        {
            self.go_to(index, gallery);
            ctx.request_repaint();
        }
        action
    }

    /// The info panel at the right (REQ-009-08); a click on a tag filters the gallery by it (REQ-009-09).
    fn info_panel(&mut self, ui: &mut egui::Ui, rect: Rect, gallery: &mut Gallery) -> Option<Action> {
        ui.painter().rect(
            rect,
            theme::RADIUS_PANEL,
            self.chrome.panel,
            egui::Stroke::new(1.0, theme::RIM),
            egui::StrokeKind::Inside,
        );
        // Clicks and wheel turns on the panel stay in it.
        ui.interact(rect, ui.make_persistent_id("info panel"), egui::Sense::click());
        let mut clicked = None;
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
                clicked = info::show_loaded(ui, gallery.detail(self.id))
            });
        clicked.map(Action::FilterByTag)
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
        ui.painter().rect(
            rect,
            theme::RADIUS_PANEL,
            self.chrome.bar,
            egui::Stroke::new(1.0, theme::RIM),
            egui::StrokeKind::Inside,
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
                painter.rect_filled(thumb, 2.0, theme::RAISED);
                continue;
            };
            let (id, video, name) = (item.id, item.kind == MediaKind::Video, item.file_name.clone());
            let current = index == self.index && !self.missing;
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
                    painter.rect_filled(thumb, 2.0, theme::RAISED);
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
                painter.rect_stroke(thumb, 2, egui::Stroke::new(2.0, theme::PAPER), egui::StrokeKind::Inside);
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
    info: Option<Rect>,
    strip: Option<Rect>,
    /// Right edge of the area for the media, the arrows, the bar and the filmstrip.
    right: f32,
    /// How far the control bar sits above the window's bottom.
    bar_bottom: f32,
}

impl Layout {
    fn new(screen: Rect, info_open: bool, filmstrip: bool) -> Self {
        let info = info_open.then(|| {
            Rect::from_min_max(
                pos2(screen.right() - INSET - info::WIDTH, screen.top() + INSET),
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
        let media = Rect::from_min_max(
            pos2(screen.left() + SIDE_SPACE, screen.top() + TOP_SPACE),
            pos2(right - SIDE_SPACE, screen.bottom() - CONTROLS_SPACE - below),
        );
        Self {
            screen,
            media,
            info,
            strip,
            right,
            bar_bottom: 16.0 + below,
        }
    }

    /// Whether `pos` is over the info panel or the filmstrip.
    fn is_chrome(&self, pos: Pos2) -> bool {
        self.info.is_some_and(|r| r.contains(pos)) || self.strip.is_some_and(|r| r.contains(pos))
    }
}

/// This frame's wheel movement in points (lines count 100), without Ctrl.
fn wheel(ui: &egui::Ui) -> Vec2 {
    ui.input(|input| {
        input
            .raw
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel {
                    unit, delta, modifiers, ..
                } if !modifiers.ctrl => Some(match unit {
                    egui::MouseWheelUnit::Point => *delta,
                    egui::MouseWheelUnit::Line => *delta * WHEEL_LINE,
                    egui::MouseWheelUnit::Page => vec2(delta.x.signum(), delta.y.signum()) * WHEEL_LINE,
                }),
                _ => None,
            })
            .fold(Vec2::ZERO, |sum, delta| sum + delta)
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

/// Asks for the originals of the shown photo and its neighbours, nearest first (REQ-009-13).
fn preload(gallery: &mut Gallery, originals: &mut Originals, libraries: &[Library], index: usize, count: usize) {
    let mut wanted = Vec::new();
    for at in std::iter::once(index).chain(zoom::neighbours(count, index)) {
        let Some(item) = gallery.item(at) else { continue };
        if item.kind != MediaKind::Image {
            continue;
        }
        let Some(path) = file_in(libraries, item.root_id, &item.rel_path) else {
            continue;
        };
        wanted.push(OriginalRequest { media: item.id, path });
    }
    originals.set(wanted);
}

/// Text in a glass pill centered at `at`.
fn message(ui: &mut egui::Ui, at: Pos2, text: &str) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.into(), egui::FontId::proportional(theme::SIZE_BODY), theme::PAPER);
    let rect = Rect::from_center_size(at, galley.size() + vec2(32.0, 12.0));
    ui.painter()
        .rect_filled(rect, rect.height() / 2.0, Chrome::new(None).bar);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, theme::PAPER);
    // Hover only: clicks still reach the photo and the scrim below.
    let response = ui.interact(rect, ui.make_persistent_id(("message", text)), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
}

fn round_button(
    ui: &mut egui::Ui,
    rect: Rect,
    icon: Icon,
    label: &str,
    tooltip: &str,
    fill: Color32,
) -> egui::Response {
    let response = ui
        .put(
            rect,
            egui::Button::image(icon.image(20.0, theme::PAPER))
                .fill(fill)
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
        max_width: (screen.width() * 0.3).max(80.0),
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

/// The control bar's buttons, right of the separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarButton {
    ShowInExplorer,
    Info,
    Filmstrip,
    Close,
}

const BAR_BUTTONS: [(BarButton, Icon, &str, &str); 4] = [
    (
        BarButton::ShowInExplorer,
        Icon::FolderOpen,
        "Show in Explorer",
        "Show in Explorer",
    ),
    (BarButton::Info, Icon::Info, "Info", "Info (i)"),
    (
        BarButton::Filmstrip,
        Icon::GalleryHorizontal,
        "Filmstrip",
        "Filmstrip (f)",
    ),
    (BarButton::Close, Icon::X, "Close", "Close (Esc)"),
];

fn control_bar_rect(layout: &Layout, text: &BarText) -> Rect {
    let mut width = 20.0 + text.name.size().x + BAR_GAP;
    if let Some(date) = &text.date {
        width += date.size().x + BAR_GAP;
    }
    if text.stars > 0 {
        width += text.stars as f32 * (STAR_SIZE + 2.0) + BAR_GAP;
    }
    // Separator, the buttons and the right padding.
    width += 1.0 + BAR_GAP + BAR_BUTTONS.len() as f32 * theme::CONTROL_HEIGHT + 6.0;
    let room = layout.right - layout.screen.left();
    let width = width.min(room - 32.0);
    Rect::from_center_size(
        pos2(
            layout.screen.left() + room / 2.0,
            layout.screen.bottom() - layout.bar_bottom - BAR_HEIGHT / 2.0,
        ),
        vec2(width, BAR_HEIGHT),
    )
}

/// File name, date, rating and the buttons in a pill at the bottom (REQ-009-01); the button clicked, if any.
fn control_bar(
    ui: &mut egui::Ui,
    rect: Rect,
    text: BarText,
    fill: Color32,
    info_open: bool,
    filmstrip: bool,
) -> Option<BarButton> {
    let painter = ui.painter();
    painter.rect(
        rect,
        rect.height() / 2.0,
        fill,
        egui::Stroke::new(1.0, theme::RIM),
        egui::StrokeKind::Inside,
    );
    // Clicks on the bar's background don't reach the scrim behind it.
    ui.interact(rect, ui.make_persistent_id("control bar"), egui::Sense::click());
    let y = rect.center().y;
    let buttons_width = BAR_BUTTONS.len() as f32 * theme::CONTROL_HEIGHT + 6.0;
    let text_clip = Rect::from_min_max(rect.min, pos2(rect.right() - buttons_width - BAR_GAP, rect.bottom()));
    let painter = ui.painter().with_clip_rect(text_clip);
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
            if text_clip.contains_rect(star) {
                Icon::Star.image(STAR_SIZE, theme::STAR).paint_at(ui, star);
            }
            x += STAR_SIZE + 2.0;
        }
    }
    let separator = rect.right() - buttons_width - BAR_GAP / 2.0;
    ui.painter()
        .vline(separator, (y - 10.0)..=(y + 10.0), egui::Stroke::new(1.0, theme::RIM));
    let mut clicked = None;
    for (i, (button, icon, label, tooltip)) in BAR_BUTTONS.into_iter().enumerate() {
        let at = Rect::from_min_size(
            pos2(
                rect.right() - 6.0 - (BAR_BUTTONS.len() - i) as f32 * theme::CONTROL_HEIGHT,
                y - theme::CONTROL_HEIGHT / 2.0,
            ),
            Vec2::splat(theme::CONTROL_HEIGHT),
        );
        let pressed = match button {
            BarButton::Info => Some(info_open),
            BarButton::Filmstrip => Some(filmstrip),
            _ => None,
        };
        let mut widget = egui::Button::image(icon.small())
            .frame(false)
            .corner_radius(theme::CONTROL_HEIGHT / 2.0);
        // A pressed toggle is filled; a fill turns the frame on.
        if pressed == Some(true) {
            widget = widget.fill(theme::SELECTED).stroke(egui::Stroke::NONE);
        }
        let response = ui.put(at, widget).on_hover_text(tooltip);
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
