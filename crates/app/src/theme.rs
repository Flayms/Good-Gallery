//! Design tokens from `docs/design.md` and their egui style. Values live here; the guide explains them.

use domain::ambient::Rgb;
use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Rect, Stroke, StrokeKind, TextStyle,
};
use eframe::epaint::text::{FontTweak, VariationCoords};
use std::sync::Arc;

// Surfaces and colors, per appearance (spec 020). Translucent values are premultiplied: `rgb(255 255 255 / 0.10)` is
// 26 per channel.

/// The colors of one appearance, named by role (`docs/design.md`: Darkroom is `base`, Paper `text`, Mist `muted`).
#[derive(Debug, PartialEq)]
pub struct Palette {
    /// Which egui theme these colors go with.
    pub theme: egui::Theme,
    /// Gallery and settings page.
    pub base: Color32,
    /// Sidebar.
    pub recess: Color32,
    /// Opaque fallback of glass, and surfaces that sit on the page.
    pub raised: Color32,
    /// Tiles and rows before their thumbnail or details are there, and skeletons while loading.
    pub placeholder: Color32,
    /// Text, icons, primary buttons.
    pub text: Color32,
    /// Secondary text, counts.
    pub muted: Color32,
    pub star: Color32,
    pub alert: Color32,
    /// Hairlines, glass edges.
    pub rim: Color32,
    pub hover: Color32,
    pub selected: Color32,
    /// Inputs and the search bar on glass.
    pub field: Color32,
    /// Focus: a 2 px ring in `ring` with a 2 px `ring_outer` ring outside it, visible over any photo (REQ-013-05).
    pub ring: Color32,
    pub ring_outer: Color32,
    /// Scrollbar thumb, and while hovered or dragged.
    pub scrollbar_thumb: Color32,
    pub scrollbar_thumb_hover: Color32,
    /// A slider's track.
    pub track: Color32,
    /// Toolbar, viewer control bar, viewer arrows, filmstrip.
    pub glass_bar: Glass,
    /// Menus, popovers, dialogs, the info panel.
    pub glass_panel: Glass,
}

const fn grey(value: f32) -> Rgb {
    Rgb {
        r: value,
        g: value,
        b: value,
    }
}

/// The dark appearance, the app's original look.
pub const DARK: Palette = Palette {
    theme: egui::Theme::Dark,
    base: Color32::from_rgb(0x1f, 0x1f, 0x1f),
    recess: Color32::from_rgb(0x17, 0x17, 0x17),
    raised: Color32::from_rgb(0x2a, 0x2a, 0x2a),
    placeholder: Color32::from_rgb(0x2a, 0x2a, 0x2a),
    text: Color32::from_rgb(0xf2, 0xf2, 0xf2),
    muted: Color32::from_rgb(0xb4, 0xb4, 0xb4),
    star: Color32::from_rgb(0xf5, 0xc4, 0x51),
    alert: Color32::from_rgb(0xf2, 0x70, 0x6a),
    // White at 10 %, 6 %, 12 %; black at 35 %.
    rim: Color32::from_rgba_premultiplied(26, 26, 26, 26),
    hover: Color32::from_rgba_premultiplied(15, 15, 15, 15),
    selected: Color32::from_rgba_premultiplied(31, 31, 31, 31),
    field: Color32::from_rgba_premultiplied(0, 0, 0, 89),
    ring: Color32::from_rgb(0xf2, 0xf2, 0xf2),
    // Black at 60 %.
    ring_outer: Color32::from_rgba_premultiplied(0, 0, 0, 153),
    // White at 24 % and 40 %, 25 %.
    scrollbar_thumb: Color32::from_rgba_premultiplied(61, 61, 61, 61),
    scrollbar_thumb_hover: Color32::from_rgba_premultiplied(102, 102, 102, 102),
    track: Color32::from_rgba_premultiplied(64, 64, 64, 64),
    // `rgb(28 28 28 / 0.72)`, blur 24, saturate 160 %.
    glass_bar: Glass {
        color: grey(28.0),
        alpha: 0.72,
        blur: 24.0,
        saturate: 1.6,
    },
    // `rgb(36 36 36 / 0.86)`, blur 32, saturate 140 %.
    glass_panel: Glass {
        color: grey(36.0),
        alpha: 0.86,
        blur: 32.0,
        saturate: 1.4,
    },
};

/// The light appearance (spec 020): the same roles in neutral light greys, dark text, and Star and Alert darkened so
/// they keep their contrast.
pub const LIGHT: Palette = Palette {
    theme: egui::Theme::Light,
    base: Color32::from_rgb(0xf4, 0xf4, 0xf4),
    recess: Color32::from_rgb(0xea, 0xea, 0xea),
    raised: Color32::from_rgb(0xff, 0xff, 0xff),
    // A step darker than the page, as Raised is a step lighter in dark: white placeholders would vanish.
    placeholder: Color32::from_rgb(0xe0, 0xe0, 0xe0),
    text: Color32::from_rgb(0x1a, 0x1a, 0x1a),
    muted: Color32::from_rgb(0x59, 0x59, 0x59),
    star: Color32::from_rgb(0x8f, 0x63, 0x00),
    alert: Color32::from_rgb(0xc4, 0x2b, 0x1c),
    // Black at 12 %, 5 %, 10 %; white at 60 %.
    rim: Color32::from_rgba_premultiplied(0, 0, 0, 31),
    hover: Color32::from_rgba_premultiplied(0, 0, 0, 13),
    selected: Color32::from_rgba_premultiplied(0, 0, 0, 26),
    field: Color32::from_rgba_premultiplied(153, 153, 153, 153),
    ring: Color32::from_rgb(0x1a, 0x1a, 0x1a),
    // White at 80 %.
    ring_outer: Color32::from_rgba_premultiplied(204, 204, 204, 204),
    // Black at 24 % and 40 %, 20 %.
    scrollbar_thumb: Color32::from_rgba_premultiplied(0, 0, 0, 61),
    scrollbar_thumb_hover: Color32::from_rgba_premultiplied(0, 0, 0, 102),
    track: Color32::from_rgba_premultiplied(0, 0, 0, 51),
    // `rgb(248 248 248 / 0.72)` and `rgb(255 255 255 / 0.86)`, blurred and saturated as in dark.
    glass_bar: Glass {
        color: grey(248.0),
        ..DARK.glass_bar
    },
    glass_panel: Glass {
        color: grey(255.0),
        ..DARK.glass_panel
    },
};

pub const RING_WIDTH: f32 = 2.0;

thread_local! {
    /// The palette of the frame being drawn. Per thread: the UI runs on one thread, and tests that run in parallel
    /// each draw on their own.
    static PALETTE: std::cell::Cell<&'static Palette> = const { std::cell::Cell::new(&DARK) };
}

/// The palette of the frame being drawn.
pub fn palette() -> &'static Palette {
    PALETTE.with(std::cell::Cell::get)
}

/// The palette of an egui theme.
pub fn palette_of(theme: egui::Theme) -> &'static Palette {
    match theme {
        egui::Theme::Dark => &DARK,
        egui::Theme::Light => &LIGHT,
    }
}

/// Picks the palette of the context's theme (the appearance setting, or the system's, REQ-020-01, REQ-020-02). Call
/// at the start of every frame.
pub fn begin_frame(ctx: &egui::Context) {
    PALETTE.with(|current| current.set(palette_of(ctx.theme())));
}

/// Draws with `palette` inside `draw`, e.g. the viewer in dark whatever the appearance (REQ-020-05).
pub fn with_palette<R>(palette: &'static Palette, draw: impl FnOnce() -> R) -> R {
    let previous = PALETTE.with(|current| current.replace(palette));
    let result = draw();
    PALETTE.with(|current| current.set(previous));
    result
}

/// egui's style for the current palette, for UIs that don't inherit it (menus and popups take the global one).
pub fn apply_style(ui: &mut egui::Ui) {
    let style = ui.ctx().style_of(palette().theme);
    if ui.style().visuals.dark_mode != style.visuals.dark_mode {
        ui.set_style(style);
    }
}

pub fn base() -> Color32 {
    palette().base
}
pub fn recess() -> Color32 {
    palette().recess
}
pub fn raised() -> Color32 {
    palette().raised
}
pub fn placeholder() -> Color32 {
    palette().placeholder
}
pub fn text() -> Color32 {
    palette().text
}
pub fn muted() -> Color32 {
    palette().muted
}
pub fn star() -> Color32 {
    palette().star
}
pub fn alert() -> Color32 {
    palette().alert
}
pub fn rim() -> Color32 {
    palette().rim
}
pub fn hover() -> Color32 {
    palette().hover
}
pub fn selected() -> Color32 {
    palette().selected
}
pub fn field() -> Color32 {
    palette().field
}
pub fn ring() -> Color32 {
    palette().ring
}
pub fn ring_outer() -> Color32 {
    palette().ring_outer
}
pub fn track() -> Color32 {
    palette().track
}
pub fn glass_bar() -> Glass {
    palette().glass_bar
}
pub fn glass_panel() -> Glass {
    palette().glass_panel
}

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

/// The whole window behind the viewer: `rgb(0 0 0 / 0.5)`, blur 28, saturate 120 % (REQ-013-14). The same in both
/// appearances: the viewer stays dark (REQ-020-05).
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

/// The focus ring around `rect` (REQ-013-05, REQ-020-04): the text color on the edge outwards, the contrasting ring
/// outside it.
pub fn focus_ring(painter: &egui::Painter, rect: Rect, radius: f32) {
    let outer = rect.expand(RING_WIDTH);
    painter.rect_stroke(
        outer,
        radius + RING_WIDTH,
        Stroke::new(RING_WIDTH, ring_outer()),
        StrokeKind::Outside,
    );
    painter.rect_stroke(rect, radius, Stroke::new(RING_WIDTH, ring()), StrokeKind::Outside);
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

/// Installs fonts and the style of both appearances. Call once per context. Which one shows follows the system until
/// [`set_appearance`] picks one.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    // Where the system reports no appearance, the app is dark (REQ-020-02).
    ctx.options_mut(|options| options.fallback_theme = egui::Theme::Dark);
    for palette in [&DARK, &LIGHT] {
        ctx.style_mut_of(palette.theme, |s| style(s, palette));
    }
}

/// Applies the appearance setting (REQ-020-01): the system's, light or dark. egui tells the window too, so its frame
/// follows.
pub fn set_appearance(ctx: &egui::Context, appearance: domain::appearance::Appearance) {
    use domain::appearance::Appearance;
    let preference = match appearance {
        Appearance::System => egui::ThemePreference::System,
        Appearance::Light => egui::ThemePreference::Light,
        Appearance::Dark => egui::ThemePreference::Dark,
    };
    if ctx.options(|options| options.theme_preference) != preference {
        ctx.set_theme(preference);
    }
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

fn style(style: &mut egui::Style, palette: &Palette) {
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
    v.dark_mode = palette.theme == egui::Theme::Dark;
    v.panel_fill = palette.base;
    v.window_fill = palette.raised;
    v.extreme_bg_color = palette.field;
    v.faint_bg_color = palette.hover;
    v.override_text_color = None;
    v.hyperlink_color = palette.text;
    v.error_fg_color = palette.alert;
    v.warn_fg_color = palette.star;
    v.window_stroke = Stroke::new(1.0, palette.rim);
    v.window_corner_radius = CornerRadius::same(RADIUS_PANEL);
    v.menu_corner_radius = CornerRadius::same(RADIUS_PANEL);
    v.selection.bg_fill = palette.selected;
    v.selection.stroke = Stroke::new(1.0, palette.text);
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
        (&mut widgets.hovered, palette.hover),
        (&mut widgets.active, palette.selected),
        (&mut widgets.open, palette.selected),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::NONE;
        state.corner_radius = CornerRadius::same(RADIUS_CONTROL);
        state.fg_stroke = Stroke::new(ICON_STROKE, palette.text);
        state.expansion = 0.0;
    }
    // Scroll handles take the widgets' fill: the thumb at 24 %, 40 % hovered or dragged.
    widgets.inactive.bg_fill = palette.scrollbar_thumb;
    widgets.hovered.bg_fill = palette.scrollbar_thumb_hover;
    widgets.active.bg_fill = palette.scrollbar_thumb_hover;
    // Labels are the text color; secondary text sets the muted one itself.
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text);
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.rim);
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

    fn text(palette: &Palette) -> [Opaque; 2] {
        [Opaque::of(palette.text), Opaque::of(palette.muted)]
    }

    /// REQ-013-04, REQ-020-04: in both appearances.
    #[test]
    fn req_013_04_req_020_04_text_is_legible_on_the_solid_surfaces() {
        for palette in [&DARK, &LIGHT] {
            for surface in [palette.base, palette.recess, palette.raised] {
                for color in text(palette) {
                    assert!(contrast(color, Opaque::of(surface)) >= 4.5, "{color:?} on {surface:?}");
                }
                // Hover and selected rows.
                for state in [palette.hover, palette.selected] {
                    for color in text(palette) {
                        let row = over(state, Opaque::of(surface));
                        assert!(contrast(color, row) >= 4.5, "{color:?} on {state:?} over {surface:?}");
                    }
                }
                assert!(
                    contrast(Opaque::of(palette.alert), Opaque::of(surface)) >= 4.5,
                    "{surface:?}"
                );
                // Stars are icons: 3:1.
                assert!(
                    contrast(Opaque::of(palette.star), Opaque::of(surface)) >= 3.0,
                    "{surface:?}"
                );
            }
            // Primary buttons: the page's color on the text color; the offline badge: on Alert.
            assert!(contrast(Opaque::of(palette.base), Opaque::of(palette.text)) >= 4.5);
            assert!(contrast(Opaque::of(palette.base), Opaque::of(palette.alert)) >= 4.5);
        }
    }

    /// REQ-013-04, REQ-020-04: on glass over any photo, from white (dark glass's worst case) to black (light glass's).
    #[test]
    fn req_013_04_req_020_04_text_is_legible_on_glass_over_any_photo() {
        for palette in [&DARK, &LIGHT] {
            for photo in backdrops() {
                let panel = over(palette.glass_panel.fill(None), photo);
                for color in text(palette) {
                    assert!(contrast(color, panel) >= 4.5, "{color:?} on the panel over {photo:?}");
                }
                // Text on the bar is the text color only; secondary text sits on a Field.
                let bar = over(palette.glass_bar.fill(None), photo);
                let field = over(palette.field, bar);
                assert!(
                    contrast(Opaque::of(palette.text), bar) >= 4.5,
                    "on the bar over {photo:?}"
                );
                for color in text(palette) {
                    assert!(contrast(color, field) >= 4.5, "{color:?} on a field over {photo:?}");
                }
            }
        }
    }

    /// REQ-013-04, REQ-013-11: also on glass tinted by any photo, which only the viewer has (dark, REQ-020-05).
    #[test]
    fn req_013_04_req_013_11_text_is_legible_on_tinted_glass() {
        for photo in backdrops() {
            let [r, g, b] = photo.0.map(|c| c as f32);
            let tint = domain::ambient::ambient_tint(Rgb { r, g, b });
            for backdrop in [WHITE, BLACK] {
                let bar = over(DARK.glass_bar.fill(Some(tint)), backdrop);
                let panel = over(DARK.glass_panel.fill(Some(tint)), backdrop);
                assert!(contrast(Opaque::of(DARK.text), bar) >= 4.5, "{photo:?}");
                assert!(
                    contrast(Opaque::of(DARK.muted), over(DARK.field, bar)) >= 4.5,
                    "{photo:?}"
                );
                for color in text(&DARK) {
                    assert!(
                        contrast(color, panel) >= 4.5,
                        "{color:?} on the panel tinted by {photo:?}"
                    );
                }
            }
        }
    }

    /// REQ-013-05, REQ-020-04: in both appearances.
    #[test]
    fn req_013_05_req_020_04_the_focus_ring_shows_over_any_background() {
        for palette in [&DARK, &LIGHT] {
            for backdrop in backdrops() {
                let inner = contrast(Opaque::of(palette.ring), backdrop);
                let outer = contrast(over(palette.ring_outer, backdrop), backdrop);
                assert!(inner.max(outer) >= 3.0, "{backdrop:?}: {inner:.2} / {outer:.2}");
            }
        }
    }

    /// REQ-020-03: photos are the only color: the light surfaces are neutral greys, hue only in Star and Alert.
    #[test]
    fn req_020_03_light_surfaces_are_neutral_grey() {
        let neutral = |color: Color32| color.r() == color.g() && color.g() == color.b();
        let p = &LIGHT;
        for color in [
            p.base,
            p.recess,
            p.raised,
            p.placeholder,
            p.text,
            p.muted,
            p.rim,
            p.hover,
            p.selected,
            p.field,
            p.ring,
            p.ring_outer,
            p.scrollbar_thumb,
            p.scrollbar_thumb_hover,
            p.track,
            p.glass_bar.fill(None),
            p.glass_panel.fill(None),
        ] {
            assert!(neutral(color), "{color:?} has a hue");
        }
        // Lighter page than text, as the name says.
        assert!(p.base.r() > p.text.r());
        // Same glass, other color.
        assert_eq!(
            (p.glass_bar.blur, p.glass_bar.alpha),
            (DARK.glass_bar.blur, DARK.glass_bar.alpha)
        );
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

    /// REQ-013-01, REQ-020-01, REQ-020-02: the app's own colors whatever the OS theme or accent color; the OS only
    /// decides between light and dark when the appearance is System, dark when it says nothing.
    #[test]
    fn req_013_01_req_020_02_uses_its_own_colors_whatever_the_os_theme() {
        use domain::appearance::Appearance;
        let frame = |ctx: &egui::Context, system: Option<egui::Theme>| {
            let input = egui::RawInput {
                system_theme: system,
                ..Default::default()
            };
            ctx.run_ui(input, |_| {}).textures_delta.clear();
        };
        let ctx = egui::Context::default();
        install(&ctx);
        for (appearance, system, expected) in [
            (Appearance::Dark, Some(egui::Theme::Light), &DARK),
            (Appearance::Light, Some(egui::Theme::Dark), &LIGHT),
            (Appearance::System, Some(egui::Theme::Light), &LIGHT),
            (Appearance::System, Some(egui::Theme::Dark), &DARK),
            (Appearance::System, None, &DARK),
        ] {
            set_appearance(&ctx, appearance);
            frame(&ctx, system);
            assert_eq!(ctx.theme(), expected.theme, "{appearance:?} with {system:?}");
            let style = ctx.global_style();
            assert_eq!(style.visuals.panel_fill, expected.base);
            assert_eq!(style.visuals.widgets.noninteractive.fg_stroke.color, expected.text);
            assert_eq!(style.visuals.selection.bg_fill, expected.selected);
            assert_eq!(style.text_styles[&TextStyle::Body].family, FontFamily::Proportional);
        }
    }

    #[test]
    fn with_palette_draws_inside_with_another_palette_and_restores_it() {
        let ctx = egui::Context::default();
        install(&ctx);
        set_appearance(&ctx, domain::appearance::Appearance::Light);
        begin_frame(&ctx);
        assert_eq!(super::text(), LIGHT.text);
        assert_eq!(with_palette(&DARK, super::text), DARK.text);
        assert_eq!(super::text(), LIGHT.text);
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
