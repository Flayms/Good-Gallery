//! Good Gallery's UI. `main.rs` runs it in a window; tests drive the same [`ui`] with `egui_kittest`.

pub mod empty_state;
pub mod format;
pub mod icons;
pub mod settings_page;
pub mod state;
pub mod theme;
pub mod toasts;

pub use state::{AppState, FolderPicker, NativeFolderPicker, Page};

use eframe::egui;
use icons::{Icon, icon_button};

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
    if state.is_busy() {
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
    state.toasts.show(&ctx);
}

/// The gallery. Until M6 it only has its empty states.
fn gallery(ui: &mut egui::Ui, state: &mut AppState) {
    // Until the toolbar exists (M6), Settings is reached from a button in the top-right corner.
    let corner = egui::Rect::from_min_size(
        ui.max_rect().right_top() + egui::vec2(-theme::CONTROL_HEIGHT - 8.0, 8.0),
        egui::Vec2::splat(theme::CONTROL_HEIGHT),
    );
    if icon_button(
        &mut ui.new_child(egui::UiBuilder::new().max_rect(corner)),
        Icon::Settings,
        "Settings",
        "Settings",
    )
    .clicked()
    {
        state.page = Page::Settings;
    }
    if state.libraries.is_empty() {
        if empty_state::no_libraries(ui).clicked() {
            state.page = Page::Settings;
        }
    } else if state.media_count == 0 {
        empty_state::no_media(ui);
    } else {
        empty_state::message(
            ui,
            Icon::Images,
            &format!("{} media items indexed", state.media_count),
            None,
        );
    }
}
