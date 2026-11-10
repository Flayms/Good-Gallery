//! Scrolling: positions of items in the layout, zooming with the wheel, and autoscroll with the middle button.

use super::*;

/// Where autoscroll started: a round marker with arrows up and down (REQ-006-14).
pub(super) fn paint_autoscroll_marker(ctx: &egui::Context, at: egui::Pos2) {
    // Chevrons pointing up and down, 2.5 points apart from the center.
    paint_autoscroll_disc(ctx, at, &[(-1.0, 2.5), (1.0, 2.5)]);
}

/// The pointer once it left the dead zone (REQ-006-23): one chevron in the middle, up (`-1.0`) or down (`1.0`).
pub(super) fn paint_autoscroll_arrow(ctx: &egui::Context, at: egui::Pos2, direction: f32) {
    paint_autoscroll_disc(ctx, at, &[(direction, -2.25)]);
}

/// A disc at `at` with a chevron 8 points wide and 4.5 high for each `(direction, from)`: pointing up (`-1.0`) or down
/// (`1.0`), its base `from` points from the center that way.
pub(super) fn paint_autoscroll_disc(ctx: &egui::Context, at: egui::Pos2, chevrons: &[(f32, f32)]) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("autoscroll"),
    ));
    painter.circle(at, 14.0, theme::raised(), egui::Stroke::new(1.0, theme::muted()));
    let stroke = egui::Stroke::new(1.5, theme::text());
    for &(direction, from) in chevrons {
        let base = at.y + direction * from;
        let tip = pos2(at.x, base + direction * 4.5);
        painter.line(vec![pos2(at.x - 4.0, base), tip, pos2(at.x + 4.0, base)], stroke);
    }
}

impl Gallery {
    /// Ctrl + wheel over the gallery zooms instead of scrolling; one step per notch, small touchpad movements add up
    /// (REQ-006-06).
    pub(super) fn handle_zoom_wheel(&mut self, ui: &egui::Ui, area: Rect) {
        let hovered = ui.rect_contains_pointer(area);
        let deltas: Vec<f32> = ui.input(|input| {
            input
                .raw
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::MouseWheel {
                        unit, delta, modifiers, ..
                    } if modifiers.ctrl => Some(match unit {
                        // Scrolling down (content moving up) zooms out. winit reports one line per notch (and
                        // several when a fast spin is coalesced); a page counts as one step.
                        egui::MouseWheelUnit::Point => -delta.y,
                        egui::MouseWheelUnit::Line => -delta.y * zoom::WHEEL_STEP,
                        egui::MouseWheelUnit::Page => -delta.y.signum() * zoom::WHEEL_STEP,
                    }),
                    _ => None,
                })
                .collect()
        });
        if !hovered || self.view == GalleryView::List {
            return;
        }
        for delta in deltas {
            let (steps, rest) = zoom::accumulate_wheel(self.wheel, delta);
            self.wheel = rest;
            if steps != 0 {
                self.zoom(steps);
            }
        }
    }

    /// Where item `index` starts in the content: its group's header if it is the group's first item.
    pub(super) fn start_of(&self, arranged: &Arranged, index: usize) -> f64 {
        if self.view == GalleryView::List {
            let entry = list_index_of_item(&arranged.groups, index);
            let first_of_group = arranged.groups.binary_search(&index).is_ok();
            list::ROWS.top(&arranged.groups, if first_of_group { entry - 1 } else { entry })
        } else {
            arranged.tiles.start_of(&arranged.groups, index)
        }
    }

    /// The scroll offset that brings item `index` into view below the toolbar, if it isn't (REQ-016-06).
    pub(super) fn reveal(&self, arranged: &Arranged, index: usize, count: usize) -> Option<f32> {
        if index >= count {
            return None;
        }
        let (top, height) = if self.view == GalleryView::List {
            let entry = list_index_of_item(&arranged.groups, index);
            (list::ROWS.top(&arranged.groups, entry), list::ROWS.row)
        } else {
            (arranged.tiles.top[index], arranged.tiles.height[index])
        };
        let (top, bottom) = (top as f32, (top + height) as f32);
        let (view_top, view_bottom) = (self.offset, self.offset + self.view_height);
        // An item taller than the view counts as shown while it covers part of it; else it would be scrolled every frame.
        let visible = if bottom - top > self.view_height {
            top < view_bottom && bottom > view_top
        } else {
            top >= view_top && bottom <= view_bottom
        };
        (!visible).then_some(top)
    }

    /// The first item at height `y` of the content.
    pub(super) fn item_at(&self, arranged: &Arranged, count: usize, y: f64) -> usize {
        if self.view == GalleryView::List {
            match list_entry_at(&arranged.groups, list::ROWS.entry_at(&arranged.groups, count, y)) {
                ListEntry::Group(group) => arranged.groups[group],
                ListEntry::Item(item) => item,
            }
        } else {
            arranged.tiles.first_item_from(y)
        }
    }

    /// Starts, stops and draws autoscroll with the middle mouse button over `area` (REQ-006-14); how far to scroll this
    /// frame.
    pub(super) fn handle_autoscroll(&mut self, ui: &egui::Ui, area: Rect) -> f32 {
        let ctx = ui.ctx();
        // Escape stops it and does nothing else.
        if self.autoscroll.is_on() && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.autoscroll.stop();
        }
        let (events, pointer, dt, all_up) = ui.input(|input| {
            (
                input.events.clone(),
                input.pointer.latest_pos(),
                input.stable_dt,
                !input.pointer.any_down() && !input.pointer.any_released(),
            )
        });
        if self.swallow_click && all_up {
            self.swallow_click = false;
        }
        for event in events {
            let (pos, button, pressed) = match event {
                egui::Event::PointerButton {
                    pos, button, pressed, ..
                } => (pos, button, pressed),
                // As in a browser, the wheel, the touchpad or a key stops it; the wheel then scrolls as usual.
                egui::Event::MouseWheel { .. } | egui::Event::Key { pressed: true, .. } => {
                    self.autoscroll.stop();
                    continue;
                }
                _ => continue,
            };
            match (button, pressed) {
                (egui::PointerButton::Middle, true) if self.autoscroll.is_on() => self.autoscroll.stop(),
                // Only over the gallery itself: not the toolbar, a menu or the viewer.
                (egui::PointerButton::Middle, true)
                    if area.contains(pos) && ctx.layer_id_at(pos) == Some(ui.layer_id()) =>
                {
                    self.autoscroll.press_middle(pos.y);
                    self.autoscroll_at = pos;
                }
                (egui::PointerButton::Middle, false) => self.autoscroll.release_middle(),
                (_, true) if self.autoscroll.is_on() => {
                    self.autoscroll.stop();
                    self.swallow_click = true;
                }
                _ => {}
            }
        }
        let Some(pointer) = pointer.filter(|_| self.autoscroll.is_on()) else {
            return 0.0;
        };
        self.autoscroll.pointer_at(pointer.y);
        // The app draws the pointer: no system cursor points one way only on every platform (REQ-006-23).
        ctx.set_cursor_icon(egui::CursorIcon::None);
        ctx.request_repaint();
        match self.autoscroll.heading(pointer.y) {
            Some(domain::autoscroll::Heading::Up) => paint_autoscroll_arrow(ctx, pointer, -1.0),
            Some(domain::autoscroll::Heading::Down) => paint_autoscroll_arrow(ctx, pointer, 1.0),
            _ => paint_autoscroll_marker(ctx, self.autoscroll_at),
        }
        self.autoscroll.velocity(pointer.y) * dt.min(domain::autoscroll::MAX_FRAME)
    }
}
