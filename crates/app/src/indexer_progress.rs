//! What the indexer is doing: "Indexing Photos · 1,204 files · 310 updated · 1 queued" (REQ-002-08). Drawn over
//! the bottom-left corner of the window, where the sidebar's foot is.

use crate::icons::Icon;
use crate::state::AppState;
use crate::{format, theme};
use eframe::egui::{self, Align2, CornerRadius, FontId, Frame, Margin, RichText, Stroke};
use indexer::ScanKind;

pub fn show(ctx: &egui::Context, state: &AppState) {
    let Some(current) = state.indexer_status.current else {
        return;
    };
    let label = state
        .libraries
        .iter()
        .find(|library| library.id == current.library)
        .map_or("library", |library| library.label.as_str());
    let verb = if current.kind == ScanKind::Full {
        "Indexing"
    } else {
        "Checking"
    };
    let mut details = format!(
        "{} files · {} updated",
        format::count(current.scanned),
        format::count(current.indexed)
    );
    let queued = state.indexer_status.queue.len();
    if queued > 0 {
        details.push_str(&format!(" · {} queued", format::count(queued as u64)));
    }
    egui::Area::new(egui::Id::unique("indexer-progress"))
        .anchor(Align2::LEFT_BOTTOM, [16.0, -16.0])
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            Frame::new()
                .fill(theme::raised())
                .stroke(Stroke::new(1.0, theme::rim()))
                .corner_radius(CornerRadius::same(theme::RADIUS_CONTROL))
                .inner_margin(Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.horizontal(|ui| {
                        spinner(ui);
                        ui.label(RichText::new(format!("{verb} {label}")).color(theme::text()));
                    });
                    ui.label(
                        RichText::new(details)
                            .font(FontId::new(theme::SIZE_CAPTION, theme::numbers()))
                            .color(theme::muted()),
                    );
                });
        });
}

/// lucide `loader-circle`, turning.
fn spinner(ui: &mut egui::Ui) {
    let angle = (ui.input(|i| i.time) * std::f64::consts::TAU) as f32;
    ui.add(
        Icon::LoaderCircle
            .image(14.0, theme::text())
            .rotate(angle, egui::vec2(0.5, 0.5)),
    );
    // 20 fps is smooth enough for a 14 px spinner and keeps an hours-long scan cheap.
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
}
