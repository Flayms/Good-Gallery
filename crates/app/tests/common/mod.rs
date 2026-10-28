//! Test doubles and a harness for UI tests.

#![allow(dead_code, reason = "each test file uses a different part")]

use app::{AppState, FolderPicker};
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
}

#[derive(Default)]
pub struct RecordedScans {
    pub scans: Mutex<Vec<(i64, ScanKind)>>,
    pub removed: Mutex<Vec<i64>>,
}

impl ScanRequests for RecordedScans {
    fn request_scan(&self, library: i64, kind: ScanKind) {
        self.scans.lock().unwrap().push((library, kind));
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

impl Fixture {
    pub fn new() -> Self {
        pin_dates();
        Self {
            scans: Arc::default(),
            picker: Arc::default(),
            dir: tempfile::tempdir().unwrap(),
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
        .build_ui_state(app::ui, state)
}

/// Runs a few frames, waits for background jobs, and runs a few more, so their results are on screen.
pub fn settle(harness: &mut Harness<'_, AppState>) {
    harness.run_steps(2);
    harness.state_mut().wait_for_jobs();
    harness.run_steps(3);
}
