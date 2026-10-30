//! Small widgets drawn in the design's look where egui's own don't fit it.

use crate::theme;
use eframe::egui::{self, CornerRadius, FontId, Frame, Margin, Rect, RichText, Stroke, pos2, vec2};
use std::ops::RangeInclusive;

/// Radius of a slider's knob while hovered or dragged.
pub const KNOB: f32 = 7.0;

/// A thin slider: a 4 px rail, Paper up to the value, a round knob. Pressing anywhere on it jumps there (egui's
/// `Slider` rail doesn't show with the app's widget visuals). With a `step` above 0 the value snaps to it. Arrow keys move
/// it by 1 while it has focus.
pub fn slider(
    ui: &mut egui::Ui,
    rect: Rect,
    value: &mut f64,
    range: RangeInclusive<f64>,
    step: f64,
    label: &str,
) -> egui::Response {
    let (min, max) = (*range.start(), *range.end());
    let mut response = ui.interact(
        rect,
        ui.make_persistent_id(("slider", label)),
        egui::Sense::click_and_drag(),
    );
    // Room for the knob at both ends.
    let track = rect.shrink2(vec2(KNOB, 0.0));
    let set = |value: &mut f64, to: f64, response: &mut egui::Response| {
        let to = if step > 0.0 { (to / step).round() * step } else { to };
        let to = to.clamp(min, max.max(min));
        if to != *value {
            *value = to;
            response.mark_changed();
        }
    };
    if response.is_pointer_button_down_on()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let fraction = f64::from(((pos.x - track.left()) / track.width()).clamp(0.0, 1.0));
        set(value, min + fraction * (max - min), &mut response);
    }
    if response.has_focus() {
        let step = ui.input(|input| {
            i32::from(input.key_pressed(egui::Key::ArrowRight)) - i32::from(input.key_pressed(egui::Key::ArrowLeft))
        });
        if step != 0 {
            set(value, *value + f64::from(step), &mut response);
        }
    }
    let fraction = if max > min {
        ((*value - min) / (max - min)).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    let track = Rect::from_center_size(rect.center(), vec2(track.width(), 4.0));
    let knob = pos2(track.left() + track.width() * fraction, track.center().y);
    let painter = ui.painter();
    painter.rect_filled(track, 2.0, theme::track());
    painter.rect_filled(
        Rect::from_min_max(track.min, pos2(knob.x, track.max.y)),
        2.0,
        theme::text(),
    );
    let radius = if response.hovered() || response.dragged() {
        KNOB
    } else {
        KNOB - 1.0
    };
    painter.circle_filled(knob, radius, theme::text());
    let (label, value) = (label.to_owned(), *value);
    response.widget_info(|| egui::WidgetInfo::slider(true, value, &label));
    response
}

/// A dialog with `title`, `text` and `buttons`, the first one filled (the main choice), right to left. Returns the
/// index of the button chosen; closing it otherwise (a click beside it) chooses the last one.
pub fn dialog(ctx: &egui::Context, id: &str, title: &str, text: &str, buttons: &[&str]) -> Option<usize> {
    let mut decided = None;
    let modal = egui::Modal::new(egui::Id::unique(id))
        .frame(
            Frame::new()
                .fill(theme::raised())
                .stroke(Stroke::new(1.0, theme::rim()))
                .corner_radius(CornerRadius::same(theme::RADIUS_PANEL))
                .inner_margin(Margin::same(20)),
        )
        .show(ctx, |ui| {
            crate::glass::behind_frame(ui, Margin::same(20), |ui| {
                ui.set_max_width(400.0);
                ui.label(RichText::new(title).font(FontId::new(theme::SIZE_TITLE, theme::semibold())));
                ui.add_space(theme::SPACE * 2.0);
                ui.label(text);
                ui.add_space(theme::SPACE * 4.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    for (i, label) in buttons.iter().enumerate() {
                        let button = if i == 0 {
                            egui::Button::new(
                                RichText::new(*label)
                                    .color(theme::base())
                                    .font(FontId::new(theme::SIZE_BODY, theme::semibold())),
                            )
                            .fill(theme::text())
                        } else {
                            egui::Button::new(*label).stroke(Stroke::new(1.0, theme::rim()))
                        };
                        if ui.add(button).clicked() {
                            decided = Some(i);
                        }
                    }
                });
            });
        });
    if modal.should_close() && decided.is_none() {
        decided = Some(buttons.len().saturating_sub(1));
    }
    decided
}
