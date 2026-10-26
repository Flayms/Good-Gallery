//! Design tokens from `docs/design.md` and their egui style. Values live here; the guide explains them.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle};
use eframe::epaint::text::{FontTweak, VariationCoords};
use std::sync::Arc;

// Surfaces and colors. Translucent values are premultiplied: `rgb(255 255 255 / 0.10)` is 26 per channel.
pub const DARKROOM: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
pub const RECESS: Color32 = Color32::from_rgb(0x17, 0x17, 0x17);
pub const RAISED: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub const PAPER: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
pub const MIST: Color32 = Color32::from_rgb(0xb4, 0xb4, 0xb4);
pub const STAR: Color32 = Color32::from_rgb(0xf5, 0xc4, 0x51);
pub const ALERT: Color32 = Color32::from_rgb(0xf2, 0x70, 0x6a);
pub const RIM: Color32 = Color32::from_rgba_premultiplied(26, 26, 26, 26);
pub const HOVER: Color32 = Color32::from_rgba_premultiplied(15, 15, 15, 15);
pub const SELECTED: Color32 = Color32::from_rgba_premultiplied(31, 31, 31, 31);
pub const FIELD: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 89);

// Type scale (px). Don't add sizes in between.
pub const SIZE_CAPTION: f32 = 12.0;
pub const SIZE_BODY: f32 = 14.0;
pub const SIZE_TITLE: f32 = 16.0;
pub const SIZE_SECTION: f32 = 18.0;
pub const SIZE_PAGE: f32 = 24.0;
pub const SIZE_HEADLINE: f32 = 36.0;

// Shape and space.
pub const RADIUS_CONTROL: u8 = 6;
pub const RADIUS_PANEL: u8 = 12;
pub const SPACE: f32 = 4.0;
pub const CONTROL_HEIGHT: f32 = 32.0;
pub const ICON_SIZE: f32 = 16.0;
/// In the icon's 24-unit grid (lucide), so it scales with the icon size.
pub const ICON_STROKE: f32 = 1.75;

// Motion (seconds).
pub const DURATION_FAST: f32 = 0.12;
pub const DURATION_BASE: f32 = 0.2;
pub const DURATION_HERO: f32 = 0.32;

const REGULAR: &str = "ahn-400";
const SEMIBOLD: &str = "ahn-600";
const NUMBERS: &str = "ahn-tnum-400";
const NUMBERS_SEMIBOLD: &str = "ahn-tnum-600";

/// Weight 600: active items, labels of groups, titles.
pub fn semibold() -> FontFamily {
    FontFamily::Name(SEMIBOLD.into())
}

/// Tabular figures, for numbers that line up or change in place.
pub fn numbers() -> FontFamily {
    FontFamily::Name(NUMBERS.into())
}

pub fn numbers_semibold() -> FontFamily {
    FontFamily::Name(NUMBERS_SEMIBOLD.into())
}

/// Installs fonts and style. Call once per context.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, style);
}

/// Atkinson Hyperlegible Next at 400 and 600 from one variable TTF, plus a copy with `tnum` baked in (egui can't
/// request OpenType features). egui's default fonts stay as fallbacks for symbols the font lacks.
fn fonts() -> FontDefinitions {
    static AHN: &[u8] = include_bytes!("../assets/AtkinsonHyperlegibleNext.ttf");
    static AHN_TNUM: &[u8] = include_bytes!("../assets/AtkinsonHyperlegibleNext-Tnum.ttf");
    let face = |bytes: &'static [u8], weight: f32| {
        Arc::new(FontData::from_static(bytes).tweak(FontTweak {
            coords: VariationCoords::new([(b"wght", weight)]),
            ..Default::default()
        }))
    };
    let mut fonts = FontDefinitions::default();
    for (name, bytes, weight) in [
        (REGULAR, AHN, 400.0),
        (SEMIBOLD, AHN, 600.0),
        (NUMBERS, AHN_TNUM, 400.0),
        (NUMBERS_SEMIBOLD, AHN_TNUM, 600.0),
    ] {
        fonts.font_data.insert(name.into(), face(bytes, weight));
    }
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    for (family, first) in [
        (FontFamily::Proportional, REGULAR),
        (semibold(), SEMIBOLD),
        (numbers(), NUMBERS),
        (numbers_semibold(), NUMBERS_SEMIBOLD),
    ] {
        let names = std::iter::once(first.to_string()).chain(fallback.iter().cloned());
        fonts.families.insert(family, names.collect());
    }
    fonts
}

fn style(style: &mut egui::Style) {
    style.text_styles = [
        (TextStyle::Small, FontId::new(SIZE_CAPTION, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(SIZE_BODY, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(SIZE_BODY, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(SIZE_TITLE, semibold())),
        (TextStyle::Monospace, FontId::new(SIZE_BODY, numbers())),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(2.0 * SPACE, 2.0 * SPACE);
    style.spacing.button_padding = egui::vec2(3.0 * SPACE, 1.5 * SPACE);
    style.spacing.interact_size.y = CONTROL_HEIGHT;
    style.animation_time = DURATION_FAST;

    let v = &mut style.visuals;
    v.dark_mode = true;
    v.panel_fill = DARKROOM;
    v.window_fill = RAISED;
    v.extreme_bg_color = FIELD;
    v.faint_bg_color = HOVER;
    v.override_text_color = None;
    v.hyperlink_color = PAPER;
    v.error_fg_color = ALERT;
    v.warn_fg_color = STAR;
    v.window_stroke = Stroke::new(1.0, RIM);
    v.window_corner_radius = CornerRadius::same(RADIUS_PANEL);
    v.menu_corner_radius = CornerRadius::same(RADIUS_PANEL);
    v.selection.bg_fill = SELECTED;
    v.selection.stroke = Stroke::new(1.0, PAPER);

    // No brand accent: hover and active states use lightness.
    let widgets = &mut v.widgets;
    for (state, fill) in [
        (&mut widgets.noninteractive, Color32::TRANSPARENT),
        (&mut widgets.inactive, Color32::TRANSPARENT),
        (&mut widgets.hovered, HOVER),
        (&mut widgets.active, SELECTED),
        (&mut widgets.open, SELECTED),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::NONE;
        state.corner_radius = CornerRadius::same(RADIUS_CONTROL);
        state.fg_stroke = Stroke::new(ICON_STROKE, PAPER);
        state.expansion = 0.0;
    }
    // Labels are Paper; secondary text sets Mist itself.
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, PAPER);
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, RIM);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A context with the theme installed and one frame run, so fonts are loaded.
    fn themed() -> egui::Context {
        let ctx = egui::Context::default();
        install(&ctx);
        // Unapplied texture deltas panic when dropped in debug builds.
        ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
        ctx
    }

    fn text_width(ctx: &egui::Context, family: FontFamily) -> f32 {
        let font = FontId::new(SIZE_BODY, family);
        ctx.fonts_mut(|f| {
            f.layout_no_wrap("IMG_0011 Il1 O0".into(), font, Color32::WHITE)
                .size()
                .x
        })
    }

    #[test]
    fn install_uses_the_bundled_font_at_both_weights() {
        let ctx = themed();
        let default = egui::Context::default();
        default.run_ui(Default::default(), |_| {}).textures_delta.clear();

        let regular = text_width(&ctx, FontFamily::Proportional);
        assert_ne!(regular, text_width(&default, FontFamily::Proportional));
        assert!(text_width(&ctx, semibold()) > regular);
        assert!(text_width(&ctx, numbers_semibold()) > text_width(&ctx, numbers()));
    }

    #[test]
    fn tabular_figures_have_equal_widths() {
        let ctx = themed();
        let widths: Vec<f32> = "0123456789"
            .chars()
            .map(|c| ctx.fonts_mut(|f| f.glyph_width(&FontId::new(SIZE_BODY, numbers()), c)))
            .collect();
        assert!(widths.iter().all(|w| (w - widths[0]).abs() < 0.01), "{widths:?}");
    }
}
