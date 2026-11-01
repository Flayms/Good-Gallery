//! Rating an item by hand (spec 024): what a key or a star sets, and where the rating is written.

use crate::media::MediaKind;

/// The most stars an item can have.
pub const MAX: i64 = 5;

/// The rating after the user pressed `stars` (a key or a star): that many stars, or none when the item already has
/// exactly that many (REQ-024-01, REQ-024-02).
pub fn toggled(current: Option<i64>, stars: i64) -> Option<i64> {
    let stars = stars.min(MAX);
    (stars > 0 && current != Some(stars)).then_some(stars)
}

/// Where a rating is written (REQ-024-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Into the file itself.
    File,
    /// Into its sidecar: the existing one, else a new `<file name>.xmp`.
    Sidecar,
}

impl Target {
    /// Photos are written in place. Videos get a sidecar, unless the file carries a rating of its own, which would be
    /// read before the sidecar's (REQ-004-05).
    pub fn of(kind: MediaKind, file_has_rating: bool) -> Self {
        match kind {
            MediaKind::Video if !file_has_rating => Self::Sidecar,
            _ => Self::File,
        }
    }
}

/// The name of a new sidecar for `file_name`: the darktable / digiKam form, which can't be mistaken for the sidecar of
/// another file with the same stem.
pub fn sidecar_name(file_name: &str) -> String {
    format!("{file_name}.xmp")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ-024-01, REQ-024-02: a key or a star sets that many stars; the current rating's removes it.
    #[test]
    fn req_024_01_req_024_02_sets_the_stars_or_removes_the_same_rating() {
        assert_eq!(toggled(None, 3), Some(3));
        assert_eq!(toggled(Some(1), 5), Some(5));
        assert_eq!(toggled(Some(4), 2), Some(2));
        assert_eq!(toggled(Some(4), 4), None);
        assert_eq!(toggled(None, 0), None);
        assert_eq!(toggled(None, 9), Some(MAX));
    }

    /// REQ-024-03: photos in place, videos in a sidecar unless the video has a rating itself.
    #[test]
    fn req_024_03_writes_photos_in_place_and_videos_beside_them() {
        assert_eq!(Target::of(MediaKind::Image, false), Target::File);
        assert_eq!(Target::of(MediaKind::Image, true), Target::File);
        assert_eq!(Target::of(MediaKind::Video, false), Target::Sidecar);
        assert_eq!(Target::of(MediaKind::Video, true), Target::File);
        assert_eq!(sidecar_name("clip.MP4"), "clip.MP4.xmp");
    }
}
