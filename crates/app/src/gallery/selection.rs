//! Selecting items in the gallery (spec 025) and what is asked of a selection.

use super::*;

impl Gallery {
    /// The selection (spec 025).
    pub fn selection(&self) -> &domain::selection::Selection {
        &self.selection
    }

    /// How many items are selected (REQ-025-06).
    pub fn selected_count(&self) -> usize {
        self.data.as_ref().map_or(0, |data| self.selection.count(&data.ids))
    }

    /// The selected items, in gallery order.
    pub fn selected_ids(&self) -> Vec<i64> {
        self.data
            .as_ref()
            .map_or_else(Vec::new, |data| self.selection.ids(&data.ids))
    }

    /// Selects nothing (REQ-025-04).
    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// Asks to rate the selection (REQ-025-07): stars, or `None` to remove the rating.
    pub fn rate_selection(&mut self, rating: Option<i64>) {
        if self.selected_count() > 0 {
            self.selection_change = Some(crate::state::Many::Rate(rating));
        }
    }

    /// Asks to add the tag with this path to the selection (REQ-026-06).
    pub fn tag_selection(&mut self, path: Vec<String>) {
        if self.selected_count() > 0 {
            self.selection_change = Some(crate::state::Many::Tag(path));
        }
    }

    /// Asks to rename the selection (REQ-035-01).
    pub fn ask_rename(&mut self) {
        self.rename_asked = self.selected_count() > 0;
    }

    /// _Rename…_ was chosen this frame.
    pub fn take_rename_asked(&mut self) -> bool {
        std::mem::take(&mut self.rename_asked)
    }

    /// Asks to fix the dates of the selection (REQ-038-01).
    pub fn ask_fix_dates(&mut self) {
        self.fix_dates_asked = self.selected_count() > 0;
    }

    /// _Fix dates…_ was chosen this frame.
    pub fn take_fix_dates_asked(&mut self) -> bool {
        std::mem::take(&mut self.fix_dates_asked)
    }

    /// Delete was pressed with a selection this frame (REQ-009-23).
    pub fn take_delete_asked(&mut self) -> bool {
        std::mem::take(&mut self.delete_asked)
    }

    /// The rating or tag asked for the selection this frame, if any (REQ-025-07, REQ-026-06).
    pub fn take_selection_change(&mut self) -> Option<crate::state::Many> {
        self.selection_change.take()
    }

    /// The gallery's own keys while it is in front (no viewer, no focused text field): Ctrl+A selects all, Escape
    /// clears (REQ-025-04), and `1`–`5` rate the selection in the desktop app (REQ-025-07).
    pub fn selection_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let empty = self.selected_count() == 0;
        let menu_open = egui::Popup::is_any_open(ctx);
        ctx.input_mut(|input| {
            if input.consume_key(egui::Modifiers::COMMAND, egui::Key::A) {
                self.selection.select_all();
            }
            // Escape closes an open menu first.
            if !empty && !menu_open && input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                self.selection.clear();
            }
            if !empty && !crate::WEB {
                let stars = [
                    egui::Key::Num1,
                    egui::Key::Num2,
                    egui::Key::Num3,
                    egui::Key::Num4,
                    egui::Key::Num5,
                ];
                for (rating, key) in (1..).zip(stars) {
                    if input.consume_key(egui::Modifiers::NONE, key) {
                        self.selection_change = Some(crate::state::Many::Rate(Some(rating)));
                    }
                }
                if input.consume_key(egui::Modifiers::NONE, egui::Key::Delete) {
                    self.delete_asked = true;
                }
            }
        });
    }

    /// Selects all items of the search (REQ-025-04).
    pub fn select_all(&mut self) {
        self.selection.select_all();
    }

    /// Clicks on the item at `index` (a tile or a row): without a selection a click opens it, with one it selects or
    /// deselects it (REQ-025-10, REQ-025-12). Enter on the focused item always opens it, Space always toggles it, and
    /// Shift adds the range from the anchor (REQ-025-02).
    pub(super) fn respond(&mut self, ui: &egui::Ui, response: &egui::Response, index: usize) {
        let Some(data) = self.data.clone() else { return };
        let Some(&id) = data.ids.get(index) else { return };
        if !response.clicked() {
            return;
        }
        let (modifiers, space, enter) = ui.input(|input| {
            (
                input.modifiers,
                input.key_pressed(egui::Key::Space),
                input.key_pressed(egui::Key::Enter),
            )
        });
        let focused = response.has_focus();
        if modifiers.shift {
            self.selection.add_range(&data.ids, id);
        } else if space && focused {
            self.selection.toggle(id);
        } else if (enter && focused) || self.selection.is_empty(&data.ids) {
            self.opened = Some(index);
        } else {
            self.selection.toggle(id);
        }
    }

    /// The circle of the item at `index` was clicked (REQ-025-11, REQ-025-02).
    pub(super) fn circle_clicked(&mut self, ui: &egui::Ui, index: usize) {
        let Some(data) = self.data.clone() else { return };
        let Some(&id) = data.ids.get(index) else { return };
        if ui.input(|input| input.modifiers.shift) {
            self.selection.add_range(&data.ids, id);
        } else {
            self.selection.toggle(id);
        }
    }

    /// Selects the item at `index`, or no longer (REQ-025-11).
    pub fn toggle_selected_at(&mut self, index: usize) {
        if let Some(&id) = self.data.as_ref().and_then(|data| data.ids.get(index)) {
            self.selection.toggle(id);
        }
    }

    /// Before drawing items: what a Shift-click on the item under the pointer would add, without any scanning of the
    /// search unless Shift is held over an item with a selection (REQ-025-14).
    pub(super) fn begin_items(&mut self, ui: &egui::Ui, ids: &[i64]) {
        let hovered = self.hovered.take();
        self.range_preview = hovered
            .filter(|_| ui.input(|input| input.modifiers.shift) && !self.selection.is_empty(ids))
            .and_then(|index| ids.get(index))
            .and_then(|&id| self.selection.range_to(ids, id));
    }

    /// The items a Shift-click would add right now, those already selected too (REQ-025-14).
    pub fn range_preview(&self) -> Option<std::ops::RangeInclusive<usize>> {
        self.range_preview.clone()
    }
}
