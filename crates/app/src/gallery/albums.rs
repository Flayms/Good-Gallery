//! Smart albums (spec 034): saving the search, naming, renaming and deleting.

use super::Gallery;
use crate::widgets;
use domain::search::GallerySearch;
use domain::smart_albums::{SmartAlbum, clean_name, suggested_name};
use eframe::egui;

/// What the name is for.
enum Naming {
    Rename(i64),
}

pub(super) struct NameDialog {
    naming: Naming,
    name: String,
}

/// The dialog open over the gallery: the editor of a smart album's rules (REQ-039-01), or its name alone.
pub(super) enum AlbumDialog {
    Name(NameDialog),
    Editor(Box<super::album_editor::AlbumEditor>),
}

impl Gallery {
    /// The search can be saved: it isn't the default one (REQ-034-01).
    pub fn can_save_album(&self) -> bool {
        let default = GallerySearch::default();
        GallerySearch {
            sort_by: default.sort_by,
            descending: default.descending,
            ..self.search.clone()
        } != default
    }

    /// Opens the editor for a smart album of the current search, its filters as conditions (REQ-034-01,
    /// REQ-039-01).
    pub fn ask_save_album(&mut self) {
        let editor = super::album_editor::AlbumEditor::new(
            None,
            suggested_name(&self.search),
            &self.search,
            self.album_libraries.clone(),
        );
        self.album_dialog = Some(AlbumDialog::Editor(Box::new(editor)));
    }

    /// Opens the editor of `album`'s rules (REQ-039-01).
    pub fn ask_edit_album(&mut self, album: &SmartAlbum) {
        let editor = super::album_editor::AlbumEditor::new(
            Some(album.id),
            album.name.clone(),
            &album.search,
            self.album_libraries.clone(),
        );
        self.album_dialog = Some(AlbumDialog::Editor(Box::new(editor)));
    }

    /// The libraries the editor offers, by id and name.
    pub fn set_album_libraries(&mut self, libraries: Vec<(i64, String)>) {
        if self.album_libraries != libraries {
            self.album_libraries = libraries;
        }
    }

    /// The open album editor, for tests.
    pub fn album_editor(&mut self) -> Option<&mut super::album_editor::AlbumEditor> {
        match &mut self.album_dialog {
            Some(AlbumDialog::Editor(editor)) => Some(editor),
            _ => None,
        }
    }

    /// Asks for a new name of `album` (REQ-034-05).
    pub fn ask_rename_album(&mut self, album: &SmartAlbum) {
        self.album_dialog = Some(AlbumDialog::Name(NameDialog {
            name: album.name.clone(),
            naming: Naming::Rename(album.id),
        }));
    }

    /// Removes a smart album, never a file (REQ-034-05).
    pub fn delete_album(&mut self, id: i64) {
        let result = db::smart_albums::delete(&self.write_conn(), id);
        self.after_album_change(result.map(|_| ()));
    }

    /// The naming dialog, while it is open.
    pub fn album_dialog(&mut self, ctx: &egui::Context) {
        match &mut self.album_dialog {
            None => {}
            Some(AlbumDialog::Name(dialog)) => {
                let Naming::Rename(id) = dialog.naming;
                let Some(confirmed) = widgets::name_dialog(
                    ctx,
                    "smart-album-name",
                    "Rename smart album",
                    &mut dialog.name,
                    "Rename",
                ) else {
                    return;
                };
                let name = clean_name(&dialog.name).filter(|_| confirmed);
                self.album_dialog = None;
                let Some(name) = name else { return };
                let result = db::smart_albums::rename(&self.write_conn(), id, &name);
                self.after_album_change(result);
            }
            Some(AlbumDialog::Editor(editor)) => {
                let Some(outcome) = editor.show(ctx) else { return };
                let id = editor.id;
                self.album_dialog = None;
                let super::album_editor::Outcome::Save { name, search } = outcome else {
                    return;
                };
                let mut search = *search;
                if let Some(rules) = &mut search.rules {
                    rules.name = name.clone();
                }
                let result = {
                    let conn = self.write_conn();
                    match id {
                        Some(id) => db::smart_albums::update(&conn, id, &name, &search),
                        None => db::smart_albums::insert(&conn, &name, &search).map(|_| ()),
                    }
                };
                // The album shows as saved, so it is the open one (REQ-034-03).
                if result.is_ok() {
                    self.set_search(search);
                }
                self.after_album_change(result);
            }
        }
    }

    fn write_conn(&self) -> std::sync::MutexGuard<'_, db::rusqlite::Connection> {
        self.settings.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn after_album_change(&mut self, result: db::Result<()>) {
        match result {
            Ok(()) => self.reload_sidebar(),
            Err(error) => tracing::error!("can't change a smart album: {error}"),
        }
    }
}
