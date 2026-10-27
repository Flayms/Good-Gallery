//! Video playback rules (REQ-009-06): seeking, volume, rotation and the size frames are rendered at.

/// ← / → seek this far while the video controls have focus (REQ-009-02).
pub const SEEK_STEP: f64 = 5.0;
/// Volume in percent, as mpv counts it.
pub const MAX_VOLUME: f64 = 100.0;

/// The position `delta` seconds from `position`, within the video; only kept above 0 while its duration isn't known
/// (0, e.g. still loading from a slow share).
pub fn seek_by(position: f64, delta: f64, duration: f64) -> f64 {
    let at = (position + delta).max(0.0);
    if duration > 0.0 { at.min(duration) } else { at }
}

/// ↑ / ↓ change the volume by this much while the video controls have focus.
pub const VOLUME_STEP: f64 = 5.0;

/// A volume kept within 0–100 %.
pub fn clamp_volume(volume: f64) -> f64 {
    volume.clamp(0.0, MAX_VOLUME)
}

/// The volume `steps` steps (± [`VOLUME_STEP`]) from `volume`.
pub fn step_volume(volume: f64, steps: i32) -> f64 {
    clamp_volume(volume + f64::from(steps) * VOLUME_STEP)
}

/// Quarter turns (0–3) of a video's rotation in degrees, as its metadata gives it (e.g. 90 for a phone held
/// upright), rounded to the nearest quarter.
pub fn quarter_turns(degrees: i64) -> u8 {
    ((degrees.rem_euclid(360) + 45) / 90 % 4) as u8
}

/// The size a video is shown at: its decoded size, sides swapped for a quarter or three-quarter turn.
pub fn shown_size([width, height]: [u32; 2], quarters: u8) -> [u32; 2] {
    if quarters % 2 == 1 {
        [height, width]
    } else {
        [width, height]
    }
}

/// The pixel size to render frames at: the size the unrotated frame takes on screen (`on_screen`, physical pixels),
/// but never larger than the video (`video`, decoded size), since the GPU scales up for free and the CPU renderer
/// pays for every pixel. Each side at least 2 px.
pub fn render_size(on_screen: [f32; 2], video: [u32; 2]) -> [u32; 2] {
    let [width, height] = video;
    let wanted = [
        on_screen[0].round().max(2.0) as u32,
        on_screen[1].round().max(2.0) as u32,
    ];
    if wanted[0] >= width || wanted[1] >= height {
        [width, height]
    } else {
        wanted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_009_02_seeks_within_the_video() {
        assert_eq!(seek_by(10.0, SEEK_STEP, 60.0), 15.0);
        assert_eq!(seek_by(3.0, -SEEK_STEP, 60.0), 0.0);
        assert_eq!(seek_by(58.0, SEEK_STEP, 60.0), 60.0);
        assert_eq!(seek_by(1.0, SEEK_STEP, 0.0), 6.0, "duration not known yet");
    }

    #[test]
    fn req_009_06_keeps_the_volume_between_0_and_100() {
        assert_eq!(clamp_volume(120.0), 100.0);
        assert_eq!(clamp_volume(-5.0), 0.0);
        assert_eq!(clamp_volume(42.0), 42.0);
    }

    #[test]
    fn req_009_06_steps_the_volume_in_5_percent_steps() {
        assert_eq!(step_volume(50.0, 1), 55.0);
        assert_eq!(step_volume(98.0, 1), 100.0);
        assert_eq!(step_volume(3.0, -1), 0.0);
    }

    #[test]
    fn req_009_06_turns_rotations_into_quarters_and_swaps_sides() {
        assert_eq!(quarter_turns(0), 0);
        assert_eq!(quarter_turns(90), 1);
        assert_eq!(quarter_turns(-90), 3);
        assert_eq!(quarter_turns(181), 2);
        assert_eq!(quarter_turns(359), 0);
        assert_eq!(shown_size([1920, 1080], 1), [1080, 1920]);
        assert_eq!(shown_size([1920, 1080], 2), [1920, 1080]);
    }

    #[test]
    fn req_009_06_renders_at_the_size_on_screen_but_never_larger_than_the_video() {
        assert_eq!(render_size([960.4, 540.0], [1920, 1080]), [960, 540]);
        assert_eq!(render_size([2500.0, 1400.0], [1920, 1080]), [1920, 1080]);
        assert_eq!(render_size([0.0, 0.0], [1920, 1080]), [2, 2]);
    }
}
