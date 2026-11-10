//! Blurs: the kinds, the pyramid they sample and the lens blur (spec 023).

use super::*;

/// How a photo is blurred (REQ-023-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlurKind {
    #[default]
    None,
    /// Everything but the subject (spec 022).
    Background,
    /// All but a horizontal band through the focus.
    TiltShift,
    /// All but a round spot around the focus.
    Radial,
}

impl BlurKind {
    pub const ALL: [BlurKind; 4] = [Self::None, Self::Background, Self::TiltShift, Self::Radial];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Background => "Behind subject",
            Self::TiltShift => "Tilt-shift",
            Self::Radial => "Radial",
        }
    }

    /// Whether it has a focus to place, a size and a transition.
    pub fn has_focus(self) -> bool {
        matches!(self, Self::TiltShift | Self::Radial)
    }
}

pub(super) fn look_or_none<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<crate::look::LookChoice>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}

pub(super) fn blur_kind_or_none<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<BlurKind, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// The strongest blur, as σ in parts of the photo's longer side.
pub(super) const MAX_BLUR: f32 = 0.015;
/// The σ, in pixels of a pyramid's base, of one level of [`Pyramid`] (a 2 × 2 box per level, then a B-spline).
pub const BLUR_SIGMA_PER_LEVEL: f32 = 0.65;

/// The level of detail of a [`Pyramid`] with `levels` levels for a blur of `sigma` pixels of its base (REQ-023-04).
pub fn blur_lod(sigma: f32, levels: usize) -> f32 {
    if sigma <= 0.0 {
        return 0.0;
    }
    (sigma / BLUR_SIGMA_PER_LEVEL)
        .log2()
        .clamp(0.0, levels.saturating_sub(1) as f32)
}

/// A photo halved again and again by 2 × 2 boxes while its shorter side is at least 32 px, as the GPU's mip chain
/// (`app::textures::mip_chain`); blurs sample it with a cubic B-spline (REQ-023-04).
#[derive(Debug, Clone)]
pub struct Pyramid {
    /// From the second level on: the first is the photo itself.
    levels: Vec<(usize, usize, Vec<[f32; 4]>)>,
}

impl Pyramid {
    /// From the photo's RGBA values (as stored, 0–1), row by row.
    pub fn new(width: u32, height: u32, pixels: Vec<[f32; 4]>) -> Self {
        let mut levels = Vec::new();
        let (mut w, mut h) = (width as usize, height as usize);
        let mut source = pixels;
        while w.min(h) >= 32 {
            let (nw, nh) = (w / 2, h / 2);
            let half: Vec<[f32; 4]> = (0..nw * nh)
                .map(|i| {
                    let (x, y) = (2 * (i % nw), 2 * (i / nw));
                    let at = |x: usize, y: usize| source[y * w + x];
                    let (a, b, c, d) = (at(x, y), at(x + 1, y), at(x, y + 1), at(x + 1, y + 1));
                    std::array::from_fn(|k| (a[k] + b[k] + c[k] + d[k]) / 4.0)
                })
                .collect();
            levels.push((nw, nh, half.clone()));
            source = half;
            (w, h) = (nw, nh);
        }
        Self { levels }
    }

    /// Levels, the photo itself included.
    pub fn levels(&self) -> usize {
        self.levels.len() + 1
    }

    /// The photo at `uv` blurred to level of detail `lod`: `sharp` (the pixel itself) at 0, between levels a mix.
    pub fn blurred(&self, sharp: [f32; 4], uv: [f32; 2], lod: f32) -> [f32; 4] {
        let lod = lod.clamp(0.0, self.levels.len() as f32);
        if lod <= 0.0 {
            return sharp;
        }
        let low = lod.floor() as usize;
        let t = lod - low as f32;
        let below = if low == 0 { sharp } else { self.bspline(low, uv) };
        if t <= 0.0 {
            return below;
        }
        let above = self.bspline(low + 1, uv);
        std::array::from_fn(|k| below[k] + (above[k] - below[k]) * t)
    }

    /// Between the levels around `lod` (1 or more) at `uv`, each read bilinearly, clamped at the edges: what a GPU's
    /// trilinear sampler reads (REQ-023-05).
    pub fn trilinear(&self, uv: [f32; 2], lod: f32) -> [f32; 4] {
        let lod = lod.clamp(1.0, self.levels.len() as f32);
        let low = lod.floor() as usize;
        let t = lod - low as f32;
        let below = self.bilinear(low, uv);
        if t <= 0.0 {
            return below;
        }
        let above = self.bilinear(low + 1, uv);
        std::array::from_fn(|k| below[k] + (above[k] - below[k]) * t)
    }

    /// Level `level` (1 or more) at `uv`, between its four nearest pixels, clamped at the edges.
    fn bilinear(&self, level: usize, uv: [f32; 2]) -> [f32; 4] {
        let (w, h, pixels) = &self.levels[level - 1];
        let (px, py) = (uv[0] * *w as f32 - 0.5, uv[1] * *h as f32 - 0.5);
        let (ix, iy) = (px.floor(), py.floor());
        let (fx, fy) = (px - ix, py - iy);
        let at = |x: f32, y: f32| {
            let x = (x as i64).clamp(0, *w as i64 - 1) as usize;
            let y = (y as i64).clamp(0, *h as i64 - 1) as usize;
            pixels[y * w + x]
        };
        let (a, b, c, d) = (at(ix, iy), at(ix + 1.0, iy), at(ix, iy + 1.0), at(ix + 1.0, iy + 1.0));
        std::array::from_fn(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bottom = c[k] + (d[k] - c[k]) * fx;
            top + (bottom - top) * fy
        })
    }

    /// Level `level` (1 or more) at `uv`, a cubic B-spline over its pixels, clamped at the edges.
    fn bspline(&self, level: usize, uv: [f32; 2]) -> [f32; 4] {
        let (w, h, pixels) = &self.levels[level - 1];
        let (px, py) = (uv[0] * *w as f32 - 0.5, uv[1] * *h as f32 - 0.5);
        let (ix, iy) = (px.floor(), py.floor());
        let (wx, wy) = (bspline_weights(px - ix), bspline_weights(py - iy));
        let mut out = [0.0; 4];
        for (j, wy) in wy.into_iter().enumerate() {
            let y = (iy as i64 - 1 + j as i64).clamp(0, *h as i64 - 1) as usize;
            for (i, wx) in wx.into_iter().enumerate() {
                let x = (ix as i64 - 1 + i as i64).clamp(0, *w as i64 - 1) as usize;
                let pixel = pixels[y * w + x];
                for k in 0..4 {
                    out[k] += pixel[k] * wx * wy;
                }
            }
        }
        out
    }
}

/// Taps of the lens blur's disc (REQ-023-05), on a golden-angle spiral that covers it evenly.
pub const LENS_TAPS: u32 = 48;
/// A lens disc's radius per σ of the soft blur, so the rest of the photo blurs about as much.
pub const LENS_RADIUS_PER_SIGMA: f32 = 2.0;
/// How much more than a midtone a full highlight weighs, so it spreads into a bright disc.
pub const LENS_BOOST: f32 = 40.0;
/// The angle between a spiral's taps.
pub(super) const GOLDEN_ANGLE: f32 = 2.399_963;

/// What a pixel (as stored, 0–1) weighs in the lens blur: highlights far more than the rest (REQ-023-05).
pub fn lens_weight([r, g, b]: [f32; 3]) -> f32 {
    let t = ((r.max(g).max(b) - 0.7) / 0.3).clamp(0.0, 1.0);
    1.0 + LENS_BOOST * t * t
}

/// A pixel times its lens weight, and the weight: what a lens pyramid holds, so halving keeps a light's share.
pub fn lens_weighted([r, g, b, _]: [f32; 4]) -> [f32; 4] {
    let w = lens_weight([r, g, b]);
    [r * w, g * w, b * w, w]
}

/// The level of detail a lens tap samples for a disc of `radius` pixels: wide enough that the taps cover the disc
/// without gaps, and never the base itself, which a lens pyramid needn't hold (REQ-023-05).
pub fn lens_lod(radius: f32, levels: usize) -> f32 {
    let spacing = radius * (std::f32::consts::PI / LENS_TAPS as f32).sqrt();
    blur_lod(spacing, levels).max(1.0)
}

/// Where tap `i` of a lens disc lies, in parts of its radius.
pub fn lens_tap(i: u32) -> [f32; 2] {
    let r = ((i as f32 + 0.5) / LENS_TAPS as f32).sqrt();
    let angle = i as f32 * GOLDEN_ANGLE;
    [r * angle.cos(), r * angle.sin()]
}

/// The lens blur at `uv`: the mean of the disc of `radius` pixels of the base (of `size` pixels), weighted so lights
/// spread into evenly lit discs. `weighted` holds [`lens_weighted`] pixels. `None` for a disc too small to sample, or
/// a photo too small for a pyramid (REQ-023-05).
pub fn lens_blurred(weighted: &Pyramid, uv: [f32; 2], radius: f32, size: [f32; 2]) -> Option<[f32; 3]> {
    if radius < 1.0 || weighted.levels() < 2 {
        return None;
    }
    let lod = lens_lod(radius, weighted.levels());
    let mut sum = [0.0; 4];
    for i in 0..LENS_TAPS {
        let [x, y] = lens_tap(i);
        let tap = [uv[0] + x * radius / size[0], uv[1] + y * radius / size[1]];
        let value = weighted.trilinear(tap, lod);
        for k in 0..4 {
            sum[k] += value[k];
        }
    }
    (sum[3] > 0.0).then(|| [sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3]])
}

/// A pixel `sharp` at `uv` blurred by σ `sigma` pixels of the pyramids' base (of `size` pixels): in discs from `lens`
/// (of [`lens_weighted`] pixels) where given and the disc is large enough, else softly from `pyramid` (REQ-023-04,
/// REQ-023-05). Transparency stays sharp.
pub fn blur_pixel(
    pyramid: &Pyramid,
    lens: Option<&Pyramid>,
    sharp: [f32; 4],
    uv: [f32; 2],
    sigma: f32,
    size: [f32; 2],
) -> [f32; 4] {
    if let Some(lens) = lens
        && let Some([r, g, b]) = lens_blurred(lens, uv, sigma * LENS_RADIUS_PER_SIGMA, size)
    {
        return [r, g, b, sharp[3]];
    }
    pyramid.blurred(sharp, uv, blur_lod(sigma, pyramid.levels()))
}

/// The cubic B-spline's weights of the four pixels around a position `f` (0–1) past the second.
pub(super) fn bspline_weights(f: f32) -> [f32; 4] {
    let (f2, f3) = (f * f, f * f * f);
    [
        (1.0 - f).powi(3) / 6.0,
        (4.0 - 6.0 * f2 + 3.0 * f3) / 6.0,
        (1.0 + 3.0 * f + 3.0 * f2 - 3.0 * f3) / 6.0,
        f3 / 6.0,
    ]
}
