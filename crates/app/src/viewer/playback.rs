//! Playing videos in the viewer (REQ-009-06): the player kept in step with the item shown, and its controls.

use super::{Playing, Viewer};
use crate::video::controls::Control;
use crate::video::{PlaybackState, Videos};
use db::gallery::GalleryItem;
use db::libraries::Library;
use db::media::file_in;
use domain::media::MediaKind;
use eframe::egui;

impl Viewer {
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
    pub(super) fn playing_shown(&self) -> Option<&Playing> {
        self.playing
            .as_ref()
            .filter(|playing| playing.id == self.id && playing.unavailable.is_none() && !self.missing)
    }

    /// Asks the shown video for `control` (applied after the frame is drawn).
    pub(super) fn control_video(&mut self, control: Control) {
        if let Some(playing) = self.playing.as_mut().filter(|playing| playing.unavailable.is_none()) {
            playing.control = Some(control);
        }
    }

    /// A press on the video or its controls gives them focus, a press anywhere else takes it (REQ-009-02).
    pub(super) fn track_video_focus(&mut self, ctx: &egui::Context) {
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
    pub(super) fn sync_video(
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
    pub(super) fn apply_video_control(&mut self, videos: &mut Videos) {
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
}
