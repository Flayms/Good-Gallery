//! The viewer's photo-tinted chrome (REQ-013-11): a dark, muted version of the item's average color mixed into the
//! glass.

/// Upper bounds of the tint in OKLCH, so text on the tinted glass keeps its contrast (REQ-013-04).
pub const MAX_LIGHTNESS: f32 = 0.28;
pub const MAX_CHROMA: f32 = 0.06;
/// Share of the tint in the glass color.
pub const AMOUNT: f32 = 0.3;

/// An sRGB color, channels 0–255.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oklch {
    pub l: f32,
    pub c: f32,
    /// Hue in degrees.
    pub h: f32,
}

fn to_linear(channel: f32) -> f32 {
    let c = channel / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn from_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    255.0
        * if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
}

/// sRGB → OKLCH (Björn Ottosson's OKLab).
pub fn oklch(Rgb { r, g, b }: Rgb) -> Oklch {
    let (lr, lg, lb) = (to_linear(r), to_linear(g), to_linear(b));
    let l = (0.412_221_47 * lr + 0.536_332_55 * lg + 0.051_445_995 * lb).cbrt();
    let m = (0.211_903_5 * lr + 0.680_699_5 * lg + 0.107_396_96 * lb).cbrt();
    let s = (0.088_302_46 * lr + 0.281_718_85 * lg + 0.629_978_7 * lb).cbrt();
    let big_l = 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s;
    let a = 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s;
    let bb = 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s;
    Oklch {
        l: big_l,
        c: a.hypot(bb),
        h: (bb.atan2(a).to_degrees() + 360.0) % 360.0,
    }
}

/// OKLCH → sRGB, clipped to the gamut.
fn srgb(Oklch { l: big_l, c, h }: Oklch) -> Rgb {
    let (a, b) = (c * h.to_radians().cos(), c * h.to_radians().sin());
    let l = (big_l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m = (big_l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s = (big_l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    Rgb {
        r: from_linear(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s),
        g: from_linear(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s),
        b: from_linear(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s),
    }
}

/// A dark, muted version of a photo's color with the same hue.
pub fn ambient_tint(color: Rgb) -> Rgb {
    let Oklch { l, c, h } = oklch(color);
    srgb(Oklch {
        l: l.min(MAX_LIGHTNESS),
        c: c.min(MAX_CHROMA),
        h,
    })
}

/// `glass` with `amount` of `tint` mixed into its color, channels rounded; the glass keeps its opacity.
pub fn tint_glass(glass: Rgb, tint: Rgb, amount: f32) -> Rgb {
    let mix = |base: f32, add: f32| (base * (1.0 - amount) + add * amount).round();
    Rgb {
        r: mix(glass.r, tint.r),
        g: mix(glass.g, tint.g),
        b: mix(glass.b, tint.b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: f32, g: f32, b: f32) -> Rgb {
        Rgb { r, g, b }
    }

    #[test]
    fn req_013_11_darkens_and_desaturates_bright_colors() {
        for color in [
            rgb(255.0, 255.0, 255.0),
            rgb(255.0, 230.0, 0.0),
            rgb(0.0, 255.0, 255.0),
            rgb(255.0, 0.0, 0.0),
        ] {
            let tint = oklch(ambient_tint(color));
            assert!(tint.l <= MAX_LIGHTNESS + 0.01, "{color:?}: {tint:?}");
            assert!(tint.c <= MAX_CHROMA + 0.005, "{color:?}: {tint:?}");
        }
    }

    #[test]
    fn req_013_11_keeps_the_hue_of_the_photo() {
        let orange = rgb(249.0, 115.0, 22.0);
        assert!((oklch(ambient_tint(orange)).h - oklch(orange).h).abs() < 0.5);
    }

    #[test]
    fn req_013_11_leaves_dark_muted_colors_as_they_are() {
        let tint = ambient_tint(rgb(30.0, 32.0, 38.0));
        assert!(
            (tint.r - 30.0).abs() < 0.5 && (tint.g - 32.0).abs() < 0.5 && (tint.b - 38.0).abs() < 0.5,
            "{tint:?}"
        );
    }

    #[test]
    fn req_013_11_mixes_the_tint_into_the_glass() {
        assert_eq!(
            tint_glass(rgb(28.0, 28.0, 28.0), rgb(128.0, 28.0, 28.0), 0.25),
            rgb(53.0, 28.0, 28.0)
        );
    }
}
