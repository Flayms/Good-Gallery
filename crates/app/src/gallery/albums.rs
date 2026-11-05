//! Smart albums (spec 034): saving the search, naming, renaming and deleting.

use super::Gallery;
use crate::widgets;
use domain::search::GallerySearch;
use domain::smart_albums::{SmartAlbum, clean_name, suggested_name};
use eframe::egui;

/// What the name is for.
enum Naming {
    Save(GallerySearch),
    Rename(i64),
}

pub(super) struct NameDialog {
    naming: Naming,
    name: String,
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

    /// Asks for the name of a smart album of the current search (REQ-034-01).
    pub fn ask_save_album(&mut self) {
        self.album_dialog = Some(NameDialog {
            name: suggested_name(&self.search),
            naming: Naming::Save(self.search.clone()),
        });
    }

    /// Asks for a new name of `album` (REQ-034-05).
    pub fn ask_rename_album(&mut self, album: &SmartAlbum) {
        self.album_dialog = Some(NameDialog {
            name: album.name.clone(),
            naming: Naming::Rename(album.id),
        });
    }

    /// Removes a smart album, never a file (REQ-034-05).
    pub fn delete_album(&mut self, id: i64) {
        let result = db::smart_albums::delete(&self.write_conn(), id);
        self.after_album_change(result.map(|_| ()));
    }

    /// The naming dialog, while it is open.
    pub fn album_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.album_dialog else { return };
        let (title, verb) = match dialog.naming {
            Naming::Save(_) => ("Save as smart album", "Save"),
            Naming::Rename(_) => ("Rename smart album", "Rename"),
        };
        let Some(confirmed) = widgets::name_dialog(ctx, "smart-album-name", title, &mut dialog.name, verb) else {
            return;
        };
        let dialog = self.album_dialog.take().expect("open above");
        let name = clean_name(&dialog.name).filter(|_| confirmed);
        let Some(name) = name else { return };
        let result = {
            let conn = self.write_conn();
            match &dialog.naming {
                Naming::Save(search) => db::smart_albums::insert(&conn, &name, search).map(|_| ()),
                Naming::Rename(id) => db::smart_albums::rename(&conn, *id, &name),
            }
        };
        self.after_album_change(result);
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
