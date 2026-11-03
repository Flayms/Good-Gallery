//! The People page (REQ-031-15): groups of similar faces nobody has named yet, largest first.

use crate::icons::Icon;
use crate::state::{AppState, FaceAction};
use crate::theme;
use domain::thumbs::ThumbKey;
use eframe::egui::{self, FontId, Margin, Rect, RichText, Sense, Stroke, Vec2, pos2, vec2};

const MAX_WIDTH: f32 = 720.0;
const CROP: f32 = 72.0;
/// Faces shown per group; the name applies to all of them.
const SHOWN_FACES: usize = 8;

/// A name typed for a group and waiting for the user to confirm it.
pub struct NamingGroup {
    pub name: String,
    pub places: Vec<(i64, [f32; 4])>,
}

/// The state of the page's fields, by the group's first face.
#[derive(Default)]
pub struct PeoplePage {
    names: std::collections::HashMap<i64, String>,
    pub confirm: Option<NamingGroup>,
}

/// The square of a thumbnail `size` pixels large around a face (fractions of the photo), a little wider than its box, so
/// the face isn't stretched by a box that isn't square. Moved inside the photo where it would reach past its edge.
fn crop_uv([x, y, w, h]: [f32; 4], size: [u32; 2]) -> Rect {
    const WIDER: f32 = 1.5;
    let (width, height) = (size[0].max(1) as f32, size[1].max(1) as f32);
    let side = (w * width).max(h * height) * WIDER;
    let side = side.min(width).min(height);
    let (half_u, half_v) = (side / width / 2.0, side / height / 2.0);
    let center = pos2(
        (x + w / 2.0).clamp(half_u, 1.0 - half_u),
        (y + h / 2.0).clamp(half_v, 1.0 - half_v),
    );
    Rect::from_center_size(center, vec2(half_u * 2.0, half_v * 2.0))
}

/// One face, cropped out of its thumbnail, a little wider than its box.
fn crop(ui: &mut egui::Ui, state: &mut AppState, media: i64, rect: [f32; 4]) -> egui::Response {
    let (area, response) = ui.allocate_exact_size(Vec2::splat(CROP), Sense::hover());
    ui.painter().rect_filled(area, theme::RADIUS_CONTROL, theme::selected());
    let texture = state.gallery.as_mut().and_then(|gallery| gallery.thumbnail(media));
    if let Some(texture) = texture {
        let uv = crop_uv(rect, texture.size);
        egui::Image::new((texture.id, area.size()))
            .uv(uv)
            .corner_radius(theme::RADIUS_CONTROL)
            .paint_at(ui, area);
    }
    response
}

pub fn ui(ui: &mut egui::Ui, state: &mut AppState) {
    let groups = state
        .gallery
        .as_mut()
        .and_then(|gallery| gallery.face_groups().map(<[_]>::to_vec));
    let header_height = crate::gallery::TOOLBAR_HEIGHT;
    let area = ui.max_rect();
    let mut action = None;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let width = (ui.available_width() - 48.0).min(MAX_WIDTH);
        let margin = ((ui.available_width() - width) / 2.0).max(0.0);
        ui.add_space(header_height + 24.0);
        ui.horizontal_top(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_width(width);
                match &groups {
                    None => {
                        ui.label(RichText::new("Looking through the faces…").color(theme::muted()));
                    }
                    Some(groups) if groups.is_empty() => {
                        ui.label(RichText::new("No groups of unknown faces yet.").color(theme::muted()));
                    }
                    Some(groups) => {
                        let wanted: Vec<(i64, ThumbKey)> = groups
                            .iter()
                            .flat_map(|group| group.iter().take(SHOWN_FACES))
                            .filter_map(|(face, key)| Some((face.media, key.clone()?)))
                            .collect();
                        if let Some(gallery) = &mut state.gallery {
                            gallery.want_thumbnails(&wanted);
                        }
                        for group in groups {
                            if let Some(chosen) = group_card(ui, state, group) {
                                action = Some(chosen);
                            }
                            ui.add_space(theme::SPACE * 6.0);
                        }
                    }
                }
                ui.add_space(24.0);
            });
        });
    });
    let header = Rect::from_min_size(area.min, Vec2::new(area.width(), header_height));
    crate::settings_page::page_header(ui, header, state, "People");
    if let Some(action) = action {
        state.face_action(action);
    }
    confirm_naming(ui.ctx(), state);
}

/// One group with its faces, a field for the name, and _Ignore_. Without saying who, nothing is written.
fn group_card(ui: &mut egui::Ui, state: &mut AppState, group: &crate::gallery::FaceGroup) -> Option<FaceAction> {
    let key = group.first().map_or(0, |(face, _)| face.id);
    let mut action = None;
    egui::Frame::new()
        .stroke(Stroke::new(1.0, theme::rim()))
        .corner_radius(theme::RADIUS_PANEL)
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(format!("{} faces", group.len())).font(FontId::new(theme::SIZE_BODY, theme::semibold())),
            );
            ui.add_space(theme::SPACE * 2.0);
            ui.horizontal_wrapped(|ui| {
                for (face, _) in group.iter().take(SHOWN_FACES) {
                    let shown = crop(ui, state, face.media, face.rect);
                    // A wrong face: out of the group, as ignored faces are (REQ-031-15).
                    let corner = Rect::from_min_size(shown.rect.right_top() + vec2(-22.0, 2.0), Vec2::splat(20.0));
                    let spoken = format!("Take face {} out of the group", face.id);
                    let take_out = ui.interact(corner, ui.make_persistent_id(("take out", face.id)), Sense::click());
                    ui.painter()
                        .rect_filled(corner, 4.0, egui::Color32::from_black_alpha(153));
                    Icon::X
                        .image(12.0, egui::Color32::WHITE)
                        .paint_at(ui, Rect::from_center_size(corner.center(), Vec2::splat(12.0)));
                    take_out.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &spoken));
                    if take_out.on_hover_text("Not one of them").clicked() {
                        action = Some(FaceAction::Ignore { faces: vec![face.id] });
                    }
                }
            });
            ui.add_space(theme::SPACE * 2.0);
            ui.horizontal(|ui| {
                let page = &mut state.people_page;
                let name = page.names.entry(key).or_default();
                let field = ui.add(
                    egui::TextEdit::singleline(name)
                        .hint_text("Who is this?")
                        .desired_width(240.0),
                );
                let spoken = format!("Who is this? Group of {}", group.len());
                field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, &spoken));
                let enter = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                let save = ui.button("Name them").clicked();
                if (enter || save)
                    && let Some(name) = clean_name(name)
                {
                    page.confirm = Some(NamingGroup {
                        name,
                        places: group.iter().map(|(face, _)| (face.media, face.rect)).collect(),
                    });
                }
                if ui.button("Ignore").clicked() {
                    action = Some(FaceAction::Ignore {
                        faces: group.iter().map(|(face, _)| face.id).collect(),
                    });
                }
            });
        });
    action
}

fn clean_name(name: &str) -> Option<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    (!name.is_empty()).then_some(name)
}

/// Naming a group writes to many files, so it asks first (REQ-031-15).
fn confirm_naming(ctx: &egui::Context, state: &mut AppState) {
    let Some(naming) = &state.people_page.confirm else {
        return;
    };
    let text = format!("Write {} into {} files?", naming.name, naming.places.len());
    let Some(chosen) = crate::widgets::dialog(ctx, "name-group", "Name this group", &text, &["Name them", "Cancel"])
    else {
        return;
    };
    let naming = state.people_page.confirm.take();
    if chosen == 0
        && let Some(naming) = naming
    {
        state.face_action(FaceAction::Name {
            name: naming.name,
            places: naming.places,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ-031-15: a crop is square in pixels, whatever the box and the photo's shape, and stays on the photo.
    #[test]
    fn req_031_15_crops_faces_without_stretching_them() {
        let uv = crop_uv([0.4, 0.4, 0.1, 0.2], [800, 400]);
        let (pixels_wide, pixels_high) = (uv.width() * 800.0, uv.height() * 400.0);
        assert!((pixels_wide - pixels_high).abs() < 0.5, "{uv:?}");
        assert!(pixels_wide > 80.0, "wider than the box: {uv:?}");
        assert!(
            (uv.center().x - 0.45).abs() < 1e-4 && (uv.center().y - 0.5).abs() < 1e-4,
            "{uv:?}"
        );

        let corner = crop_uv([0.0, 0.0, 0.1, 0.1], [400, 400]);
        assert!(corner.min.x >= 0.0 && corner.min.y >= 0.0, "{corner:?}");
        let huge = crop_uv([0.0, 0.0, 1.0, 1.0], [400, 300]);
        assert!(huge.max.x <= 1.0 && huge.max.y <= 1.0, "{huge:?}");
        assert!((huge.width() * 400.0 - huge.height() * 300.0).abs() < 0.5, "{huge:?}");
    }
}
