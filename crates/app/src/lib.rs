//! Good Gallery's UI. `main.rs` runs it in a window; tests drive the same [`ui`] with `egui_kittest`.

pub mod empty_state;
pub mod format;
pub mod gallery;
pub mod icons;
pub mod indexer_progress;
pub mod originals;
pub mod search_bar;
pub mod settings_page;
pub mod sidebar;
pub mod state;
pub mod textures;
pub mod theme;
pub mod toasts;
pub mod toolbar;
pub mod viewer;

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
    // The viewer's keys go to it alone (REQ-009-02).
    if let Some(viewer) = &mut state.viewer {
        viewer.input(&ctx);
    }
    ctx.layer_painter(egui::LayerId::background())
        .rect_filled(ctx.content_rect(), 0.0, theme::DARKROOM);
    if state.is_active() {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }

    if let Some(error) = &state.error {
        empty_state::error(ui, "Couldn't open the index", error);
    } else {
        let input = sidebar::SidebarInput {
            libraries: &state.libraries,
            scanning: state.indexer_status.current.map(|scan| scan.library),
            page: &mut state.page,
            db: state.db.clone(),
        };
        // The sidebar's data comes with the gallery, also while the Settings page shows.
        if let Some(gallery) = &mut state.gallery {
            gallery.poll();
        }
        let gallery = if state.libraries.is_empty() {
            None
        } else {
            state.gallery.as_mut()
        };
        egui::Panel::left("sidebar")
            .exact_size(sidebar::WIDTH)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::RECESS).inner_margin(8))
            .show(ui, |ui| sidebar::show(ui, &mut state.sidebar, gallery, input));
        egui::CentralPanel::no_frame().show(ui, |ui| match state.page {
            Page::Gallery => gallery_page(ui, state),
            Page::Settings => settings_page::ui(ui, state),
        });
        show_viewer(&ctx, state);
    }
    indexer_progress::show(&ctx, state);
    state.toasts.show(&ctx);
}

/// The viewer over the window while it is open; closing it leaves the gallery where it was (REQ-009-03).
fn show_viewer(ctx: &egui::Context, state: &mut AppState) {
    let (Some(viewer), Some(gallery)) = (&mut state.viewer, &mut state.gallery) else {
        return;
    };
    let originals = state
        .originals
        .get_or_insert_with(|| originals::Originals::new(ctx.clone(), None, None));
    if !viewer.show(ctx, gallery, originals, &state.libraries) {
        state.viewer = None;
        gallery.set_viewed(None);
        originals.set(Vec::new());
    }
}

/// The gallery, or its empty state without libraries.
fn gallery_page(ui: &mut egui::Ui, state: &mut AppState) {
    if state.libraries.is_empty() {
        if empty_state::no_libraries(ui).clicked() {
            state.page = Page::Settings;
        }
    } else if let Some(gallery) = &mut state.gallery {
        gallery.show(ui);
        // A click or Enter on a tile or row opens the viewer (REQ-009-01).
        if let Some(index) = gallery.take_opened()
            && state.viewer.is_none()
            && let Some(id) = gallery.id_at(index)
        {
            state.viewer = Some(viewer::Viewer::new(index, id, gallery.data_version()));
            // Typing and Enter go to the viewer now, not to the search bar or the tile.
            ui.memory_mut(|memory| {
                if let Some(focused) = memory.focused() {
                    memory.surrender_focus(focused);
                }
            });
        }
    }
    // The toolbar floats over the gallery, which scrolls underneath it (REQ-013-03).
    let area = ui.max_rect();
    let gallery = if state.libraries.is_empty() {
        None
    } else {
        state.gallery.as_mut()
    };
    toolbar::show(ui, area, gallery, &mut state.search_bar);
}
