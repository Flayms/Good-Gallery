//! Good Gallery's UI. `main.rs` runs it in a window; tests drive the same [`ui`] with `egui_kittest`.

pub mod empty_state;
pub mod format;
pub mod gallery;
pub mod icons;
pub mod indexer_progress;
pub mod settings_page;
pub mod state;
pub mod textures;
pub mod theme;
pub mod toasts;
pub mod toolbar;

pub use state::{AppState, FolderPicker, NativeFolderPicker, Page};

use eframe::egui;

/// Once per context: theme, fonts and image loaders (icons are SVG).
pub fn setup(ctx: &egui::Context) {
    theme::install(ctx);
    egui_extras::install_image_loaders(ctx);
}

/// Builds one frame of the whole window.
pub fn ui(ui: &mut egui::Ui, state: &mut AppState) {
    state.poll();
    let ctx = ui.ctx().clone();
    ctx.layer_painter(egui::LayerId::background())
        .rect_filled(ctx.content_rect(), 0.0, theme::DARKROOM);
    if state.is_active() {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }

    if let Some(error) = &state.error {
        empty_state::error(ui, "Couldn't open the index", error);
    } else {
        match state.page {
            Page::Gallery => gallery(ui, state),
            Page::Settings => settings_page::ui(ui, state),
        }
    }
    indexer_progress::show(&ctx, state);
    state.toasts.show(&ctx);
}

/// The gallery, or its empty state without libraries.
fn gallery(ui: &mut egui::Ui, state: &mut AppState) {
    if state.libraries.is_empty() {
        if empty_state::no_libraries(ui).clicked() {
            state.page = Page::Settings;
        }
    } else if let Some(gallery) = &mut state.gallery {
        gallery.show(ui);
    }
    // The toolbar floats over the gallery, which scrolls underneath it (REQ-013-03).
    let area = ui.max_rect();
    let gallery = if state.libraries.is_empty() {
        None
    } else {
        state.gallery.as_mut()
    };
    if toolbar::show(ui, area, gallery) == Some(toolbar::ToolbarAction::OpenSettings) {
        state.page = Page::Settings;
    }
}
