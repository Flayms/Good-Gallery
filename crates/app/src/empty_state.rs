//! Centered messages for a page with nothing to show, inviting the next step.

use crate::icons::Icon;
use crate::theme;
use eframe::egui::{self, CornerRadius, FontId, Rect, RichText};

const ICON: f32 = 40.0;

/// First run: no library added yet. The response is the *Add a library* button's (*Pick a folder* in the browser,
/// with what stays private: REQ-019-01, REQ-019-12).
pub fn no_libraries(ui: &mut egui::Ui) -> egui::Response {
    let (detail, button) = if crate::WEB {
        (
            Some("Your photos are indexed in this browser and never leave your computer."),
            "Pick a folder",
        )
    } else {
        (None, "Add a library")
    };
    centered(ui, Icon::FolderPlus, "No libraries yet", detail, Some(button)).expect("the button was added")
}

/// Libraries exist, but nothing matches (or nothing is indexed yet).
pub fn no_media(ui: &mut egui::Ui) {
    message(
        ui,
        Icon::Images,
        "No media found",
        Some("Nothing matches the current filters yet — new files appear while indexing."),
    );
}

pub fn error(ui: &mut egui::Ui, title: &str, detail: &str) {
    centered(ui, Icon::TriangleAlert, title, Some(detail), None);
}

pub fn message(ui: &mut egui::Ui, icon: Icon, title: &str, detail: Option<&str>) {
    centered(ui, icon, title, detail, None);
}

/// Icon, title, optional detail and optional primary button, centered in the available space.
fn centered(
    ui: &mut egui::Ui,
    icon: Icon,
    title: &str,
    detail: Option<&str>,
    button: Option<&str>,
) -> Option<egui::Response> {
    let rect = ui.max_rect();
    let gap = 3.0 * theme::SPACE;
    let title_font = FontId::new(theme::SIZE_BODY, theme::semibold());
    let row = |font: &FontId| ui.fonts_mut(|f| f.row_height(font));
    let mut height = ICON + gap + row(&title_font);
    if let Some(detail) = detail {
        // The detail wraps (e.g. an error with a path), so measure it at the width it gets.
        let font = FontId::new(theme::SIZE_BODY, egui::FontFamily::Proportional);
        let wrap_width = rect.width() - 48.0;
        let galley = ui.fonts_mut(|f| f.layout(detail.to_owned(), font, theme::MIST, wrap_width));
        height += gap + galley.size().y;
    }
    if button.is_some() {
        height += gap + ui.spacing().interact_size.y;
    }
    let top = rect.center().y - height / 2.0;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(
                egui::pos2(rect.left() + 24.0, top),
                rect.max - egui::vec2(24.0, 0.0),
            ))
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.y = gap;
    child.add(icon.image(ICON, theme::MIST));
    child.label(RichText::new(title).font(title_font).color(theme::PAPER));
    if let Some(detail) = detail {
        child.label(RichText::new(detail).color(theme::MIST));
    }
    button.map(|label| primary_button(&mut child, label))
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
