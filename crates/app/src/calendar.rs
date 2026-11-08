//! The date calendar under _Filters_ (spec 041): quick ranges, a month with a photo on each day that has one, and a
//! year's months.

use crate::gallery::{CalendarView, Gallery};
use crate::icons::Icon;
use crate::{format, theme};
use chrono::{Datelike, Months, NaiveDate};
use domain::calendar::{Picking, Quick};
use domain::search::GallerySearch;
use domain::thumbs::ThumbKey;
use eframe::egui::{self, Align2, FontId, Rect, RichText, Sense, Stroke, StrokeKind, vec2};

/// Room between day cells.
const GAP: f32 = 2.0;

/// Draws the calendar for `search`'s date range and changes it with the user's clicks (REQ-041-01 … REQ-041-04).
pub fn show(ui: &mut egui::Ui, gallery: &mut Gallery, search: &mut GallerySearch) {
    let today = domain::time_zone::today();
    let mut view = *gallery.calendar_view();
    // A range changed elsewhere (Clear, a smart album) isn't waiting for a second click any more.
    if view
        .anchor
        .is_some_and(|anchor| (search.from, search.to) != (Some(anchor), Some(anchor)))
    {
        view.anchor = None;
    }
    quick_ranges(ui, search, &mut view, today);
    header(ui, &mut view);
    if view.year_view {
        year(ui, gallery, &mut view);
    } else {
        month(ui, gallery, search, &mut view, today);
    }
    *gallery.calendar_view() = view;
}

/// _This month_, _Last 30 days_, _This year_ and _Last year_ (REQ-041-04).
fn quick_ranges(ui: &mut egui::Ui, search: &mut GallerySearch, view: &mut CalendarView, today: NaiveDate) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        // Equal widths, so the row looks the same from the first frame on.
        let width = ((ui.available_width() - 3.0 * 4.0) / 4.0).floor();
        for quick in Quick::ALL {
            let (from, to) = quick.range(today);
            let on = (search.from, search.to) == (Some(from), Some(to));
            let button = egui::Button::new(RichText::new(quick.label()).size(theme::SIZE_CAPTION))
                .selected(on)
                .min_size(vec2(width, theme::CONTROL_HEIGHT - 6.0));
            if ui.add_sized(vec2(width, theme::CONTROL_HEIGHT - 6.0), button).clicked() {
                (search.from, search.to) = (Some(from), Some(to));
                *view = CalendarView {
                    month: domain::calendar::month_of(from),
                    year_view: false,
                    anchor: None,
                };
            }
        }
    });
}

/// The arrows and the month's name, or the year's in the year view (REQ-041-03).
fn header(ui: &mut egui::Ui, view: &mut CalendarView) {
    let step = if view.year_view { 12 } else { 1 };
    let (unit, title) = if view.year_view {
        ("year", view.month.year().to_string())
    } else {
        ("month", format::month_year(view.month))
    };
    ui.horizontal(|ui| {
        let arrow = |ui: &mut egui::Ui, icon: Icon, label: String| {
            let response = ui.add(egui::Button::new(icon.small()).frame_when_inactive(false));
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
            response.on_hover_text(label).clicked()
        };
        if arrow(ui, Icon::ChevronLeft, format!("Previous {unit}")) {
            view.month = view.month.checked_sub_months(Months::new(step)).unwrap_or(view.month);
        }
        let width = ui.available_width() - 32.0;
        let title_button = egui::Button::new(RichText::new(&title).family(theme::semibold()))
            .frame_when_inactive(false)
            .min_size(vec2(width, 0.0));
        let response = ui.add(title_button);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &title));
        let tip = if view.year_view {
            "Show the month"
        } else {
            "Show the year's months"
        };
        if response.on_hover_text(tip).clicked() {
            view.year_view = !view.year_view;
        }
        if arrow(ui, Icon::ChevronRight, format!("Next {unit}")) {
            view.month = view.month.checked_add_months(Months::new(step)).unwrap_or(view.month);
        }
    });
}

/// A day's name as the calendar says it: "3 July 2025".
fn day_name(day: NaiveDate) -> String {
    day.format("%-d %B %Y").to_string()
}

/// The month's weeks, each day a button with its first photo (REQ-041-01, REQ-041-02).
fn month(
    ui: &mut egui::Ui,
    gallery: &mut Gallery,
    search: &mut GallerySearch,
    view: &mut CalendarView,
    today: NaiveDate,
) {
    let days = domain::calendar::weeks(view.month);
    let (Some(&first), Some(&last)) = (days.first(), days.last()) else {
        return;
    };
    let data = gallery.calendar_days(first, last, true).cloned().unwrap_or_default();
    let cell = ((ui.available_width() - 6.0 * GAP) / 7.0).floor();
    ui.spacing_mut().item_spacing = vec2(GAP, GAP);
    ui.horizontal(|ui| {
        for name in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
            let (rect, _) = ui.allocate_exact_size(vec2(cell, 16.0), Sense::hover());
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                name,
                FontId::proportional(theme::SIZE_CAPTION),
                theme::muted(),
            );
        }
    });
    let mut wanted: Vec<(i64, ThumbKey)> = Vec::new();
    for week in days.chunks(7) {
        ui.horizontal(|ui| {
            for &day in week {
                let info = data.days.get(&day);
                let name = match info {
                    Some(info) => format!("{}, {}", day_name(day), format::group_count(info.photos, info.videos)),
                    None => day_name(day),
                };
                let (rect, response) = ui.allocate_exact_size(vec2(cell, cell), Sense::click());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                let response = match info {
                    Some(info) => response.on_hover_text(format::group_count(info.photos, info.videos)),
                    None => response,
                };
                let thumb = info.and_then(|info| info.first.clone());
                if let Some(thumb) = &thumb {
                    wanted.push(thumb.clone());
                }
                paint_day(
                    ui,
                    gallery,
                    rect,
                    day,
                    view.month,
                    search,
                    thumb.map(|(id, _)| id),
                    info.is_some(),
                    today,
                    response.hovered(),
                );
                if response.clicked() {
                    let picked = domain::calendar::pick(
                        Picking {
                            from: search.from,
                            to: search.to,
                            anchor: view.anchor,
                        },
                        day,
                    );
                    (search.from, search.to, view.anchor) = (picked.from, picked.to, picked.anchor);
                }
            }
        });
    }
    gallery.want_calendar_thumbnails(wanted, ui.ctx().pixels_per_point());
}

/// One day: the range's fill, its photo, its number, and rings for the range's ends and today (REQ-041-01).
#[allow(clippy::too_many_arguments)]
fn paint_day(
    ui: &egui::Ui,
    gallery: &mut Gallery,
    rect: Rect,
    day: NaiveDate,
    month: NaiveDate,
    search: &GallerySearch,
    thumb: Option<i64>,
    has_items: bool,
    today: NaiveDate,
    hovered: bool,
) {
    let painter = ui.painter();
    let radius = theme::RADIUS_CONTROL;
    let in_range = match (search.from, search.to) {
        (Some(from), Some(to)) => (from..=to).contains(&day),
        (Some(from), None) => day >= from,
        (None, Some(to)) => day <= to,
        (None, None) => false,
    };
    let end = search.from == Some(day) || search.to == Some(day);
    let other_month = day.month() != month.month();
    if in_range {
        painter.rect_filled(rect, radius, theme::selected());
    }
    let texture = thumb.and_then(|id| gallery.thumbnail(id));
    let has_texture = texture.is_some();
    if let Some(texture) = texture {
        let inner = rect.shrink(if in_range { 2.0 } else { 0.0 });
        let fade = if other_month { 0.45 } else { 1.0 };
        egui::Image::new((texture.id, inner.size()))
            .uv(square_uv(texture.size))
            .corner_radius(radius)
            .tint(egui::Color32::from_white_alpha((255.0 * fade) as u8))
            .paint_at(ui, inner);
    } else if has_items {
        painter.rect_filled(rect.shrink(2.0), radius, theme::placeholder());
    }
    if hovered {
        painter.rect_filled(rect, radius, theme::hover());
    }
    // Days without items are dimmed, days of other months more so (REQ-041-02).
    let color = match (has_items, other_month) {
        (true, false) => theme::text(),
        (true, true) | (false, false) => theme::muted(),
        (false, true) => theme::muted().gamma_multiply(0.5),
    };
    let number = day.day().to_string();
    let font = FontId::proportional(theme::SIZE_CAPTION);
    if has_texture {
        // Over a photo, on a dark pill so it stays readable.
        let galley = painter.layout_no_wrap(number, font, egui::Color32::WHITE);
        let pill = Rect::from_min_size(rect.min + vec2(3.0, 3.0), galley.size() + vec2(6.0, 0.0));
        painter.rect_filled(pill, 4.0, egui::Color32::from_black_alpha(150));
        painter.galley(pill.min + vec2(3.0, 0.0), galley, egui::Color32::WHITE);
    } else {
        painter.text(rect.center(), Align2::CENTER_CENTER, number, font, color);
    }
    if end {
        painter.rect_stroke(rect, radius, Stroke::new(2.0, theme::text()), StrokeKind::Inside);
    } else if day == today {
        painter.rect_stroke(rect, radius, Stroke::new(1.5, theme::muted()), StrokeKind::Inside);
    }
}

/// The middle square of a texture, so a day shows its photo without stretching it.
fn square_uv([width, height]: [u32; 2]) -> Rect {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    if w >= h {
        let side = h / w;
        Rect::from_min_size(egui::pos2((1.0 - side) / 2.0, 0.0), vec2(side, 1.0))
    } else {
        let side = w / h;
        Rect::from_min_size(egui::pos2(0.0, (1.0 - side) / 2.0), vec2(1.0, side))
    }
}

/// The twelve months of the shown year with their counts; a click shows the month (REQ-041-03).
fn year(ui: &mut egui::Ui, gallery: &mut Gallery, view: &mut CalendarView) {
    let year = view.month.year();
    let (Some(first), Some(last)) = (
        NaiveDate::from_ymd_opt(year, 1, 1),
        NaiveDate::from_ymd_opt(year, 12, 31),
    ) else {
        return;
    };
    let groups = gallery
        .calendar_days(first, last, false)
        .map(|data| data.groups.clone())
        .unwrap_or_default();
    // Counts of another year or month would mislead while this year's load.
    let counts = gallery
        .calendar_is_current()
        .then(|| domain::calendar::months(year, &groups));
    let width = ((ui.available_width() - 2.0 * GAP) / 3.0).floor();
    ui.spacing_mut().item_spacing = vec2(GAP, GAP);
    for row in 0..4 {
        ui.horizontal(|ui| {
            for column in 0..3 {
                let month0 = row * 3 + column;
                let Some(month) = NaiveDate::from_ymd_opt(year, month0 + 1, 1) else {
                    continue;
                };
                let (photos, videos) = counts.map_or((0, 0), |counts| counts[month0 as usize]);
                let empty = photos + videos == 0;
                let name = if empty {
                    format::month_year(month)
                } else {
                    format!("{}, {}", format::month_year(month), format::group_count(photos, videos))
                };
                let (rect, response) = ui.allocate_exact_size(vec2(width, 44.0), Sense::click());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                let painter = ui.painter();
                if month == view.month {
                    painter.rect_filled(rect, theme::RADIUS_CONTROL, theme::selected());
                }
                if response.hovered() {
                    painter.rect_filled(rect, theme::RADIUS_CONTROL, theme::hover());
                }
                let color = if empty && counts.is_some() {
                    theme::muted()
                } else {
                    theme::text()
                };
                painter.text(
                    rect.center() - vec2(0.0, 7.0),
                    Align2::CENTER_CENTER,
                    month.format("%B").to_string(),
                    FontId::proportional(theme::SIZE_BODY),
                    color,
                );
                if !empty {
                    painter.text(
                        rect.center() + vec2(0.0, 10.0),
                        Align2::CENTER_CENTER,
                        format::count(u64::from(photos + videos)),
                        FontId::proportional(theme::SIZE_CAPTION),
                        theme::muted(),
                    );
                }
                if response.clicked() {
                    view.month = month;
                    view.year_view = false;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_uv_takes_the_middle() {
        let wide = square_uv([400, 200]);
        assert_eq!((wide.min.x, wide.width(), wide.height()), (0.25, 0.5, 1.0));
        let tall = square_uv([100, 400]);
        assert_eq!((tall.min.y, tall.width(), tall.height()), (0.375, 1.0, 0.25));
    }
}
