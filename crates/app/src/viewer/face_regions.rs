//! Faces on the photo (spec 032): their regions, the popup to name one, and drawing a new one.

use super::{FaceTarget, Viewer};
use crate::gallery::Gallery;
use crate::theme;
use domain::faces;
use eframe::egui::{self, Color32, Rect, pos2, vec2};

impl Viewer {
    pub fn faces_shown(&self) -> bool {
        self.faces
    }

    /// The face regions drawn in the last frame (REQ-009-17), by name, on screen.
    pub fn faces_drawn(&self) -> &[(String, Rect)] {
        &self.faces_drawn
    }

    /// The popup of the face clicked or drawn: what it offers depends on whether it has a name.
    pub(super) fn face_popup_ui(
        &mut self,
        ui: &egui::Ui,
        photo: Rect,
        people: &[Vec<String>],
    ) -> Option<crate::state::FaceAction> {
        use crate::people_page::{decision, naming, tips};
        use crate::state::FaceAction;
        let mut popup = self.face_popup.take()?;
        let rect = match &popup.target {
            FaceTarget::Unknown(face) => face.rect,
            FaceTarget::Named { rect, .. } | FaceTarget::Drawn(rect) => *rect,
        };
        let anchor = on_screen(photo, rect);
        let media = popup.media;
        let mut action = None;
        let mut close = false;
        egui::Area::new(ui.make_persistent_id("face popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(anchor.left_bottom() + vec2(0.0, FACE_LABEL_HEIGHT + 8.0))
            .show(ui.ctx(), |ui| {
                crate::glass::panel(ui, egui::Margin::same(10), |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    let id = ui.make_persistent_id("face popup name");
                    let field = crate::names::field(ui, id, &mut popup.name, "Who is this?", 200.0);
                    field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Who is this?"));
                    // A known person picked, or Enter on the field (REQ-032-34).
                    let picked = crate::names::suggestions(ui, id, &field, &popup.name, people);
                    let suggestion = match &popup.target {
                        FaceTarget::Unknown(face) => face.suggestion.clone(),
                        _ => None,
                    };
                    if let (FaceTarget::Unknown(face), Some(name)) = (&popup.target, &suggestion) {
                        ui.horizontal(|ui| {
                            if ui.button(format!("It's {name}")).clicked() {
                                action = Some(FaceAction::Name {
                                    name: name.clone(),
                                    places: vec![(media, face.rect)],
                                });
                            }
                            // The second person as likely (REQ-032-36).
                            if let Some(other) = &face.alternative
                                && ui.button(format!("It's {other}")).clicked()
                            {
                                action = Some(FaceAction::Name {
                                    name: other.clone(),
                                    places: vec![(media, face.rect)],
                                });
                            }
                            if ui.button("Not this person").clicked() {
                                action = Some(FaceAction::NotThisPerson {
                                    face: face.id,
                                    name: name.clone(),
                                });
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        let typed = crate::names::clean(&popup.name);
                        match &popup.target {
                            FaceTarget::Named { name, rect } => {
                                let renamed = typed.as_ref().is_some_and(|to| to != name);
                                let to = if naming(ui, "Rename", renamed) {
                                    typed.clone()
                                } else {
                                    picked.clone()
                                };
                                if let Some(to) = to.filter(|to| to != name) {
                                    action = Some(FaceAction::Rename {
                                        media,
                                        from: name.clone(),
                                        to,
                                        rect: *rect,
                                    });
                                }
                                if decision(ui, "Remove name", tips::REMOVE_NAME) {
                                    action = Some(FaceAction::Remove {
                                        media,
                                        name: name.clone(),
                                        rect: *rect,
                                    });
                                }
                            }
                            FaceTarget::Unknown(face) => {
                                let save = naming(ui, "Save", typed.is_some());
                                if let Some(name) = if save { typed.clone() } else { picked.clone() } {
                                    action = Some(FaceAction::Name {
                                        name,
                                        places: vec![(media, face.rect)],
                                    });
                                }
                                if decision(ui, "Ignore", tips::IGNORE) {
                                    action = Some(FaceAction::Ignore { faces: vec![face.id] });
                                }
                                if decision(ui, "Not a face", tips::NOT_A_FACE) {
                                    action = Some(FaceAction::NotAFace { faces: vec![face.id] });
                                }
                                if decision(ui, "Remove box", tips::REMOVE_BOX) {
                                    action = Some(FaceAction::Delete { face: face.id });
                                }
                            }
                            FaceTarget::Drawn(rect) => {
                                let save = naming(ui, "Save", typed.is_some());
                                if let Some(name) = if save { typed.clone() } else { picked.clone() } {
                                    action = Some(FaceAction::Name {
                                        name,
                                        places: vec![(media, *rect)],
                                    });
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                });
            });
        if action.is_none() && !close {
            self.face_popup = Some(popup);
        }
        action
    }

    /// Shows the face regions this time, without changing the remembered choice (REQ-032-26).
    pub fn show_faces(&mut self) {
        self.faces = true;
    }

    pub(super) fn toggle_faces(&mut self, gallery: &Gallery) {
        self.faces = !self.faces;
        gallery.store_setting(faces::FACES_KEY, if self.faces { "shown" } else { "hidden" });
    }
}

/// Room for a name label under its face region.
const FACE_LABEL_HEIGHT: f32 = 20.0;

/// A box around each face region of the photo drawn at `photo`, with the person's name below it (above it at the
/// bottom of `area`), clipped to the media area (REQ-009-17). On photos, not glass: like badges on tiles, they keep a
/// fixed light-on-dark look in both appearances.
pub(super) fn draw_faces(ui: &egui::Ui, area: Rect, photo: Rect, faces: &[faces::FaceRegion]) -> Vec<(String, Rect)> {
    let painter = ui.painter().with_clip_rect(area);
    let shadow = egui::Stroke::new(3.0, Color32::from_black_alpha(110));
    let line = egui::Stroke::new(1.5, Color32::WHITE);
    let font = egui::FontId::new(theme::SIZE_CAPTION, theme::semibold());
    let mut drawn = Vec::new();
    for face in faces {
        let rect = Rect::from_min_size(
            pos2(
                photo.left() + face.x * photo.width(),
                photo.top() + face.y * photo.height(),
            ),
            vec2(face.w * photo.width(), face.h * photo.height()),
        );
        painter.rect_stroke(rect, 4.0, shadow, egui::StrokeKind::Middle);
        painter.rect_stroke(rect, 4.0, line, egui::StrokeKind::Middle);
        let galley = painter.layout_no_wrap(face.name.clone(), font.clone(), Color32::WHITE);
        let size = vec2(galley.size().x + 12.0, FACE_LABEL_HEIGHT);
        let below = rect.bottom() + 4.0 + size.y <= area.bottom();
        let top = if below {
            rect.bottom() + 4.0
        } else {
            rect.top() - 4.0 - size.y
        };
        let label = Rect::from_min_size(pos2(rect.center().x - size.x / 2.0, top), size);
        painter.rect_filled(label, size.y / 2.0, Color32::from_black_alpha(153));
        painter.galley(label.center() - galley.size() / 2.0, galley, Color32::WHITE);
        drawn.push((face.name.clone(), rect));
    }
    drawn
}

/// A fraction-of-the-photo rectangle where the photo is drawn.
fn on_screen(photo: Rect, [x, y, w, h]: [f32; 4]) -> Rect {
    Rect::from_min_size(
        pos2(photo.left() + x * photo.width(), photo.top() + y * photo.height()),
        vec2(w * photo.width(), h * photo.height()),
    )
}

/// Draws the faces without a name, with their suggestions, and returns the face clicked, named or not (REQ-032-09). A
/// face is its box's own widget, so a click there doesn't reach the photo.
pub(super) fn face_clicks(
    ui: &egui::Ui,
    photo: Rect,
    named: &[faces::FaceRegion],
    unknown: &[db::faces::ShownFace],
    drawing: bool,
) -> Option<FaceTarget> {
    if drawing {
        return None;
    }
    let mut clicked = None;
    for region in named {
        let rect = [region.x, region.y, region.w, region.h];
        let label = format!("Face of {}", region.name);
        let response = ui.interact(
            on_screen(photo, rect),
            ui.make_persistent_id(("named face", rect.map(f32::to_bits))),
            egui::Sense::click(),
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        if response.clicked() {
            clicked = Some(FaceTarget::Named {
                name: region.name.clone(),
                rect,
            });
        }
    }
    let painter = ui.painter();
    let font = egui::FontId::new(theme::SIZE_CAPTION, theme::semibold());
    for face in unknown {
        let area = on_screen(photo, face.rect);
        let shadow = egui::Stroke::new(3.0, Color32::from_black_alpha(110));
        let line = egui::Stroke::new(1.5, Color32::from_white_alpha(170));
        painter.rect_stroke(area, 4.0, shadow, egui::StrokeKind::Middle);
        painter.extend(egui::Shape::dashed_line(
            &[
                area.left_top(),
                area.right_top(),
                area.right_bottom(),
                area.left_bottom(),
                area.left_top(),
            ],
            line,
            6.0,
            4.0,
        ));
        // A second person as likely shows beside the first (REQ-032-36).
        let text = match (&face.suggestion, &face.alternative) {
            (Some(name), Some(other)) => format!("{name}? or {other}?"),
            (Some(name), None) => format!("{name}?"),
            _ => "Who is this?".to_owned(),
        };
        let galley = painter.layout_no_wrap(text.clone(), font.clone(), Color32::WHITE);
        let size = vec2(galley.size().x + 12.0, FACE_LABEL_HEIGHT);
        let label = Rect::from_min_size(pos2(area.center().x - size.x / 2.0, area.bottom() + 4.0), size);
        painter.rect_filled(label, size.y / 2.0, Color32::from_black_alpha(153));
        painter.galley(label.center() - galley.size() / 2.0, galley, Color32::WHITE);
        let response = ui.interact(
            area,
            ui.make_persistent_id(("unknown face", face.id)),
            egui::Sense::click(),
        );
        let spoken = format!("Unknown face: {text}");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &spoken));
        if response.clicked() {
            clicked = Some(FaceTarget::Unknown(face.clone()));
        }
    }
    clicked
}

/// Drawing a new face: a drag across the photo gives its rectangle, as fractions of the photo (REQ-032-12).
pub(super) fn draw_new_face(ui: &egui::Ui, photo: Rect, drawing: &mut Option<Option<egui::Pos2>>) -> Option<[f32; 4]> {
    let start = (*drawing)?;
    let response = ui.interact(photo, ui.make_persistent_id("draw a face"), egui::Sense::drag());
    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    if response.drag_started() {
        *drawing = Some(response.interact_pointer_pos());
    }
    let start = (*drawing).flatten().or(start);
    let (Some(start), Some(now)) = (start, ui.ctx().pointer_latest_pos()) else {
        return None;
    };
    let area = Rect::from_two_pos(start, now).intersect(photo);
    ui.painter().rect_stroke(
        area,
        4.0,
        egui::Stroke::new(1.5, Color32::WHITE),
        egui::StrokeKind::Middle,
    );
    if !response.drag_stopped() {
        return None;
    }
    *drawing = None;
    (area.width() > 8.0 && area.height() > 8.0).then(|| {
        [
            (area.left() - photo.left()) / photo.width(),
            (area.top() - photo.top()) / photo.height(),
            area.width() / photo.width(),
            area.height() / photo.height(),
        ]
    })
}
