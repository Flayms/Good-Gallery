//! The List view (REQ-006-05): one row per media item under a column header that stays at the top, with a header row
//! before each date group.

use super::tiles::{group_header, tile_label};
use super::{
    Arranged, FrameState, Gallery, OVERSCAN, PADDING, PREFETCH_AHEAD, PREFETCH_BEHIND, TOOLBAR_HEIGHT, one_line,
};
use crate::icons::Icon;
use crate::{format, theme};
use db::gallery::GalleryItem;
use domain::layout::{ListEntry, ListRows, list_entry_at};
use domain::media::MediaKind;
use eframe::egui::{self, Align2, Rect, Vec2, pos2, vec2};
use std::time::Instant;

pub(super) const ROWS: ListRows = ListRows {
    row: 52.0,
    header: 40.0,
};
pub(super) const COLUMN_HEADER_HEIGHT: f32 = 36.0;
const THUMB: f32 = 40.0;
const GAP: f32 = 16.0;
/// Fixed column widths: thumbnail, date, size, dimensions, rating. Name and folder share the rest 3 : 2.
const FIXED: [f32; 5] = [THUMB, 176.0, 88.0, 120.0, 96.0];

/// Left edge and width of each column: thumbnail, name, folder, date, size, dimensions, rating.
fn columns(left: f32, width: f32) -> [(f32, f32); 7] {
    let flexible = (width - 2.0 * PADDING - 6.0 * GAP - FIXED.iter().sum::<f32>()).max(0.0);
    let widths = [
        FIXED[0],
        flexible * 0.6,
        flexible * 0.4,
        FIXED[1],
        FIXED[2],
        FIXED[3],
        FIXED[4],
    ];
    let mut x = left + PADDING;
    widths.map(|w| {
        let column = (x, w);
        x += w + GAP;
        column
    })
}

/// The folder of a `/`-separated path.
fn folder_of(rel_path: &str) -> &str {
    rel_path.rfind('/').map_or("", |slash| &rel_path[..slash])
}

impl Gallery {
    pub(super) fn draw_list(
        &mut self,
        ui: &mut egui::Ui,
        arranged: &Arranged,
        viewport: Rect,
        origin: egui::Pos2,
        inset_top: f32,
    ) {
        let Some(data) = self.data.clone() else { return };
        let groups = &arranged.groups;
        let count = data.ids.len();
        let width = ui.max_rect().width();
        self.list_width = width;
        let view_top = f64::from(viewport.min.y - inset_top);
        let view_bottom = f64::from(viewport.max.y - inset_top);
        let overscan = f64::from(OVERSCAN);
        let first = ROWS.entry_at(groups, count, view_top - overscan);
        let last = ROWS.entry_at(groups, count, view_bottom + overscan) + 1;
        let item_of = |entry: usize| match list_entry_at(groups, entry) {
            ListEntry::Group(group) => groups[group],
            ListEntry::Item(item) => item,
        };
        self.load_range(item_of(first), item_of(last - 1) + 1);

        let mut frame = FrameState {
            now: Instant::now(),
            made: 0,
            fading: false,
        };
        for entry in first..last {
            let top = origin.y + ROWS.top(groups, entry) as f32;
            match list_entry_at(groups, entry) {
                ListEntry::Group(group) => {
                    // Headers are shown for every day group, in order.
                    if let Some(group) = data.groups.get(group) {
                        let rect = Rect::from_min_size(
                            pos2(origin.x + PADDING, top),
                            vec2(width - 2.0 * PADDING, ROWS.header as f32),
                        );
                        group_header(ui, rect, group);
                    }
                }
                ListEntry::Item(index) => {
                    let rect = Rect::from_min_size(pos2(origin.x, top), vec2(width, ROWS.row as f32));
                    self.draw_row(ui, rect, data.ids[index], &mut frame);
                }
            }
        }
        if frame.fading {
            ui.ctx().request_repaint();
        }

        // Thumbnails of visible rows first, then those about to scroll into view.
        let height = f64::from(viewport.height());
        let range = |from: f64, to: f64| {
            let start = item_of(ROWS.entry_at(groups, count, from));
            let end = item_of(ROWS.entry_at(groups, count, to)) + 1;
            start..end.min(count)
        };
        let wanted: Vec<(usize, f32)> = [
            range(view_top, view_bottom),
            range(view_bottom, view_bottom + height * f64::from(PREFETCH_AHEAD)),
            range(view_top - height * f64::from(PREFETCH_BEHIND), view_top),
        ]
        .into_iter()
        .flatten()
        .map(|index| (index, THUMB))
        .collect();
        self.request_textures(wanted, ui.ctx().pixels_per_point());
    }

    fn draw_row(&mut self, ui: &mut egui::Ui, rect: Rect, id: i64, frame: &mut FrameState) {
        let response = ui.interact(rect, ui.make_persistent_id(("row", id)), egui::Sense::click());
        let Some(item) = self.items.get(&id) else {
            let bar = Rect::from_min_max(rect.min + vec2(PADDING, 6.0), rect.max - vec2(PADDING, 6.0));
            ui.painter().rect_filled(bar, theme::RADIUS_CONTROL, theme::RAISED);
            return;
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Row, true, tile_label(item, &[])));
        if !ui.is_rect_visible(rect) {
            return;
        }
        if response.hovered() {
            let hover = Rect::from_min_max(rect.min + vec2(4.0, 0.0), rect.max - vec2(4.0, 0.0));
            ui.painter().rect_filled(hover, theme::RADIUS_CONTROL, theme::HOVER);
        }
        let [thumb, name, folder, date, size, dims, rating] = columns(rect.left(), rect.width());
        let y = rect.center().y;
        let thumb_rect = Rect::from_center_size(pos2(thumb.0 + THUMB / 2.0, y), Vec2::splat(THUMB));
        self.paint_thumbnail(ui, thumb_rect, 4, id, frame);
        let Some(item) = self.items.get(&id) else { return };
        name_cell(ui, name, y, item);
        text_cell(ui, folder, y, folder_of(&item.rel_path), theme::MIST, false);

        // The date taken; without one, the date modified, dimmed and explained (REQ-006-05).
        let undated = item.taken_at.is_none();
        let shown = format::medium_date_time(item.taken_at.unwrap_or(item.mtime));
        let date_rect = text_cell(
            ui,
            date,
            y,
            &shown,
            if undated { theme::MIST } else { theme::PAPER },
            true,
        );
        if undated {
            ui.interact(date_rect, ui.make_persistent_id(("modified", id)), egui::Sense::hover())
                .on_hover_text("Date modified (no capture date)");
        }
        let size_text = format::bytes(item.size);
        let size_galley = one_line(ui, &size_text, numbers(), theme::PAPER, size.1);
        ui.painter().galley(
            pos2(size.0 + size.1 - size_galley.size().x, y - size_galley.size().y / 2.0),
            size_galley,
            theme::PAPER,
        );
        if let (Some(w), Some(h)) = (item.width, item.height) {
            text_cell(ui, dims, y, &format!("{w} × {h}"), theme::MIST, true);
        }
        if let Some(stars) = item.rating.filter(|r| *r > 0) {
            for star in 0..stars.min(5) {
                let at = Rect::from_min_size(pos2(rating.0 + star as f32 * 14.0, y - 6.0), Vec2::splat(12.0));
                Icon::Star.image(12.0, theme::STAR).paint_at(ui, at);
            }
        }
    }
}

fn numbers() -> egui::FontId {
    egui::FontId::new(theme::SIZE_BODY, theme::numbers())
}

/// Text in a column, vertically centered on `y`; returns where it went.
fn text_cell(ui: &egui::Ui, (x, width): (f32, f32), y: f32, text: &str, color: egui::Color32, figures: bool) -> Rect {
    let font = if figures {
        numbers()
    } else {
        egui::FontId::new(theme::SIZE_BODY, egui::FontFamily::Proportional)
    };
    let galley = one_line(ui, text, font, color, width);
    let rect = Rect::from_min_size(pos2(x, y - galley.size().y / 2.0), galley.size());
    ui.painter().galley(rect.min, galley, color);
    rect
}

/// The file name; videos with a play icon and their duration.
fn name_cell(ui: &egui::Ui, (x, width): (f32, f32), y: f32, item: &GalleryItem) {
    let badge = (item.kind == MediaKind::Video).then(|| item.duration.map(format::duration).unwrap_or_default());
    let badge_galley = badge.as_ref().map(|text| {
        one_line(
            ui,
            text,
            egui::FontId::new(theme::SIZE_CAPTION, theme::numbers()),
            theme::MIST,
            width,
        )
    });
    let badge_width = badge_galley.as_ref().map_or(0.0, |g| 6.0 + 12.0 + 4.0 + g.size().x);
    let name = one_line(
        ui,
        &item.file_name,
        egui::FontId::new(theme::SIZE_BODY, theme::semibold()),
        theme::PAPER,
        width - badge_width,
    );
    let name_width = name.size().x;
    ui.painter()
        .galley(pos2(x, y - name.size().y / 2.0), name, theme::PAPER);
    if let Some(galley) = badge_galley {
        let icon = Rect::from_min_size(pos2(x + name_width + 6.0, y - 6.0), Vec2::splat(12.0));
        Icon::Play.image(12.0, theme::MIST).paint_at(ui, icon);
        ui.painter()
            .galley(pos2(icon.right() + 4.0, y - galley.size().y / 2.0), galley, theme::MIST);
    }
}

/// The column names above the rows, staying below the toolbar while the rows scroll (REQ-006-05).
/// `width` is the rows' width, so the names line up with their columns.
pub(super) fn column_header(ui: &mut egui::Ui, area: Rect, width: f32) {
    let rect = Rect::from_min_size(
        pos2(area.left(), area.top() + TOOLBAR_HEIGHT),
        vec2(area.width(), COLUMN_HEADER_HEIGHT),
    );
    // Rows scrolled under the header don't react to the pointer.
    ui.interact(rect, ui.make_persistent_id("list header"), egui::Sense::click());
    // Painted after the rows, so it covers them.
    let painter = ui.painter();
    painter.rect_filled(rect, 0, theme::RAISED);
    let font = egui::FontId::new(theme::SIZE_CAPTION, theme::semibold());
    let names = ["", "Name", "Folder", "Date", "Size", "Dimensions", "Rating"];
    for (index, ((x, width), name)) in columns(rect.left(), width).into_iter().zip(names).enumerate() {
        // Size is right-aligned, like its numbers.
        let (anchor, at) = if index == 4 {
            (Align2::RIGHT_CENTER, pos2(x + width, rect.center().y))
        } else {
            (Align2::LEFT_CENTER, pos2(x, rect.center().y))
        };
        painter.text(at, anchor, name, font.clone(), theme::MIST);
    }
}

/// Gray rows while the layout loads (REQ-006-10).
pub(super) fn skeleton(ui: &egui::Ui) {
    let area = ui.max_rect();
    let mut y = area.top() + TOOLBAR_HEIGHT + PADDING;
    for _ in 0..12 {
        let rect = Rect::from_min_size(pos2(area.left() + PADDING, y), vec2(area.width() - 2.0 * PADDING, 40.0));
        ui.painter().rect_filled(rect, theme::RADIUS_CONTROL, theme::RAISED);
        y += 52.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_006_05_splits_the_width_into_columns() {
        let columns = columns(0.0, 1280.0);
        // Thumbnail first, fixed columns at their widths, name and folder sharing the rest 3 : 2.
        assert_eq!(columns[0], (12.0, 40.0));
        assert!((columns[1].1 / columns[2].1 - 1.5).abs() < 1e-3);
        let (last_x, last_w) = columns[6];
        assert_eq!(last_x + last_w, 1280.0 - 12.0);
        assert_eq!(folder_of("trip/day 1/a.jpg"), "trip/day 1");
        assert_eq!(folder_of("a.jpg"), "");
    }
}
