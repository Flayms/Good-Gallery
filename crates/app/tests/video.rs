//! The real player (libmpv) on generated fixtures in the four containers the app supports (M9 exit criterion). Skipped
//! without libmpv or ffmpeg unless `GG_REQUIRE_HELPERS` is set.

use app::video::{MpvPlayer, Playback, PlaybackState};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

fn helper(name: &str, found: Option<PathBuf>) -> Option<PathBuf> {
    match found {
        Some(path) => Some(path),
        None if std::env::var_os("GG_REQUIRE_HELPERS").is_some() => panic!("{name} not found"),
        None => {
            eprintln!("skipped: {name} not found");
            None
        }
    }
}

/// A 2 s clip with a test pattern and a tone.
fn make(ffmpeg: &Path, out: &Path, codecs: &[&str]) {
    let mut args = vec![
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=320x240:rate=30:duration=2",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000:duration=2",
        "-shortest",
        "-pix_fmt",
        "yuv420p",
    ];
    args.extend_from_slice(codecs);
    let status = Command::new(ffmpeg).args(&args).arg(out).status().expect("ffmpeg runs");
    assert!(status.success(), "ffmpeg made {}", out.display());
}

/// Fixtures: MP4 H.264 + AAC, MOV HEVC + AAC turned like a phone held upright, MKV H.264 + Opus, WebM VP9 + Opus.
fn fixtures(ffmpeg: &Path, dir: &Path) -> Vec<(PathBuf, u8)> {
    let mp4 = dir.join("h264-aac.mp4");
    make(ffmpeg, &mp4, &["-c:v", "libx264", "-c:a", "aac"]);
    let mov = dir.join("hevc-aac.mov");
    make(
        ffmpeg,
        &mov,
        &[
            "-c:v",
            "libx265",
            "-x265-params",
            "log-level=error",
            "-tag:v",
            "hvc1",
            "-c:a",
            "aac",
        ],
    );
    let rotated = dir.join("hevc-aac-rotated.mov");
    let status = Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-display_rotation:v:0",
            "-90",
            "-i",
        ])
        .arg(&mov)
        .args(["-c", "copy"])
        .arg(&rotated)
        .status()
        .expect("ffmpeg runs");
    assert!(status.success(), "ffmpeg rotated the MOV");
    let mkv = dir.join("h264-opus.mkv");
    make(ffmpeg, &mkv, &["-c:v", "libx264", "-c:a", "libopus"]);
    let webm = dir.join("vp9-opus.webm");
    make(
        ffmpeg,
        &webm,
        &[
            "-c:v",
            "libvpx-vp9",
            "-deadline",
            "realtime",
            "-cpu-used",
            "8",
            "-c:a",
            "libopus",
        ],
    );
    vec![(mp4, 0), (rotated, 1), (mkv, 0), (webm, 0)]
}

/// Asks the player until `done` holds, at most 20 s.
fn wait_for(player: &mut MpvPlayer, what: &str, done: impl Fn(&PlaybackState, bool) -> bool) -> PlaybackState {
    let start = Instant::now();
    loop {
        let state = player.state();
        let frame = player.frame(Some([160, 120])).is_some();
        if done(&state, frame) {
            return state;
        }
        assert!(state.error.is_none(), "{what}: {:?}", state.error);
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "{what}: timed out at {state:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn req_009_06_plays_all_four_containers_and_seeks() {
    let Some(mpv) = helper("libmpv", media::helpers::find_mpv()) else {
        return;
    };
    let Some(ffmpeg) = helper("ffmpeg", media::ffmpeg::find_ffmpeg()) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let mut player = MpvPlayer::new(&mpv, ctx, None, false).expect("libmpv loads");
    for (file, quarters) in fixtures(&ffmpeg, dir.path()) {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        player.load(&file);
        // Frames arrive and time passes.
        let state = wait_for(&mut player, &name, |state, frame| frame && state.position > 0.3);
        assert_eq!(state.video_size, Some([320, 240]), "{name}: decoded size, unrotated");
        assert_eq!(state.quarters, quarters, "{name}: rotation");
        assert!(
            (state.duration - 2.0).abs() < 0.2,
            "{name}: duration {}",
            state.duration
        );
        assert!(!state.paused, "{name} plays");
        // Paused, it stays put; an exact seek lands where it was asked to.
        player.set_paused(true);
        player.seek(1.5, true);
        let state = wait_for(&mut player, &name, |state, _| (state.position - 1.5).abs() < 0.05);
        assert!(state.paused, "{name} stays paused");
    }
    player.stop();
    assert!(player.frame(None).is_none(), "nothing shows after stop");
}

#[test]
fn req_009_06_says_why_a_file_cant_play() {
    let Some(mpv) = helper("libmpv", media::helpers::find_mpv()) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join("broken.mp4");
    std::fs::write(&broken, b"not a video").unwrap();
    let mut player = MpvPlayer::new(&mpv, egui::Context::default(), None, false).expect("libmpv loads");
    player.load(&broken);
    let start = Instant::now();
    loop {
        let state = player.state();
        if state.error.is_some() {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no error for a broken file: {state:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(player.frame(None).is_none());
}
