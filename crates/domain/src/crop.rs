//! The crop frame's drag logic (REQ-029-02, REQ-029-03). A frame is `x, y, w, h` as fractions of the whole photo as it
//! is turned on screen; `size` is that photo's size in pixels.

/// The smallest frame side, in pixels of the photo (REQ-029-02).
pub const MIN_SIDE: f32 = 16.0;

/// What a drag takes hold of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grip {
    Move,
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// A frame's shape (REQ-029-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ratio {
    #[default]
    Free,
    Original,
    Square,
    FourThree,
    SixteenNine,
}

impl Ratio {
    pub const ALL: [Self; 5] = [
        Self::Free,
        Self::Original,
        Self::Square,
        Self::FourThree,
        Self::SixteenNine,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Original => "Original",
            Self::Square => "1:1",
            Self::FourThree => "4:3",
            Self::SixteenNine => "16:9",
        }
    }

    /// Whether it has a landscape and a portrait form.
    pub fn can_swap(self) -> bool {
        matches!(self, Self::FourThree | Self::SixteenNine)
    }

    /// Width over height in pixels; `None` for [`Self::Free`].
    pub fn value(self, portrait: bool, size: [f32; 2]) -> Option<f32> {
        let long_over_short = match self {
            Self::Free => return None,
            Self::Original => return Some(size[0] / size[1]),
            Self::Square => 1.0,
            Self::FourThree => 4.0 / 3.0,
            Self::SixteenNine => 16.0 / 9.0,
        };
        Some(if portrait {
            1.0 / long_over_short
        } else {
            long_over_short
        })
    }
}

/// Whether a frame is taller than wide on screen.
pub fn is_portrait(frame: [f32; 4], size: [f32; 2]) -> bool {
    frame[3] * size[1] > frame[2] * size[0]
}

/// The grip at `point` (fractions of the photo); `tolerance` is how far from an edge still counts, in fractions of the
/// photo's width and height. `None` outside the frame.
pub fn grip_at(frame: [f32; 4], point: [f32; 2], tolerance: [f32; 2]) -> Option<Grip> {
    let [x, y, w, h] = frame;
    let near = |value: f32, edge: f32, tolerance: f32| (value - edge).abs() <= tolerance;
    let within = point[0] >= x - tolerance[0]
        && point[0] <= x + w + tolerance[0]
        && point[1] >= y - tolerance[1]
        && point[1] <= y + h + tolerance[1];
    if !within {
        return None;
    }
    let left = near(point[0], x, tolerance[0]);
    let right = near(point[0], x + w, tolerance[0]);
    let top = near(point[1], y, tolerance[1]);
    let bottom = near(point[1], y + h, tolerance[1]);
    Some(match (left, right, top, bottom) {
        (true, _, true, _) => Grip::TopLeft,
        (_, true, true, _) => Grip::TopRight,
        (true, _, _, true) => Grip::BottomLeft,
        (_, true, _, true) => Grip::BottomRight,
        (true, ..) => Grip::Left,
        (_, true, ..) => Grip::Right,
        (_, _, true, _) => Grip::Top,
        (_, _, _, true) => Grip::Bottom,
        _ => Grip::Move,
    })
}

/// `frame` with `grip` dragged by `delta` (fractions of the photo): it stays on the photo, keeps `ratio` (width over
/// height in pixels) if it has one, and doesn't get smaller than [`MIN_SIDE`] pixels.
pub fn drag(frame: [f32; 4], grip: Grip, delta: [f32; 2], ratio: Option<f32>, size: [f32; 2]) -> [f32; 4] {
    let [x, y, w, h] = frame;
    if grip == Grip::Move {
        return [
            (x + delta[0]).clamp(0.0, 1.0 - w),
            (y + delta[1]).clamp(0.0, 1.0 - h),
            w,
            h,
        ];
    }
    let (min_w, min_h) = (min_side(size[0]), min_side(size[1]));
    let (moves_left, moves_right, moves_top, moves_bottom) = match grip {
        Grip::Top => (false, false, true, false),
        Grip::Bottom => (false, false, false, true),
        Grip::Left => (true, false, false, false),
        Grip::Right => (false, true, false, false),
        Grip::TopLeft => (true, false, true, false),
        Grip::TopRight => (false, true, true, false),
        Grip::BottomLeft => (true, false, false, true),
        Grip::BottomRight => (false, true, false, true),
        Grip::Move => (false, false, false, false),
    };
    let (mut left, mut right, mut top, mut bottom) = (x, x + w, y, y + h);
    if moves_left {
        left = (left + delta[0]).clamp(0.0, right - min_w);
    }
    if moves_right {
        right = (right + delta[0]).clamp(left + min_w, 1.0);
    }
    if moves_top {
        top = (top + delta[1]).clamp(0.0, bottom - min_h);
    }
    if moves_bottom {
        bottom = (bottom + delta[1]).clamp(top + min_h, 1.0);
    }
    let Some(ratio) = ratio else {
        return [left, top, right - left, bottom - top];
    };
    // The ratio in fractions of the photo: width over height.
    let shape = ratio * size[1] / size[0];
    // How much room each side has, from what holds still: the opposite corner, or an edge's center.
    let (center_x, center_y) = (x + w / 2.0, y + h / 2.0);
    let room_w = match (moves_left, moves_right) {
        (true, _) => right,
        (_, true) => 1.0 - left,
        _ => 2.0 * center_x.min(1.0 - center_x),
    };
    let room_h = match (moves_top, moves_bottom) {
        (true, _) => bottom,
        (_, true) => 1.0 - top,
        _ => 2.0 * center_y.min(1.0 - center_y),
    };
    // A corner goes by whichever side was dragged further, an edge by the side it moves.
    let wanted_w = match (moves_left || moves_right, moves_top || moves_bottom) {
        (true, true) => (right - left).max((bottom - top) * shape),
        (true, false) => right - left,
        _ => (bottom - top) * shape,
    };
    let width = wanted_w.min(room_w).min(room_h * shape).max(min_w.min(room_w));
    let height = width / shape;
    let new_x = match (moves_left, moves_right) {
        (true, _) => right - width,
        (_, true) => left,
        _ => center_x - width / 2.0,
    };
    let new_y = match (moves_top, moves_bottom) {
        (true, _) => bottom - height,
        (_, true) => top,
        _ => center_y - height / 2.0,
    };
    [new_x, new_y, width, height]
}

/// The largest frame of `ratio` centered in `frame`, not smaller than [`MIN_SIDE`] pixels where the photo has the room.
pub fn fit_ratio(frame: [f32; 4], ratio: f32, size: [f32; 2]) -> [f32; 4] {
    let shape = ratio * size[1] / size[0];
    let [x, y, w, h] = frame;
    let width = w.min(h * shape);
    around_center([x + w / 2.0, y + h / 2.0], width, shape, size)
}

/// `frame` with its pixel sides swapped around its center, shrunk to stay on the photo (REQ-029-03, _Swap_).
pub fn swap_sides(frame: [f32; 4], size: [f32; 2]) -> [f32; 4] {
    let [x, y, w, h] = frame;
    let (pixel_w, pixel_h) = (w * size[0], h * size[1]);
    let shape = (pixel_h / pixel_w) * size[1] / size[0];
    around_center([x + w / 2.0, y + h / 2.0], pixel_h / size[0], shape, size)
}

/// A frame of `shape` (width over height, in fractions) as wide as `width`, centered on `center` as far as the photo
/// lets it, and only as large as fits on the photo.
fn around_center(center: [f32; 2], width: f32, shape: f32, size: [f32; 2]) -> [f32; 4] {
    let smallest = min_side(size[0]).max(min_side(size[1]) * shape);
    let width = width.max(smallest).min(1.0).min(shape);
    let height = width / shape;
    [
        (center[0] - width / 2.0).clamp(0.0, 1.0 - width),
        (center[1] - height / 2.0).clamp(0.0, 1.0 - height),
        width,
        height,
    ]
}

/// [`MIN_SIDE`] pixels as a fraction of a side, at most the whole side.
fn min_side(pixels: f32) -> f32 {
    (MIN_SIDE / pixels.max(1.0)).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: [f32; 2] = [1000.0, 500.0];

    fn near(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4)
    }

    #[test]
    fn req_029_02_edges_and_corners_resize_and_inside_moves() {
        let frame = [0.2, 0.2, 0.4, 0.4];
        let at = |grip, delta| drag(frame, grip, delta, None, SIZE);
        assert!(near(at(Grip::Right, [0.1, 0.5]), [0.2, 0.2, 0.5, 0.4]));
        assert!(near(at(Grip::Left, [0.1, 0.5]), [0.3, 0.2, 0.3, 0.4]));
        assert!(near(at(Grip::Top, [0.5, 0.1]), [0.2, 0.3, 0.4, 0.3]));
        assert!(near(at(Grip::Bottom, [0.5, 0.1]), [0.2, 0.2, 0.4, 0.5]));
        assert!(near(at(Grip::TopLeft, [-0.1, -0.1]), [0.1, 0.1, 0.5, 0.5]));
        assert!(near(at(Grip::BottomRight, [0.1, 0.1]), [0.2, 0.2, 0.5, 0.5]));
        assert!(near(at(Grip::Move, [0.1, 0.1]), [0.3, 0.3, 0.4, 0.4]));
    }

    #[test]
    fn req_029_02_the_frame_stays_on_the_photo_and_above_the_smallest_size() {
        let frame = [0.2, 0.2, 0.4, 0.4];
        assert!(near(
            drag(frame, Grip::Move, [-1.0, 1.0], None, SIZE),
            [0.0, 0.6, 0.4, 0.4]
        ));
        assert!(near(
            drag(frame, Grip::TopLeft, [-1.0, -1.0], None, SIZE),
            [0.0, 0.0, 0.6, 0.6]
        ));
        let small = drag(frame, Grip::Right, [-1.0, 0.0], None, SIZE);
        assert!(near(small, [0.2, 0.2, 16.0 / 1000.0, 0.4]), "{small:?}");
        let flat = drag(frame, Grip::Bottom, [0.0, -1.0], None, SIZE);
        assert!(near(flat, [0.2, 0.2, 0.4, 16.0 / 500.0]), "{flat:?}");
        // A photo smaller than the smallest frame: the frame is the whole side.
        let tiny = drag([0.0, 0.0, 1.0, 1.0], Grip::Right, [-1.0, 0.0], None, [10.0, 10.0]);
        assert!(near(tiny, [0.0, 0.0, 1.0, 1.0]), "{tiny:?}");
    }

    #[test]
    fn req_029_02_finds_what_a_press_takes_hold_of() {
        let frame = [0.2, 0.2, 0.4, 0.4];
        let at = |point| grip_at(frame, point, [0.01, 0.01]);
        assert_eq!(at([0.2, 0.2]), Some(Grip::TopLeft));
        assert_eq!(at([0.6, 0.6]), Some(Grip::BottomRight));
        assert_eq!(at([0.2, 0.4]), Some(Grip::Left));
        assert_eq!(at([0.4, 0.6]), Some(Grip::Bottom));
        assert_eq!(at([0.4, 0.4]), Some(Grip::Move));
        assert_eq!(at([0.9, 0.9]), None);
    }

    fn pixel_ratio(frame: [f32; 4]) -> f32 {
        frame[2] * SIZE[0] / (frame[3] * SIZE[1])
    }

    #[test]
    fn req_029_03_a_ratio_gives_the_largest_centered_frame() {
        // 400 × 200 pixels: a square of 200 in its middle.
        let frame = fit_ratio([0.2, 0.2, 0.4, 0.4], 1.0, SIZE);
        assert!(near(frame, [0.3, 0.2, 0.2, 0.4]), "{frame:?}");
        assert!(Ratio::Free.value(false, SIZE).is_none());
        assert_eq!(Ratio::Original.value(true, SIZE), Some(2.0));
        assert_eq!(Ratio::FourThree.value(false, SIZE), Some(4.0 / 3.0));
        assert_eq!(Ratio::FourThree.value(true, SIZE), Some(3.0 / 4.0));
        assert_eq!(Ratio::SixteenNine.value(true, SIZE), Some(9.0 / 16.0));
        assert!(is_portrait([0.0, 0.0, 0.1, 0.9], SIZE));
        assert!(!is_portrait([0.0, 0.0, 0.5, 0.5], SIZE));
        assert!(Ratio::FourThree.can_swap() && !Ratio::Square.can_swap());
    }

    #[test]
    fn req_029_03_a_ratio_is_kept_while_resizing() {
        let frame = fit_ratio([0.2, 0.2, 0.4, 0.4], 16.0 / 9.0, SIZE);
        for (grip, delta) in [
            (Grip::BottomRight, [0.1, 0.0]),
            (Grip::TopLeft, [0.0, -0.1]),
            (Grip::Right, [0.1, 0.0]),
            (Grip::Top, [0.0, -0.1]),
            (Grip::BottomRight, [5.0, 5.0]),
            (Grip::TopLeft, [5.0, 5.0]),
        ] {
            let dragged = drag(frame, grip, delta, Some(16.0 / 9.0), SIZE);
            assert!((pixel_ratio(dragged) - 16.0 / 9.0).abs() < 1e-3, "{grip:?} {dragged:?}");
            let [x, y, w, h] = dragged;
            assert!(
                x >= -1e-5 && y >= -1e-5 && x + w <= 1.0001 && y + h <= 1.0001,
                "{grip:?} {dragged:?}"
            );
        }
    }

    #[test]
    fn req_029_03_swap_turns_the_shape_around_and_back() {
        let frame = fit_ratio([0.2, 0.2, 0.4, 0.4], 4.0 / 3.0, SIZE);
        let swapped = swap_sides(frame, SIZE);
        assert!((pixel_ratio(swapped) - 3.0 / 4.0).abs() < 1e-3, "{swapped:?}");
        assert!(near(swap_sides(swapped, SIZE), frame), "swapping back loses nothing");
        // Where the photo has no room, it shrinks to stay on it.
        let tall = swap_sides([0.0, 0.0, 1.0, 0.5], SIZE);
        assert!(tall[1] >= 0.0 && tall[1] + tall[3] <= 1.0001, "{tall:?}");
    }
}
