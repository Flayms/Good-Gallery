//! Flips, straightening, crop and quarter turns (spec 029).

/// A crop inside the photo; `None` for an invalid one and for the whole photo (REQ-011-10).
pub(super) fn sanitized_crop([x, y, w, h]: [f32; 4]) -> Option<[f32; 4]> {
    if ![x, y, w, h].iter().all(|n| n.is_finite()) || !(0.0..1.0).contains(&x) || !(0.0..1.0).contains(&y) {
        return None;
    }
    let (w, h) = (w.min(1.0 - x), h.min(1.0 - y));
    let whole = x == 0.0 && y == 0.0 && w >= 1.0 && h >= 1.0;
    (w > 0.0 && h > 0.0 && !whole).then_some([x, y, w, h])
}

/// The steepest straightening either way, in degrees (REQ-029-10).
pub const MAX_STRAIGHTEN: f32 = 100.0;

/// A recipe's flips, straightening, crop and quarter turns: how the rendered photo becomes the output (spec 029). The
/// upright original is flipped, then straightened about its center onto a canvas the size of the turned photo's
/// bounding box, then cropped on that canvas, then turned by quarter turns. Without flips and straightening the canvas
/// is the original itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    crop: [f32; 4],
    quarter_turns: u8,
    /// Degrees clockwise, within ±[`MAX_STRAIGHTEN`].
    straighten: f32,
    /// Mirrored left to right, top to bottom, before straightening.
    flip: [bool; 2],
    /// The upright original's width over its height; only straightening needs it.
    aspect: f32,
}

impl Geometry {
    pub fn new(crop: Option<[f32; 4]>, quarter_turns: u8) -> Self {
        Self {
            crop: crop.and_then(sanitized_crop).unwrap_or([0.0, 0.0, 1.0, 1.0]),
            quarter_turns: quarter_turns % 4,
            straighten: 0.0,
            flip: [false; 2],
            aspect: 1.0,
        }
    }

    /// Straightened by `degrees` clockwise, for an original `aspect` wide per unit of height (REQ-029-10).
    pub fn with_straighten(mut self, degrees: f32, aspect: f32) -> Self {
        self.straighten = sanitized_straighten(degrees);
        self.aspect = sanitized_aspect(aspect);
        self
    }

    /// Mirrored as the original, before straightening (REQ-029-11).
    pub fn with_flips(mut self, horizontal: bool, vertical: bool) -> Self {
        self.flip = [horizontal, vertical];
        self
    }

    /// The same, without its crop: the whole canvas.
    pub fn uncropped(mut self) -> Self {
        self.crop = [0.0, 0.0, 1.0, 1.0];
        self
    }

    pub fn is_identity(&self) -> bool {
        self.quarter_turns == 0 && self.crop == [0.0, 0.0, 1.0, 1.0] && !self.transforms()
    }

    /// Whether flips or straightening move the pixels about, beyond a crop and quarter turns.
    pub fn transforms(&self) -> bool {
        self.straighten != 0.0 || self.flip.contains(&true)
    }

    /// The crop as `x, y, w, h`, fractions of the canvas (the upright, unturned original without straightening); the
    /// whole canvas without one.
    pub fn crop(&self) -> [f32; 4] {
        self.crop
    }

    pub fn quarter_turns(&self) -> u8 {
        self.quarter_turns
    }

    pub fn straighten(&self) -> f32 {
        self.straighten
    }

    pub fn flips(&self) -> [bool; 2] {
        self.flip
    }

    pub fn aspect(&self) -> f32 {
        self.aspect
    }

    /// The cosine and sine of the straightening.
    fn turn(&self) -> (f32, f32) {
        let radians = self.straighten.to_radians();
        (radians.cos(), radians.sin())
    }

    /// The canvas's size per the original's: its width per the original's width, its height per its height.
    pub fn canvas_scale(&self) -> [f32; 2] {
        if self.straighten == 0.0 {
            return [1.0, 1.0];
        }
        let (c, s) = self.turn();
        let (c, s) = (c.abs(), s.abs());
        [c + s / self.aspect, s * self.aspect + c]
    }

    /// The canvas's size in pixels for an upright original of this size.
    pub fn canvas_size(&self, width: u32, height: u32) -> (u32, u32) {
        let [sx, sy] = self.canvas_scale();
        (
            ((width as f32 * sx).round() as u32).max(1),
            ((height as f32 * sy).round() as u32).max(1),
        )
    }

    /// Where a point of the original (fractions of its size) lies on the canvas.
    pub fn to_canvas(&self, [u, v]: [f32; 2]) -> [f32; 2] {
        let u = if self.flip[0] { 1.0 - u } else { u };
        let v = if self.flip[1] { 1.0 - v } else { v };
        if self.straighten == 0.0 {
            return [u, v];
        }
        let (c, s) = self.turn();
        let [sx, sy] = self.canvas_scale();
        // In units of the original's height, about its center.
        let (x, y) = ((u - 0.5) * self.aspect, v - 0.5);
        let (x, y) = (x * c - y * s, x * s + y * c);
        [x / (self.aspect * sx) + 0.5, y / sy + 0.5]
    }

    /// Where a point of the canvas lies on the original; outside 0–1 where the canvas shows none of it.
    pub fn from_canvas(&self, [u, v]: [f32; 2]) -> [f32; 2] {
        let [u, v] = if self.straighten == 0.0 {
            [u, v]
        } else {
            let (c, s) = self.turn();
            let [sx, sy] = self.canvas_scale();
            let (x, y) = ((u - 0.5) * self.aspect * sx, (v - 0.5) * sy);
            let (x, y) = (x * c + y * s, -x * s + y * c);
            [x / self.aspect + 0.5, y + 0.5]
        };
        [
            if self.flip[0] { 1.0 - u } else { u },
            if self.flip[1] { 1.0 - v } else { v },
        ]
    }

    /// Whether a point of the canvas shows the photo, not the empty corners around a straightened one.
    pub fn shows_photo(&self, point: [f32; 2]) -> bool {
        const SLACK: f32 = 1e-4;
        self.from_canvas(point)
            .iter()
            .all(|side| (-SLACK..=1.0 + SLACK).contains(side))
    }

    /// Whether a point of the original (fractions of its size, as [`Self::to_original`] gives it) is on the photo.
    pub fn shows_photo_at(&self, point: [f32; 2]) -> bool {
        const SLACK: f32 = 1e-3;
        point.iter().all(|side| (-SLACK..=1.0 + SLACK).contains(side))
    }

    /// Whether all of a frame on the whole canvas as it is turned on screen shows the photo (REQ-029-10).
    pub fn frame_shows_photo(&self, [x, y, w, h]: [f32; 4]) -> bool {
        [[x, y], [x + w, y], [x, y + h], [x + w, y + h]]
            .into_iter()
            .all(|corner| self.shows_photo(unturn(corner, self.quarter_turns)))
    }

    /// The largest crop of `ratio` (width per height in pixels, on the unturned canvas), centered, that shows only the
    /// photo (REQ-029-10).
    pub fn largest_crop(&self, ratio: f32) -> [f32; 4] {
        let ratio = if ratio.is_finite() && ratio > 0.0 {
            ratio
        } else {
            self.aspect
        };
        let (c, s) = self.turn();
        let (c, s) = (c.abs(), s.abs());
        let [sx, sy] = self.canvas_scale();
        // Half sizes in units of the original's height: corners turned back must stay within its half sizes.
        let (half_w, half_h) = (self.aspect / 2.0, 0.5);
        let a = (half_w / (c + s / ratio)).min(half_h / (s + c / ratio));
        let b = a / ratio;
        // Within rounding of the whole side, the whole side.
        let side = |n: f32| if n > 1.0 - 1e-4 { 1.0 } else { n };
        let (w, h) = (side(2.0 * a / (self.aspect * sx)), side(2.0 * b / sy));
        [(1.0 - w) / 2.0, (1.0 - h) / 2.0, w, h]
    }

    /// The crop's width per height in pixels, on the unturned canvas.
    pub fn crop_ratio(&self) -> f32 {
        let [sx, sy] = self.canvas_scale();
        (self.crop[2] * self.aspect * sx) / (self.crop[3] * sy)
    }

    /// The crop in pixels of the canvas of an upright original of this size: `x, y, w, h`, at least one pixel and
    /// inside it.
    pub fn crop_pixels(&self, width: u32, height: u32) -> [u32; 4] {
        let (width, height) = self.canvas_size(width, height);
        let side = |start: f32, size: f32, whole: u32| {
            let whole_f = whole as f32;
            let first = ((start * whole_f).round() as u32).min(whole.saturating_sub(1));
            let end = (((start + size) * whole_f).round() as u32).clamp(first + 1, whole.max(first + 1));
            (first, end - first)
        };
        let (x, w) = side(self.crop[0], self.crop[2], width);
        let (y, h) = side(self.crop[1], self.crop[3], height);
        [x, y, w, h]
    }

    /// The size of the output for an upright original of this size.
    pub fn output_size(&self, width: u32, height: u32) -> (u32, u32) {
        let [_, _, w, h] = self.crop_pixels(width, height);
        if self.quarter_turns % 2 == 1 { (h, w) } else { (w, h) }
    }

    /// The crop as a frame on the whole canvas as it is turned on screen (`domain::crop`).
    pub fn frame(&self) -> [f32; 4] {
        let [x, y, w, h] = self.crop;
        let a = turn(self.quarter_turns, [x, y]);
        let b = turn(self.quarter_turns, [x + w, y + h]);
        [a[0].min(b[0]), a[1].min(b[1]), (a[0] - b[0]).abs(), (a[1] - b[1]).abs()]
    }

    /// The crop for a frame on the whole canvas turned by `quarter_turns`; `None` for the whole canvas.
    pub fn crop_of_frame([x, y, w, h]: [f32; 4], quarter_turns: u8) -> Option<[f32; 4]> {
        let a = unturn([x, y], quarter_turns);
        let b = unturn([x + w, y + h], quarter_turns);
        sanitized_crop([a[0].min(b[0]), a[1].min(b[1]), (a[0] - b[0]).abs(), (a[1] - b[1]).abs()])
    }

    /// The upright original's size, about, from its output's: a saved edit's item knows only that one.
    pub fn original_size(&self, width: u32, height: u32) -> (u32, u32) {
        let (width, height) = if self.quarter_turns % 2 == 1 {
            (height, width)
        } else {
            (width, height)
        };
        let [sx, sy] = self.canvas_scale();
        (
            (width as f32 / self.crop[2] / sx).round() as u32,
            (height as f32 / self.crop[3] / sy).round() as u32,
        )
    }

    /// Where a point of the original (fractions of its size) lies on the output; outside 0–1 if cropped away.
    pub fn to_output(&self, point: [f32; 2]) -> [f32; 2] {
        let [u, v] = self.to_canvas(point);
        let [x, y, w, h] = self.crop;
        turn(self.quarter_turns, [(u - x) / w, (v - y) / h])
    }

    /// Where a point of the output lies on the original.
    pub fn to_original(&self, point: [f32; 2]) -> [f32; 2] {
        let [u, v] = unturn(point, self.quarter_turns);
        let [x, y, w, h] = self.crop;
        self.from_canvas([x + u * w, y + v * h])
    }

    /// A rectangle of the original (`x, y, w, h`) on the output, cut at the crop's edge; `None` if entirely outside.
    /// Straightened, it is the box around the turned rectangle.
    pub fn place(&self, [x, y, w, h]: [f32; 4]) -> Option<[f32; 4]> {
        let corners = [[x, y], [x + w, y], [x, y + h], [x + w, y + h]].map(|corner| self.to_output(corner));
        let fold = |pick: fn(&[f32; 2]) -> f32, min: bool| {
            corners
                .iter()
                .map(pick)
                .fold(if min { f32::INFINITY } else { f32::NEG_INFINITY }, |a, b| {
                    if min { a.min(b) } else { a.max(b) }
                })
        };
        let (left, right) = (fold(|p| p[0], true).max(0.0), fold(|p| p[0], false).min(1.0));
        let (top, bottom) = (fold(|p| p[1], true).max(0.0), fold(|p| p[1], false).min(1.0));
        (right > left && bottom > top).then_some([left, top, right - left, bottom - top])
    }
}

/// A point of the unturned canvas on the canvas turned by `quarter_turns` clockwise.
pub(super) fn turn(quarter_turns: u8, [u, v]: [f32; 2]) -> [f32; 2] {
    match quarter_turns % 4 {
        1 => [1.0 - v, u],
        2 => [1.0 - u, 1.0 - v],
        3 => [v, 1.0 - u],
        _ => [u, v],
    }
}

/// A point of the canvas turned by `quarter_turns` clockwise on the unturned one.
pub(super) fn unturn([u, v]: [f32; 2], quarter_turns: u8) -> [f32; 2] {
    match quarter_turns % 4 {
        1 => [v, 1.0 - u],
        2 => [1.0 - u, 1.0 - v],
        3 => [1.0 - v, u],
        _ => [u, v],
    }
}

/// A straightening within ±[`MAX_STRAIGHTEN`], in steps of a tenth of a degree; 0 for an invalid one.
pub(super) fn sanitized_straighten(degrees: f32) -> f32 {
    if degrees.is_finite() {
        ((degrees.clamp(-MAX_STRAIGHTEN, MAX_STRAIGHTEN) * 10.0).round() / 10.0) + 0.0
    } else {
        0.0
    }
}

/// A photo's width per height, from a hundredth to a hundred; square for an invalid one.
pub(super) fn sanitized_aspect(aspect: f32) -> f32 {
    if aspect.is_finite() && aspect > 0.0 {
        aspect.clamp(0.01, 100.0)
    } else {
        1.0
    }
}
