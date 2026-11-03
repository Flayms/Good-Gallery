//! Face regions embedded in photos (REQ-004-13, REQ-009-17): where each named person is, relative to the photo.

/// A named region on the upright photo: the top-left corner and size, each a fraction of the photo's width or height.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceRegion {
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// How far a region may reach past the photo's edges (rounding in the apps that wrote it) before it counts as broken.
const SLACK: f32 = 0.01;

impl FaceRegion {
    /// A region from its top-left corner and size on the stored photo, turned upright by the EXIF `orientation` (1–8,
    /// others as 1). Clamped to the photo; `None` for a region without a name, area or place on the photo.
    pub fn new(name: &str, x: f32, y: f32, w: f32, h: f32, orientation: u8) -> Option<Self> {
        let name = name.trim();
        let finite = [x, y, w, h].iter().all(|n| n.is_finite());
        if name.is_empty() || !finite || w <= 0.0 || h <= 0.0 {
            return None;
        }
        let inside = |start: f32, size: f32| start >= -SLACK && start + size <= 1.0 + SLACK;
        if !inside(x, w) || !inside(y, h) {
            return None;
        }
        let [x, y, w, h] = upright_rect([x, y, w, h], orientation);
        Some(Self {
            name: name.to_string(),
            x,
            y,
            w,
            h,
        })
    }

    /// A region of the MWG struct: its center and size (`stArea`).
    pub fn from_center(name: &str, cx: f32, cy: f32, w: f32, h: f32, orientation: u8) -> Option<Self> {
        Self::new(name, cx - w / 2.0, cy - h / 2.0, w, h, orientation)
    }
}

/// A rectangle of the stored photo (`x, y, w, h`, fractions of its size) on the upright photo, clamped to it.
pub fn upright_rect([x, y, w, h]: [f32; 4], orientation: u8) -> [f32; 4] {
    let (left, top) = (x.max(0.0), y.max(0.0));
    let (right, bottom) = ((x + w).min(1.0), (y + h).min(1.0));
    let (ax, ay) = upright_point(left, top, orientation);
    let (bx, by) = upright_point(right, bottom, orientation);
    [ax.min(bx), ay.min(by), (ax - bx).abs(), (ay - by).abs()]
}

/// A rectangle of the upright photo on the stored photo, as a file's regions need it (REQ-031-14).
pub fn stored_rect(rect: [f32; 4], orientation: u8) -> [f32; 4] {
    let inverse = match orientation {
        6 => 8,
        8 => 6,
        other => other,
    };
    upright_rect(rect, inverse)
}

/// Where a point of the stored photo (fractions of its width and height) lies on the upright photo.
fn upright_point(u: f32, v: f32, orientation: u8) -> (f32, f32) {
    match orientation {
        2 => (1.0 - u, v),
        3 => (1.0 - u, 1.0 - v),
        4 => (u, 1.0 - v),
        5 => (v, u),
        6 => (1.0 - v, u),
        7 => (1.0 - v, 1.0 - u),
        8 => (v, 1.0 - u),
        _ => (u, v),
    }
}

/// The setting that remembers whether the viewer shows face regions.
pub const FACES_KEY: &str = "viewer.faces";

/// Whether the stored choice shows face regions; they are hidden until the user shows them.
pub fn are_faces_shown(stored: Option<&str>) -> bool {
    stored == Some("shown")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(region: &FaceRegion) -> [f32; 4] {
        [region.x, region.y, region.w, region.h].map(|n| (n * 100.0).round() / 100.0)
    }

    /// REQ-004-13: a region in the top-left area of the stored photo, 0.1 from the left and 0.2 from the top, 0.3
    /// wide and 0.4 high, lands where the turned photo shows that corner.
    #[test]
    fn req_004_13_turns_regions_upright_for_every_orientation() {
        let expected = [
            (1, [0.1, 0.2, 0.3, 0.4]),
            (2, [0.6, 0.2, 0.3, 0.4]),
            (3, [0.6, 0.4, 0.3, 0.4]),
            (4, [0.1, 0.4, 0.3, 0.4]),
            (5, [0.2, 0.1, 0.4, 0.3]),
            (6, [0.4, 0.1, 0.4, 0.3]),
            (7, [0.4, 0.6, 0.4, 0.3]),
            (8, [0.2, 0.6, 0.4, 0.3]),
            (0, [0.1, 0.2, 0.3, 0.4]),
        ];
        for (orientation, upright) in expected {
            let region = FaceRegion::new("Anna", 0.1, 0.2, 0.3, 0.4, orientation).unwrap();
            assert_eq!(rect(&region), upright, "orientation {orientation}");
        }
    }

    #[test]
    fn req_004_13_reads_mwg_centers_and_leaves_out_broken_regions() {
        let centered = FaceRegion::from_center(" Anna ", 0.5, 0.5, 0.2, 0.4, 1).unwrap();
        assert_eq!(
            (centered.name.as_str(), rect(&centered)),
            ("Anna", [0.4, 0.3, 0.2, 0.4])
        );
        // Slightly past the edge is clamped.
        assert_eq!(
            rect(&FaceRegion::new("A", -0.005, 0.0, 0.5, 1.005, 1).unwrap()),
            [0.0, 0.0, 0.5, 1.0]
        );

        assert_eq!(FaceRegion::new("", 0.1, 0.1, 0.2, 0.2, 1), None);
        assert_eq!(FaceRegion::new("A", 0.1, 0.1, 0.0, 0.2, 1), None);
        assert_eq!(FaceRegion::new("A", 0.9, 0.1, 0.2, 0.2, 1), None);
        assert_eq!(FaceRegion::new("A", f32::NAN, 0.1, 0.2, 0.2, 1), None);
        assert_eq!(FaceRegion::from_center("A", 1.5, 0.5, 0.2, 0.2, 1), None);
    }

    #[test]
    fn req_031_14_stored_rect_undoes_turning_upright() {
        let stored = [0.1, 0.2, 0.3, 0.4];
        for orientation in 1..=8 {
            let back = stored_rect(upright_rect(stored, orientation), orientation);
            for (a, b) in back.iter().zip(stored) {
                assert!((a - b).abs() < 1e-5, "orientation {orientation}: {back:?}");
            }
        }
    }

    #[test]
    fn req_009_17_face_regions_are_hidden_until_shown() {
        assert!(are_faces_shown(Some("shown")));
        assert!(!are_faces_shown(Some("hidden")));
        assert!(!are_faces_shown(None));
    }
}
