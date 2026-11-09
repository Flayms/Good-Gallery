//! The row of thumbnails under the photo to jump through the gallery (spec 015).

use super::display::wheel;
use super::{STRIP, Viewer};
use crate::gallery::Gallery;
use crate::icons::Icon;
use crate::{gallery, theme};
use domain::media::MediaKind;
use domain::viewer::{self as zoom, FILMSTRIP_KEY};
use eframe::egui::{self, Color32, Rect, Vec2, pos2, vec2};

impl Viewer {
    pub fn filmstrip_shown(&self) -> bool {
        self.filmstrip
    }

    pub(super) fn toggle_filmstrip(&mut self, gallery: &Gallery) {
        self.filmstrip = !self.filmstrip;
        self.strip_centered = None;
        gallery.store_setting(FILMSTRIP_KEY, if self.filmstrip { "shown" } else { "hidden" });
    }

    /// The filmstrip (spec 015): thumbnails of the whole gallery in one row, the shown item highlighted and centered;
    /// the wheel scrolls it. Returns a clicked position.
    pub(super) fn filmstrip(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        count: usize,
        gallery: &mut Gallery,
    ) -> Option<usize> {
        let view = rect.width();
        if (self.strip_centered != Some(self.index) || self.strip_width != view) && !self.missing {
            self.strip_offset = STRIP.centered(self.index, count, view);
            self.strip_centered = Some(self.index);
        }
        self.strip_width = view;
        self.strip_offset = STRIP.clamp(self.strip_offset, count, view);
        let response = ui.interact(rect, ui.make_persistent_id("filmstrip"), egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Navigation, true, "Filmstrip"));
        // Over the thumbnails too, which lie on top of the strip.
        if ui.rect_contains_pointer(rect) {
            let delta = wheel(ui);
            // egui's deltas point the other way than a browser's.
            let sideways = -zoom::sideways_delta(delta.x, delta.y);
            if sideways != 0.0 {
                self.strip_offset = STRIP.clamp(self.strip_offset + sideways, count, view);
            }
        }
        self.chrome.backing(theme::glass_bar(), self.chrome_blur).paint(
            ui.painter(),
            rect,
            f32::from(theme::RADIUS_PANEL),
        );
        // A short row stays centered.
        let left = rect.left() + ((view - STRIP.width(count)) / 2.0).max(0.0) - self.strip_offset;
        let visible = STRIP.visible(count, self.strip_offset, view);
        gallery.set_strip(visible.clone());
        let painter = ui.painter().with_clip_rect(rect.shrink(1.0));
        let mut clicked = None;
        for index in visible {
            let thumb = Rect::from_min_size(
                pos2(left + STRIP.left(index), rect.top() + STRIP.padding),
                Vec2::splat(STRIP.size),
            );
            let Some(item) = gallery.item(index) else {
                painter.rect_filled(thumb, 2.0, theme::raised());
                continue;
            };
            let (id, video, name) = (item.id, item.kind == MediaKind::Video, item.file_name.clone());
            let current = index == self.index && !self.missing && !self.left_result;
            let response = ui.interact(
                thumb.intersect(rect),
                ui.make_persistent_id(("filmstrip item", id)),
                egui::Sense::click(),
            );
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, current, &name));
            let response = response.on_hover_text(&name);
            if response.clicked() {
                clicked = Some(index);
            }
            let tint = if current || response.hovered() {
                Color32::WHITE
            } else {
                Color32::WHITE.gamma_multiply(0.6)
            };
            match (gallery.thumbnail(id), gallery.placeholder(ui.ctx(), id)) {
                (Some(texture), _) => {
                    painter.add(
                        egui::epaint::RectShape::filled(thumb, 2, tint)
                            .with_texture(texture.id, gallery::cover_uv(texture.size, thumb.size())),
                    );
                }
                (None, Some(placeholder)) => {
                    painter.add(
                        egui::epaint::RectShape::filled(thumb, 2, tint)
                            .with_texture(placeholder, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))),
                    );
                }
                (None, None) => {
                    painter.rect_filled(thumb, 2.0, theme::raised());
                }
            }
            // The tile's duration badge doesn't fit; a play symbol still marks videos (REQ-015-07).
            if video {
                let badge = Rect::from_center_size(thumb.center(), Vec2::splat(24.0));
                painter.circle_filled(badge.center(), 12.0, Color32::from_black_alpha(153));
                let icon = Rect::from_center_size(badge.center() + vec2(1.0, 0.0), Vec2::splat(12.0));
                // Images paint unclipped: only while the symbol is inside the strip.
                if rect.contains_rect(icon) {
                    Icon::Play.image(12.0, Color32::WHITE).paint_at(ui, icon);
                }
                let marker = ui.interact(icon, ui.make_persistent_id(("video marker", id)), egui::Sense::hover());
                marker.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Video"));
            }
            if current {
                painter.rect_stroke(
                    thumb,
                    2,
                    egui::Stroke::new(2.0, theme::text()),
                    egui::StrokeKind::Inside,
                );
            }
        }
        clicked
    }
}
