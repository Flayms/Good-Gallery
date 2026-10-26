//! Centered messages for a gallery with nothing to show, inviting the next step.

use crate::theme;
use eframe::egui::{self, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, Vec2};

const ICON: f32 = 40.0;

/// First run: no library added yet.
pub fn no_libraries(ui: &mut egui::Ui) -> egui::Response {
    let rect = ui.max_rect();
    let gap = 3.0 * theme::SPACE;
    let title_font = FontId::new(theme::SIZE_BODY, theme::semibold());
    let title_height = ui.fonts_mut(|f| f.row_height(&title_font));
    let content_height = ICON + gap + title_height + gap + ui.spacing().interact_size.y;
    let top = rect.center().y - content_height / 2.0;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(egui::pos2(rect.left(), top), rect.max))
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.y = gap;
    let (icon, _) = child.allocate_exact_size(Vec2::splat(ICON), Sense::hover());
    folder_plus(child.painter(), icon, theme::MIST);
    child.label(RichText::new("No libraries yet").font(title_font).color(theme::PAPER));
    primary_button(&mut child, "Add a library")
}

/// Paper fill with Darkroom text.
pub fn primary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let text = RichText::new(label)
        .font(FontId::new(theme::SIZE_BODY, theme::semibold()))
        .color(theme::DARKROOM);
    ui.add(
        egui::Button::new(text)
            .fill(theme::PAPER)
            .corner_radius(CornerRadius::same(theme::RADIUS_CONTROL)),
    )
}

/// lucide `folder-plus`, drawn on its 24-unit grid scaled to `rect`; the stroke scales with it, as in SVG.
fn folder_plus(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = rect.width() / 24.0;
    let p = |x: f32, y: f32| rect.min + Vec2::new(x, y) * s;
    let stroke = Stroke::new(theme::ICON_STROKE * s, color);
    let outline = vec![
        p(2.0, 6.0),
        p(4.0, 4.0),
        p(9.0, 4.0),
        p(11.0, 7.0),
        p(20.0, 7.0),
        p(22.0, 9.0),
        p(22.0, 18.0),
        p(20.0, 20.0),
        p(4.0, 20.0),
        p(2.0, 18.0),
    ];
    painter.add(egui::Shape::closed_line(outline, stroke));
    painter.line_segment([p(12.0, 10.0), p(12.0, 16.0)], stroke);
    painter.line_segment([p(9.0, 13.0), p(15.0, 13.0)], stroke);
}
