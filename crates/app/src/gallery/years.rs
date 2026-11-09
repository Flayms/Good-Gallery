//! The years beside the scrollbar, and the month while dragging it or at the pointer (REQ-006-16 … REQ-006-20). Where
//! they are works like the scrollbar itself (REQ-006-21, REQ-006-22).

use super::Arranged;
use super::Gallery;
use super::tiles::BADGE;
use crate::{format, theme};
use domain::scrollbar::{ScrollTrack, spread, year_marks};
use eframe::egui::{self, Color32, Rect, pos2, vec2};

/// How close the pointer must come to the window's edge for the years to show; wider than the labels, so they stay
/// while it moves onto one.
const NEAR: f32 = 64.0;
/// The scrollbar's full width with its margin (`theme`), which the labels stay clear of.
const BAR_ROOM: f32 = 10.0;
const LABEL_HEIGHT: f32 = 18.0;
/// The least distance between the middles of two labels.
const MIN_GAP: f64 = 22.0;
const PADDING: f32 = 6.0;
/// How long the years stay after the user stopped scrolling (REQ-006-18), in seconds.
const LINGER: f64 = 1.0;

/// The gallery index of the first item of the month of date group `group`, in the gallery's order (REQ-006-20).
fn first_of_month(groups: &[domain::date_groups::DateGroup], group: usize) -> usize {
    use chrono::Datelike as _;
    let month = |index: usize| (groups[index].day.year(), groups[index].day.month());
    let mut first = group;
    while first > 0 && month(first - 1) == month(group) {
        first -= 1;
    }
    groups[first].start
}

/// A badge at height `y`, its right edge beside the scrollbar.
fn badge_rect(bar: Rect, galley: &egui::Galley, y: f32, height: f32) -> Rect {
    let size = vec2(galley.size().x + 2.0 * PADDING, height);
    let right = bar.right() - BAR_ROOM - theme::SPACE;
    Rect::from_min_size(pos2(right - size.x, y - size.y / 2.0), size)
}

impl Gallery {
    /// `bar` is where the scrollbar's track is; `content` and `view` are the heights it scrolls; `scrolled` whether the
    /// user scrolled it this frame.
    pub(super) fn year_labels(
        &mut self,
        ui: &mut egui::Ui,
        arranged: &Arranged,
        bar: Rect,
        content: f32,
        view: f32,
        scrolled: bool,
    ) {
        let lane = self.year_lane.take();
        let now = ui.input(|input| input.time);
        if scrolled {
            self.years_until = now + LINGER;
        }
        let Some(data) = self.data.clone() else { return };
        if data.groups.is_empty() || content <= view {
            return;
        }
        let (pointer, press, primary_down, moved) = ui.input(|input| {
            (
                input.pointer.latest_pos(),
                input.pointer.press_origin(),
                input.pointer.primary_down(),
                input.pointer.is_decidedly_dragging(),
            )
        });
        let on_bar = Rect::from_min_max(pos2(bar.right() - BAR_ROOM, bar.top()), bar.max);
        // A press beside the bar, where the years are, drags like one on the bar (REQ-006-21).
        let on_lane = |press: egui::Pos2| on_bar.contains(press) || lane.is_some_and(|lane| lane.contains(press));
        let dragging = primary_down && press.is_some_and(on_lane);
        let dragging_lane = dragging && moved && press.is_some_and(|press| !on_bar.contains(press));
        let near = Rect::from_min_max(pos2(bar.right() - NEAR, bar.top()), bar.max);
        let near = pointer.is_some_and(|pos| near.contains(pos) && ui.ctx().layer_id_at(pos) == Some(ui.layer_id()));
        let scrolling = now < self.years_until;
        if scrolling {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(self.years_until - now));
        }
        let shown = (near || dragging || scrolling) && !self.autoscroll.is_on();
        let opacity = ui
            .ctx()
            .animate_bool_with_time(ui.scope_id().with("years"), shown, theme::DURATION_FAST);
        if opacity == 0.0 {
            return;
        }
        let track = ScrollTrack {
            top: f64::from(bar.top()),
            bottom: f64::from(bar.bottom()),
            content: f64::from(content),
            view: f64::from(view),
            min_handle: f64::from(ui.spacing().scroll.handle_min_length),
        };
        let font = egui::FontId::new(theme::SIZE_CAPTION, theme::numbers());

        let marks = year_marks(&data.groups);
        let offsets: Vec<f64> = marks
            .iter()
            .map(|mark| self.start_of(arranged, mark.first_item).min(track.max_offset()))
            .collect();
        let heights: Vec<f64> = offsets.iter().map(|&offset| track.thumb_center(offset)).collect();
        let years: Vec<(usize, String, std::sync::Arc<egui::Galley>, Rect)> = spread(&heights, MIN_GAP)
            .into_iter()
            .map(|index| {
                let year = marks[index].year.to_string();
                let galley = ui.painter().layout_no_wrap(year.clone(), font.clone(), Color32::WHITE);
                let rect = badge_rect(bar, &galley, heights[index] as f32, LABEL_HEIGHT);
                (index, year, galley, rect)
            })
            .collect();

        // The years' lane, from the widest year to the bar: pressed, it scrolls as the bar does (REQ-006-21); hovered, it
        // hides the tiles below from the pointer (REQ-006-22). Under the years, which take their own clicks.
        let lane_rect = years
            .iter()
            .map(|(_, _, _, rect)| rect.left())
            .reduce(f32::min)
            .map(|left| Rect::from_min_max(pos2(left, bar.top()), pos2(on_bar.left(), bar.bottom())));
        let mut lane_clicked = false;
        if let Some(rect) = lane_rect {
            self.year_lane = Some(rect);
            let response = ui.interact(rect, ui.scope_id().with("year lane"), egui::Sense::click_and_drag());
            lane_clicked = response.clicked();
        }

        // The month goes over any year it would touch: at the thumb while dragging (REQ-006-17), else at the pointer
        // where the thumb would be, unless the pointer is on a year, which it may click (REQ-006-19).
        let on_year = pointer.is_some_and(|pos| years.iter().any(|(_, _, _, rect)| rect.contains(pos)));
        let at = if dragging {
            Some((self.anchor, track.thumb_center(f64::from(self.offset))))
        } else {
            pointer.filter(|_| near && shown && !on_year).map(|pos| {
                let offset = track.offset_at(f64::from(pos.y));
                let count = data.ids.len();
                (self.item_at(arranged, count, offset), track.thumb_center(offset))
            })
        };
        let month = at.map(|(item, y)| {
            let group = data
                .groups
                .partition_point(|group| group.start <= item)
                .saturating_sub(1);
            let text = format::month_year(data.groups[group].day);
            let galley = ui.painter().layout_no_wrap(text.clone(), font.clone(), Color32::WHITE);
            let first = first_of_month(&data.groups, group);
            (
                text,
                badge_rect(bar, &galley, y as f32, LABEL_HEIGHT + 4.0),
                galley,
                first,
            )
        });

        let badge = BADGE.gamma_multiply(opacity);
        let text_color = Color32::WHITE.gamma_multiply(opacity);
        for (index, year, galley, rect) in years {
            if month.as_ref().is_some_and(|(_, month, _, _)| month.intersects(rect)) {
                continue;
            }
            let response = ui
                .interact(
                    rect,
                    ui.scope_id().with(("year", marks[index].year)),
                    egui::Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, year.clone()));
            if response.clicked() {
                self.scroll_to = Some(offsets[index] as f32);
                ui.ctx().request_repaint();
            }
            let fill = if response.hovered() {
                Color32::BLACK.gamma_multiply(opacity)
            } else {
                badge
            };
            ui.painter().rect_filled(rect, 4, fill);
            let pos = rect.center() - galley.size() / 2.0;
            ui.painter().galley(pos, galley, text_color);
        }
        if let Some((text, rect, galley, first)) = month {
            // A click goes to the month's first item, as a year's does to its own (REQ-006-20).
            let response = ui
                .interact(rect, ui.scope_id().with("month"), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, text.clone()));
            if response.clicked() && !dragging {
                self.scroll_to = Some(self.start_of(arranged, first).min(track.max_offset()) as f32);
                ui.ctx().request_repaint();
            }
            ui.painter()
                .rect_filled(rect, 4, Color32::BLACK.gamma_multiply(opacity));
            let pos = rect.center() - galley.size() / 2.0;
            ui.painter().galley(pos, galley, text_color);
        }
        // As on the bar, the thumb's middle follows the pointer (REQ-006-21). Read from the pointer, not a widget's drag:
        // the year it started on may hide under the month.
        if (dragging_lane || lane_clicked)
            && let Some(pos) = pointer
        {
            self.scroll_to = Some(track.offset_at(f64::from(pos.y)) as f32);
            ui.ctx().request_repaint();
        }
    }
}
