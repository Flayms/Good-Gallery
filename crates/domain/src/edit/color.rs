//! Color and noise helpers of the formulas: histogram, white balance, transfer, Oklab, grain.

use super::*;

/// Bins of a [`Histogram`].
pub const HISTOGRAM_BINS: usize = 128;

/// Counts of the red, green and blue values in [`HISTOGRAM_BINS`] even steps from 0 to 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Histogram {
    pub(super) channels: [[u32; HISTOGRAM_BINS]; 3],
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            channels: [[0; HISTOGRAM_BINS]; 3],
        }
    }
}

impl Histogram {
    /// One channel's counts; for [`Channel::All`] the three channels' together.
    pub fn get(&self, channel: Channel) -> [u32; HISTOGRAM_BINS] {
        match channel {
            Channel::All => std::array::from_fn(|i| self.channels.iter().map(|c| c[i]).sum()),
            Channel::Red => self.channels[0],
            Channel::Green => self.channels[1],
            Channel::Blue => self.channels[2],
        }
    }
}

pub(super) fn lookup(table: &[[f32; 3]], channel: usize, x: f32) -> f32 {
    let at = x.clamp(0.0, 1.0) * (CURVE_SAMPLES - 1) as f32;
    let i = (at.floor() as usize).min(CURVE_SAMPLES - 2);
    let t = at - i as f32;
    table[i][channel] * (1.0 - t) + table[i + 1][channel] * t
}

/// Gains for a temperature and tint (−100 to +100): warmer is more red and less blue, more tint is less green; the
/// luminance of white stays.
pub fn white_balance_gains(temperature: f32, tint: f32) -> [f32; 3] {
    let t = temperature / 100.0 * WHITE_BALANCE_STOPS;
    let n = tint / 100.0 * WHITE_BALANCE_STOPS;
    let gains = [t.exp2(), (-n).exp2(), (-t).exp2()];
    let y = luminance(gains);
    gains.map(|g| g / y)
}

/// The temperature and tint that turn `rgb` (sRGB-encoded, as stored) neutral grey (REQ-021-05).
pub fn neutralize(rgb: [f32; 3]) -> (f32, f32) {
    let [r, g, b] = rgb.map(|c| to_linear(c).max(1e-4).log2());
    let t = (b - r) / 2.0;
    let n = g - (r + b) / 2.0;
    let scale = 100.0 / WHITE_BALANCE_STOPS;
    ((t * scale).clamp(-100.0, 100.0), (n * scale).clamp(-100.0, 100.0))
}

/// sRGB's transfer curve, extended beyond 1.
pub fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn to_encoded(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

pub(super) fn luminance([r, g, b]: [f32; 3]) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

pub(super) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The size of halation's blur, in parts of the photo's longer side (REQ-028-08).
pub(super) const HALATION_SIZE: f32 = 0.006;
/// Halation's color, on linear light: the red layer catches most of what scatters back.
pub(super) const HALATION_COLOR: [f32; 3] = [1.0, 0.22, 0.06];

/// A film look's change per hue (REQ-028-07), blended between the two nearest nodes. Greys have no hue, so the lightness
/// only applies as a color gets some chroma.
pub(super) fn turn_hues(nodes: &[[f32; 3]; crate::look::HUE_NODES], lin: [f32; 3]) -> [f32; 3] {
    let [l, a, b] = to_oklab(lin);
    let at = (b.atan2(a) / std::f32::consts::TAU).rem_euclid(1.0) * crate::look::HUE_NODES as f32;
    let (i, f) = (at.floor() as usize % crate::look::HUE_NODES, at.fract());
    let (low, high) = (nodes[i], nodes[(i + 1) % crate::look::HUE_NODES]);
    let [turn, chroma, light] = std::array::from_fn(|k| low[k] + (high[k] - low[k]) * f);
    let colorful = ((a * a + b * b).sqrt() / 0.1).min(1.0);
    let (sin, cos) = turn.sin_cos();
    let (a, b) = (a * chroma, b * chroma);
    from_oklab([l + light * colorful, a * cos - b * sin, a * sin + b * cos])
}

pub(super) fn to_oklab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let [l, m, s] = [l, m, s].map(f32::cbrt);
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

pub(super) fn from_oklab([l, a, b]: [f32; 3]) -> [f32; 3] {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let [l, m, s] = [l_, m_, s_].map(|v| v * v * v);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

/// A hash of a lattice point, the same on the GPU (wrapping u32 arithmetic).
pub fn hash(x: u32, y: u32) -> u32 {
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

pub(super) fn lattice(x: i32, y: i32) -> f32 {
    (hash(x as u32, y as u32) >> 8) as f32 / 16_777_216.0 - 0.5
}

/// Smooth value noise in −0.5…0.5, one lattice cell per unit.
pub fn value_noise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (ix, iy) = (ix as i32, iy as i32);
    let top = lattice(ix, iy) * (1.0 - sx) + lattice(ix + 1, iy) * sx;
    let bottom = lattice(ix, iy + 1) * (1.0 - sx) + lattice(ix + 1, iy + 1) * sx;
    top * (1.0 - sy) + bottom * sy
}

/// Triangular dither for quantizing to `levels` steps at pixel `(x, y)`: −1…+1 steps, the same each time.
pub fn dither(x: u32, y: u32, channel: u32) -> f32 {
    let a = (hash(x, y.wrapping_mul(3).wrapping_add(channel)) >> 8) as f32 / 16_777_216.0;
    let b = (hash(y.wrapping_add(0x9e37), x.wrapping_mul(7).wrapping_add(channel)) >> 8) as f32 / 16_777_216.0;
    a - b
}
