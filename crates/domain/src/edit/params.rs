//! The per-pixel formulas: a recipe turned into numbers, applied to a pixel.

use super::*;

/// A recipe turned into the numbers the formulas use. The GPU shader gets the same numbers (`app::edit`).
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// The edit's LUT and its strength, 0–1, once [`Self::with_lut`] gave its table (REQ-043-02).
    pub lut: Option<(Arc<crate::lut::Lut>, f32)>,
    /// The strength the recipe asks its LUT for, 0–1; nothing without the table.
    lut_strength: f32,
    /// White balance gains on linear RGB.
    pub gains: [f32; 3],
    /// `2^EV`.
    pub exposure: f32,
    /// Black and white points of the perceptual luminance.
    pub black: f32,
    pub white: f32,
    /// The midtones' gamma, 1 for none.
    pub brightness: f32,
    pub shadows: f32,
    pub highlights: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub vibrance: f32,
    /// The turn around the color wheel, in radians.
    pub hue: f32,
    pub vignette: f32,
    pub vignette_start: f32,
    /// The geometry, whose output's center and edges the vignette takes (REQ-029-04).
    pub vignette_geometry: Geometry,
    pub grain: f32,
    /// Grain cell size in pixels of the full photo.
    pub grain_cell: f32,
    /// A film look's grain: strongest in the midtones (REQ-028-06).
    pub grain_midtones: bool,
    /// How much the grain differs between the color channels, 0–1 (REQ-028-06).
    pub grain_color: f32,
    /// The masks' numbers, in order (REQ-030-02).
    pub masks: Vec<MaskParams>,
    /// How strongly bright lights glow red around them, for a film look (REQ-028-08).
    pub halation: f32,
    /// What a film look does to each hue, at its strength: turn in radians, chroma's factor, lightness added
    /// (REQ-028-07). `None` where it changes nothing.
    pub hue_nodes: Option<[[f32; 3]; crate::look::HUE_NODES]>,
    /// The curves, all channels composed into one table per channel; `None` for straight curves.
    pub curves: Option<Vec<[f32; 3]>>,
    pub blur_kind: BlurKind,
    /// The strongest blur, as σ in parts of the longer side; 0 for none.
    pub blur: f32,
    /// Half the sharp area's size and the transition's width, in parts of the longer side.
    pub blur_size: f32,
    pub blur_feather: f32,
    pub blur_focus: [f32; 2],
    /// Discs instead of a soft blur (REQ-023-05).
    pub blur_lens: bool,
    /// The subject's and the background's own numbers (REQ-022-03).
    pub subject: Option<Box<Params>>,
    pub background: Option<Box<Params>>,
}

impl Params {
    pub fn new(recipe: &Recipe) -> Self {
        // A film look adds to the adjustments, and its curves go before the recipe's own (REQ-028-04).
        let look = recipe.look.map(|choice| choice.look);
        let recipe_look = recipe.look;
        let hue_nodes = recipe.look.and_then(|choice| {
            let strength = choice.strength / 100.0;
            let nodes = choice.look.hue_nodes();
            (strength > 0.0 && nodes != [[0.0, 1.0, 0.0]; crate::look::HUE_NODES]).then(|| {
                nodes.map(|[turn, chroma, light]| {
                    [
                        turn.to_radians() * strength,
                        1.0 + (chroma - 1.0) * strength,
                        light * strength,
                    ]
                })
            })
        });
        let (folded, look_curves) = crate::look::fold(recipe);
        let recipe = &folded;
        let curves = (!recipe.curves.is_identity() || look_curves.is_some()).then(|| {
            let c = &recipe.curves;
            let channel = |curves: &Curves, x: f32| {
                let all = curves.all.eval(x);
                [curves.red.eval(all), curves.green.eval(all), curves.blue.eval(all)]
            };
            (0..CURVE_SAMPLES)
                .map(|i| {
                    let x = i as f32 / (CURVE_SAMPLES - 1) as f32;
                    let looked = look_curves.as_ref().map_or([x; 3], |look| channel(look, x));
                    std::array::from_fn(|k| channel(c, looked[k])[k])
                })
                .collect()
        });
        Self {
            gains: white_balance_gains(recipe.temperature, recipe.tint),
            exposure: recipe.exposure.exp2(),
            black: -recipe.blacks / 100.0 * 0.1,
            white: 1.0 - recipe.whites / 100.0 * 0.2,
            brightness: (-recipe.brightness / 100.0).exp2(),
            shadows: recipe.shadows / 100.0 * 0.25,
            highlights: recipe.highlights / 100.0 * 0.25,
            contrast: recipe.contrast / 100.0,
            saturation: recipe.saturation / 100.0,
            vibrance: recipe.vibrance / 100.0,
            hue: recipe.hue.to_radians(),
            vignette: recipe.vignette / 100.0,
            vignette_start: recipe.vignette_midpoint / 100.0 * 0.8,
            vignette_geometry: recipe.geometry(),
            grain: recipe.grain / 100.0,
            grain_cell: 1.0 + recipe.grain_size / 100.0 * 3.0,
            grain_midtones: look.is_some(),
            grain_color: look.map_or(0.0, crate::look::Look::grain_color),
            hue_nodes,
            masks: recipe
                .masks
                .iter()
                .map(|mask| MaskParams {
                    shape: mask.shape,
                    invert: mask.invert,
                    params: Box::new(Params::new(&mask.recipe)),
                    paintings: Vec::new(),
                })
                .collect(),
            halation: recipe_look.map_or(0.0, |choice| choice.look.halation() * choice.strength / 100.0),
            curves,
            blur_kind: recipe.blur_kind,
            blur: if recipe.blur_kind == BlurKind::None {
                0.0
            } else {
                recipe.blur / 100.0 * MAX_BLUR
            },
            blur_size: recipe.blur_size / 100.0 * 0.5,
            blur_feather: 0.02 + recipe.blur_feather / 100.0 * 0.5,
            blur_focus: recipe.blur_focus,
            blur_lens: recipe.blur_lens,
            subject: recipe.subject.as_deref().map(|part| Box::new(Self::new(part))),
            background: recipe.background.as_deref().map(|part| Box::new(Self::new(part))),
            lut: None,
            lut_strength: recipe.lut.as_ref().map_or(0.0, |choice| choice.strength / 100.0),
        }
    }

    /// These numbers with the table of the recipe's LUT, if this computer has it (REQ-043-03); without it the photo
    /// renders as if it had none.
    pub fn with_lut(mut self, table: Option<Arc<crate::lut::Lut>>) -> Self {
        self.lut = table
            .filter(|_| self.lut_strength > 0.0)
            .map(|table| (table, self.lut_strength));
        self
    }

    /// `rgb` (encoded) through the LUT at its strength (REQ-043-02).
    pub fn through_lut(&self, rgb: [f32; 3]) -> [f32; 3] {
        match &self.lut {
            Some((table, strength)) => {
                let mapped = table.apply(rgb);
                std::array::from_fn(|c| rgb[c] + (mapped[c] - rgb[c]) * strength)
            }
            None => rgb,
        }
    }

    /// These numbers with `paintings`, what the recipe's paintings select (REQ-030-12). [`Self::new`] leaves them out, as
    /// it runs every frame: without them, painted shapes select nothing.
    pub fn with_paintings(mut self, paintings: &[Arc<Mask>]) -> Self {
        for mask in &mut self.masks {
            mask.paintings = paintings.to_vec();
        }
        self
    }

    /// One pixel: `rgb` as stored (sRGB-encoded, 0–1), at `uv` (0–1 across the photo) of a photo `size` pixels large
    /// at full resolution. Returns encoded values in 0–1.
    pub fn pixel(&self, rgb: [f32; 3], uv: [f32; 2], size: [f32; 2]) -> [f32; 3] {
        self.pixel_glowing(rgb, [0.0; 3], uv, size, None)
    }

    /// [`Self::pixel`] with the subject's and the background's own adjustments, `subject` being how much the pixel
    /// belongs to the subject (REQ-022-03).
    pub fn pixel_masked(&self, rgb: [f32; 3], uv: [f32; 2], size: [f32; 2], subject: f32) -> [f32; 3] {
        self.pixel_glowing(rgb, [0.0; 3], uv, size, Some(subject))
    }

    /// One pixel with the [`Self::halation_glow`] it gets, and with its parts if `subject` is given.
    pub fn pixel_glowing(
        &self,
        rgb: [f32; 3],
        glow: [f32; 3],
        uv: [f32; 2],
        size: [f32; 2],
        subject: Option<f32>,
    ) -> [f32; 3] {
        // The LUT first, on the photo's own colors; the rest adjusts what it made (REQ-043-02).
        let rgb = self.through_lut(rgb);
        let mut lin = self.develop(rgb.map(to_linear));
        if let Some(subject) = subject {
            let part = |params: &Option<Box<Params>>| params.as_ref().map_or(lin, |params| params.develop(lin));
            let (inside, outside) = (part(&self.subject), part(&self.background));
            lin = std::array::from_fn(|c| outside[c] + (inside[c] - outside[c]) * subject);
        }
        for mask in &self.masks {
            let weight = mask.weight(rgb, uv, size);
            let developed = mask.params.develop(lin);
            lin = std::array::from_fn(|c| lin[c] + (developed[c] - lin[c]) * weight);
        }
        self.finish(std::array::from_fn(|c| lin[c] + glow[c]), uv, size)
    }

    /// The blur that halation gathers the light with, as σ in pixels of a photo `size` large (REQ-028-08).
    pub fn halation_sigma(size: [f32; 2]) -> f32 {
        HALATION_SIZE * size[0].max(size[1])
    }

    /// The red glow on linear light of a pixel, `sharp` and `blurred` being it and its surroundings as stored
    /// (REQ-028-08): only where the surroundings are brighter highlights than it, so that an evenly bright sky keeps
    /// its color.
    pub fn halation_glow(&self, sharp: [f32; 3], blurred: [f32; 3]) -> [f32; 3] {
        let light = |rgb: [f32; 3]| smoothstep(0.3, 0.9, rgb[0].max(rgb[1]).max(rgb[2]));
        let glow = (light(blurred) - light(sharp)).max(0.0) * self.halation;
        HALATION_COLOR.map(|c| c * glow)
    }

    /// The blur at `uv` of a photo `size` pixels large, as σ in its pixels, `subject` being how much the spot belongs to
    /// the subject (REQ-023-02, REQ-023-03). The photo is first blurred so, then developed.
    pub fn blur_sigma(&self, uv: [f32; 2], size: [f32; 2], subject: f32) -> f32 {
        if self.blur <= 0.0 {
            return 0.0;
        }
        let longer = size[0].max(size[1]);
        let offset = [
            (uv[0] - self.blur_focus[0]) * size[0],
            (uv[1] - self.blur_focus[1]) * size[1],
        ];
        let weight = match self.blur_kind {
            BlurKind::None => 0.0,
            BlurKind::Background => 1.0 - subject.clamp(0.0, 1.0),
            BlurKind::TiltShift | BlurKind::Radial => {
                let distance = if self.blur_kind == BlurKind::TiltShift {
                    offset[1].abs()
                } else {
                    (offset[0] * offset[0] + offset[1] * offset[1]).sqrt()
                } / longer;
                smoothstep(self.blur_size, self.blur_size + self.blur_feather, distance)
            }
        };
        weight * self.blur * longer
    }

    /// White balance, exposure, tone, curves and chroma, on linear light.
    fn develop(&self, lin: [f32; 3]) -> [f32; 3] {
        let mut lin = self.light(lin);
        if let Some(table) = &self.curves {
            for (i, c) in lin.iter_mut().enumerate() {
                *c = to_linear(lookup(table, i, to_encoded(c.max(0.0)).min(1.0)));
            }
        }
        if self.saturation != 0.0 || self.vibrance != 0.0 || self.hue != 0.0 {
            lin = self.chroma(lin);
        }
        if let Some(nodes) = &self.hue_nodes {
            lin = turn_hues(nodes, lin);
        }
        lin
    }

    /// Vignette and grain, over the whole photo; returns encoded values in 0–1.
    fn finish(&self, mut lin: [f32; 3], uv: [f32; 2], size: [f32; 2]) -> [f32; 3] {
        if self.vignette != 0.0 {
            let [u, v] = self.vignette_geometry.to_output(uv);
            let (u, v) = ((u - 0.5) * 2.0, (v - 0.5) * 2.0);
            let r = (u * u + v * v).sqrt() / std::f32::consts::SQRT_2;
            let t = self.vignette * smoothstep(self.vignette_start, self.vignette_start + 0.5, r);
            lin = if t < 0.0 {
                lin.map(|c| c * (1.0 + t))
            } else {
                lin.map(|c| c + t * (1.0 - c).max(0.0))
            };
        }
        let mut out = lin.map(|c| to_encoded(c.max(0.0)));
        if self.grain != 0.0 {
            let (x, y) = (uv[0] * size[0] / self.grain_cell, uv[1] * size[1] / self.grain_cell);
            let n = value_noise(x, y);
            // Each color channel's own noise, as a color film's three layers have.
            let own = if self.grain_color > 0.0 {
                [value_noise(x + 37.0, y + 17.0), n, value_noise(x + 59.0, y + 91.0)]
            } else {
                [n; 3]
            };
            let amount = if self.grain_midtones {
                let v = (out[0] + out[1] + out[2]) / 3.0;
                self.grain * (0.35 + 0.65 * 4.0 * v.clamp(0.0, 1.0) * (1.0 - v.clamp(0.0, 1.0)))
            } else {
                self.grain
            };
            for (c, own) in out.iter_mut().zip(own) {
                *c += (n + (own - n) * self.grain_color) * amount * 0.2;
            }
        }
        out.map(|c| c.clamp(0.0, 1.0))
    }

    /// White balance, exposure and tone: the steps before the curves, on linear light.
    fn light(&self, mut lin: [f32; 3]) -> [f32; 3] {
        // White balance and exposure: multiplications on linear light.
        for (c, gain) in lin.iter_mut().zip(self.gains) {
            *c *= gain * self.exposure;
        }
        // Tone on the perceptual luminance, keeping the RGB ratios so hues don't shift.
        let y = luminance(lin).max(0.0);
        let toned = to_linear(self.tone(to_encoded(y)).max(0.0));
        if y > 1e-6 {
            lin.map(|c| c * toned / y)
        } else {
            [toned; 3]
        }
    }

    /// The histogram of what the curves receive from `pixels` (sRGB-encoded, 0–1) (REQ-021-18).
    pub fn histogram(&self, pixels: impl Iterator<Item = [f32; 3]>) -> Histogram {
        let mut histogram = Histogram::default();
        for rgb in pixels {
            // What the LUT made, as the curves get it (REQ-043-02).
            for (c, value) in self.light(self.through_lut(rgb).map(to_linear)).into_iter().enumerate() {
                let value = to_encoded(value.max(0.0)).min(1.0);
                let bin = ((value * HISTOGRAM_BINS as f32) as usize).min(HISTOGRAM_BINS - 1);
                histogram.channels[c][bin] += 1;
            }
        }
        histogram
    }

    /// The tone curve of blacks, whites, brightness, shadows, highlights and contrast, on a perceptual value.
    fn tone(&self, l: f32) -> f32 {
        let mut l = (l - self.black) / (self.white - self.black);
        if self.brightness != 1.0 && l > 0.0 && l < 1.0 {
            l = l.powf(self.brightness);
        }
        let t = l.clamp(0.0, 1.0);
        // Masks peaking at 1/3 and 2/3 of the range, zero at black and white.
        l += self.shadows * 6.75 * t * (1.0 - t) * (1.0 - t);
        l += self.highlights * 6.75 * (1.0 - t) * t * t;
        if self.contrast >= 0.0 {
            let x = l.clamp(0.0, 1.0);
            l + self.contrast * (x * x * (3.0 - 2.0 * x) - x)
        } else {
            l + self.contrast * 0.5 * (l - 0.5)
        }
    }

    /// Saturation and vibrance as chroma scaling in OkLab; vibrance favours muted colors and spares skin tones. Then
    /// the hue turns the colors around OkLab's lightness axis, keeping lightness and chroma (REQ-021-20).
    fn chroma(&self, lin: [f32; 3]) -> [f32; 3] {
        let [l, a, b] = to_oklab(lin);
        let chroma = (a * a + b * b).sqrt();
        let mut factor = 1.0 + self.saturation;
        if self.vibrance != 0.0 {
            let muted = 1.0 - smoothstep(0.0, 0.25, chroma);
            let hue = b.atan2(a).to_degrees();
            let skin = (-((hue - 60.0) / 25.0).powi(2)).exp();
            let protect = if self.vibrance > 0.0 { 1.0 - 0.7 * skin } else { 1.0 };
            factor *= 1.0 + self.vibrance * muted * protect;
        }
        let (a, b) = (a * factor, b * factor);
        let (sin, cos) = self.hue.sin_cos();
        from_oklab([l, a * cos - b * sin, a * sin + b * cos])
    }
}
