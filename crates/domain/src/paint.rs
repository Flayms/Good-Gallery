//! Brush and freeform masks (spec 030): strokes and outlines as points, and drawing them into a grid. Painting takes the
//! larger value and erasing the smaller, so a stroke comes out the same however many of its segments are drawn at a time:
//! the preview adds segments as the user paints, saving draws it all at once.

use crate::edit::Mask;
use serde::{Deserialize, Serialize};

/// Painted shapes a photo can have: the preview's texture array has as many layers (REQ-030-14).
pub const MAX_PAINTINGS: usize = 4;
/// Points a photo's paintings can have, so that the saved edit fits the file's metadata (REQ-030-14).
pub const MAX_POINTS: usize = 1500;
/// The longer side of the grid paintings are drawn into.
pub const GRID_SIDE: u32 = 1024;
/// How far a thinned stroke may leave what was drawn, in fractions of the photo.
const THINNING: f32 = 0.001;

/// A brushed stroke, or an outline if `closed`. Points are fractions of the upright, unturned original, as the crop's are.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub points: Vec<[f32; 2]>,
    /// The brush's radius, in percent of the photo's longer side.
    #[serde(default)]
    pub size: f32,
    /// How much of the radius fades, or of an outline's edge, in percent.
    #[serde(default)]
    pub softness: f32,
    #[serde(default)]
    pub erase: bool,
    #[serde(default)]
    pub closed: bool,
}

/// What one brush or freeform shape painted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Painting {
    pub strokes: Vec<Stroke>,
}

impl Painting {
    /// The points it has.
    pub fn points(&self) -> usize {
        self.strokes.iter().map(|stroke| stroke.points.len()).sum()
    }
}

/// What `paintings` select, for a photo `size` large.
pub fn draw_all(paintings: &[Painting], size: [f32; 2]) -> Vec<std::sync::Arc<Mask>> {
    let (width, height) = grid_size(size);
    paintings
        .iter()
        .map(|painting| std::sync::Arc::new(draw(painting, width, height)))
        .collect()
}

/// The points of all `paintings`.
pub fn total_points(paintings: &[Painting]) -> usize {
    paintings.iter().map(Painting::points).sum()
}

/// The grid for a photo `size` large: [`GRID_SIDE`] on its longer side, and its shape.
pub fn grid_size(size: [f32; 2]) -> (u32, u32) {
    let longer = size[0].max(size[1]).max(1.0);
    let side = |length: f32| ((length / longer * GRID_SIDE as f32).round() as u32).max(1);
    (side(size[0]), side(size[1]))
}

/// A painting drawn into a new grid of `width` × `height`.
pub fn draw(painting: &Painting, width: u32, height: u32) -> Mask {
    let mut values = vec![0.0; width as usize * height as usize];
    for stroke in &painting.strokes {
        draw_stroke(&mut values, width, height, stroke, 0);
    }
    Mask::new(width, height, values).expect("as many values as the grid has")
}

/// Draws a stroke into `values`, an open one from its segment `from` on (0 for all of it). An outline is always whole.
pub fn draw_stroke(values: &mut [f32], width: u32, height: u32, stroke: &Stroke, from: usize) {
    if stroke.closed {
        draw_outline(values, width, height, stroke);
    } else {
        draw_brush(values, width, height, stroke, from);
    }
}

fn longer_side(width: u32, height: u32) -> f32 {
    width.max(height) as f32
}

/// `a` to `b`'s distance from `p`.
fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let along = [b[0] - a[0], b[1] - a[1]];
    let length = along[0] * along[0] + along[1] * along[1];
    let t = if length > 0.0 {
        (((p[0] - a[0]) * along[0] + (p[1] - a[1]) * along[1]) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (x, y) = (a[0] + along[0] * t - p[0], a[1] + along[1] * t - p[1]);
    (x * x + y * y).sqrt()
}

fn smoothstep(low: f32, high: f32, x: f32) -> f32 {
    let t = ((x - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn put(value: &mut f32, amount: f32, erase: bool) {
    *value = if erase {
        value.min(1.0 - amount)
    } else {
        value.max(amount)
    };
}

fn draw_brush(values: &mut [f32], width: u32, height: u32, stroke: &Stroke, from: usize) {
    let radius = (stroke.size / 100.0 * longer_side(width, height)).max(0.5);
    let inner = radius * (1.0 - stroke.softness.clamp(0.0, 100.0) / 100.0);
    let on_grid = |[u, v]: [f32; 2]| [u * width as f32, v * height as f32];
    let points: Vec<[f32; 2]> = stroke.points.iter().copied().map(on_grid).collect();
    let segments: Vec<([f32; 2], [f32; 2])> = match points.len() {
        0 => return,
        1 => vec![(points[0], points[0])],
        _ => points
            .windows(2)
            .skip(from.saturating_sub(1))
            .map(|pair| (pair[0], pair[1]))
            .collect(),
    };
    for (a, b) in segments {
        let range = |low: f32, high: f32, end: u32| {
            let first = (low - radius - 1.0).floor().max(0.0) as u32;
            let last = ((high + radius + 1.0).ceil().max(0.0) as u32).min(end);
            first..last
        };
        for y in range(a[1].min(b[1]), a[1].max(b[1]), height) {
            for x in range(a[0].min(b[0]), a[0].max(b[0]), width) {
                let distance = segment_distance([x as f32 + 0.5, y as f32 + 0.5], a, b);
                if distance < radius {
                    let amount = 1.0 - smoothstep(inner, radius.max(inner + 1e-3), distance);
                    put(&mut values[(y * width + x) as usize], amount, stroke.erase);
                }
            }
        }
    }
}

fn draw_outline(values: &mut [f32], width: u32, height: u32, stroke: &Stroke) {
    if stroke.points.len() < 3 {
        return;
    }
    let points: Vec<[f32; 2]> = stroke
        .points
        .iter()
        .map(|[u, v]| [u * width as f32, v * height as f32])
        .collect();
    // The softness is at most a tenth of the longer side, inside the outline.
    let soft = stroke.softness.clamp(0.0, 100.0) / 1000.0 * longer_side(width, height);
    let bound = |values: &mut dyn Iterator<Item = f32>, end: u32| {
        let (mut low, mut high) = (f32::MAX, f32::MIN);
        for value in values {
            low = low.min(value);
            high = high.max(value);
        }
        (low.floor().max(0.0) as u32)..(high.ceil().max(0.0) as u32).min(end)
    };
    let columns = bound(&mut points.iter().map(|point| point[0]), width);
    for y in bound(&mut points.iter().map(|point| point[1]), height) {
        for x in columns.clone() {
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            let mut inside = false;
            let mut nearest = f32::MAX;
            for i in 0..points.len() {
                let (a, b) = (points[i], points[(i + 1) % points.len()]);
                if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                    inside = !inside;
                }
                if soft > 0.0 {
                    nearest = nearest.min(segment_distance(p, a, b));
                }
            }
            if inside {
                let amount = if soft > 0.0 {
                    smoothstep(0.0, soft, nearest)
                } else {
                    1.0
                };
                put(&mut values[(y * width + x) as usize], amount, stroke.erase);
            }
        }
    }
}

/// The stroke's points without those it doesn't need (Ramer–Douglas–Peucker), in four decimals (REQ-030-14).
pub fn thin(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    fn keep(points: &[[f32; 2]], kept: &mut Vec<[f32; 2]>) {
        let (a, b) = (points[0], points[points.len() - 1]);
        let (mut far, mut at) = (0.0, 0);
        for (i, &point) in points.iter().enumerate().take(points.len() - 1).skip(1) {
            let distance = segment_distance(point, a, b);
            if distance > far {
                (far, at) = (distance, i);
            }
        }
        if far > THINNING {
            keep(&points[..=at], kept);
            keep(&points[at..], kept);
        } else {
            kept.push(b);
        }
    }
    let Some(&first) = points.first() else {
        return Vec::new();
    };
    let mut kept = vec![first];
    if points.len() > 1 {
        keep(points, &mut kept);
    }
    kept.iter()
        .map(|point| point.map(|c| (c * 10_000.0).round() / 10_000.0))
        .collect()
}

/// Paintings from a file brought into the limits (REQ-011-10): at most [`MAX_PAINTINGS`] and [`MAX_POINTS`], with points
/// on the photo and sizes in their ranges. Strokes beyond the limit are left out.
pub fn sanitize(paintings: &mut Vec<Painting>) {
    paintings.truncate(MAX_PAINTINGS);
    let mut left = MAX_POINTS;
    for painting in paintings {
        painting.strokes.retain_mut(|stroke| {
            stroke.points.truncate(left);
            left -= stroke.points.len();
            for point in &mut stroke.points {
                *point = point.map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.5 });
            }
            let fit = |value: f32, low: f32, high: f32| {
                if value.is_finite() { value.clamp(low, high) } else { low }
            };
            stroke.size = fit(stroke.size, 0.1, 50.0);
            stroke.softness = fit(stroke.softness, 0.0, 100.0);
            !stroke.points.is_empty()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brush(points: &[[f32; 2]], erase: bool) -> Stroke {
        Stroke {
            points: points.to_vec(),
            size: 10.0,
            softness: 50.0,
            erase,
            closed: false,
        }
    }

    fn at(mask: &Mask, u: f32, v: f32) -> f32 {
        mask.values()[(v * mask.height() as f32) as usize * mask.width() as usize + (u * mask.width() as f32) as usize]
    }

    /// REQ-030-12: full along the stroke, fading at its edge, nothing beyond; a single point is a dot.
    #[test]
    fn req_030_12_a_stroke_selects_what_it_covers_softly() {
        let painting = Painting {
            strokes: vec![brush(&[[0.2, 0.5], [0.8, 0.5]], false)],
        };
        let mask = draw(&painting, 200, 100);
        assert_eq!(at(&mask, 0.5, 0.5), 1.0, "on it");
        assert_eq!(at(&mask, 0.5, 0.0), 0.0, "beyond its radius");
        let edge = at(&mask, 0.5, 0.35);
        assert!(edge > 0.0 && edge < 1.0, "{edge}");

        let dot = Painting {
            strokes: vec![brush(&[[0.5, 0.5]], false)],
        };
        let mask = draw(&dot, 200, 100);
        assert_eq!(at(&mask, 0.5, 0.5), 1.0);
        assert_eq!(at(&mask, 0.1, 0.5), 0.0);
    }

    /// REQ-030-12: an erasing stroke takes out what it covers, and only that.
    #[test]
    fn req_030_12_erasing_takes_it_out_again() {
        let painting = Painting {
            strokes: vec![
                brush(&[[0.1, 0.5], [0.9, 0.5]], false),
                brush(&[[0.5, 0.1], [0.5, 0.9]], true),
            ],
        };
        let mask = draw(&painting, 200, 100);
        assert_eq!(at(&mask, 0.5, 0.5), 0.0, "erased");
        assert_eq!(at(&mask, 0.2, 0.5), 1.0, "still painted");
    }

    /// REQ-030-12: adding segments as the user paints gives what drawing it all at once gives, so that the preview and
    /// the saved file agree.
    #[test]
    fn req_030_12_drawing_on_top_equals_drawing_at_once() {
        let points = [[0.1, 0.2], [0.3, 0.6], [0.5, 0.3], [0.7, 0.8], [0.9, 0.4]];
        for erase in [false, true] {
            let whole = brush(&points, erase);
            let (width, height) = (160, 90);
            let start = if erase { 0.6 } else { 0.0 };
            let mut at_once = vec![start; 160 * 90];
            draw_stroke(&mut at_once, width, height, &whole, 0);
            let mut bit_by_bit = vec![start; 160 * 90];
            for count in 1..=points.len() {
                let part = brush(&points[..count], erase);
                draw_stroke(&mut bit_by_bit, width, height, &part, count - 1);
            }
            assert_eq!(at_once, bit_by_bit, "erase: {erase}");
        }
    }

    /// REQ-030-13: inside the outline, softly at its edge; a further one with _Erase_ takes out what lies inside it.
    #[test]
    fn req_030_13_an_outline_selects_what_lies_inside() {
        let square = |low: f32, high: f32, erase: bool, softness: f32| Stroke {
            points: vec![[low, low], [high, low], [high, high], [low, high]],
            size: 0.0,
            softness,
            erase,
            closed: true,
        };
        let hard = draw(
            &Painting {
                strokes: vec![square(0.2, 0.8, false, 0.0)],
            },
            100,
            100,
        );
        assert_eq!(at(&hard, 0.5, 0.5), 1.0);
        assert_eq!(at(&hard, 0.21, 0.5), 1.0, "right up to the edge");
        assert_eq!(at(&hard, 0.1, 0.5), 0.0);

        let soft = draw(
            &Painting {
                strokes: vec![square(0.2, 0.8, false, 100.0)],
            },
            100,
            100,
        );
        assert_eq!(at(&soft, 0.5, 0.5), 1.0, "a tenth of the side inside");
        let edge = at(&soft, 0.22, 0.5);
        assert!(edge > 0.0 && edge < 1.0, "{edge}");

        let holed = draw(
            &Painting {
                strokes: vec![square(0.2, 0.8, false, 0.0), square(0.4, 0.6, true, 0.0)],
            },
            100,
            100,
        );
        assert_eq!(at(&holed, 0.5, 0.5), 0.0);
        assert_eq!(at(&holed, 0.3, 0.5), 1.0);

        let too_few = Stroke {
            points: vec![[0.1, 0.1], [0.9, 0.9]],
            ..square(0.0, 0.0, false, 0.0)
        };
        let nothing = draw(&Painting { strokes: vec![too_few] }, 10, 10);
        assert!(nothing.values().iter().all(|&value| value == 0.0));
    }

    /// REQ-030-14: thinning drops what a stroke doesn't need and stays on it, within a pixel of the grid.
    #[test]
    fn req_030_14_thinning_keeps_the_stroke() {
        // A straight line with many points, then a sharp corner.
        let mut points: Vec<[f32; 2]> = (0..=100).map(|i| [i as f32 / 200.0, 0.25]).collect();
        points.extend((0..=100).map(|i| [0.5, 0.25 + i as f32 / 200.0]));
        let thinned = thin(&points);
        assert_eq!(thinned, [[0.0, 0.25], [0.5, 0.25], [0.5, 0.75]]);

        assert_eq!(thin(&[]), Vec::<[f32; 2]>::new());
        assert_eq!(thin(&[[0.12344, 0.5]]), [[0.1234, 0.5]], "four decimals");
        // A closed-looking stroke isn't lost to its ends being equal.
        let round = [[0.1, 0.1], [0.5, 0.5], [0.1, 0.9], [0.1, 0.1]];
        assert_eq!(thin(&round).len(), 4);
    }

    /// REQ-030-14: paintings from a file stay within the limits and on the photo.
    #[test]
    fn req_030_14_values_from_a_file_stay_in_the_limits() {
        let stroke = |points: usize, x: f32| Stroke {
            points: vec![[x, -3.0]; points],
            size: 900.0,
            softness: f32::NAN,
            erase: false,
            closed: false,
        };
        let mut paintings = vec![
            Painting {
                strokes: vec![stroke(MAX_POINTS - 10, f32::INFINITY), stroke(40, 2.0), stroke(5, 0.5)],
            };
            MAX_PAINTINGS + 3
        ];
        sanitize(&mut paintings);
        assert_eq!(paintings.len(), MAX_PAINTINGS);
        assert_eq!(total_points(&paintings), MAX_POINTS);
        let first = &paintings[0].strokes[0];
        assert_eq!(first.points[0], [0.5, 0.0]);
        assert_eq!((first.size, first.softness), (50.0, 0.0));
        assert_eq!(paintings[3].strokes.len(), 0, "no points left for these");
    }

    #[test]
    fn grids_keep_the_photos_shape() {
        assert_eq!(grid_size([4000.0, 3000.0]), (1024, 768));
        assert_eq!(grid_size([3000.0, 4000.0]), (768, 1024));
        assert_eq!(grid_size([10000.0, 1.0]), (1024, 1));
    }
}
