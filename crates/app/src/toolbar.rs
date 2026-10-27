//! The toolbar over the top of the gallery (REQ-013-08): the _View_ menu and Settings for now; search and filters
//! come with spec 007 (M7), glass and the title bar with M10, until then it is Raised.

use crate::gallery::{Gallery, TOOLBAR_HEIGHT};
use crate::icons::{Icon, icon_button};
use crate::theme;
use domain::view::{GalleryView, TagIndicatorKind};
use domain::zoom::{MAX_COLUMNS, MIN_COLUMNS};
use eframe::egui::{self, Rect, pos2, vec2};

/// What the user asked for in the toolbar, besides changes to the gallery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarAction {
    OpenSettings,
}

fn view_icon(view: GalleryView) -> Icon {
    match view {
        GalleryView::Masonry => Icon::LayoutDashboard,
        GalleryView::Justified => Icon::Rows3,
        GalleryView::Grid => Icon::LayoutGrid,
        GalleryView::List => Icon::List,
    }
}

/// Draws the toolbar over the top of `area`.
pub fn show(ui: &mut egui::Ui, area: Rect, gallery: Option<&mut Gallery>) -> Option<ToolbarAction> {
    let bar = Rect::from_min_size(area.min, vec2(area.width(), TOOLBAR_HEIGHT));
    // Tiles scrolled under the bar don't react to the pointer.
    ui.interact(bar, ui.make_persistent_id("toolbar"), egui::Sense::click());
    ui.painter().rect_filled(bar, 0, theme::RAISED);
    ui.painter()
        .hline(bar.x_range(), bar.bottom() - 0.5, egui::Stroke::new(1.0, theme::RIM));
    let controls = Rect::from_min_max(
        pos2(bar.left() + 12.0, bar.top() + 8.0),
        pos2(bar.right() - 12.0, bar.bottom() - 8.0),
    );
    let mut action = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(controls), |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_button(ui, Icon::Settings, "Settings", "Settings").clicked() {
                action = Some(ToolbarAction::OpenSettings);
            }
            if let Some(gallery) = gallery {
                view_button(ui, gallery);
            }
        });
    });
    action
}

/// The _View_ button, showing the current view's icon, and its menu (REQ-006-01, -06, REQ-016-03, REQ-017-03).
fn view_button(ui: &mut egui::Ui, gallery: &mut Gallery) {
    let button = egui::Button::new((
        view_icon(gallery.view()).small(),
        "View",
        Icon::ChevronDown.image(theme::ICON_SIZE, theme::MIST),
    ))
    .frame(false)
    .min_size(vec2(0.0, theme::CONTROL_HEIGHT));
    let response = ui.add(button);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "View options"));
    egui::Popup::from_toggle_button_response(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::BOTTOM_END)
        .width(288.0)
        .show(|ui| view_menu(ui, gallery));
}

/// A section title with its current value on the right.
fn section(ui: &mut egui::Ui, label: &str, value: Option<&str>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).family(theme::semibold()));
        if let Some(value) = value {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(value).color(theme::MIST));
            });
        }
    });
}

/// A toggle showing `content`, named `label` for screen readers and tests.
fn toggle(ui: &mut egui::Ui, selected: bool, enabled: bool, content: egui::Button<'_>, label: &str) -> bool {
    let response = ui.add_enabled(enabled, content.selected(selected)).on_hover_text(label);
    let label = label.to_string();
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, selected, &label));
    response.clicked()
}

fn view_menu(ui: &mut egui::Ui, gallery: &mut Gallery) {
    ui.spacing_mut().item_spacing.y = 8.0;
    let view = gallery.view();
    let zoomable = view != GalleryView::List;

    section(ui, "Layout", Some(view.label()));
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 3.0 * ui.spacing().item_spacing.x) / 4.0;
        for option in GalleryView::ALL {
            let button = egui::Button::image(view_icon(option).small()).min_size(vec2(width, theme::CONTROL_HEIGHT));
            // Clicking the active view keeps it.
            if toggle(ui, option == view, true, button, option.label()) {
                gallery.set_view(option);
            }
        }
    });

    let columns = gallery.columns;
    let zoom_value = if zoomable {
        format!("{columns} columns")
    } else {
        "Not in the List view".into()
    };
    section(ui, "Zoom", Some(&zoom_value));
    ui.horizontal(|ui| {
        // Zooming in means fewer, wider columns.
        ui.add_enabled_ui(zoomable && columns < MAX_COLUMNS, |ui| {
            if icon_button(ui, Icon::ZoomOut, "Zoom out", "Zoom out (Ctrl + wheel)").clicked() {
                gallery.zoom(1);
            }
        });
        let mut zoom = MIN_COLUMNS + MAX_COLUMNS - columns;
        let slider_width = ui.available_width() - theme::CONTROL_HEIGHT - ui.spacing().item_spacing.x;
        ui.spacing_mut().slider_width = slider_width.max(40.0);
        let response = ui.add_enabled(
            zoomable,
            egui::Slider::new(&mut zoom, MIN_COLUMNS..=MAX_COLUMNS)
                .show_value(false)
                .trailing_fill(true),
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, zoomable, "Zoom"));
        if response.changed() {
            gallery.set_columns(MIN_COLUMNS + MAX_COLUMNS - zoom);
        }
        ui.add_enabled_ui(zoomable && columns > MIN_COLUMNS, |ui| {
            if icon_button(ui, Icon::ZoomIn, "Zoom in", "Zoom in (Ctrl + wheel)").clicked() {
                gallery.zoom(-1);
            }
        });
    });

    ui.separator();
    // Days only make sense in date order; the choice is kept for when the user sorts by a date again.
    let date_sort = gallery.is_date_sort();
    let mut grouped = gallery.group_by_day();
    if ui
        .add_enabled(date_sort, egui::Checkbox::new(&mut grouped, "Group by day"))
        .changed()
    {
        gallery.set_group_by_day(grouped);
    }
    if !date_sort {
        ui.label(egui::RichText::new("Only when sorted by date").color(theme::MIST));
    }

    // The List view has no tiles to show indicators on (it has a rating column).
    let mut choice = gallery.tag_indicators().clone();
    if ui
        .add_enabled(zoomable, egui::Checkbox::new(&mut choice.shown, "Tag indicators"))
        .changed()
    {
        gallery.set_tag_indicators(choice.clone());
    }
    if !zoomable {
        ui.label(egui::RichText::new("Not in the List view").color(theme::MIST));
    }
    // The picks are kept while indicators are off, so turning them on again restores them.
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 3.0 * ui.spacing().item_spacing.x) / 4.0;
        for kind in TagIndicatorKind::ALL {
            let button = egui::Button::new(kind.label()).min_size(vec2(width, theme::CONTROL_HEIGHT - 4.0));
            if toggle(
                ui,
                choice.picked.contains(&kind),
                choice.shown && zoomable,
                button,
                kind.label(),
            ) {
                choice.toggle(kind);
                gallery.set_tag_indicators(choice.clone());
            }
        }
    });
}
