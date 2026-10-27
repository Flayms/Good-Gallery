//! Videos in the viewer (REQ-009-06) with a fake player: what the viewer asks the player for, and what it shows.

mod common;

use app::video::{Call, FakePlayback, PlaybackState, VideoFrame, Videos};
use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::writer::{IndexWriter, IndexedFile};
use domain::media::{MediaKind, MediaMetadata};
use eframe::egui::accesskit::Role;
use eframe::egui::{self, Key, Modifiers, PointerButton, pos2};
use egui_kittest::kittest::Queryable as _;
use std::path::PathBuf;

type App = egui_kittest::Harness<'static, app::AppState>;

/// Center of the media area in the 900 × 600 window.
const CENTER: egui::Pos2 = pos2(450.0, 284.0);

/// `0.png`, `1.mp4` (a minute long, 400 × 300), `2.png`, newest first; photos on disk, the video in the fake player.
fn app(fixture: &Fixture, fake: Option<&FakePlayback>) -> (App, PathBuf) {
    let dir = fixture.folder("media");
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, &dir, "Media", LibraryStatus::Online)
        .unwrap()
        .id;
    let files: Vec<IndexedFile> = ["0.png", "1.mp4", "2.png"]
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let video = name.ends_with(".mp4");
            if !video {
                image::RgbImage::from_pixel(400, 300, image::Rgb([40, 120, 200]))
                    .save(std::path::Path::new(&dir).join(name))
                    .unwrap();
            }
            IndexedFile {
                rel_path: (*name).to_owned(),
                file_name: (*name).to_owned(),
                kind: if video { MediaKind::Video } else { MediaKind::Image },
                size: 1,
                mtime: 1_000,
                sidecar_mtime: None,
                metadata: MediaMetadata {
                    width: Some(400),
                    height: Some(300),
                    duration: video.then_some(60.0),
                    taken_at: Some(2_000_000 - i as i64),
                    ..Default::default()
                },
            }
        })
        .collect();
    let mut writer = IndexWriter::new(conn);
    writer.write(root, &files).unwrap();
    let mut state = fixture.state(writer.into());
    if let Some(fake) = fake {
        {
            let mut player = fake.lock();
            player.state.duration = 60.0;
            player.state.video_size = Some([400, 300]);
            player.state.volume = 50.0;
        }
        state = state.with_videos(Videos::with(Box::new(fake.clone())));
    }
    let mut harness = common::harness_with_step(state, 1.0 / 60.0);
    settle(&mut harness);
    (harness, std::path::Path::new(&dir).join("1.mp4"))
}

fn open(harness: &mut App, name: &str) {
    harness.get_by_role_and_label(Role::Image, name).click();
    settle(harness);
    // Past the opening transition.
    harness.run_steps(30);
}

fn viewer(harness: &App) -> &app::viewer::Viewer {
    harness.state().viewer.as_ref().expect("the viewer is open")
}

fn press(harness: &App, pos: egui::Pos2, pressed: bool) {
    harness.event(egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
}

fn click_at(harness: &mut App, pos: egui::Pos2) {
    harness.hover_at(pos);
    harness.run_steps(1);
    press(harness, pos, true);
    harness.run_steps(1);
    press(harness, pos, false);
    harness.run_steps(30);
}

fn click(harness: &mut App, role: Role, label: &str) {
    let center = harness.get_by_role_and_label(role, label).rect().center();
    click_at(harness, center);
}

/// The slider's point at `fraction` of its width.
fn slider_at(harness: &App, label: &str, fraction: f32) -> egui::Pos2 {
    let rect = harness.get_by_role_and_label(Role::Slider, label).rect();
    pos2(rect.left() + rect.width() * fraction, rect.center().y)
}

fn key(harness: &mut App, key: Key) {
    harness.key_press(key);
    harness.run_steps(2);
}

/// REQ-009-06, REQ-009-04: the shown video plays from its file in the library, with controls; stepping to a photo,
/// back, and closing stop and start it again.
#[test]
fn req_009_06_req_009_04_plays_the_shown_video_and_stops_it() {
    let fixture = Fixture::new();
    let fake = FakePlayback::default();
    let (mut harness, file) = app(&fixture, Some(&fake));

    open(&mut harness, "1.mp4, video, 1:00");
    assert_eq!(fake.take_calls(), [Call::Load(file.clone())]);
    assert!(
        harness
            .query_by_role_and_label(Role::Toolbar, "Video controls")
            .is_some()
    );
    assert!(
        harness.query_by_role_and_label(Role::Button, "Pause").is_some(),
        "it plays"
    );
    assert!(harness.query_by_label("0:00 / 1:00").is_some());

    key(&mut harness, Key::ArrowRight);
    assert_eq!(fake.take_calls(), [Call::Stop]);
    assert!(
        harness
            .query_by_role_and_label(Role::Toolbar, "Video controls")
            .is_none()
    );
    key(&mut harness, Key::ArrowLeft);
    assert_eq!(fake.take_calls(), [Call::Load(file)]);

    key(&mut harness, Key::Escape);
    harness.run_steps(30);
    assert!(harness.state().viewer.is_none());
    assert!(fake.take_calls().contains(&Call::Stop));
}

/// REQ-009-06: play / pause by button, Space and a click on the video; seeking with the slider; mute and volume.
#[test]
fn req_009_06_controls_play_pause_seek_mute_and_volume() {
    let fixture = Fixture::new();
    let fake = FakePlayback::default();
    let (mut harness, _) = app(&fixture, Some(&fake));
    open(&mut harness, "1.mp4, video, 1:00");
    fake.take_calls();

    key(&mut harness, Key::Space);
    assert_eq!(fake.take_calls(), [Call::Paused(true)]);
    assert!(harness.query_by_role_and_label(Role::Button, "Play").is_some());
    click_at(&mut harness, CENTER);
    assert_eq!(
        fake.take_calls(),
        [Call::Paused(false)],
        "a click on the video plays it"
    );
    assert!(harness.state().viewer.is_some(), "and doesn't close the viewer");
    click(&mut harness, Role::Button, "Pause");
    assert_eq!(fake.take_calls(), [Call::Paused(true)]);

    let half = slider_at(&harness, "Seek", 0.5);
    click_at(&mut harness, half);
    let calls = fake.take_calls();
    let Some(Call::Seek(position, true)) = calls.last() else {
        panic!("an exact seek: {calls:?}")
    };
    assert!((position - 30.0).abs() < 3.0, "{position}");
    assert!(harness.query_by_label("0:30 / 1:00").is_some() || harness.query_by_label("0:29 / 1:00").is_some());

    click(&mut harness, Role::Button, "Mute");
    assert_eq!(fake.take_calls(), [Call::Muted(true)]);
    assert!(harness.query_by_role_and_label(Role::Button, "Unmute").is_some());
    let quarter = slider_at(&harness, "Volume", 0.25);
    click_at(&mut harness, quarter);
    let calls = fake.take_calls();
    assert!(
        matches!(calls.as_slice(), [Call::Volume(volume), Call::Muted(false)] if (volume - 25.0).abs() < 8.0),
        "volume set and unmuted: {calls:?}"
    );
}

/// REQ-009-06: at the end, play starts over.
#[test]
fn req_009_06_play_at_the_end_starts_over() {
    let fixture = Fixture::new();
    let fake = FakePlayback::default();
    let (mut harness, _) = app(&fixture, Some(&fake));
    open(&mut harness, "1.mp4, video, 1:00");
    {
        let mut player = fake.lock();
        (player.state.ended, player.state.paused, player.state.position) = (true, true, 60.0);
    }
    harness.run_steps(2);
    fake.take_calls();
    click(&mut harness, Role::Button, "Play");
    assert_eq!(fake.take_calls(), [Call::Seek(0.0, true), Call::Paused(false)]);
}

/// REQ-009-02: while the video controls have focus (the last click was on them or the video), ← / → seek 5 s and
/// ↑ / ↓ change the volume; otherwise ← / → step.
#[test]
fn req_009_02_arrows_seek_while_the_video_controls_have_focus() {
    let fixture = Fixture::new();
    let fake = FakePlayback::default();
    let (mut harness, _) = app(&fixture, Some(&fake));
    open(&mut harness, "1.mp4, video, 1:00");
    assert!(!viewer(&harness).video_focused());

    click(&mut harness, Role::Button, "Pause");
    assert!(viewer(&harness).video_focused());
    fake.take_calls();
    key(&mut harness, Key::ArrowRight);
    assert_eq!(fake.take_calls(), [Call::Seek(5.0, true)]);
    key(&mut harness, Key::ArrowRight);
    key(&mut harness, Key::ArrowLeft);
    assert_eq!(fake.take_calls(), [Call::Seek(10.0, true), Call::Seek(5.0, true)]);
    key(&mut harness, Key::ArrowUp);
    assert_eq!(fake.take_calls(), [Call::Volume(55.0)]);
    // Muted, a step up starts from the volume set before and unmutes.
    click(&mut harness, Role::Button, "Mute");
    fake.take_calls();
    key(&mut harness, Key::ArrowUp);
    assert_eq!(fake.take_calls(), [Call::Volume(60.0), Call::Muted(false)]);
    assert_eq!(viewer(&harness).index(), 1, "still on the video");

    // A click elsewhere (the Info button) takes the focus: → steps again.
    click(&mut harness, Role::Button, "Info");
    assert!(!viewer(&harness).video_focused());
    key(&mut harness, Key::ArrowRight);
    assert_eq!(viewer(&harness).index(), 2);

    // A click on the video gives it focus.
    key(&mut harness, Key::ArrowLeft);
    click_at(&mut harness, CENTER);
    assert!(viewer(&harness).video_focused());
    fake.take_calls();
    key(&mut harness, Key::ArrowLeft);
    assert_eq!(fake.take_calls(), [Call::Seek(0.0, true)]);
}

/// REQ-013-10: the controls step aside while a video plays, and stay while it is paused.
#[test]
fn req_013_10_a_paused_video_keeps_the_controls() {
    let fixture = Fixture::new();
    let fake = FakePlayback::default();
    let (mut harness, _) = app(&fixture, Some(&fake));
    open(&mut harness, "1.mp4, video, 1:00");
    harness.hover_at(pos2(450.0, 150.0));
    harness.run_steps(1);

    std::thread::sleep(std::time::Duration::from_millis(2600));
    harness.run_steps(20);
    assert!(harness.query_by_label("Close").is_none(), "hidden while it plays");
    assert!(
        harness
            .query_by_role_and_label(Role::Toolbar, "Video controls")
            .is_none()
    );

    fake.lock().state.paused = true;
    harness.run_steps(20);
    assert!(harness.query_by_label("Close").is_some(), "shown while paused");
    assert!(
        harness
            .query_by_role_and_label(Role::Toolbar, "Video controls")
            .is_some()
    );
}

/// REQ-009-06: frames replace the preview, fitted and turned as the video's metadata says (a phone held upright).
#[test]
fn req_009_06_shows_frames_fitted_and_turned_upright() {
    let fixture = Fixture::new();
    let fake = FakePlayback::default();
    let (mut harness, _) = app(&fixture, Some(&fake));
    // A frame: left half red, right half blue, 400 × 300 as decoded.
    let mut image = egui::ColorImage::filled([400, 300], egui::Color32::BLUE);
    for y in 0..300 {
        for x in 0..200 {
            image.pixels[y * 400 + x] = egui::Color32::RED;
        }
    }
    let texture = harness.ctx.load_texture("frame", image, egui::TextureOptions::LINEAR);
    {
        let mut player = fake.lock();
        player.frame = Some(VideoFrame {
            id: texture.id(),
            size: [400, 300],
        });
        player.state.quarters = 1;
    }
    open(&mut harness, "1.mp4, video, 1:00");
    let shown = viewer(&harness).photo_rect().expect("drawn");
    assert!(shown.height() > shown.width(), "upright: {shown:?}");
    let ratio = shown.width() / shown.height();
    assert!((ratio - 0.75).abs() < 0.01, "{ratio}");
    let size = fake.lock().render_size.expect("a render size");
    assert!(size[0] > size[1], "rendered unrotated: {size:?}");
    // Turned clockwise: the frame's left (red) half is on top.
    harness.hover_at(pos2(450.0, 20.0));
    harness.run_steps(1);
    harness.snapshot("viewer_video");
}

/// REQ-009-06: without a player, or for a file it can't play, the preview stays and a message says so.
#[test]
fn req_009_06_says_when_a_video_cant_play() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, None);
    open(&mut harness, "1.mp4, video, 1:00");
    assert!(viewer(&harness).video_unavailable().is_some());
    assert!(
        harness
            .query_by_label("Can't play videos here. Showing a preview.")
            .is_some()
    );
    assert!(
        harness
            .query_by_role_and_label(Role::Toolbar, "Video controls")
            .is_none()
    );
    drop(harness);
    drop(fixture);

    let fixture = Fixture::new();
    let fake = FakePlayback::default();
    let (mut harness, _) = app(&fixture, Some(&fake));
    fake.lock().state.error = Some("unknown format".into());
    open(&mut harness, "1.mp4, video, 1:00");
    let state: &PlaybackState = viewer(&harness).video().expect("loaded");
    assert!(state.error.is_some());
    assert!(
        harness
            .query_by_label("Can't play this video. Showing a preview.")
            .is_some()
    );
    assert!(
        harness
            .query_by_role_and_label(Role::Toolbar, "Video controls")
            .is_none(),
        "no controls over the message"
    );
}
