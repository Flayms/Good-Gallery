//! Parts of the photo and local masks (spec 022, spec 030).

use super::*;

/// What the adjustments of the panel change: the whole photo, or only its subject or its background (REQ-022-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Part {
    #[default]
    Photo,
    Subject,
    Background,
    /// A mask of the recipe, by its place in the list (REQ-030-01).
    Mask(usize),
}

impl Part {
    pub const ALL: [Part; 3] = [Self::Photo, Self::Subject, Self::Background];

    pub fn label(self) -> &'static str {
        match self {
            Self::Photo => "Photo",
            Self::Subject => "Subject",
            Self::Background => "Background",
            Self::Mask(_) => "Mask",
        }
    }
}

/// Masks a recipe can have (REQ-030-01).
pub const MAX_MASKS: usize = 8;

/// What a mask selects (REQ-030-03 … REQ-030-05). Positions are fractions of the upright, unturned original, as the
/// crop's are; brightness and softness in percent of the encoded scale, hue, range and color softness in degrees of
/// Oklab's color wheel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MaskShape {
    /// Full at `from`, fading to nothing at `to`.
    Linear {
        from: [f32; 2],
        to: [f32; 2],
    },
    Luminance {
        low: f32,
        high: f32,
        feather: f32,
    },
    Color {
        hue: f32,
        range: f32,
        feather: f32,
    },
    /// A painting of the recipe, by its place: brushed strokes, or a filled outline if `freeform`.
    Painted {
        layer: u8,
        freeform: bool,
    },
}

impl MaskShape {
    /// From the top to the middle of the photo: a sky, usually.
    pub const LINEAR: Self = Self::Linear {
        from: [0.5, 0.1],
        to: [0.5, 0.5],
    };
    /// The bright half.
    pub const LUMINANCE: Self = Self::Luminance {
        low: 60.0,
        high: 100.0,
        feather: 15.0,
    };
    /// Greens.
    pub const COLOR: Self = Self::Color {
        hue: 140.0,
        range: 30.0,
        feather: 20.0,
    };

    pub fn label(self) -> &'static str {
        match self {
            Self::Linear { .. } => "Gradient",
            Self::Luminance { .. } => "Luminance",
            Self::Color { .. } => "Color",
            Self::Painted { freeform: false, .. } => "Brush",
            Self::Painted { freeform: true, .. } => "Freeform",
        }
    }

    /// The panel's sliders of this shape: name, value, range, unit (REQ-030-04, REQ-030-05). A gradient has handles.
    pub fn sliders(self) -> Vec<(&'static str, f32, (f32, f32), &'static str)> {
        match self {
            Self::Linear { .. } | Self::Painted { .. } => Vec::new(),
            Self::Luminance { low, high, feather } => vec![
                ("From", low, (0.0, 100.0), " %"),
                ("To", high, (0.0, 100.0), " %"),
                ("Softness", feather, (0.0, 50.0), " %"),
            ],
            Self::Color { hue, range, feather } => vec![
                ("Mask hue", hue, (0.0, 360.0), "°"),
                ("Range", range, (0.0, 180.0), "°"),
                ("Softness", feather, (0.0, 90.0), "°"),
            ],
        }
    }

    /// Sets slider `i` of [`Self::sliders`].
    pub fn set_slider(&mut self, i: usize, value: f32) {
        match self {
            Self::Linear { .. } | Self::Painted { .. } => {}
            Self::Luminance { low, high, feather } => *[low, high, feather][i] = value,
            Self::Color { hue, range, feather } => *[hue, range, feather][i] = value,
        }
    }

    /// Values from a file brought into their ranges (REQ-011-10).
    pub(super) fn sanitize(&mut self) {
        let fit = |value: &mut f32, low: f32, high: f32, or: f32| {
            *value = if value.is_finite() { value.clamp(low, high) } else { or };
        };
        match self {
            Self::Linear { from, to } => {
                for c in from.iter_mut().chain(to.iter_mut()) {
                    fit(c, -1.0, 2.0, 0.5);
                }
            }
            Self::Luminance { low, high, feather } => {
                fit(low, 0.0, 100.0, 0.0);
                fit(high, 0.0, 100.0, 100.0);
                fit(feather, 0.0, 100.0, 0.0);
            }
            Self::Color { hue, range, feather } => {
                fit(hue, 0.0, 360.0, 0.0);
                fit(range, 0.0, 180.0, 30.0);
                fit(feather, 0.0, 90.0, 0.0);
            }
            Self::Painted { .. } => {}
        }
    }

    /// How much of the pixel `rgb` as stored, at `uv` of a photo `size` pixels large, this selects: 0 to 1.
    pub fn weight(self, rgb: [f32; 3], uv: [f32; 2], size: [f32; 2]) -> f32 {
        match self {
            Self::Linear { from, to } => {
                let along = [(to[0] - from[0]) * size[0], (to[1] - from[1]) * size[1]];
                let length = along[0] * along[0] + along[1] * along[1];
                if length <= 0.0 {
                    return 0.0;
                }
                let at = [(uv[0] - from[0]) * size[0], (uv[1] - from[1]) * size[1]];
                1.0 - smoothstep(0.0, 1.0, (at[0] * along[0] + at[1] * along[1]) / length)
            }
            Self::Luminance { low, high, feather } => {
                let value = to_encoded(luminance(rgb.map(to_linear)).max(0.0)) * 100.0;
                let feather = feather.max(0.5);
                smoothstep(low - feather, low, value) * (1.0 - smoothstep(high, high + feather, value))
            }
            Self::Color { hue, range, feather } => {
                let [_, a, b] = to_oklab(rgb.map(to_linear));
                let chroma = (a * a + b * b).sqrt();
                // The short way around the wheel.
                let away = ((b.atan2(a).to_degrees() - hue + 180.0).rem_euclid(360.0) - 180.0).abs();
                (1.0 - smoothstep(range, range + feather.max(0.5), away)) * smoothstep(0.02, 0.06, chroma)
            }
            // `MaskParams::weight` reads the painting.
            Self::Painted { .. } => 0.0,
        }
    }

    /// The numbers the shader's `LocalMask` holds: its kind, then the shape's.
    pub fn numbers(self) -> [f32; 5] {
        match self {
            Self::Linear { from, to } => [0.0, from[0], from[1], to[0], to[1]],
            Self::Luminance { low, high, feather } => [1.0, low / 100.0, high / 100.0, feather / 100.0, 0.0],
            Self::Color { hue, range, feather } => [2.0, hue, range, feather, 0.0],
            Self::Painted { layer, .. } => [3.0, f32::from(layer), 0.0, 0.0, 0.0],
        }
    }
}

/// A mask of a recipe: what it selects, and the light and color it gives that (REQ-030-02).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalMask {
    pub shape: MaskShape,
    #[serde(default)]
    pub invert: bool,
    #[serde(default)]
    pub recipe: Box<Recipe>,
}

impl LocalMask {
    pub fn new(shape: MaskShape) -> Self {
        Self {
            shape,
            invert: false,
            recipe: Box::default(),
        }
    }
}

/// The masks of a recipe from a file; a kind only a newer version knows is left out (REQ-030-08).
pub(super) fn masks_known<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<LocalMask>, D::Error> {
    let masks = Vec::<serde_json::Value>::deserialize(deserializer)?;
    Ok(masks
        .into_iter()
        .filter_map(|mask| serde_json::from_value(mask).ok())
        .collect())
}

/// A mask's numbers in [`Params`].
#[derive(Debug, Clone, PartialEq)]
pub struct MaskParams {
    pub shape: MaskShape,
    pub invert: bool,
    pub params: Box<Params>,
    /// What the recipe's paintings select, once [`Params::with_paintings`] drew them.
    pub paintings: Vec<Arc<Mask>>,
}

impl MaskParams {
    /// How much of the pixel this mask selects (REQ-030-02, REQ-030-06).
    pub fn weight(&self, rgb: [f32; 3], uv: [f32; 2], size: [f32; 2]) -> f32 {
        let weight = match self.shape {
            MaskShape::Painted { layer, .. } => self
                .paintings
                .get(usize::from(layer))
                .map_or(0.0, |painting| painting.sample(uv)),
            shape => shape.weight(rgb, uv, size),
        };
        if self.invert { 1.0 - weight } else { weight }
    }
}

/// How much each spot of a photo belongs to its subject, 0 to 1, on a grid laid over the whole photo (REQ-022-03).
#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    width: u32,
    height: u32,
    values: Vec<f32>,
}

impl Mask {
    /// `None` unless there are `width × height` values.
    pub fn new(width: u32, height: u32, values: Vec<f32>) -> Option<Self> {
        (width > 0 && height > 0 && values.len() == width as usize * height as usize).then_some(Self {
            width,
            height,
            values,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Row by row from the top left.
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    /// The value at `uv` (0–1 across the photo), bilinear between the grid's cell centers, as a GPU samples it.
    pub fn sample(&self, uv: [f32; 2]) -> f32 {
        let at = |c: f32, side: u32| (c * side as f32 - 0.5).clamp(0.0, (side - 1) as f32);
        let (x, y) = (at(uv[0], self.width), at(uv[1], self.height));
        let (x0, y0) = (x.floor() as u32, y.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (tx, ty) = (x - x0 as f32, y - y0 as f32);
        let value = |x: u32, y: u32| self.values[(y * self.width + x) as usize];
        let top = value(x0, y0) * (1.0 - tx) + value(x1, y0) * tx;
        let bottom = value(x0, y1) * (1.0 - tx) + value(x1, y1) * tx;
        top * (1.0 - ty) + bottom * ty
    }
}
