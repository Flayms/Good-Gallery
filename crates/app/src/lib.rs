//! Good Gallery's UI. `main.rs` runs it in a window; tests drive the same [`ui`] with `egui_kittest`.

pub mod empty_state;
pub mod theme;

use eframe::egui;

/// Everything the UI shows. Background work reports into it over channels (from M3 on).
#[derive(Debug, Default)]
pub struct AppState {}

/// Builds one frame of the whole window.
pub fn ui(ui: &mut egui::Ui, _state: &mut AppState) {
    let ctx = ui.ctx().clone();
    ctx.layer_painter(egui::LayerId::background())
        .rect_filled(ctx.content_rect(), 0.0, theme::DARKROOM);
    empty_state::no_libraries(ui);
}
