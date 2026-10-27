//! The Masonry, Justified and Grid views: tiles and date group headers positioned by the layout.

use super::{
    Arranged, FrameState, Gallery, OVERSCAN, PADDING, PREFETCH_AHEAD, PREFETCH_BEHIND, TOOLBAR_HEIGHT, one_line,
};
use crate::icons::Icon;
use crate::{format, theme};
use db::gallery::GalleryItem;
use domain::date_groups::DateGroup;
use domain::layout::TileLayout;
use domain::media::MediaKind;
use domain::view::{TagIndicator, TagIndicatorKind, indicators_of};
use eframe::egui::{self, Color32, Rect, Vec2, pos2, vec2};
use std::time::Instant;

/// Solid badges on tiles (`docs/design.md`: never glass on tiles).
const BADGE: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 153);
/// Room a video badge takes in a tile's bottom-right corner, which tag indicators leave free.
const VIDEO_BADGE_ROOM: f32 = 88.0;

impl Gallery {
    pub(super) fn draw_tiles(
        &mut self,
        ui: &mut egui::Ui,
        arranged: &Arranged,
        viewport: Rect,
        origin: egui::Pos2,
        inset_top: f32,
    ) {
        let Some(data) = self.data.clone() else { return };
        let layout = &arranged.tiles;
        let view_top = f64::from(viewport.min.y - inset_top);
        let view_bottom = f64::from(viewport.max.y - inset_top);
        let overscan = f64::from(OVERSCAN);
        let (start, end) = layout.items_in_view(view_top - overscan, view_bottom + overscan);
        self.load_range(start, end);

        let (header_start, header_end) = layout.headers_in_view(view_top, view_bottom);
        for header in &layout.headers[header_start..header_end] {
            // Headers are shown for every day group, in order.
            let Some(group) = data.groups.get(header.group) else {
                continue;
            };
            let rect = Rect::from_min_size(
                origin + vec2(0.0, header.top as f32),
                vec2(ui.max_rect().width() - 2.0 * PADDING, layout.header_height as f32),
            );
            group_header(ui, rect, group);
        }

        let radius = if self.columns >= super::DENSE_COLUMNS { 2 } else { 4 };
        let mut frame = FrameState {
            now: Instant::now(),
            made: 0,
            fading: false,
        };
        for index in start..end {
            let rect = tile_rect(layout, index, origin);
            if f64::from(rect.max.y - origin.y) < view_top - overscan {
                continue;
            }
            let id = data.ids[index];
            let response = ui.interact(rect, ui.make_persistent_id(("tile", id)), egui::Sense::click());
            // A click, or Enter on the focused tile, opens the viewer (REQ-009-01).
            if response.clicked() {
                self.opened = Some(index);
            }
            let indicators = self
                .items
                .get(&id)
                .map(|item| {
                    indicators_of(
                        item.rating,
                        (item.tagged.places, item.tagged.people, item.tagged.other),
                        &self.indicators,
                    )
                })
                .unwrap_or_default();
            if let Some(item) = self.items.get(&id) {
                response
                    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, tile_label(item, &indicators)));
            }
            if !ui.is_rect_visible(rect) {
                continue;
            }
            self.paint_thumbnail(ui, rect, radius, id, &mut frame);
            let Some(item) = self.items.get(&id) else { continue };
            let video = item.kind == MediaKind::Video;
            if video {
                video_badge(ui, rect, item.duration);
            }
            if !indicators.is_empty() {
                indicator_badge(ui, rect, &indicators, video);
            }
        }
        if frame.fading {
            ui.ctx().request_repaint();
        }

        // Visible tiles first, then those about to scroll into view.
        let height = f64::from(viewport.height());
        let ranges = [
            layout.items_in_view(view_top, view_bottom),
            layout.items_in_view(view_bottom, view_bottom + height * f64::from(PREFETCH_AHEAD)),
            layout.items_in_view(view_top - height * f64::from(PREFETCH_BEHIND), view_top),
        ];
        let wanted: Vec<(usize, f32)> = ranges
            .into_iter()
            .flat_map(|(start, end)| start..end)
            .map(|index| (index, layout.width[index] as f32))
            .collect();
        self.request_textures(wanted, ui.ctx().pixels_per_point());
    }
}

fn tile_rect(layout: &TileLayout, index: usize, origin: egui::Pos2) -> Rect {
    Rect::from_min_size(
        origin + vec2(layout.left[index] as f32, layout.top[index] as f32),
        vec2(layout.width[index] as f32, layout.height[index] as f32),
    )
}

/// What a screen reader says for a tile: the file name, for videos with the duration, and the tag indicators
/// (REQ-017-02).
pub(super) fn tile_label(item: &GalleryItem, indicators: &[TagIndicator]) -> String {
    let mut label = match (item.kind, item.duration) {
        (MediaKind::Video, Some(duration)) => format!("{}, video, {}", item.file_name, format::duration(duration)),
        (MediaKind::Video, None) => format!("{}, video", item.file_name),
        _ => item.file_name.clone(),
    };
    for indicator in indicators {
        label.push_str(", ");
        label.push_str(&indicator.label);
    }
    label
}

/// The day and the number of media items above a date group, at the bottom of its space (REQ-016-02).
pub(super) fn group_header(ui: &mut egui::Ui, rect: Rect, group: &DateGroup) {
    let (day, count) = (format::day(group.day), format::group_count(group.photos, group.videos));
    ui.interact(
        rect,
        ui.make_persistent_id(("group", group.start)),
        egui::Sense::hover(),
    )
    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Heading, true, format!("{day}, {count}")));
    let day = one_line(
        ui,
        &day,
        egui::FontId::new(theme::SIZE_BODY, theme::semibold()),
        theme::PAPER,
        rect.width() * 0.7,
    );
    let count = one_line(
        ui,
        &count,
        egui::FontId::new(theme::SIZE_CAPTION, theme::numbers()),
        theme::MIST,
        rect.width() * 0.3,
    );
    let baseline = rect.bottom() - 8.0;
    let day_pos = pos2(rect.left(), baseline - day.size().y);
    let count_pos = pos2(day_pos.x + day.size().x + 8.0, baseline - count.size().y - 1.0);
    ui.painter().galley(day_pos, day, theme::PAPER);
    ui.painter().galley(count_pos, count, theme::MIST);
}

/// A play badge with the duration in the tile's bottom-right corner (REQ-006-09).
fn video_badge(ui: &egui::Ui, tile: Rect, duration: Option<f64>) {
    let text = duration.map(format::duration);
    let font = egui::FontId::new(theme::SIZE_CAPTION, theme::numbers());
    let galley = text.map(|text| ui.painter().layout_no_wrap(text, font, Color32::WHITE));
    let icon = 12.0;
    let text_width = galley.as_ref().map_or(0.0, |g| g.size().x + 4.0);
    let size = vec2(6.0 + icon + text_width + 6.0, 20.0);
    let rect = Rect::from_min_size(tile.max - size - vec2(6.0, 6.0), size);
    if rect.min.x < tile.min.x || rect.min.y < tile.min.y {
        return;
    }
    ui.painter().rect_filled(rect, 4, BADGE);
    let icon_rect = Rect::from_min_size(rect.min + vec2(6.0, (size.y - icon) / 2.0), Vec2::splat(icon));
    Icon::Play.image(icon, Color32::WHITE).paint_at(ui, icon_rect);
    if let Some(galley) = galley {
        let pos = pos2(icon_rect.max.x + 4.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, Color32::WHITE);
    }
}

/// Stars and tag icons in the tile's bottom-left corner, cut short before the video badge (REQ-017-01), each named
/// on hover (REQ-017-02).
fn indicator_badge(ui: &mut egui::Ui, tile: Rect, indicators: &[TagIndicator], video: bool) {
    const ICON: f32 = 12.0;
    const SPACING: f32 = 6.0;
    let max_width = tile.width() - 12.0 - if video { VIDEO_BADGE_ROOM } else { 0.0 };
    // Each indicator's width; those that don't fit are left out.
    let mut parts = Vec::new();
    let mut width = 12.0 - SPACING;
    for indicator in indicators {
        let part = if indicator.kind == TagIndicatorKind::Rating {
            f32::from(indicator.stars) * ICON
        } else {
            ICON
        };
        if width + SPACING + part > max_width {
            break;
        }
        width += SPACING + part;
        parts.push((indicator, part));
    }
    if parts.is_empty() {
        return;
    }
    let rect = Rect::from_min_size(pos2(tile.left() + 6.0, tile.bottom() - 6.0 - 20.0), vec2(width, 20.0));
    ui.painter().rect_filled(rect, 4, BADGE);
    let mut x = rect.left() + 6.0;
    for (indicator, part) in parts {
        let area = Rect::from_min_size(pos2(x, rect.center().y - ICON / 2.0), vec2(part, ICON));
        match indicator.kind {
            TagIndicatorKind::Rating => {
                for star in 0..indicator.stars {
                    let at = Rect::from_min_size(area.min + vec2(f32::from(star) * ICON, 0.0), Vec2::splat(ICON));
                    Icon::Star.image(ICON, theme::STAR).paint_at(ui, at);
                }
            }
            kind => {
                let icon = match kind {
                    TagIndicatorKind::Places => Icon::MapPin,
                    TagIndicatorKind::People => Icon::User,
                    _ => Icon::Tag,
                };
                icon.image(ICON, Color32::WHITE).paint_at(ui, area);
            }
        }
        let id = ui.make_persistent_id((
            "indicator",
            tile.min.x.to_bits(),
            tile.min.y.to_bits(),
            indicator.kind.key(),
        ));
        ui.interact(area, id, egui::Sense::hover())
            .on_hover_text(&indicator.label);
        x += part + SPACING;
    }
}

/// Gray tiles while the layout loads (REQ-006-10).
pub(super) fn skeleton(ui: &egui::Ui, columns: usize, gap: f32) {
    let area = ui.max_rect();
    let width = (area.width() - 2.0 * PADDING).max(0.0);
    let side = domain::layout::column_width(f64::from(width), columns, f64::from(gap)) as f32;
    if side <= 0.0 {
        return;
    }
    let top = area.top() + TOOLBAR_HEIGHT + PADDING;
    for row in 0.. {
        let y = top + row as f32 * (side + gap);
        if y > area.bottom() {
            break;
        }
        for column in 0..columns {
            let min = pos2(area.left() + PADDING + column as f32 * (side + gap), y);
            ui.painter()
                .rect_filled(Rect::from_min_size(min, Vec2::splat(side)), 4, theme::RAISED);
        }
    }
}
