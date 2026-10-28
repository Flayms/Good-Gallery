//! The _Indexing_ and _Thumbnail cache_ sections of the Settings page (spec 010).

mod common;

use app::{AppState, Page, SettingsTargets};
use common::{Fixture, settle};
use db::settings::Settings;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
enum Call {
    Interval(u32),
    Concurrency(usize),
    CacheLimit(u64),
    Clear,
}

/// Records what the page hands to the running parts; the cache reports `usage` (`None` while measuring).
#[derive(Default)]
struct FakeTargets {
    calls: Mutex<Vec<Call>>,
    usage: Mutex<Option<u64>>,
}

impl FakeTargets {
    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
}

impl SettingsTargets for FakeTargets {
    fn set_quick_check_interval(&self, minutes: u32) {
        self.calls.lock().unwrap().push(Call::Interval(minutes));
    }

    fn set_io_concurrency(&self, reads: usize) {
        self.calls.lock().unwrap().push(Call::Concurrency(reads));
    }

    fn set_cache_limit(&self, bytes: u64) {
        self.calls.lock().unwrap().push(Call::CacheLimit(bytes));
    }

    fn cache_usage(&self) -> Option<u64> {
        *self.usage.lock().unwrap()
    }

    fn clear_cache(&self) -> u64 {
        self.calls.lock().unwrap().push(Call::Clear);
        *self.usage.lock().unwrap() = Some(0);
        0
    }
}

const GIB: u64 = 1 << 30;

/// The Settings page on the index at `db` (a file, so a restart can open it again), tall enough for every section.
fn on_settings(fixture: &Fixture, db: &std::path::Path, targets: Arc<FakeTargets>) -> Harness<'static, AppState> {
    let state = fixture.state(db::open(db).unwrap()).with_settings_targets(targets);
    let mut harness = common::harness(state);
    harness.set_size(eframe::egui::vec2(900.0, 1100.0));
    harness.state_mut().page = Page::Settings;
    settle(&mut harness);
    harness
}

/// The _Parallel file reads_ slider's value.
fn reads(harness: &Harness<'static, AppState>) -> Option<f64> {
    harness
        .get_by_role_and_label(Role::Slider, "Parallel file reads")
        .accesskit_node()
        .numeric_value()
}

const INTERVAL: &str = "Check interval";
const CACHE_SIZE: &str = "Maximum cache size";

/// The value a combo box shows.
fn combo(harness: &Harness<'static, AppState>, name: &str) -> String {
    let node = harness.get_by_role_and_label(Role::ComboBox, name);
    node.accesskit_node().value().unwrap_or_default().to_string()
}

/// Opens the combo box `name` and picks `option`.
fn pick(harness: &mut Harness<'static, AppState>, name: &str, option: &str) {
    harness.get_by_role_and_label(Role::ComboBox, name).click();
    harness.run_steps(2);
    harness.get_by_label(option).click();
    settle(harness);
}

#[test]
fn req_010_01_shows_the_defaults_and_keeps_changes_across_restarts() {
    let fixture = Fixture::new();
    let db = fixture.dir.path().join("gallery.db");
    let targets = Arc::new(FakeTargets::default());
    let mut harness = on_settings(&fixture, &db, targets.clone());

    assert_eq!(combo(&harness, INTERVAL), "Every hour");
    assert_eq!(combo(&harness, CACHE_SIZE), "5 GB");
    assert_eq!(reads(&harness), Some(4.0));

    pick(&mut harness, INTERVAL, "Every 3 hours");
    pick(&mut harness, CACHE_SIZE, "20 GB");
    drop(harness);

    let restarted = on_settings(&fixture, &db, Arc::new(FakeTargets::default()));
    assert_eq!(combo(&restarted, INTERVAL), "Every 3 hours");
    assert_eq!(combo(&restarted, CACHE_SIZE), "20 GB");
    assert_eq!(
        restarted.state().settings,
        Settings {
            quick_check_interval_minutes: 180,
            thumb_cache_gib: 20.0,
            io_concurrency: 4,
        }
    );
}

#[test]
fn req_010_02_rejects_invalid_values_with_an_error_and_changes_nothing() {
    let fixture = Fixture::new();
    let db = fixture.dir.path().join("gallery.db");
    let targets = Arc::new(FakeTargets::default());
    let mut harness = on_settings(&fixture, &db, targets.clone());

    let invalid = Settings {
        io_concurrency: 17,
        ..Settings::default()
    };
    harness.state_mut().update_settings(invalid);
    settle(&mut harness);

    harness.get_by_label("Parallel file reads is out of range");
    assert_eq!(harness.state().settings, Settings::default());
    assert_eq!(
        db::settings::load(&db::open(&db).unwrap()).unwrap(),
        Settings::default()
    );
    assert_eq!(targets.calls(), []);
}

#[test]
fn req_010_03_follows_the_new_interval_and_offers_a_stored_odd_value() {
    let fixture = Fixture::new();
    let db = fixture.dir.path().join("gallery.db");
    let odd = Settings {
        quick_check_interval_minutes: 45,
        ..Settings::default()
    };
    db::settings::save(&db::open(&db).unwrap(), &odd).unwrap();
    let targets = Arc::new(FakeTargets::default());
    let mut harness = on_settings(&fixture, &db, targets.clone());

    assert_eq!(combo(&harness, INTERVAL), "Every 45 minutes");
    // The stored value is among the options, between 30 minutes and an hour.
    harness.get_by_role_and_label(Role::ComboBox, INTERVAL).click();
    harness.run_steps(2);
    let options: Vec<String> = harness
        .query_all_by_role(Role::Button)
        .filter_map(|node| node.accesskit_node().label())
        .filter(|label| label.starts_with("Every") || label == "Daily" || label == "Only at startup")
        .collect();
    assert_eq!(
        options,
        [
            "Only at startup",
            "Every 15 minutes",
            "Every 30 minutes",
            "Every 45 minutes",
            "Every hour",
            "Every 3 hours",
            "Every 6 hours",
            "Daily"
        ]
    );
    harness.get_by_label("Only at startup").click();
    settle(&mut harness);
    assert_eq!(targets.calls(), [Call::Interval(0)]);
    assert_eq!(combo(&harness, INTERVAL), "Only at startup");
}

#[test]
fn req_010_04_saves_parallel_reads_when_the_slider_is_released() {
    let fixture = Fixture::new();
    let db = fixture.dir.path().join("gallery.db");
    let targets = Arc::new(FakeTargets::default());
    let mut harness = on_settings(&fixture, &db, targets.clone());

    let slider = harness.get_by_role_and_label(Role::Slider, "Parallel file reads");
    let rect = slider.rect();
    let center = rect.center();
    let right = eframe::egui::pos2(rect.max.x - 2.0, center.y);
    // Pressed and dragged to the far right: shown, but not saved yet.
    harness.hover_at(center);
    harness.run_steps(1);
    harness.event(eframe::egui::Event::PointerButton {
        pos: center,
        button: eframe::egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    harness.run_steps(1);
    harness.hover_at(right);
    harness.run_steps(2);
    assert_eq!(reads(&harness), Some(16.0));
    assert_eq!(targets.calls(), []);
    assert_eq!(harness.state().settings.io_concurrency, 4);

    harness.event(eframe::egui::Event::PointerButton {
        pos: right,
        button: eframe::egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    settle(&mut harness);
    assert_eq!(targets.calls(), [Call::Concurrency(16)]);
    assert_eq!(db::settings::load(&db::open(&db).unwrap()).unwrap().io_concurrency, 16);
}

#[test]
fn req_010_05_applies_a_new_cache_limit_and_shows_the_new_usage() {
    let fixture = Fixture::new();
    let db = fixture.dir.path().join("gallery.db");
    let targets = Arc::new(FakeTargets::default());
    *targets.usage.lock().unwrap() = Some(3 * GIB);
    let mut harness = on_settings(&fixture, &db, targets.clone());
    harness.get_by_label("3.0 GB");

    pick(&mut harness, CACHE_SIZE, "2 GB");
    assert_eq!(targets.calls(), [Call::CacheLimit(2 * GIB)]);
    // The eviction runs in the background; the page reads the usage again soon after.
    *targets.usage.lock().unwrap() = Some(GIB + GIB / 2);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    harness.run_steps(2);
    harness.get_by_label("1.5 GB");
}

#[test]
fn req_010_06_shows_usage_and_clears_after_confirmation() {
    let fixture = Fixture::new();
    let db = fixture.dir.path().join("gallery.db");
    let targets = Arc::new(FakeTargets::default());
    let mut harness = on_settings(&fixture, &db, targets.clone());
    // Still being measured.
    harness.get_by_label("…");

    *targets.usage.lock().unwrap() = Some(512 << 20);
    std::thread::sleep(app::state::USAGE_REFRESH);
    harness.run_steps(2);
    harness.get_by_label("512.0 MB");

    harness.get_by_label("Clear").click();
    harness.run_steps(2);
    harness.get_by_label("Delete all cached thumbnails?");
    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    assert_eq!(targets.calls(), []);

    harness.get_by_label("Clear").click();
    harness.run_steps(2);
    harness
        .query_all_by_label("Clear")
        .last()
        .expect("the dialog's Clear button")
        .click();
    settle(&mut harness);
    assert_eq!(targets.calls(), [Call::Clear]);
    harness.get_by_label("Thumbnail cache cleared");
    harness.get_by_label("0 B");
}

#[test]
fn req_010_06_says_unknown_without_a_thumbnail_cache() {
    let fixture = Fixture::new();
    let mut harness = fixture.harness(db::open_in_memory().unwrap());
    harness.set_size(eframe::egui::vec2(900.0, 1100.0));
    harness.state_mut().page = Page::Settings;
    settle(&mut harness);
    harness.get_by_label("unknown");
}

#[test]
fn req_013_05_rings_the_focused_control_but_not_a_focused_text_input() {
    let fixture = Fixture::new();
    let db = fixture.dir.path().join("gallery.db");
    let mut harness = on_settings(&fixture, &db, Arc::new(FakeTargets::default()));
    assert_eq!(app::theme::focus_ring_rect(&harness.ctx), None);

    harness.key_press(eframe::egui::Key::Tab);
    harness.run_steps(2);
    let focused = harness.ctx.memory(|memory| memory.focused());
    let ring = app::theme::focus_ring_rect(&harness.ctx).expect("a ring around the focused control");
    assert_eq!(
        Some(ring),
        focused.and_then(|id| harness.ctx.read_response(id)).map(|r| r.rect)
    );

    harness.get_by_role(Role::TextInput).click();
    harness.run_steps(2);
    assert_eq!(app::theme::focus_ring_rect(&harness.ctx), None);
}
