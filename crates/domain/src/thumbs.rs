//! Thumbnail widths (glossary: _Thumbnail_).

/// Widths thumbnails are rendered at, in pixels; part of the thumbnail cache's layout.
pub const THUMB_WIDTHS: [u32; 2] = [400, 800];
pub const MAX_THUMB_WIDTH: u32 = 800;
/// Thumbnails are at most this many times as tall as wide (the tiles crop taller strips anyway).
pub const MAX_THUMB_ASPECT: u32 = 3;
pub const MAX_THUMB_HEIGHT: u32 = MAX_THUMB_WIDTH * MAX_THUMB_ASPECT;
/// The ThumbHash is computed from the image fit into this square (glossary: _Placeholder_).
pub const THUMBHASH_SIZE: u32 = 100;
/// A video's thumbnail is its frame at this fraction of its duration (glossary: _Thumbnail_).
pub const VIDEO_FRAME_POSITION: f64 = 0.1;

/// `width` × `height` scaled to fit inside `max_width` × `max_height`, keeping the aspect ratio; only enlarged when
/// `enlarge` is set. Sides are at least 1 px.
pub fn fit_inside(width: u32, height: u32, max_width: u32, max_height: u32, enlarge: bool) -> (u32, u32) {
    let scale = (f64::from(max_width) / f64::from(width)).min(f64::from(max_height) / f64::from(height));
    let scale = if enlarge { scale } else { scale.min(1.0) };
    let side = |n: u32| ((f64::from(n) * scale).round() as u32).max(1);
    (side(width), side(height))
}

/// The thumbnail cache's name for a thumbnail: the SHA-1 of the key as JavaScript's
/// `JSON.stringify([rootId, relPath, size, mtime, width])` writes it, which `serde_json` matches for integers and
/// strings. Part of the cache's layout (REQ-005-15).
pub fn thumb_hash(key: &ThumbKey, width: u32) -> String {
    use sha1::{Digest, Sha1};
    let json = serde_json::json!([key.root_id, key.rel_path, key.size, key.mtime, width]).to_string();
    Sha1::digest(json.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

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
