//! Test doubles and a harness for UI tests.

#![allow(dead_code, reason = "each test file uses a different part")]

use app::{AppState, FolderPicker};
use eframe::egui;
use egui_kittest::Harness;
use indexer::{ScanKind, ScanRequests};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

/// wgpu harnesses created on parallel test threads crash the process on Windows (heap corruption in the
/// driver), so every test holding a [`Fixture`] runs alone.
static GPU: Mutex<()> = Mutex::new(());

/// A folder dialog that "picks" a preset folder, or cancels when there is none.
#[derive(Default)]
pub struct FakePicker(pub Mutex<Option<PathBuf>>);

impl FolderPicker for FakePicker {
    fn pick_folder(&self) -> Option<PathBuf> {
        self.0.lock().unwrap().clone()
    }

    fn pick_lut_file(&self) -> Option<PathBuf> {
        self.0.lock().unwrap().clone()
    }
}

#[derive(Default)]
pub struct RecordedScans {
    pub scans: Mutex<Vec<(i64, ScanKind)>>,
    /// Changes scans of named files, which are also recorded in `scans`.
    pub paths: Mutex<Vec<(i64, Vec<String>)>>,
    /// Changes scans that go ahead of the others (REQ-002-18), which are also recorded in `paths`.
    pub first: Mutex<Vec<(i64, Vec<String>)>>,
    pub removed: Mutex<Vec<i64>>,
}

impl ScanRequests for RecordedScans {
    fn request_scan(&self, library: i64, kind: ScanKind) {
        self.scans.lock().unwrap().push((library, kind));
    }

    fn request_paths(&self, library: i64, paths: Vec<String>) {
        self.paths.lock().unwrap().push((library, paths));
        self.request_scan(library, ScanKind::Changes);
    }

    fn request_paths_first(&self, library: i64, paths: Vec<String>) {
        self.first.lock().unwrap().push((library, paths.clone()));
        self.request_paths(library, paths);
    }

    fn remove_library(&self, library: i64) {
        self.removed.lock().unwrap().push(library);
    }
}

pub struct Fixture {
    pub scans: Arc<RecordedScans>,
    pub picker: Arc<FakePicker>,
    /// Folders the tests add as libraries.
    pub dir: tempfile::TempDir,
    _gpu: MutexGuard<'static, ()>,
}

/// Where fixtures put their folders. Snapshots show a media item's location, and a long `TMPDIR` would wrap it to
/// two lines, so on Unix it is always `/tmp`; temp folder names have a fixed length.
fn temp_root() -> PathBuf {
    if cfg!(unix) {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    }
}

impl Fixture {
    pub fn new() -> Self {
        pin_dates();
        Self {
            scans: Arc::default(),
            picker: Arc::default(),
            dir: tempfile::tempdir_in(temp_root()).unwrap(),
            // A failed test poisons the lock; the next one may still run.
            _gpu: GPU.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
        }
    }

    /// A new folder below the fixture's temp folder.
    pub fn folder(&self, name: &str) -> String {
        let path = self.dir.path().join(name);
        std::fs::create_dir_all(&path).unwrap();
        path.to_str().unwrap().to_string()
    }

    pub fn state(&self, conn: db::rusqlite::Connection) -> AppState {
        AppState::new(conn, self.scans.clone(), self.picker.clone())
    }

    pub fn harness(&self, conn: db::rusqlite::Connection) -> Harness<'static, AppState> {
        harness(self.state(conn))
    }
}

/// Dates as in the specs' examples, whatever the machine's locale and time zone.
fn pin_dates() {
    app::format::set_locale("en-GB");
    domain::time_zone::set_fixed(chrono::FixedOffset::east_opt(0));
    // The day the snapshots that show today (the calendar) were taken.
    domain::time_zone::set_today(chrono::NaiveDate::from_ymd_opt(2026, 11, 8));
}

pub fn harness(state: AppState) -> Harness<'static, AppState> {
    harness_with_step(state, 0.25)
}

/// A harness whose steps advance time by `step_dt` seconds (kittest's default is 0.25 s, too slow for double-clicks).
pub fn harness_with_step(state: AppState, step_dt: f32) -> Harness<'static, AppState> {
    pin_dates();
    Harness::builder()
        .with_size([900.0, 600.0])
        .with_step_dt(step_dt)
        .with_context_setup(app::setup)
        .wgpu()
        .build_ui_state(whole_window, state)
}

/// A harness drawing with `render_state`, the one the thumbnails' textures are made on, so they can be drawn.
pub fn harness_on(state: AppState, render_state: eframe::egui_wgpu::RenderState) -> Harness<'static, AppState> {
    pin_dates();
    Harness::builder()
        .with_size([900.0, 600.0])
        .with_step_dt(1.0 / 60.0)
        .with_context_setup(app::setup)
        .renderer(egui_kittest::wgpu::WgpuTestRenderer::from_render_state(render_state))
        .build_ui_state(whole_window, state)
}

/// Draws the app over the whole window, as eframe does. kittest insets its UI by 8 px, but the Linux caption buttons
/// align to the window's edge, so they would sit above the toolbar row (#110).
fn whole_window(ui: &mut egui::Ui, state: &mut AppState) {
    let window = ui.ctx().content_rect();
    ui.scope_builder(egui::UiBuilder::new().max_rect(window), |ui| {
        ui.set_clip_rect(window);
        app::ui(ui, state);
    });
}

/// Runs a few frames, waits for background jobs, and runs a few more, so their results are on screen.
pub fn settle(harness: &mut Harness<'_, AppState>) {
    harness.run_steps(2);
    harness.state_mut().wait_for_jobs();
    // The frame after a new layout asks for the items in view; waiting for them too keeps a busy machine (tests in
    // parallel processes) from showing tiles without their items.
    harness.run_steps(1);
    harness.state_mut().wait_for_jobs();
    harness.run_steps(2);
}

/// A snapshot's name on this platform. Linux draws its own caption buttons and keeps less room for them (REQ-018-04),
/// so its snapshots of the window are files of their own (`<name>_linux.png`).
pub fn snapshot_name(name: &str) -> String {
    if cfg!(target_os = "linux") {
        format!("{name}_linux")
    } else {
        name.to_owned()
    }
}

/// A click at `pos`, half a second after any earlier one, so the two aren't taken as a double-click.
pub fn click_at(harness: &mut Harness<'_, AppState>, pos: egui::Pos2) {
    harness.run_steps(30);
    harness.hover_at(pos);
    harness.run_steps(1);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(1);
    }
}

/// Opens the tile or list row of `name` in the viewer with a click (REQ-025-10). With a selection the click selects
/// it instead.
pub fn open_item(harness: &mut Harness<'_, AppState>, name: &str) {
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable as _;
    let selected = format!("{name}, selected");
    let pos = [Role::Image, Role::Row]
        .into_iter()
        .flat_map(|role| [(role, name), (role, selected.as_str())])
        .find_map(|(role, label)| harness.query_by_role_and_label(role, label))
        .unwrap_or_else(|| panic!("no tile or row {name}"))
        .rect()
        .center();
    click_at(harness, pos);
}
