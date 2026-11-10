//! The context menu of tiles and list rows.

use super::*;

/// What a tile's, row's or the viewer's context menu asks for (REQ-009-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAction {
    /// Puts the photo on the clipboard (REQ-009-25).
    Copy,
    ShowInExplorer,
    Information,
    /// Only in the viewer (REQ-021-01).
    Edit,
    /// Only in the viewer, for an edit saved over its original (REQ-021-12).
    Revert,
    /// Only in the gallery: selects the item or no longer does, also where nothing can hover (REQ-025-11).
    Select,
    /// Opens the rename dialog (REQ-009-21).
    Rename,
    /// Sets the rating; `None` removes it (REQ-009-21).
    Rate(Option<i64>),
    /// Turns photos a quarter by their orientation tag (REQ-009-22).
    Turn(domain::orientation::Turn),
    /// Asks before deleting (REQ-009-23).
    Delete,
    /// Opens the date fix dialog (REQ-038-01).
    FixDates,
}

/// Which of the items that write files a context menu has (REQ-009-21).
#[derive(Debug, Clone, Copy, Default)]
pub struct MenuItems {
    /// _Copy_: a photo, in the desktop app (REQ-009-25).
    pub copy: bool,
    /// _Edit_, only in the viewer (REQ-021-01).
    pub edit: bool,
    /// _Revert to original_, only in the viewer (REQ-021-12).
    pub revert: bool,
    /// _Rename…_, _Rating_, _Rotate_ and _Delete…_: the desktop app, for an indexed item.
    pub write: bool,
    /// _Rotate_ is on: a photo whose format has an orientation.
    pub turn: bool,
}

impl MenuItems {
    /// The items for a gallery item of this kind and file name.
    pub fn for_item(kind: MediaKind, file_name: &str) -> Self {
        Self {
            copy: !crate::WEB && kind == MediaKind::Image,
            write: !crate::WEB,
            turn: kind == MediaKind::Image && domain::orientation::can_turn(file_name),
            ..Self::default()
        }
    }
}

/// _Show in Explorer_ and _Information_ in a context menu (REQ-009-11), and `items`; what was chosen.
pub fn context_menu(ui: &mut egui::Ui, selected: bool, items: MenuItems) -> Option<MediaAction> {
    crate::glass::behind_menu(ui, |ui| {
        let select = ui.button(if selected { "Deselect" } else { "Select" }).clicked();
        if select {
            ui.close();
            return Some(MediaAction::Select);
        }
        menu_items(ui, items)
    })
}

/// The viewer's context menu, with _Edit_ and _Revert to original_ where they apply (spec 021).
pub fn context_menu_with(ui: &mut egui::Ui, items: MenuItems) -> Option<MediaAction> {
    crate::glass::behind_menu(ui, |ui| menu_items(ui, items))
}

/// The ratings _Rating_ offers, with their labels (REQ-009-21).
pub(super) const RATINGS: [(&str, Option<i64>); 6] = [
    ("No rating", None),
    ("1 star", Some(1)),
    ("2 stars", Some(2)),
    ("3 stars", Some(3)),
    ("4 stars", Some(4)),
    ("5 stars", Some(5)),
];

pub(super) fn menu_items(ui: &mut egui::Ui, items: MenuItems) -> Option<MediaAction> {
    let MenuItems { edit, revert, .. } = items;
    let mut action = None;
    if items.copy && ui.button("Copy").clicked() {
        action = Some(MediaAction::Copy);
    }
    // The browser can't open a file manager (REQ-019-13).
    if !crate::WEB && ui.button(crate::state::SHOW_IN_FOLDER).clicked() {
        action = Some(MediaAction::ShowInExplorer);
    }
    if ui.button("Information").clicked() {
        action = Some(MediaAction::Information);
    }
    if edit && ui.button("Edit").clicked() {
        action = Some(MediaAction::Edit);
    }
    if revert && ui.button("Revert to original").clicked() {
        action = Some(MediaAction::Revert);
    }
    if items.write {
        ui.separator();
        if ui.button("Rename…").clicked() {
            action = Some(MediaAction::Rename);
        }
        if ui.button("Fix dates…").clicked() {
            action = Some(MediaAction::FixDates);
        }
        ui.menu_button("Rating", |ui| {
            crate::glass::behind_menu(ui, |ui| {
                for (label, rating) in RATINGS {
                    if ui.button(label).clicked() {
                        action = Some(MediaAction::Rate(rating));
                    }
                }
            });
        });
        let turns = [
            ("Rotate left", domain::orientation::Turn::Left),
            ("Rotate right", domain::orientation::Turn::Right),
        ];
        for (label, turn) in turns {
            if ui.add_enabled(items.turn, egui::Button::new(label)).clicked() {
                action = Some(MediaAction::Turn(turn));
            }
        }
        ui.separator();
        if ui.button("Delete…").clicked() {
            action = Some(MediaAction::Delete);
        }
    }
    if action.is_some() {
        ui.close();
    }
    action
}

impl Gallery {
    /// What a context menu asked for this frame, and for which gallery position (REQ-009-11).
    /// The items a context menu on item `index` acts on: the selection if the item is in it, else the item alone
    /// (REQ-009-21).
    pub fn menu_targets(&self, index: usize) -> Vec<i64> {
        match self.id_at(index) {
            Some(id) if self.selection.contains(id) => self.selected_ids(),
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }

    pub fn take_menu_action(&mut self) -> Option<(usize, MediaAction)> {
        self.menu_action.take()
    }
}
