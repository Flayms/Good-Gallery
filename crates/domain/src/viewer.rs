//! The viewer's zoom and pan (REQ-009-05, REQ-009-16), which photos it preloads (REQ-009-13) and when its controls
//! step aside (REQ-013-10). The content is centered in the viewport, moved by `(x, y)` and scaled around its center;
//! scale 1 is the fitted size.

/// Zoom stays between fitted and this many times the fitted size.
pub const MIN_SCALE: f32 = 1.0;
pub const MAX_SCALE: f32 = 8.0;
/// A toggle zooms to actual pixels, but at least this far, so small photos zoom too.
pub const MIN_TOGGLE_SCALE: f32 = 2.0;
/// How many places before and after the shown item photos are preloaded.
pub const PRELOAD_RADIUS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomView {
    pub scale: f32,
    pub x: f32,
    pub y: f32,
}

pub const FIT: ZoomView = ZoomView {
    scale: 1.0,
    x: 0.0,
    y: 0.0,
};

impl Default for ZoomView {
    fn default() -> Self {
        FIT
    }
}

impl ZoomView {
    pub fn is_zoomed(&self) -> bool {
        self.scale > MIN_SCALE
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// Limits the scale and keeps zoomed content covering the viewport; content smaller than it stays centered.
pub fn clamp_view(view: ZoomView, content: Size, viewport: Size) -> ZoomView {
    let scale = view.scale.clamp(MIN_SCALE, MAX_SCALE);
    let max_x = ((content.width * scale - viewport.width) / 2.0).max(0.0);
    let max_y = ((content.height * scale - viewport.height) / 2.0).max(0.0);
    ZoomView {
        scale,
        x: view.x.clamp(-max_x, max_x),
        y: view.y.clamp(-max_y, max_y),
    }
}

/// Zooms to `scale`, keeping the content point under `point` (relative to the viewport's center) in place.
pub fn zoom_to(view: ZoomView, scale: f32, point: (f32, f32), content: Size, viewport: Size) -> ZoomView {
    let next = scale.clamp(MIN_SCALE, MAX_SCALE);
    let ratio = next / view.scale;
    clamp_view(
        ZoomView {
            scale: next,
            x: point.0 - (point.0 - view.x) * ratio,
            y: point.1 - (point.1 - view.y) * ratio,
        },
        content,
        viewport,
    )
}

/// Size of `content` fitted into `viewport` (scale 1), never enlarged beyond its own size.
pub fn fit_size(content: Size, viewport: Size) -> Size {
    let ratio = 1.0_f32
        .min(viewport.width / content.width)
        .min(viewport.height / content.height);
    Size::new(content.width * ratio, content.height * ratio)
}

/// The scale a click or double-click switches to: a zoomed photo fits again, a fitted one shows its actual pixels
/// (`natural_width` pixels drawn `fitted_width` wide), at least 2× and at most 8×.
pub fn toggle_scale(view: ZoomView, natural_width: f32, fitted_width: f32) -> f32 {
    if view.is_zoomed() {
        MIN_SCALE
    } else {
        (natural_width / fitted_width.max(1.0)).clamp(MIN_TOGGLE_SCALE, MAX_SCALE)
    }
}

/// Gallery positions up to [`PRELOAD_RADIUS`] places around `index` in a gallery of `count`, nearest first, the next
/// before the previous.
pub fn neighbours(count: usize, index: usize) -> Vec<usize> {
    let mut result = Vec::new();
    for distance in 1..=PRELOAD_RADIUS {
        if let Some(next) = index.checked_add(distance).filter(|&at| at < count) {
            result.push(next);
        }
        if let Some(previous) = index.checked_sub(distance).filter(|&at| at < count) {
            result.push(previous);
        }
    }
    result
}

/// Whether the viewer's controls are hidden: only when idle, and never while the pointer is over them, the info
/// panel is open or a video is paused.
pub fn controls_hidden(idle: bool, hovering: bool, info_open: bool, video_paused: bool) -> bool {
    idle && !hovering && !info_open && !video_paused
}

/// Size of the filmstrip's square thumbnails, points.
pub const FILMSTRIP_THUMB: f32 = 56.0;

/// Settings key of the filmstrip choice (REQ-015-02), as the Electron version's `localStorage` key.
pub const FILMSTRIP_KEY: &str = "viewer.filmstrip";

/// Whether the stored choice shows the filmstrip; it is hidden until the user shows it.
pub fn is_filmstrip_shown(stored: Option<&str>) -> bool {
    stored == Some("shown")
}

/// How far the filmstrip scrolls sideways for a wheel movement: the mouse wheel scrolls it, as does a touchpad swipe.
pub fn sideways_delta(dx: f32, dy: f32) -> f32 {
    if dx.abs() > dy.abs() { dx } else { dy }
}

/// The filmstrip's thumbnails: `size` wide with `gap` between them and `padding` at both ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strip {
    pub size: f32,
    pub gap: f32,
    pub padding: f32,
}

impl Strip {
    /// Width of the whole row of `count` thumbnails.
    pub fn width(&self, count: usize) -> f32 {
        if count == 0 {
            return 2.0 * self.padding;
        }
        2.0 * self.padding + count as f32 * self.size + (count - 1) as f32 * self.gap
    }

    /// Left edge of thumbnail `index` in the row.
    pub fn left(&self, index: usize) -> f32 {
        self.padding + index as f32 * (self.size + self.gap)
    }

    /// The thumbnails at least partly within `offset .. offset + view`.
    pub fn visible(&self, count: usize, offset: f32, view: f32) -> std::ops::Range<usize> {
        let step = self.size + self.gap;
        let first = ((offset - self.padding - self.size) / step).ceil().max(0.0) as usize;
        let end = (((offset + view - self.padding) / step).ceil().max(0.0) as usize).min(count);
        first.min(end)..end
    }

    /// The scroll offset that centers thumbnail `index` in a `view` wide strip, as far as the ends allow (REQ-015-03).
    pub fn centered(&self, index: usize, count: usize, view: f32) -> f32 {
        let center = self.left(index) + self.size / 2.0;
        self.clamp(center - view / 2.0, count, view)
    }

    /// Keeps a scroll offset within the row.
    pub fn clamp(&self, offset: f32, count: usize, view: f32) -> f32 {
        offset.clamp(0.0, (self.width(count) - view).max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: Size = Size::new(1000.0, 800.0);
    const CONTENT: Size = Size::new(1000.0, 500.0);

    fn view(scale: f32, x: f32, y: f32) -> ZoomView {
        ZoomView { scale, x, y }
    }

    #[test]
    fn req_009_05_zooms_around_the_point_under_the_pointer() {
        // The content point under the pointer before: (point - x) / scale = (200, 100); after: x + 2 · c = point.
        assert_eq!(
            zoom_to(FIT, 2.0, (200.0, 100.0), CONTENT, VIEWPORT),
            view(2.0, -200.0, -100.0)
        );
    }

    #[test]
    fn req_009_05_limits_the_scale_between_fitted_and_8x() {
        assert_eq!(zoom_to(FIT, 100.0, (0.0, 0.0), CONTENT, VIEWPORT).scale, MAX_SCALE);
        assert_eq!(zoom_to(view(2.0, 300.0, 0.0), 0.5, (0.0, 0.0), CONTENT, VIEWPORT), FIT);
    }

    #[test]
    fn req_009_05_keeps_zoomed_content_covering_the_viewport() {
        // At 2× the content is 2000 × 1000: 500 px of horizontal and 100 px of vertical slack on each side.
        assert_eq!(
            clamp_view(view(2.0, 900.0, -900.0), CONTENT, VIEWPORT),
            view(2.0, 500.0, -100.0)
        );
    }

    #[test]
    fn req_009_05_centers_content_smaller_than_the_viewport() {
        assert_eq!(clamp_view(view(1.5, 0.0, 50.0), CONTENT, VIEWPORT), view(1.5, 0.0, 0.0));
    }

    #[test]
    fn req_009_04_fits_large_content_keeping_its_aspect_ratio() {
        assert_eq!(fit_size(Size::new(4000.0, 2000.0), VIEWPORT), Size::new(1000.0, 500.0));
        assert_eq!(fit_size(Size::new(2000.0, 4000.0), VIEWPORT), Size::new(400.0, 800.0));
    }

    #[test]
    fn req_009_04_never_enlarges_content_beyond_its_own_size() {
        assert_eq!(fit_size(Size::new(300.0, 200.0), VIEWPORT), Size::new(300.0, 200.0));
    }

    #[test]
    fn req_009_16_toggles_between_fitted_and_actual_pixels_at_least_2x() {
        assert_eq!(toggle_scale(FIT, 4000.0, 1000.0), 4.0);
        assert_eq!(toggle_scale(FIT, 1200.0, 1000.0), MIN_TOGGLE_SCALE);
        assert_eq!(toggle_scale(FIT, 20_000.0, 1000.0), MAX_SCALE);
        assert_eq!(toggle_scale(view(3.0, 10.0, 0.0), 4000.0, 1000.0), MIN_SCALE);
    }

    #[test]
    fn req_009_13_lists_the_items_up_to_two_places_around_nearest_first() {
        assert_eq!(neighbours(6, 2), [3, 1, 4, 0]);
    }

    #[test]
    fn req_009_13_stops_at_the_ends_of_the_gallery() {
        assert_eq!(neighbours(6, 0), [1, 2]);
        assert_eq!(neighbours(6, 5), [4, 3]);
        assert!(neighbours(6, 9).is_empty());
    }

    #[test]
    fn req_015_02_shows_the_filmstrip_only_once_the_user_showed_it() {
        assert!(is_filmstrip_shown(Some("shown")));
        assert!(!is_filmstrip_shown(Some("hidden")));
        assert!(!is_filmstrip_shown(None));
    }

    #[test]
    fn req_015_05_turns_the_vertical_wheel_into_sideways_scrolling_and_keeps_touchpad_swipes() {
        assert_eq!(sideways_delta(0.0, 120.0), 120.0);
        assert_eq!(sideways_delta(0.0, -120.0), -120.0);
        assert_eq!(sideways_delta(-40.0, 3.0), -40.0);
    }

    const STRIP: Strip = Strip {
        size: 56.0,
        gap: 4.0,
        padding: 6.0,
    };

    #[test]
    fn req_015_03_centers_the_shown_thumbnail_as_far_as_the_ends_allow() {
        // 100 thumbnails: 6 + 100 · 56 + 99 · 4 + 6 = 6008 wide; thumbnail 50 is centered at 6 + 50 · 60 + 28.
        assert_eq!(STRIP.width(100), 6008.0);
        assert_eq!(STRIP.centered(50, 100, 600.0), 3034.0 - 300.0);
        assert_eq!(STRIP.centered(1, 100, 600.0), 0.0);
        assert_eq!(STRIP.centered(99, 100, 600.0), 6008.0 - 600.0);
        assert_eq!(STRIP.centered(2, 5, 600.0), 0.0, "a short row doesn't scroll");
    }

    #[test]
    fn req_015_05_finds_the_thumbnails_in_view() {
        assert_eq!(STRIP.visible(100, 0.0, 600.0), 0..10);
        // From 3000: thumbnail 49 ends at 6 + 49 · 60 + 56 = 3002, still in view.
        assert_eq!(STRIP.visible(100, 3000.0, 600.0), 49..60);
        assert_eq!(STRIP.visible(100, 5800.0, 600.0), 96..100);
        assert_eq!(STRIP.visible(0, 0.0, 600.0), 0..0);
    }

    #[test]
    fn req_013_10_hides_the_controls_only_when_idle_and_nothing_keeps_them() {
        assert!(controls_hidden(true, false, false, false));
        assert!(!controls_hidden(false, false, false, false));
        assert!(!controls_hidden(true, true, false, false));
        assert!(!controls_hidden(true, false, true, false));
        assert!(!controls_hidden(true, false, false, true));
    }
}
