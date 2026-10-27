//! Video playback (REQ-009-06). The viewer talks to a [`Playback`]: the app's is an [`MpvPlayer`] (libmpv), UI tests use
//! a [`FakePlayback`]. [`Videos`] makes the player on first use, since loading libmpv costs time and memory.

pub mod controls;
mod mpv;
mod player;

pub use player::MpvPlayer;

use eframe::egui::TextureId;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// What the player reports about the loaded video.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackState {
    pub paused: bool,
    /// Seconds.
    pub position: f64,
    pub duration: f64,
    /// The decoded frame's size, unrotated; `None` until known.
    pub video_size: Option<[u32; 2]>,
    /// Clockwise quarter turns the frame is shown at (a phone video held upright: 1).
    pub quarters: u8,
    /// Played to its end; the last frame stays.
    pub ended: bool,
    /// Percent, 0–100.
    pub volume: f64,
    pub muted: bool,
    /// The file couldn't be played: mpv's reason.
    pub error: Option<String>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            paused: false,
            position: 0.0,
            duration: 0.0,
            video_size: None,
            quarters: 0,
            ended: false,
            volume: domain::video::MAX_VOLUME,
            muted: false,
            error: None,
        }
    }
}

/// The latest frame of the loaded video as a texture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoFrame {
    pub id: TextureId,
    /// Pixels, unrotated.
    pub size: [u32; 2],
}

/// A video player. One video at a time; volume and mute stay across videos.
pub trait Playback {
    /// Loads `file` and plays it from the start.
    fn load(&mut self, file: &Path);
    /// Stops playing and forgets the video.
    fn stop(&mut self);
    fn set_paused(&mut self, paused: bool);
    /// Seeks to `position` seconds: exactly, or to the nearest keyframe (fast, while dragging the slider).
    fn seek(&mut self, position: f64, exact: bool);
    fn set_volume(&mut self, volume: f64);
    fn set_muted(&mut self, muted: bool);
    /// The loaded video's state; may be up to 100 ms old.
    fn state(&mut self) -> PlaybackState;
    /// The latest frame of the loaded video, `None` before the first. `render_size` is the pixel size wanted for the
    /// coming frames. Call it once per UI frame while the video shows: display-synced timing counts on it.
    fn frame(&mut self, render_size: Option<[u32; 2]>) -> Option<VideoFrame>;
}

type MakePlayer = Box<dyn FnOnce() -> Result<Box<dyn Playback>, String>>;

/// The app's player, made on first use; or why videos can't play.
pub struct Videos {
    make: Option<MakePlayer>,
    player: Option<Result<Box<dyn Playback>, String>>,
}

impl Videos {
    /// A player that `make` makes when the first video shows.
    pub fn new(make: impl FnOnce() -> Result<Box<dyn Playback>, String> + 'static) -> Self {
        Self {
            make: Some(Box::new(make)),
            player: None,
        }
    }

    /// Videos can't play, for `reason`; the viewer shows their preview and says why.
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            make: None,
            player: Some(Err(reason.into())),
        }
    }

    /// An already made player (tests).
    pub fn with(player: Box<dyn Playback>) -> Self {
        Self {
            make: None,
            player: Some(Ok(player)),
        }
    }

    /// The player, made now if it wasn't yet; or why there is none.
    pub fn player(&mut self) -> Result<&mut dyn Playback, String> {
        if self.player.is_none() {
            let start = std::time::Instant::now();
            let made = match self.make.take() {
                Some(make) => make(),
                None => Err("no video player".to_owned()),
            };
            match &made {
                Ok(_) => tracing::info!("video player ready in {:?}", start.elapsed()),
                Err(error) => tracing::error!("can't play videos: {error}"),
            }
            self.player = Some(made);
        }
        match self.player.as_mut() {
            Some(Ok(player)) => Ok(player.as_mut()),
            Some(Err(error)) => Err(error.clone()),
            None => unreachable!("made above"),
        }
    }

    /// Stops the player if it was made.
    pub fn stop(&mut self) {
        if let Some(Ok(player)) = &mut self.player {
            player.stop();
        }
    }
}

impl Default for Videos {
    fn default() -> Self {
        Self::unavailable("Video playback isn't set up")
    }
}

/// A call on a [`FakePlayback`].
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Load(PathBuf),
    Stop,
    Paused(bool),
    Seek(f64, bool),
    Volume(f64),
    Muted(bool),
}

#[derive(Debug, Default)]
pub struct FakeState {
    pub calls: Vec<Call>,
    /// What [`Playback::state`] returns; calls change it as a player would (without time passing).
    pub state: PlaybackState,
    /// What [`Playback::frame`] returns while a video is loaded.
    pub frame: Option<VideoFrame>,
    pub loaded: Option<PathBuf>,
    /// The render size asked for last.
    pub render_size: Option<[u32; 2]>,
}

/// A player for UI tests: records the calls and plays nothing. Clones share their state.
#[derive(Debug, Clone, Default)]
pub struct FakePlayback(pub Arc<Mutex<FakeState>>);

impl FakePlayback {
    pub fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The calls so far, emptied.
    pub fn take_calls(&self) -> Vec<Call> {
        std::mem::take(&mut self.lock().calls)
    }
}

impl Playback for FakePlayback {
    fn load(&mut self, file: &Path) {
        let mut fake = self.lock();
        fake.calls.push(Call::Load(file.to_owned()));
        fake.loaded = Some(file.to_owned());
        // Plays from the start; the rest stays as the test set it up.
        (fake.state.position, fake.state.paused, fake.state.ended) = (0.0, false, false);
    }

    fn stop(&mut self) {
        let mut fake = self.lock();
        fake.calls.push(Call::Stop);
        fake.loaded = None;
    }

    fn set_paused(&mut self, paused: bool) {
        let mut fake = self.lock();
        fake.calls.push(Call::Paused(paused));
        fake.state.paused = paused;
    }

    fn seek(&mut self, position: f64, exact: bool) {
        let mut fake = self.lock();
        fake.calls.push(Call::Seek(position, exact));
        fake.state.position = position;
        fake.state.ended = false;
    }

    fn set_volume(&mut self, volume: f64) {
        let mut fake = self.lock();
        fake.calls.push(Call::Volume(volume));
        fake.state.volume = volume;
    }

    fn set_muted(&mut self, muted: bool) {
        let mut fake = self.lock();
        fake.calls.push(Call::Muted(muted));
        fake.state.muted = muted;
    }

    fn state(&mut self) -> PlaybackState {
        self.lock().state.clone()
    }

    fn frame(&mut self, render_size: Option<[u32; 2]>) -> Option<VideoFrame> {
        let mut fake = self.lock();
        fake.render_size = render_size;
        fake.loaded.as_ref().and(fake.frame)
    }
}
