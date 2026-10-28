//! The *Libraries* section of the Settings page (spec 001).

mod common;

use app::Page;
use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use eframe::egui::accesskit::Role;
use egui_kittest::kittest::Queryable as _;
use indexer::ScanKind;

fn on_settings(fixture: &Fixture) -> egui_kittest::Harness<'static, app::AppState> {
    let mut harness = fixture.harness(db::open_in_memory().unwrap());
    harness.state_mut().page = Page::Settings;
    settle(&mut harness);
    harness
}

fn type_path(harness: &mut egui_kittest::Harness<'static, app::AppState>, path: &str) {
    harness.get_by_role(Role::TextInput).click();
    harness.run_steps(1);
    harness.get_by_role(Role::TextInput).type_text(path);
    harness.run_steps(1);
}

#[test]
fn req_001_01_adds_a_typed_path_and_confirms() {
    let fixture = Fixture::new();
    let path = fixture.folder("Holidays");
    let mut harness = on_settings(&fixture);

    type_path(&mut harness, &path);
    harness.get_by_label("Add").click();
    settle(&mut harness);

    harness.get_by_label("Added Holidays, indexing…");
    harness.get_by_role_and_label(Role::Label, "Holidays");
    harness.get_by_label("online");
    let library = harness.state().libraries[0].clone();
    assert_eq!(
        (library.path.as_str(), library.status),
        (path.as_str(), LibraryStatus::Online)
    );
    assert_eq!(*fixture.scans.scans.lock().unwrap(), [(library.id, ScanKind::Full)]);
    assert_eq!(harness.state().settings_page.path_input, "");
}

#[test]
fn req_001_02_adds_the_picked_folder_and_nothing_when_cancelled() {
    let fixture = Fixture::new();
    let mut harness = on_settings(&fixture);

    harness.get_by_label("Browse…").click();
    settle(&mut harness);
    assert!(harness.state().libraries.is_empty());

    *fixture.picker.0.lock().unwrap() = Some(fixture.folder("Picked").into());
    harness.get_by_label("Browse…").click();
    settle(&mut harness);

    harness.get_by_label("Added Picked, indexing…");
    assert_eq!(harness.state().libraries.len(), 1);
}

#[test]
fn req_001_03_explains_invalid_paths() {
    let fixture = Fixture::new();
    let missing = format!("{}\\missing", fixture.dir.path().display());
    let mut harness = on_settings(&fixture);

    type_path(&mut harness, "photos");
    harness.get_by_label("Add").click();
    settle(&mut harness);
    harness.get_by_label("Path must be absolute");

    harness.state_mut().settings_page.path_input = missing;
    harness.run_steps(1);
    harness.get_by_label("Add").click();
    settle(&mut harness);
    harness.get_by_label("Folder not found or not reachable");
    assert!(harness.state().libraries.is_empty());
}

#[test]
fn req_001_04_rejects_a_duplicate_in_another_letter_case() {
    let fixture = Fixture::new();
    let path = fixture.folder("Photos");
    let mut harness = on_settings(&fixture);
    harness.state_mut().add_library(path.clone());
    settle(&mut harness);

    harness.state_mut().settings_page.path_input = path.to_lowercase();
    harness.run_steps(1);
    harness.get_by_label("Add").click();
    settle(&mut harness);

    harness.get_by_label("Library already added");
    assert_eq!(harness.state().libraries.len(), 1);
}

#[test]
fn req_001_05_removes_a_library_after_confirmation_without_touching_files() {
    let fixture = Fixture::new();
    let path = fixture.folder("Photos");
    std::fs::write(format!("{path}\\a.jpg"), "jpeg").unwrap();
    let mut harness = on_settings(&fixture);
    harness.state_mut().add_library(path.clone());
    settle(&mut harness);
    let id = harness.state().libraries[0].id;

    harness.get_by_label("Remove Photos").click();
    settle(&mut harness);
    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    assert_eq!(harness.state().libraries.len(), 1);

    harness.get_by_label("Remove Photos").click();
    settle(&mut harness);
    harness.get_by_label("Remove \"Photos\" from the gallery? Files on disk are not touched.");
    harness.get_by_label("Remove").click();
    settle(&mut harness);

    assert!(harness.state().libraries.is_empty());
    harness.get_by_label("No libraries yet.");
    assert_eq!(*fixture.scans.removed.lock().unwrap(), [id]);
    assert_eq!(std::fs::read_to_string(format!("{path}\\a.jpg")).unwrap(), "jpeg");
}

#[test]
fn req_001_06_rescan_queues_a_full_scan_and_confirms() {
    let fixture = Fixture::new();
    let mut harness = on_settings(&fixture);
    harness.state_mut().add_library(fixture.folder("Photos"));
    settle(&mut harness);
    let id = harness.state().libraries[0].id;
    fixture.scans.scans.lock().unwrap().clear();

    harness.get_by_label("Rescan Photos").click();
    settle(&mut harness);

    harness.get_by_label("Rescan queued");
    assert_eq!(*fixture.scans.scans.lock().unwrap(), [(id, ScanKind::Full)]);
}

#[test]
fn req_001_07_lists_libraries_with_status_path_and_last_indexed_time() {
    let fixture = Fixture::new();
    let conn = db::open_in_memory().unwrap();
    assert!(conn.is_autocommit());
    db::libraries::insert(&conn, "D:\\Zoo", "Zoo", LibraryStatus::Offline).unwrap();
    db::libraries::insert(&conn, "\\\\nas\\archive", "archive", LibraryStatus::Unknown).unwrap();
    conn.execute(
        "UPDATE library_roots SET last_scan_at = 1751433120000 WHERE label = 'Zoo'",
        [],
    )
    .unwrap();
    let mut harness = fixture.harness(conn);
    harness.state_mut().page = Page::Settings;
    settle(&mut harness);

    let labels: Vec<_> = harness.state().libraries.iter().map(|l| l.label.clone()).collect();
    assert_eq!(labels, ["Zoo", "archive"]);
    harness.get_by_label("offline");
    // The library's status, and the thumbnail cache's usage further down (no cache in this test).
    assert_eq!(harness.query_all_by_label("unknown").count(), 2);
    harness.get_by_label(&format!(
        "D:\\Zoo · last indexed {}",
        app::format::medium_date_time(1751433120000)
    ));
    harness.get_by_label("\\\\nas\\archive");
    harness.snapshot("settings_libraries");
}

#[test]
fn req_001_07_says_when_there_are_no_libraries() {
    let fixture = Fixture::new();
    let harness = on_settings(&fixture);

    harness.get_by_label("No libraries yet.");
}
