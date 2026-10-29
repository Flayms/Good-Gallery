//! Design tokens from `docs/design.md` and their egui style. Values live here; the guide explains them.

use domain::ambient::Rgb;
use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Rect, Stroke, StrokeKind, TextStyle,
};
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
/// Focus: a 2 px Paper ring with a 2 px `rgb(0 0 0 / 0.6)` ring outside it, visible over any photo (REQ-013-05).
pub const RING: Color32 = PAPER;
pub const RING_OUTER: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 153);
pub const RING_WIDTH: f32 = 2.0;
/// Scrollbar thumb: white at 24 %, 40 % hovered or dragged.
pub const SCROLLBAR_THUMB: Color32 = Color32::from_rgba_premultiplied(61, 61, 61, 61);
pub const SCROLLBAR_THUMB_HOVER: Color32 = Color32::from_rgba_premultiplied(102, 102, 102, 102);

/// A translucent surface: its color and opacity before the item's tint (REQ-013-11), its blur radius and saturation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glass {
    pub color: Rgb,
    pub alpha: f32,
    /// Blur radius in points.
    pub blur: f32,
    pub saturate: f32,
}

impl Glass {
    /// Its color as egui paints it (premultiplied), optionally tinted.
    pub fn fill(&self, tint: Option<Rgb>) -> Color32 {
        let Rgb { r, g, b } = match tint {
            Some(tint) => domain::ambient::tint_glass(self.color, tint, domain::ambient::AMOUNT),
            None => self.color,
        };
        Color32::from_rgba_unmultiplied(r as u8, g as u8, b as u8, (self.alpha * 255.0).round() as u8)
    }
}

/// Toolbar, viewer control bar, viewer arrows, filmstrip: `rgb(28 28 28 / 0.72)`, blur 24, saturate 160 %.
pub const GLASS_BAR: Glass = Glass {
    color: Rgb {
        r: 28.0,
        g: 28.0,
        b: 28.0,
    },
    alpha: 0.72,
    blur: 24.0,
    saturate: 1.6,
};
/// Menus, popovers, dialogs, the info panel: `rgb(36 36 36 / 0.86)`, blur 32, saturate 140 %.
pub const GLASS_PANEL: Glass = Glass {
    color: Rgb {
        r: 36.0,
        g: 36.0,
        b: 36.0,
    },
    alpha: 0.86,
    blur: 32.0,
    saturate: 1.4,
};
/// The whole window behind the viewer: `rgb(0 0 0 / 0.5)`, blur 28, saturate 120 % (REQ-013-14).
pub const SCRIM: Glass = Glass {
    color: Rgb { r: 0.0, g: 0.0, b: 0.0 },
    alpha: 0.5,
    blur: 28.0,
    saturate: 1.2,
};

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

/// Whether transitions animate: off with Windows' animation effects off (and in tests).
pub fn motion(ctx: &egui::Context) -> bool {
    ctx.global_style().animation_time > 0.0
}

/// Turns animations off for reduced motion (REQ-013-07), or back on.
pub fn set_reduced_motion(ctx: &egui::Context, reduced: bool) {
    let time = if reduced { 0.0 } else { DURATION_FAST };
    ctx.all_styles_mut(|style| style.animation_time = time);
}

fn transparency_id() -> egui::Id {
    egui::Id::unique("reduced transparency")
}

/// Makes glass opaque for reduced transparency (REQ-013-06), or translucent again.
pub fn set_reduced_transparency(ctx: &egui::Context, reduced: bool) {
    ctx.data_mut(|data| data.insert_temp(transparency_id(), reduced));
}

/// Whether glass is opaque because the OS asks for reduced transparency.
pub fn reduced_transparency(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp(transparency_id())).unwrap_or(false)
}

/// The OS asks for reduced motion: Windows' _Animation effects_, GNOME's animations, or the browser's
/// `prefers-reduced-motion`.
fn os_reduced_motion() -> bool {
    #[cfg(target_arch = "wasm32")]
    return crate::web::js::prefers_reduced_motion();
    #[cfg(not(target_arch = "wasm32"))]
    return media::helpers::reduced_motion();
}

/// The OS asks for reduced transparency: Windows' _Transparency effects_, or the browser's
/// `prefers-reduced-transparency`.
fn os_reduced_transparency() -> bool {
    #[cfg(target_arch = "wasm32")]
    return crate::web::js::prefers_reduced_transparency();
    #[cfg(not(target_arch = "wasm32"))]
    return media::helpers::reduced_transparency();
}

/// Windows' _Animation effects_ and _Transparency effects_, read again at most every 2 s while the app draws frames,
/// so it follows them while it runs (spec 013, Assumptions).
#[derive(Debug, Default)]
pub struct OsPreferences {
    checked: Option<web_time::Instant>,
    reduced_motion: Option<bool>,
    reduced_transparency: Option<bool>,
}

impl OsPreferences {
    const EVERY: std::time::Duration = std::time::Duration::from_secs(2);

    /// Reads the OS settings when due and applies what changed. Call once per frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        if self.checked.is_some_and(|at| at.elapsed() < Self::EVERY) {
            return;
        }
        self.checked = Some(web_time::Instant::now());
        let motion = os_reduced_motion();
        if self.reduced_motion != Some(motion) {
            self.reduced_motion = Some(motion);
            set_reduced_motion(ctx, motion);
        }
        let transparency = os_reduced_transparency();
        if self.reduced_transparency != Some(transparency) {
            self.reduced_transparency = Some(transparency);
            set_reduced_transparency(ctx, transparency);
            ctx.request_repaint();
        }
        // No timer of its own: an idle window shouldn't wake every 2 s. A change shows with the next frame.
    }
}

/// The focus ring around `rect` (REQ-013-05): Paper on the edge outwards, the dark ring outside it.
pub fn focus_ring(painter: &egui::Painter, rect: Rect, radius: f32) {
    let outer = rect.expand(RING_WIDTH);
    painter.rect_stroke(
        outer,
        radius + RING_WIDTH,
        Stroke::new(RING_WIDTH, RING_OUTER),
        StrokeKind::Outside,
    );
    painter.rect_stroke(rect, radius, Stroke::new(RING_WIDTH, RING), StrokeKind::Outside);
}

fn focus_ring_id() -> egui::Id {
    egui::Id::unique("focus ring")
}

/// Draws the focus ring around the widget with keyboard focus, on its layer and over it; text inputs keep their own
/// border highlight. Call once per frame, after everything is drawn.
pub fn draw_focus_ring(ctx: &egui::Context) {
    let focused = ctx
        .memory(|memory| memory.focused())
        .and_then(|id| ctx.read_response(id));
    let shown = focused
        .filter(|response| {
            let text_input = egui::text_edit::TextEditState::load(ctx, response.id).is_some();
            !text_input && response.rect.is_positive()
        })
        .map(|response| {
            let painter = ctx.layer_painter(response.layer_id);
            focus_ring(&painter, response.rect, f32::from(RADIUS_CONTROL));
            response.rect
        });
    ctx.data_mut(|data| match shown {
        Some(rect) => {
            data.insert_temp(focus_ring_id(), rect);
        }
        None => {
            data.remove::<Rect>(focus_ring_id());
        }
    });
}

/// Where the focus ring was drawn in the last frame (for tests).
pub fn focus_ring_rect(ctx: &egui::Context) -> Option<Rect> {
    ctx.data(|data| data.get_temp(focus_ring_id()))
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
    // With Windows' animation effects off, transitions are instant (`docs/design.md`); see [`motion`].
    style.animation_time = if os_reduced_motion() { 0.0 } else { DURATION_FAST };

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
    // The app's own scrollbar: a thin pill on a transparent track (`docs/design.md`).
    let bar = &mut style.spacing.scroll;
    bar.floating = true;
    bar.bar_width = 8.0;
    bar.floating_width = 3.0;
    bar.floating_allocated_width = 0.0;
    bar.bar_inner_margin = 2.0;
    bar.bar_outer_margin = 2.0;
    bar.handle_min_length = 32.0;
    bar.foreground_color = false;
    bar.dormant_background_opacity = 0.0;
    bar.active_background_opacity = 0.0;
    bar.interact_background_opacity = 0.0;
    bar.dormant_handle_opacity = 1.0;
    bar.active_handle_opacity = 1.0;
    bar.interact_handle_opacity = 1.0;

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
    // Scroll handles take the widgets' fill: the thumb at 24 %, 40 % hovered or dragged.
    widgets.inactive.bg_fill = SCROLLBAR_THUMB;
    widgets.hovered.bg_fill = SCROLLBAR_THUMB_HOVER;
    widgets.active.bg_fill = SCROLLBAR_THUMB_HOVER;
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

    /// An opaque color, channels 0–255.
    #[derive(Debug, Clone, Copy)]
    struct Opaque([f64; 3]);

    impl Opaque {
        fn of(color: Color32) -> Self {
            assert_eq!(color.a(), 255, "{color:?} isn't opaque");
            Self([color.r(), color.g(), color.b()].map(f64::from))
        }

        /// WCAG relative luminance.
        fn luminance(self) -> f64 {
            let linear = |c: f64| {
                let c = c / 255.0;
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            let [r, g, b] = self.0.map(linear);
            0.2126 * r + 0.7152 * g + 0.0722 * b
        }
    }

    /// `top` (premultiplied, as egui paints it) over `bottom`.
    fn over(top: Color32, bottom: Opaque) -> Opaque {
        let alpha = f64::from(top.a()) / 255.0;
        let top = [top.r(), top.g(), top.b()].map(f64::from);
        Opaque(std::array::from_fn(|i| top[i] + bottom.0[i] * (1.0 - alpha)))
    }

    /// WCAG contrast ratio, 1–21.
    fn contrast(a: Opaque, b: Opaque) -> f64 {
        let (x, y) = (a.luminance(), b.luminance());
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    const WHITE: Opaque = Opaque([255.0; 3]);
    const BLACK: Opaque = Opaque([0.0; 3]);

    /// Photo backgrounds from pure black to pure white, plus saturated colors.
    fn backdrops() -> Vec<Opaque> {
        let greys = (0..52).map(|step| Opaque([f64::from(step) * 5.0; 3]));
        let colors = [
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [255, 255, 0],
            [0, 255, 255],
            [255, 0, 255],
            [0xf5, 0xc4, 0x51],
        ]
        .map(|c: [u8; 3]| Opaque(c.map(f64::from)));
        greys.chain(colors).collect()
    }

    fn text() -> [Opaque; 2] {
        [Opaque::of(PAPER), Opaque::of(MIST)]
    }

    #[test]
    fn req_013_04_text_is_legible_on_the_solid_surfaces() {
        for surface in [DARKROOM, RECESS, RAISED] {
            for color in text() {
                assert!(contrast(color, Opaque::of(surface)) >= 4.5, "{color:?} on {surface:?}");
            }
        }
        // Primary buttons (Darkroom on Paper) and errors.
        assert!(contrast(Opaque::of(DARKROOM), Opaque::of(PAPER)) >= 4.5);
        assert!(contrast(Opaque::of(ALERT), Opaque::of(DARKROOM)) >= 4.5);
    }

    #[test]
    fn req_013_04_text_is_legible_on_glass_over_any_photo() {
        for photo in [WHITE, BLACK] {
            let panel = over(GLASS_PANEL.fill(None), photo);
            for color in text() {
                assert!(contrast(color, panel) >= 4.5, "{color:?} on the panel over {photo:?}");
            }
            // Text on the bar is Paper only; secondary text sits on a Field.
            let bar = over(GLASS_BAR.fill(None), photo);
            let field = over(FIELD, bar);
            assert!(contrast(Opaque::of(PAPER), bar) >= 4.5);
            assert!(contrast(Opaque::of(PAPER), field) >= 4.5);
            assert!(contrast(Opaque::of(MIST), field) >= 4.5);
        }
    }

    /// REQ-013-04, REQ-013-11: also on glass tinted by any photo.
    #[test]
    fn req_013_04_req_013_11_text_is_legible_on_tinted_glass() {
        for photo in backdrops() {
            let [r, g, b] = photo.0.map(|c| c as f32);
            let tint = domain::ambient::ambient_tint(Rgb { r, g, b });
            for backdrop in [WHITE, BLACK] {
                let bar = over(GLASS_BAR.fill(Some(tint)), backdrop);
                let panel = over(GLASS_PANEL.fill(Some(tint)), backdrop);
                assert!(contrast(Opaque::of(PAPER), bar) >= 4.5, "{photo:?}");
                assert!(contrast(Opaque::of(MIST), over(FIELD, bar)) >= 4.5, "{photo:?}");
                for color in text() {
                    assert!(
                        contrast(color, panel) >= 4.5,
                        "{color:?} on the panel tinted by {photo:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn req_013_05_the_focus_ring_shows_over_any_background() {
        for backdrop in backdrops() {
            let inner = contrast(Opaque::of(RING), backdrop);
            let outer = contrast(over(RING_OUTER, backdrop), backdrop);
            assert!(inner.max(outer) >= 3.0, "{backdrop:?}: {inner:.2} / {outer:.2}");
        }
    }

    #[test]
    fn req_013_07_req_013_06_follows_the_os_settings_for_motion_and_transparency() {
        let ctx = themed();
        set_reduced_motion(&ctx, false);
        assert!(motion(&ctx));
        set_reduced_motion(&ctx, true);
        assert!(!motion(&ctx));
        set_reduced_motion(&ctx, false);
        assert!(motion(&ctx));

        assert!(!reduced_transparency(&ctx));
        set_reduced_transparency(&ctx, true);
        assert!(reduced_transparency(&ctx));
    }

    /// REQ-013-01: the app's own dark look whatever the OS theme or accent color.
    #[test]
    fn req_013_01_uses_its_own_colors_whatever_the_os_theme() {
        let ctx = egui::Context::default();
        ctx.options_mut(|options| options.theme_preference = egui::ThemePreference::Light);
        install(&ctx);
        ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        let style = ctx.global_style();
        assert_eq!(style.visuals.panel_fill, DARKROOM);
        assert_eq!(style.visuals.widgets.noninteractive.fg_stroke.color, PAPER);
        assert_eq!(style.visuals.selection.bg_fill, SELECTED);
        assert_eq!(style.text_styles[&TextStyle::Body].family, FontFamily::Proportional);
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
