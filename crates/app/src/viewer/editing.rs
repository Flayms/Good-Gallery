//! The viewer's editing mode (spec 021): its session kept in step with the item, keys, the panel and leaving it.

use super::bar::editable;
use super::{Action, Keys, Layout, Viewer};
use crate::edit::{Dialog, EditSession, Leave, Outcome, SaveMode};
use crate::gallery::Gallery;
use crate::originals::Originals;
use crate::{edit, theme};
use db::gallery::GalleryItem;
use db::libraries::Library;
use domain::viewer::{self as zoom};
use eframe::egui;
use std::path::PathBuf;

impl Viewer {
    /// Starts editing the shown photo with the next frame, as `e` does.
    pub fn request_edit(&mut self) {
        self.toggle_edit = true;
    }

    /// The edit of the shown photo, while editing (spec 021).
    pub fn editing(&self) -> Option<&EditSession> {
        self.edit.as_ref()
    }

    pub fn editing_mut(&mut self) -> Option<&mut EditSession> {
        self.edit.as_mut()
    }

    /// The app finished the save it was asked for: editing ends (and leaves as asked), or stays open after a failure,
    /// which the app shows (REQ-021-16).
    pub fn edit_saved(&mut self, media: i64, saved: bool) {
        let Some(session) = self.edit.as_mut().filter(|session| session.media() == media) else {
            return;
        };
        session.saving = false;
        if saved {
            self.edit = None;
            self.leave_next = self.after_save.take();
        } else {
            self.after_save = None;
        }
    }

    /// Starts or stops editing the shown photo as asked (`e`, the bar's button, the context menu); loads the
    /// original once the item's details say which it is (REQ-021-01, REQ-021-11).
    pub(super) fn sync_edit(
        &mut self,
        ctx: &egui::Context,
        item: &GalleryItem,
        libraries: &[Library],
        originals: &mut Originals,
        gallery: &mut Gallery,
        key: bool,
    ) {
        let toggle = key | std::mem::take(&mut self.toggle_edit);
        if self.edit.as_ref().is_some_and(|session| session.media() != item.id) {
            self.edit = None;
        }
        if toggle && self.closing.is_none() {
            if self.edit.is_some() {
                self.leave_edit(Leave::Exit, gallery);
            } else if editable(item, libraries)
                && let Some(library) = libraries.iter().find(|library| library.id == item.root_id)
            {
                let side = |side: Option<i64>| side.and_then(|side| u32::try_from(side).ok()).unwrap_or(0);
                let target = edit::Target {
                    media: item.id,
                    root_id: item.root_id,
                    rel_path: item.rel_path.clone(),
                    file_name: item.file_name.clone(),
                    library: PathBuf::from(&library.path),
                    full_size: [side(item.width), side(item.height)],
                    sidecar_orientation: item.sidecar_orientation,
                };
                self.edit = Some(EditSession::new(target, originals.render_state()).with_luts(self.luts.clone()));
            }
        }
        if let Some(session) = &mut self.edit
            && session.is_waiting()
        {
            match gallery.detail(item.id) {
                Some(Ok(Some(detail))) => session.start(detail.edit.clone(), ctx),
                Some(_) => session.start(None, ctx),
                None => {}
            }
        }
        // The original the viewer already shows, instead of reading the file again (REQ-021-19).
        if let Some(session) = &mut self.edit {
            session.use_shown(&originals.get(item.id), || originals.lend(item.id));
        }
    }

    /// The keys while editing (REQ-021-07, REQ-021-08): undo and redo; `Esc` stops editing, stepping asks first.
    /// Returns the keys left for the viewer.
    pub(super) fn edit_keys(&mut self, keys: Keys, count: usize, gallery: &Gallery) -> Keys {
        let Some(session) = &mut self.edit else { return keys };
        if session.dialog.is_some() {
            if keys.close {
                session.dialog = None;
            }
            return Keys::default();
        }
        // Escape leaves the crop first.
        if keys.close && session.is_cropping() {
            session.set_cropping(false);
            return Keys::default();
        }
        if keys.undo {
            session.undo();
        }
        if keys.redo {
            session.redo();
        }
        if keys.close {
            self.leave_edit(Leave::Exit, gallery);
        }
        if keys.step != 0
            && let Some(target) = zoom::step_target(self.index, count, keys.step, self.left_result)
        {
            self.leave_edit(Leave::Step(target), gallery);
        }
        Keys {
            zoom: keys.zoom,
            fit: keys.fit,
            filmstrip: keys.filmstrip,
            edit: keys.edit,
            ..Keys::default()
        }
    }

    /// Leaves editing as asked, unless there are unsaved changes: then a dialog asks first (REQ-021-08).
    pub(super) fn leave_edit(&mut self, leave: Leave, gallery: &Gallery) {
        if let Some(session) = &mut self.edit
            && let Some(Outcome::Leave(leave)) = session.leave(leave)
        {
            self.apply_leave(leave, gallery);
        }
    }

    pub(super) fn apply_leave(&mut self, leave: Leave, gallery: &Gallery) {
        self.edit = None;
        self.comparing = false;
        match leave {
            Leave::Close => self.close(gallery, Action::Close),
            Leave::Step(index) => self.go_to(index, gallery),
            Leave::Exit => {}
        }
    }

    fn apply_outcome(&mut self, outcome: Option<Outcome>, gallery: &Gallery) -> Option<Action> {
        match outcome? {
            Outcome::Save(request, then) => {
                self.after_save = then;
                Some(Action::SaveEdit(request))
            }
            Outcome::Leave(leave) => {
                self.apply_leave(leave, gallery);
                None
            }
        }
    }

    /// The adjustments panel in the info panel's place (REQ-021-01).
    pub(super) fn edit_panel(&mut self, ui: &mut egui::Ui, layout: &Layout, gallery: &Gallery) -> Option<Action> {
        let rect = layout.info?;
        let backing = self.chrome.backing(theme::glass_panel(), self.chrome_blur);
        let session = self.edit.as_mut()?;
        backing.paint(ui.painter(), rect, f32::from(theme::RADIUS_PANEL));
        // Clicks and wheel turns on the panel stay in it.
        ui.interact(rect, ui.make_persistent_id("edit panel"), egui::Sense::click());
        let (outcome, compare) = session.panel(ui, rect);
        self.comparing = compare;
        if session.take_import_lut() {
            return Some(Action::ImportLut);
        }
        self.apply_outcome(outcome, gallery)
    }

    /// The dialogs of editing: unsaved changes (REQ-021-08), replacing the original (REQ-021-10).
    pub(super) fn edit_dialog(&mut self, ctx: &egui::Context, gallery: &Gallery) -> Option<Action> {
        let session = self.edit.as_mut()?;
        let dialog = session.dialog?;
        let name = session.target.file_name.clone();
        let mut outcome = None;
        match dialog {
            Dialog::Leave(leave) => {
                let text = format!("Your changes to {name} aren't saved yet.");
                match crate::widgets::dialog(
                    ctx,
                    "edit-leave",
                    "Save your changes?",
                    &text,
                    &["Save", "Discard", "Cancel"],
                ) {
                    Some(0) => outcome = session.save(SaveMode::Replace, Some(leave)),
                    Some(1) => {
                        session.dialog = None;
                        outcome = Some(Outcome::Leave(leave));
                    }
                    Some(_) => session.dialog = None,
                    None => {}
                }
            }
            Dialog::Replace(then) => {
                let text = format!(
                    "The edited photo replaces {name}. The original is kept in the library's .good-gallery folder, and \
                     Revert to original brings it back."
                );
                match crate::widgets::dialog(
                    ctx,
                    "edit-replace",
                    "Replace the original?",
                    &text,
                    &["Replace", "Cancel"],
                ) {
                    Some(0) => outcome = session.save(SaveMode::Replace, then),
                    Some(_) => session.dialog = None,
                    None => {}
                }
            }
            Dialog::WithoutBackup(then) => {
                let text = format!(
                    "The edited photo replaces {name}, and its original is deleted. The edit can't be changed or \
                     undone afterwards."
                );
                match crate::widgets::dialog(
                    ctx,
                    "edit-without-backup",
                    "Save without a backup?",
                    &text,
                    &["Save without backup", "Cancel"],
                ) {
                    Some(0) => outcome = session.save(SaveMode::WithoutBackup, then),
                    Some(_) => session.dialog = None,
                    None => {}
                }
            }
        }
        self.apply_outcome(outcome, gallery)
    }
}
