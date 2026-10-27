//! Single frames through ffmpeg: video thumbnails and AVIF images.

use domain::thumbs::MAX_THUMB_WIDTH;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// How long one frame may take before ffmpeg is killed.
const TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum FfmpegError {
    #[error("can't start ffmpeg ({path}): {source}")]
    Start { path: PathBuf, source: std::io::Error },
    #[error("ffmpeg didn't finish within {0:?}")]
    Timeout(Duration),
    #[error("ffmpeg extracted no frame: {0}")]
    NoFrame(String),
}

/// The ffmpeg executable: `GOOD_GALLERY_FFMPEG`, `ffmpeg\ffmpeg.exe` next to the app, or `ffmpeg` on `PATH`.
pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("GOOD_GALLERY_FFMPEG") {
        return Some(PathBuf::from(path));
    }
    let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("ffmpeg").join(name)))
        .filter(|path| path.is_file());
    bundled.or_else(|| {
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    })
}

/// The frame at `position` seconds as a PNG, at most as wide as the largest thumbnail. ffmpeg applies the file's
/// rotation itself. `file` must be absolute, so it can't be taken as an option (REQ-011-09).
pub fn extract_frame(ffmpeg: &Path, file: &Path, position: f64) -> Result<Vec<u8>, FfmpegError> {
    let mut command = Command::new(ffmpeg);
    command
        .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
        // Input seeking (before `-i`) reads only around the target frame.
        .args(["-ss", &format!("{position:.3}"), "-i"])
        .arg(file)
        .args([
            "-frames:v",
            "1",
            "-vf",
            &format!("scale='min({MAX_THUMB_WIDTH},iw)':-2"),
        ])
        .args(["-f", "image2pipe", "-c:v", "png", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|source| FfmpegError::Start {
        path: ffmpeg.to_path_buf(),
        source,
    })?;
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut frame = Vec::new();
        let _ = stdout.read_to_end(&mut frame);
        let _ = tx.send(frame);
    });
    let frame = match rx.recv_timeout(TIMEOUT) {
        Ok(frame) => frame,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(FfmpegError::Timeout(TIMEOUT));
        }
    };
    let _ = child.wait();
    let errors = errors.join().unwrap_or_default();
    if frame.is_empty() {
        return Err(FfmpegError::NoFrame(errors.trim().to_string()));
    }
    Ok(frame)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::thumbnail::tests::{average_color, sizes};
    use crate::thumbnail::{NoPreviews, Renderer};
    use domain::media::MediaKind;
    use std::sync::Arc;

    /// ffmpeg for tests, or `None` (test skipped) when it isn't installed. `GG_REQUIRE_HELPERS=1` makes a missing
    /// ffmpeg a failure instead (CI).
    pub(crate) fn ffmpeg() -> Option<PathBuf> {
        match find_ffmpeg() {
            Some(path) => Some(path),
            None if std::env::var_os("GG_REQUIRE_HELPERS").is_some() => panic!("ffmpeg not found"),
            None => {
                eprintln!("skipped: ffmpeg not found (set GOOD_GALLERY_FFMPEG or put it on PATH)");
                None
            }
        }
    }

    /// Runs ffmpeg to make a fixture; false if this build can't (e.g. no AV1 encoder).
    fn make(ffmpeg: &Path, args: &[&str], out: &Path) -> bool {
        Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(args)
            .arg(out)
            .status()
            .is_ok_and(|status| status.success())
    }

    fn renderer(ffmpeg: PathBuf) -> Renderer {
        Renderer::new(Arc::new(NoPreviews), Some(ffmpeg))
    }

    /// REQ-005-05: a clip that is red for 0.5 s, then blue: 10 % of 10 s is blue, the first frame red.
    #[test]
    fn req_005_05_shows_the_frame_at_a_tenth_of_the_duration() {
        let Some(ffmpeg) = ffmpeg() else { return };
        let dir = tempfile::tempdir().unwrap();
        let clip = dir.path().join("clip.mp4");
        let made = make(
            &ffmpeg,
            &[
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=320x240:d=0.5:r=25",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=320x240:d=9.5:r=25",
                "-filter_complex",
                "[0:v][1:v]concat=n=2:v=1[v]",
                "-map",
                "[v]",
                "-pix_fmt",
                "yuv420p",
            ],
            &clip,
        );
        assert!(made, "can't make the clip");
        let renderer = renderer(ffmpeg);

        let at_tenth = renderer.render(&clip, MediaKind::Video, Some(10.0)).unwrap();
        assert_eq!(sizes(&at_tenth), [(400, (320, 240)), (800, (320, 240))]);
        let [r, _, b] = average_color(&at_tenth);
        assert!(b > 200 && r < 50, "blue at 1 s: {r} {b}");

        let unknown = renderer.render(&clip, MediaKind::Video, None).unwrap();
        let [r, _, b] = average_color(&unknown);
        assert!(r > 200 && b < 50, "red at 0 s: {r} {b}");
    }

    #[test]
    fn renders_avif_images_through_ffmpeg() {
        let Some(ffmpeg) = ffmpeg() else { return };
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.avif");
        let source = ["-f", "lavfi", "-i", "color=c=green:s=300x200", "-frames:v", "1"];
        let made = ["libaom-av1", "libsvtav1"].into_iter().any(|codec| {
            make(
                &ffmpeg,
                &[&source[..], &["-c:v", codec, "-still-picture", "1"]].concat(),
                &file,
            )
        });
        if !made {
            eprintln!("skipped: this ffmpeg can't encode AVIF");
            return;
        }

        let thumbnails = renderer(ffmpeg).render(&file, MediaKind::Image, None).unwrap();

        assert_eq!(sizes(&thumbnails)[0], (400, (300, 200)));
        let [r, g, b] = average_color(&thumbnails);
        assert!(g > 100 && r < 50 && b < 50, "green: {r} {g} {b}");
    }

    #[test]
    fn a_file_without_frames_is_an_error() {
        let Some(ffmpeg) = ffmpeg() else { return };
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("broken.mp4");
        std::fs::write(&file, b"not a video").unwrap();
        assert!(matches!(
            extract_frame(&ffmpeg, &file, 0.0),
            Err(FfmpegError::NoFrame(_))
        ));
    }
}
