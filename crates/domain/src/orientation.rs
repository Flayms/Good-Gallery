//! Turning a photo by its EXIF orientation tag alone (REQ-009-22): which tag a quarter turn gives, and which formats
//! carry one.

/// A quarter turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    Left,
    Right,
}

/// The EXIF orientations 1–8 as (clockwise quarter turns, mirrored): the stored pixels are mirrored first, if at all,
/// then turned.
const ORIENTATIONS: [(u8, (u8, bool)); 8] = [
    (1, (0, false)),
    (6, (1, false)),
    (3, (2, false)),
    (8, (3, false)),
    (2, (0, true)),
    (7, (1, true)),
    (4, (2, true)),
    (5, (3, true)),
];

/// The orientation tag of a photo with `orientation` (an unknown value counts as 1) turned once by `turn`.
pub fn turned(orientation: u8, turn: Turn) -> u8 {
    let (quarters, mirrored) = ORIENTATIONS
        .iter()
        .find(|(tag, _)| *tag == orientation)
        .map_or((0, false), |(_, turn)| *turn);
    let quarters = match turn {
        Turn::Right => (quarters + 1) % 4,
        Turn::Left => (quarters + 3) % 4,
    };
    ORIENTATIONS
        .iter()
        .find(|(_, turn)| *turn == (quarters, mirrored))
        .map_or(1, |(tag, _)| *tag)
}

/// Whether a photo of this file name can be turned by its orientation tag: its format has one that exiftool writes
/// and the app reads.
pub fn can_turn(file_name: &str) -> bool {
    const TURNABLE: [&str; 4] = [".jpg", ".jpeg", ".png", ".webp"];
    crate::media::media_kind(file_name) == Some(crate::media::MediaKind::Image)
        && file_name
            .rfind('.')
            .is_some_and(|dot| TURNABLE.contains(&file_name[dot..].to_ascii_lowercase().as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_009_22_turns_a_quarter_and_keeps_the_mirror() {
        assert_eq!(turned(1, Turn::Right), 6);
        assert_eq!(turned(6, Turn::Right), 3);
        assert_eq!(turned(3, Turn::Right), 8);
        assert_eq!(turned(8, Turn::Right), 1);
        assert_eq!(turned(1, Turn::Left), 8);
        assert_eq!(turned(6, Turn::Left), 1);
        // Mirrored: transverse (7) is the mirror turned right, transpose (5) the mirror turned left.
        assert_eq!(turned(2, Turn::Right), 7);
        assert_eq!(turned(2, Turn::Left), 5);
        assert_eq!(turned(4, Turn::Right), 5);
        // Unknown or missing values are upright.
        assert_eq!(turned(0, Turn::Right), 6);
        assert_eq!(turned(42, Turn::Left), 8);
        for orientation in 1..=8 {
            let mut right = orientation;
            for _ in 0..4 {
                right = turned(right, Turn::Right);
            }
            assert_eq!(right, orientation, "four turns are none");
            assert_eq!(turned(turned(orientation, Turn::Left), Turn::Right), orientation);
        }
    }

    #[test]
    fn req_009_21_only_formats_with_an_orientation_turn() {
        for name in ["a.jpg", "b.JPEG", "d.png", "e.webp"] {
            assert!(can_turn(name), "{name}");
        }
        // AVIF is decoded by ffmpeg, which goes by its own boxes, not the EXIF tag.
        for name in ["a.gif", "b.bmp", "c.avif", "d.mp4", "noext", ".jpg"] {
            assert!(!can_turn(name), "{name}");
        }
    }
}
