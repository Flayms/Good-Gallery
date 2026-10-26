//! The Settings page: libraries (spec 001). Indexing and thumbnail cache settings follow in M10.

use crate::icons::{Icon, icon_button};
use crate::state::{AppState, Page};
use crate::{format, theme};
use db::libraries::{Library, LibraryStatus};
use eframe::egui::{self, Color32, CornerRadius, FontId, Frame, Margin, RichText, Stroke, Vec2};

/// Content width; descriptions stay below 70 characters (design guide).
const MAX_WIDTH: f32 = 720.0;

pub fn ui(ui: &mut egui::Ui, state: &mut AppState) {
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        // Centered column, at least 24 px from the window edges.
        let width = (ui.available_width() - 48.0).min(MAX_WIDTH);
        let margin = ((ui.available_width() - width) / 2.0).max(0.0);
        ui.add_space(24.0);
        ui.horizontal_top(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_width(width);
                if ui
                    .add(egui::Button::image_and_text(Icon::ChevronLeft.small(), "Back to gallery").frame(false))
                    .clicked()
                {
                    state.page = Page::Gallery;
                }
                ui.add_space(theme::SPACE * 2.0);
                ui.label(RichText::new("Settings").font(FontId::new(theme::SIZE_PAGE, theme::semibold())));
                ui.add_space(theme::SPACE * 6.0);
                libraries(ui, state);
                ui.add_space(24.0);
            });
        });
    });
    confirm_remove(ui.ctx(), state);
}

fn libraries(ui: &mut egui::Ui, state: &mut AppState) {
    ui.label(RichText::new("Libraries").font(FontId::new(theme::SIZE_SECTION, theme::semibold())));
    ui.label(RichText::new("Folders on local disks or network shares, indexed recursively.").color(theme::MIST));
    ui.add_space(theme::SPACE * 2.0);

    let page = &mut state.settings_page;
    let busy = page.adding || page.picking;
    let mut add = false;
    let mut browse = false;
    ui.horizontal(|ui| {
        let buttons_width = 190.0;
        let input = ui.add(
            egui::TextEdit::singleline(&mut page.path_input)
                .hint_text("D:\\Photos or \\\\server\\share\\photos")
                .desired_width(ui.available_width() - buttons_width)
                .min_size(Vec2::new(0.0, theme::CONTROL_HEIGHT))
                .align(egui::Align2::LEFT_CENTER),
        );
        let enter = input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let can_add = !page.path_input.trim().is_empty() && !busy;
        let add_button = egui::Button::image_and_text(
            Icon::FolderPlus.image(theme::ICON_SIZE, theme::DARKROOM),
            RichText::new("Add")
                .color(theme::DARKROOM)
                .font(FontId::new(theme::SIZE_BODY, theme::semibold())),
        )
        .fill(theme::PAPER);
        add = ui.add_enabled(can_add, add_button).clicked() || (enter && can_add);
        let browse_button =
            egui::Button::image_and_text(Icon::FolderOpen.small(), "Browse…").stroke(Stroke::new(1.0, theme::RIM));
        browse = ui.add_enabled(!busy, browse_button).clicked();
    });
    if add {
        let path = page.path_input.clone();
        state.add_library(path);
    }
    if browse {
        state.browse_for_library();
    }

    ui.add_space(theme::SPACE * 3.0);
    let mut rescan = None;
    let libraries = &state.libraries;
    Frame::new()
        .stroke(Stroke::new(1.0, theme::RIM))
        .corner_radius(CornerRadius::same(theme::RADIUS_PANEL))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if libraries.is_empty() {
                ui.label(RichText::new("No libraries yet.").color(theme::MIST));
            }
            for (i, library) in libraries.iter().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                match library_row(ui, library) {
                    Some(RowAction::Rescan) => rescan = Some(library.id),
                    Some(RowAction::Remove) => state.settings_page.confirm_remove = Some(library.clone()),
                    None => {}
                }
            }
        });
    if let Some(id) = rescan {
        state.rescan_library(id);
    }
}

enum RowAction {
    Rescan,
    Remove,
}

fn library_row(ui: &mut egui::Ui, library: &Library) -> Option<RowAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        let buttons = 2.0 * theme::CONTROL_HEIGHT + theme::SPACE * 2.0;
        ui.allocate_ui(Vec2::new(ui.available_width() - buttons, 40.0), |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme::SPACE;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&library.label).font(FontId::new(theme::SIZE_BODY, theme::semibold())));
                    status_badge(ui, library.status);
                });
                let mut details = library.path.clone();
                if let Some(at) = library.last_scan_at {
                    details.push_str(&format!(" · last indexed {}", format::medium_date_time(at)));
                }
                ui.add(
                    egui::Label::new(RichText::new(details).size(theme::SIZE_CAPTION).color(theme::MIST)).truncate(),
                )
                .on_hover_text(&library.path);
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = &library.label;
            if icon_button(
                ui,
                Icon::Trash2,
                &format!("Remove {label}"),
                "Remove from gallery (files stay untouched)",
            )
            .clicked()
            {
                action = Some(RowAction::Remove);
            }
            if icon_button(ui, Icon::RefreshCw, &format!("Rescan {label}"), "Rescan").clicked() {
                action = Some(RowAction::Rescan);
            }
        });
    });
    action
}

fn status_badge(ui: &mut egui::Ui, status: LibraryStatus) {
    let (fill, stroke, text) = match status {
        LibraryStatus::Online => (theme::RAISED, Color32::TRANSPARENT, theme::PAPER),
        LibraryStatus::Offline => (theme::ALERT, Color32::TRANSPARENT, theme::DARKROOM),
        LibraryStatus::Unknown => (Color32::TRANSPARENT, theme::RIM, theme::MIST),
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, stroke))
        .corner_radius(CornerRadius::same(u8::MAX))
        .inner_margin(Margin::symmetric(8, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(status.as_str()).size(theme::SIZE_CAPTION).color(text));
        });
}

/// "Remove "<label>" from the gallery? Files on disk are not touched." with Cancel / Remove.
fn confirm_remove(ctx: &egui::Context, state: &mut AppState) {
    let Some(library) = state.settings_page.confirm_remove.clone() else {
        return;
    };
    let mut decided = None;
    let modal = egui::Modal::new(egui::Id::unique("confirm-remove"))
        .frame(
            Frame::new()
                .fill(theme::RAISED)
                .stroke(Stroke::new(1.0, theme::RIM))
                .corner_radius(CornerRadius::same(theme::RADIUS_PANEL))
                .inner_margin(Margin::same(20)),
        )
        .show(ctx, |ui| {
            ui.set_max_width(400.0);
            ui.label(RichText::new("Remove library").font(FontId::new(theme::SIZE_TITLE, theme::semibold())));
            ui.add_space(theme::SPACE * 2.0);
            ui.label(format!(
                "Remove \"{}\" from the gallery? Files on disk are not touched.",
                library.label
            ));
            ui.add_space(theme::SPACE * 4.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let remove = egui::Button::new(
                    RichText::new("Remove")
                        .color(theme::DARKROOM)
                        .font(FontId::new(theme::SIZE_BODY, theme::semibold())),
                )
                .fill(theme::PAPER);
                if ui.add(remove).clicked() {
                    decided = Some(true);
                }
                if ui
                    .add(egui::Button::new("Cancel").stroke(Stroke::new(1.0, theme::RIM)))
                    .clicked()
                {
                    decided = Some(false);
                }
            });
        });
    if modal.should_close() && decided.is_none() {
        decided = Some(false);
    }
    if let Some(remove) = decided {
        state.settings_page.confirm_remove = None;
        if remove {
            state.remove_library(library.id);
        }
    }
}
