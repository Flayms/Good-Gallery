//! Snapshot tests of whole screens. `UPDATE_SNAPSHOTS=1 cargo test -p app` rewrites `tests/snapshots/*.png`.

use app::{AppState, theme};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;

fn harness(state: AppState) -> Harness<'static, AppState> {
    Harness::builder()
        .with_size([800.0, 500.0])
        .with_context_setup(theme::install)
        .wgpu()
        .build_ui_state(app::ui, state)
}

#[test]
fn first_run_shows_the_no_libraries_empty_state() {
    let mut harness = harness(AppState::default());
    harness.run();

    harness.get_by_label("No libraries yet");
    harness.get_by_label("Add a library");
    harness.snapshot("first_run");
}
