//! Thumbnail widths (glossary: _Thumbnail_).

/// Widths thumbnails are rendered at, in pixels; shared with the Electron version's cache.
pub const THUMB_WIDTHS: [u32; 2] = [400, 800];
pub const MAX_THUMB_WIDTH: u32 = 800;
/// Thumbnails are at most this many times as tall as wide (the tiles crop taller strips anyway).
pub const MAX_THUMB_ASPECT: u32 = 3;

/// The file state a thumbnail was rendered from; any change gives it a new place in the cache (REQ-005-10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbKey {
    pub root_id: i64,
    pub rel_path: String,
    pub size: i64,
    /// Milliseconds since the Unix epoch.
    pub mtime: i64,
}

/// The smallest thumbnail at least as wide as the tile's physical pixels, else the largest (REQ-005-02).
pub fn pick_thumb_width(points: f32, pixels_per_point: f32) -> u32 {
    let needed = points * pixels_per_point;
    THUMB_WIDTHS
        .into_iter()
        .find(|&width| width as f32 >= needed)
        .unwrap_or(MAX_THUMB_WIDTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_005_02_picks_the_smallest_thumbnail_covering_the_physical_width() {
        assert_eq!(pick_thumb_width(300.0, 1.0), 400);
        assert_eq!(pick_thumb_width(400.0, 1.0), 400);
        assert_eq!(pick_thumb_width(300.0, 1.5), 800);
        assert_eq!(pick_thumb_width(900.0, 2.0), 800);
    }
}
