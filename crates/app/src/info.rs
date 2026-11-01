//! A media item's details (REQ-009-08): file, location, library, dates, dimensions, duration, size, rating and its tags
//! grouped into People, Places and Tags. Shown in the viewer's info panel and in the info sheet beside the gallery.
//! A field the item has no value for keeps its title and place, with nothing under it.

use crate::format;
use crate::icons;
use crate::theme;
use db::media::MediaDetail;
use domain::media::MediaKind;
use domain::tags::TagCategory;
use eframe::egui;

/// Width of the info panel and sheet.
pub const WIDTH: f32 = 320.0;

/// What a click in the details asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InfoAction {
    /// Filter the gallery by this tag (REQ-009-09).
    FilterByTag(String),
    /// Put the unedited original back (REQ-021-12).
    Revert,
}

/// The details as loaded: a spinner while loading, "Media not found" or the error; what was clicked as [`show`].
pub fn show_loaded(ui: &mut egui::Ui, detail: Option<&crate::gallery::DetailResult>) -> Option<InfoAction> {
    match detail {
        Some(Ok(Some(detail))) => return show(ui, detail),
        Some(Ok(None)) => {
            ui.label("Media not found");
        }
        Some(Err(error)) => {
            ui.label(format!("Couldn't load the details: {error}"));
        }
        None => {
            ui.spinner();
        }
    }
    None
}

/// Shows `detail`; returns a tag the user clicked, to filter the gallery by it (REQ-009-09), or _Revert to original_
/// (REQ-021-12).
pub fn show(ui: &mut egui::Ui, detail: &MediaDetail) -> Option<InfoAction> {
    let mut clicked = None;
    ui.spacing_mut().item_spacing.y = 12.0;
    field(ui, "File", |ui| {
        ui.add(egui::Label::new(&detail.file_name).wrap());
    });
    field(ui, "Location", |ui| {
        // Selectable, so the path can be copied.
        ui.add(
            egui::Label::new(detail.path.display().to_string())
                .wrap()
                .selectable(true),
        );
    });
    field(ui, "Library", |ui| {
        ui.label(&detail.library_label);
    });
    field(ui, "Taken", |ui| match detail.taken_at {
        Some(taken_at) => {
            ui.label(format::medium_date_time(taken_at));
        }
        None => empty(ui, "Unknown"),
    });
    field(ui, "Modified", |ui| {
        ui.label(format::medium_date_time(detail.mtime));
    });
    field(ui, "Dimensions", |ui| match (detail.width, detail.height) {
        (Some(width), Some(height)) => {
            ui.label(format!("{width} × {height}"));
        }
        _ => empty(ui, "Unknown"),
    });
    // Only videos have a duration.
    if detail.kind == MediaKind::Video {
        field(ui, "Duration", |ui| match detail.duration {
            Some(duration) => {
                ui.label(format::duration(duration));
            }
            None => empty(ui, "Unknown"),
        });
    }
    field(ui, "Size", |ui| {
        ui.label(format::bytes(detail.size));
    });
    field(ui, "Rating", |ui| match detail.rating {
        Some(rating) => stars(ui, rating),
        None => empty(ui, "Not rated"),
    });
    if let Some(edit) = &detail.edit {
        let replaced = edit.replaced_original(&detail.rel_path);
        field(ui, "Edited", |ui| {
            if replaced {
                ui.add(egui::Label::new("Saved over the original, which is kept").wrap());
                // The browser version doesn't edit (REQ-021-01).
                if !crate::WEB && ui.button("Revert to original").clicked() {
                    clicked = Some(InfoAction::Revert);
                }
            } else {
                let name = edit.source.rsplit('/').next().unwrap_or(&edit.source);
                ui.add(egui::Label::new(format!("An edited copy of {name}")).wrap());
            }
        });
    }
    for (label, category) in [
        ("People", Some(TagCategory::People)),
        ("Places", Some(TagCategory::Places)),
        ("Tags", None),
    ] {
        let names: Vec<&str> = detail
            .tags
            .iter()
            .filter(|tag| tag.category == category)
            .map(|tag| tag.name.as_str())
            .collect();
        if names.is_empty() {
            field(ui, label, |ui| empty(ui, "None"));
            continue;
        }
        caption(ui, label);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            for name in names {
                let chip = egui::Button::new(egui::RichText::new(name).size(theme::SIZE_CAPTION))
                    .fill(theme::selected())
                    .corner_radius(theme::RADIUS_CONTROL)
                    .min_size(egui::vec2(0.0, 24.0));
                let response = ui.add(chip);
                if response.on_hover_text(format!("Show only {name}")).clicked() {
                    clicked = Some(InfoAction::FilterByTag(name.to_owned()));
                }
            }
        });
    }
    clicked
}

fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme::SIZE_CAPTION)
            .color(theme::muted()),
    );
}

/// A label above its value.
fn field(ui: &mut egui::Ui, label: &str, value: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        caption(ui, label);
        value(ui);
    });
}

/// No value: one empty line, so the field keeps its place; screen readers hear `spoken`.
fn empty(ui: &mut egui::Ui, spoken: &str) {
    let height = ui.text_style_height(&egui::TextStyle::Body);
    let response = ui.allocate_response(egui::vec2(0.0, height), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, spoken));
}

/// Five stars, `rating` of them filled and the rest outlined, named for screen readers.
fn stars(ui: &mut egui::Ui, rating: i64) {
    let response = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for star in 1..=5 {
                ui.add(icons::rating_star(star <= rating, theme::ICON_SIZE));
            }
        })
        .response;
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, format!("Rated {rating} out of 5")));
}
