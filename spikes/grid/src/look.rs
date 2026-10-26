//! Fonts, tokens and the chrome around the gallery (sidebar, toolbar, popover), mimicking the reference screenshots.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Painter,
    Pos2, Rect, Stroke, Vec2,
};
use eframe::epaint::text::{FontTweak, VariationCoords};
use std::sync::Arc;

pub const RECESS: Color32 = Color32::from_rgb(0x17, 0x17, 0x17);
pub const PAPER: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
pub const MIST: Color32 = Color32::from_rgb(0xb4, 0xb4, 0xb4);
pub const STAR: Color32 = Color32::from_rgb(0xf5, 0xc4, 0x51);
pub const RIM: Color32 = Color32::from_rgba_premultiplied(26, 26, 26, 26);
pub const SELECTED: Color32 = Color32::from_rgba_premultiplied(31, 31, 31, 31);
pub const FIELD: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 89);

pub const SIDEBAR_WIDTH: f32 = 256.0;
pub const TOOLBAR_HEIGHT: f32 = 48.0;

const REGULAR: &str = "ahn-400";
const SEMIBOLD: &str = "ahn-600";

pub fn semibold() -> FontFamily {
    FontFamily::Name(SEMIBOLD.into())
}

/// Numbers that line up: tabular figures if `tnum` is on, else the proportional default.
pub fn numbers() -> FontFamily {
    FontFamily::Name("numbers".into())
}

pub fn numbers_semibold() -> FontFamily {
    FontFamily::Name("numbers-600".into())
}

/// Atkinson Hyperlegible Next at weights 400 / 600 from one variable TTF. `tnum` swaps the numbers families to a
/// copy with `tnum` baked in (pyftfeatfreeze), since egui can't request OpenType features.
pub fn install_fonts(ctx: &egui::Context, tnum: bool, hinting: Option<bool>) {
    static AHN: &[u8] = include_bytes!("../assets/AtkinsonHyperlegibleNext.ttf");
    static AHN_TNUM: &[u8] = include_bytes!("../assets/AtkinsonHyperlegibleNext-Tnum.ttf");
    let face = |bytes: &'static [u8], weight: f32| {
        Arc::new(FontData::from_static(bytes).tweak(FontTweak {
            coords: VariationCoords::new([(b"wght", weight)]),
            hinting,
            ..Default::default()
        }))
    };
    let mut fonts = FontDefinitions::default();
    let numbers_src = if tnum { AHN_TNUM } else { AHN };
    fonts.font_data.insert(REGULAR.into(), face(AHN, 400.0));
    fonts.font_data.insert(SEMIBOLD.into(), face(AHN, 600.0));
    fonts
        .font_data
        .insert("numbers".into(), face(numbers_src, 400.0));
    fonts
        .font_data
        .insert("numbers-600".into(), face(numbers_src, 600.0));
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    let family = |first: &str| {
        std::iter::once(first.to_string())
            .chain(fallback.iter().cloned())
            .collect::<Vec<_>>()
    };
    fonts
        .families
        .insert(FontFamily::Proportional, family(REGULAR));
    fonts.families.insert(semibold(), family(SEMIBOLD));
    fonts.families.insert(numbers(), family("numbers"));
    fonts
        .families
        .insert(numbers_semibold(), family("numbers-600"));
    ctx.set_fonts(fonts);
}

/// Five-pointed star (no bundled font has U+2605; the reference uses an icon).
fn star(p: &Painter, c: Pos2, r: f32, color: Color32) {
    let points: Vec<Pos2> = (0..10)
        .map(|i| {
            let a = std::f32::consts::PI * (i as f32 / 5.0 - 0.5);
            let rr = if i % 2 == 0 { r } else { r * 0.45 };
            c + Vec2::new(a.cos(), a.sin()) * rr
        })
        .collect();
    // Triangle fan from the center, since the outline is concave.
    for i in 0..10 {
        p.add(egui::Shape::convex_polygon(
            vec![c, points[i], points[(i + 1) % 10]],
            color,
            Stroke::NONE,
        ));
    }
}

fn text(
    p: &Painter,
    pos: Pos2,
    align: Align2,
    s: &str,
    size: f32,
    family: FontFamily,
    color: Color32,
) -> Rect {
    p.text(pos, align, s, FontId::new(size, family), color)
}

/// Sidebar like the reference's justified screenshot, with library-sized counts to show tabular figures.
pub fn sidebar(p: &Painter, rect: Rect) {
    p.rect_filled(rect, 0.0, RECESS);
    let x = rect.left() + 16.0;
    let right = rect.right() - 16.0;
    let mut y = rect.top() + 24.0;
    text(
        p,
        Pos2::new(x, y),
        Align2::LEFT_CENTER,
        "Good Gallery",
        16.0,
        semibold(),
        PAPER,
    );
    y += 40.0;

    let section = |y: &mut f32, label: &str| {
        text(
            p,
            Pos2::new(x, *y),
            Align2::LEFT_CENTER,
            label,
            12.0,
            semibold(),
            MIST,
        );
        *y += 28.0;
    };
    let row = |y: &mut f32, label: &str, count: Option<&str>, selected: bool| {
        if selected {
            p.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x - 8.0, *y - 16.0),
                    Pos2::new(right + 8.0, *y + 16.0),
                ),
                CornerRadius::same(6),
                SELECTED,
            );
        }
        let family = if selected {
            semibold()
        } else {
            FontFamily::Proportional
        };
        text(
            p,
            Pos2::new(x, *y),
            Align2::LEFT_CENTER,
            label,
            14.0,
            family,
            PAPER,
        );
        if let Some(count) = count {
            text(
                p,
                Pos2::new(right, *y),
                Align2::RIGHT_CENTER,
                count,
                12.0,
                numbers(),
                MIST,
            );
        }
        *y += 32.0;
    };

    section(&mut y, "Libraries");
    row(&mut y, "All libraries", None, true);
    row(&mut y, "Photos", Some("98,231"), false);
    row(&mut y, "NAS archive", Some("11,404"), false);
    y += 8.0;
    section(&mut y, "People");
    row(&mut y, "Mia", Some("1,118"), false);
    row(&mut y, "Jonas", Some("884"), false);
    y += 8.0;
    section(&mut y, "Ratings");
    for (stars, count) in [(5, "121"), (4, "1,017"), (3, "4,410")] {
        for s in 0..stars {
            star(p, Pos2::new(x + 7.0 + s as f32 * 18.0, y), 7.0, STAR);
        }
        text(
            p,
            Pos2::new(right, y),
            Align2::RIGHT_CENTER,
            count,
            12.0,
            numbers(),
            MIST,
        );
        y += 32.0;
    }
    y += 8.0;
    section(&mut y, "Tags");
    for (tag, count) in [
        ("Landscape", "12,481"),
        ("Mountains", "8,016"),
        ("Sunset", "1,377"),
        ("Night", "911"),
        ("IMG_0011 Il1 O0", "10"),
    ] {
        row(&mut y, tag, Some(count), false);
    }
    // Type scale samples: 12 / 14 / 16 at 400 / 600.
    y += 8.0;
    section(&mut y, "Type samples");
    for (size, family, label) in [
        (12.0, FontFamily::Proportional, "12 · 400 Counts 0123456789"),
        (14.0, FontFamily::Proportional, "14 · 400 DSC_1107.jpg"),
        (14.0, semibold(), "14 · 600 Active item"),
        (16.0, semibold(), "16 · 600 IMG_0011.HEIC"),
    ] {
        text(
            p,
            Pos2::new(x, y),
            Align2::LEFT_CENTER,
            label,
            size,
            family,
            PAPER,
        );
        y += 26.0;
    }
    p.vline(rect.right() - 0.5, rect.y_range(), Stroke::new(1.0, RIM));
}

/// Toolbar contents over the glass: search field, Filters, View. Returns the View button rect.
pub fn toolbar(p: &Painter, rect: Rect, right_reserved: f32) -> Rect {
    let field = Rect::from_min_size(
        Pos2::new(rect.left() + 48.0, rect.center().y - 16.0),
        Vec2::new(576.0, 32.0),
    );
    p.rect_filled(field, CornerRadius::same(6), FIELD);
    text(
        p,
        Pos2::new(field.left() + 30.0, field.center().y),
        Align2::LEFT_CENTER,
        "Search tags or file names",
        14.0,
        FontFamily::Proportional,
        MIST,
    );
    p.circle_stroke(
        Pos2::new(field.left() + 15.0, field.center().y - 1.0),
        5.0,
        Stroke::new(1.5, MIST),
    );
    // Sidebar toggle glyph.
    let toggle = Rect::from_center_size(
        Pos2::new(rect.left() + 24.0, rect.center().y),
        Vec2::splat(14.0),
    );
    p.rect_stroke(
        toggle,
        CornerRadius::same(2),
        Stroke::new(1.5, PAPER),
        egui::StrokeKind::Inside,
    );
    p.vline(
        toggle.left() + 5.0,
        toggle.y_range(),
        Stroke::new(1.5, PAPER),
    );

    let view_right = rect.right() - right_reserved - 16.0;
    let view = text(
        p,
        Pos2::new(view_right, rect.center().y),
        Align2::RIGHT_CENTER,
        "View",
        14.0,
        semibold(),
        PAPER,
    );
    text(
        p,
        Pos2::new(view.left() - 32.0, rect.center().y),
        Align2::RIGHT_CENTER,
        "Filters",
        14.0,
        semibold(),
        PAPER,
    );
    view.expand2(Vec2::new(10.0, 8.0))
}

/// Popover contents: the View menu, with a few counts.
pub fn panel(p: &Painter, rect: Rect) {
    let x = rect.left() + 16.0;
    let mut y = rect.top() + 24.0;
    text(
        p,
        Pos2::new(x, y),
        Align2::LEFT_CENTER,
        "Layout",
        12.0,
        semibold(),
        MIST,
    );
    y += 30.0;
    for (i, label) in ["Masonry", "Justified", "Grid", "List"].iter().enumerate() {
        let r = Rect::from_min_max(
            Pos2::new(rect.left() + 6.0, y - 15.0),
            Pos2::new(rect.right() - 6.0, y + 15.0),
        );
        if i == 1 {
            p.rect_filled(r, CornerRadius::same(6), SELECTED);
        }
        text(
            p,
            Pos2::new(x, y),
            Align2::LEFT_CENTER,
            label,
            14.0,
            if i == 1 {
                semibold()
            } else {
                FontFamily::Proportional
            },
            PAPER,
        );
        y += 32.0;
    }
    y += 6.0;
    p.hline(rect.x_range().shrink(6.0), y, Stroke::new(1.0, RIM));
    y += 20.0;
    text(
        p,
        Pos2::new(x, y),
        Align2::LEFT_CENTER,
        "Zoom",
        14.0,
        FontFamily::Proportional,
        PAPER,
    );
    text(
        p,
        Pos2::new(rect.right() - 16.0, y),
        Align2::RIGHT_CENTER,
        "12 columns",
        12.0,
        numbers(),
        MIST,
    );
}
