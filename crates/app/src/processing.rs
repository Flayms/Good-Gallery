//! What the app is working on (REQ-002-08, REQ-032-20): a spinning icon beside the toolbar's other notices, on every
//! page, that says more when clicked.

use crate::icons::Icon;
use crate::state::AppState;
use crate::{format, theme};
use eframe::egui::{self, RichText, Stroke, vec2};
use indexer::ScanKind;

/// One thing the app is working on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    pub title: String,
    pub details: String,
    /// The search for faces, whose settings are one click away.
    pub faces: bool,
}

/// What is running right now.
pub fn running(state: &AppState) -> Vec<Work> {
    let mut work = Vec::new();
    if let Some(current) = state.indexer_status.current {
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
        work.push(Work {
            title: format!("{verb} {label}"),
            details,
            faces: false,
        });
    }
    // The search for faces runs for hours.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let summary = state.faces.summary();
        if summary.working {
            work.push(Work {
                title: summary.title.to_owned(),
                details: summary.detail,
                faces: true,
            });
        }
        // So does content search's pass (REQ-036-04).
        let summary = state.content.summary();
        if summary.working {
            work.push(Work {
                title: format!("Content search: {}", summary.title.to_lowercase()),
                details: summary.detail,
                faces: false,
            });
        }
    }
    work
}

/// The icon and its menu, if anything is running; whether the user asked for the settings of the search for faces.
pub fn button(ui: &mut egui::Ui, work: &[Work]) -> bool {
    if work.is_empty() {
        return false;
    }
    let mut icon = Icon::LoaderCircle.image(theme::ICON_SIZE, theme::text());
    if theme::motion(ui.ctx()) {
        let angle = (ui.input(|input| input.time) * std::f64::consts::TAU) as f32;
        icon = icon.rotate(angle, vec2(0.5, 0.5));
        // 20 fps is smooth enough for a small spinner and keeps an hours-long scan cheap.
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    }
    let response = ui
        .add(
            egui::Button::image(icon)
                .frame(false)
                .min_size(egui::Vec2::splat(theme::CONTROL_HEIGHT - 4.0)),
        )
        .on_hover_text(LABEL);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, LABEL));
    let mut open_faces = false;
    crate::toolbar::menu(&response, "processing", 300.0, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for (i, item) in work.iter().enumerate() {
            if i > 0 {
                ui.add_space(theme::SPACE * 2.0);
            }
            ui.label(RichText::new(&item.title).family(theme::semibold()));
            ui.add(egui::Label::new(RichText::new(&item.details).color(theme::muted())).wrap());
            if item.faces {
                ui.add_space(theme::SPACE);
                open_faces |= ui
                    .add(egui::Button::new("Faces settings").stroke(Stroke::new(1.0, theme::rim())))
                    .clicked();
            }
        }
    });
    open_faces
}

/// What the icon is called.
const LABEL: &str = "What's running";
