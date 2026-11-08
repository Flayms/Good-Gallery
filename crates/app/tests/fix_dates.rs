//! Fixing dates taken of a selection (spec 038).

use crate::common;

use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::writer::{IndexWriter, IndexedFile};
use domain::date_fix::{Outcome, Problem};
use domain::media::{MediaKind, MediaMetadata};
use eframe::egui::{self, Modifiers, PointerButton, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

type App = Harness<'static, app::AppState>;

/// A photo with a date in its name and another date taken, one without either, a video, and one named in the future.
const NAMES: [&str; 4] = ["IMG_20230512_101500.jpg", "b.jpg", "c.mp4", "IMG_20991231_101500.jpg"];

fn exiftool() -> Option<PathBuf> {
    match media::exiftool::find_exiftool() {
        Some(exiftool) => Some(exiftool),
        None if std::env::var_os("GG_REQUIRE_HELPERS").is_some() => panic!("exiftool not found"),
        None => {
            eprintln!("skipped: exiftool not found");
            None
        }
    }
}

fn exif(exiftool: &Path, args: &[&str], file: &Path) {
    let status = Command::new(exiftool)
        .args(args)
        .args(["-overwrite_original"])
        .arg(file)
        .status()
        .unwrap();
    assert!(status.success());
}

fn taken(exiftool: &Path, file: &Path) -> Option<String> {
    let output = Command::new(exiftool)
        .args(["-j", "-EXIF:DateTimeOriginal"])
        .arg(file)
        .output()
        .unwrap();
    let read: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    read[0]["DateTimeOriginal"].as_str().map(str::to_owned)
}

/// The library with [`NAMES`], indexed; the first photo was taken on 2020-01-01 08:00 (UTC, as tests show dates).
fn app(fixture: &Fixture, exiftool: &Path) -> (App, PathBuf) {
    let dir = PathBuf::from(fixture.folder("photos"));
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, dir.to_str().unwrap(), "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let files: Vec<IndexedFile> = NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let path = dir.join(name);
            let kind = if name.ends_with(".mp4") {
                std::fs::write(&path, b"not really a video").unwrap();
                MediaKind::Video
            } else {
                image::RgbImage::from_pixel(8, 4, image::Rgb([200, 30, 30]))
                    .save(&path)
                    .unwrap();
                MediaKind::Image
            };
            let taken_at = (i == 0).then(|| {
                exif(exiftool, &["-DateTimeOriginal=2020:01:01 08:00:00"], &path);
                1_577_865_600_000
            });
            IndexedFile {
                rel_path: name.to_string(),
                file_name: name.to_string(),
                kind,
                size: 1,
                mtime: 1_000 + i as i64,
                sidecar_mtime: None,
                metadata: MediaMetadata {
                    width: Some(8),
                    height: Some(4),
                    taken_at,
                    ..Default::default()
                },
                read_error: None,
            }
        })
        .collect();
    let mut writer = IndexWriter::new(conn);
    writer.write(root, &files).unwrap();
    let data = fixture.dir.path().join("data");
    let config = media::edit::exiftool_config(&data).unwrap();
    let state = fixture
        .state(writer.into())
        .with_edit_tools(media::edit::Tools {
            exiftool: exiftool.to_owned(),
            config,
        })
        .with_date_journal(data.join("date-journal.json"));
    let mut harness = common::harness_with_step(state, 1.0 / 60.0);
    harness.set_size(egui::vec2(1000.0, 700.0));
    settle(&mut harness);
    (harness, dir)
}

/// Selects every item and opens the dialog from the selection bar, then waits until every photo was read.
fn open_for_all(harness: &mut App) {
    harness.state_mut().gallery.as_mut().unwrap().select_all();
    harness.run_steps(2);
    harness.get_by_label("Fix dates…").click();
    wait_until_read(harness);
}

fn wait_until_read(harness: &mut App) {
    for _ in 0..500 {
        harness.run_steps(1);
        if harness
            .state()
            .date_dialog
            .as_ref()
            .is_some_and(app::fix_dates::FixDatesDialog::is_read)
        {
            harness.run_steps(2);
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the dates weren't read");
}

/// The outcome of the dialog's row for `name`.
fn outcome(harness: &App, name: &str) -> Outcome {
    let dialog = harness.state().date_dialog.as_ref().expect("the dialog is open");
    let id: i64 = {
        let db = harness.state().db().unwrap();
        let conn = db.lock().unwrap();
        conn.query_row("SELECT id FROM media WHERE rel_path = ?1", [name], |row| row.get(0))
            .unwrap()
    };
    dialog.rows().iter().find(|row| row.id == id).unwrap().outcome.clone()
}

fn changed_to(outcome: &Outcome) -> Option<String> {
    match outcome {
        Outcome::Changed(stamp) => Some(stamp.exif_text()),
        _ => None,
    }
}

/// REQ-038-01, REQ-038-03, REQ-038-05, REQ-038-06: _Fix dates…_ in the selection bar opens the dialog; the preview
/// takes dates from the names and says why the others can't be fixed.
#[test]
fn req_038_01_req_038_05_the_preview_shows_old_and_new_dates() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, &exiftool);
    open_for_all(&mut harness);
    assert_eq!(harness.state().date_dialog.as_ref().unwrap().count(), 4);
    assert_eq!(
        changed_to(&outcome(&harness, "IMG_20230512_101500.jpg")).as_deref(),
        Some("2023:05:12 10:15:00")
    );
    assert_eq!(outcome(&harness, "b.jpg"), Outcome::Failed(Problem::NoDateInName));
    assert_eq!(outcome(&harness, "c.mp4"), Outcome::Failed(Problem::Video));
    assert_eq!(
        outcome(&harness, "IMG_20991231_101500.jpg"),
        Outcome::Failed(Problem::InFuture)
    );
    assert!(harness.query_by_label_contains("1 changed").is_some());
    harness.snapshot(common::snapshot_name("fix_dates_dialog"));

    // REQ-038-04: a shift moves the date taken the file has; one without can't be shifted.
    let dialog = harness.state_mut().date_dialog.as_mut().unwrap();
    dialog.kind = app::fix_dates::Kind::Shift;
    dialog.shift = [0, 1, 30];
    harness.run_steps(2);
    assert_eq!(
        changed_to(&outcome(&harness, "IMG_20230512_101500.jpg")).as_deref(),
        Some("2020:01:01 09:30:00")
    );
    assert_eq!(outcome(&harness, "b.jpg"), Outcome::Failed(Problem::NoDateTaken));
}

/// REQ-038-08, REQ-038-09: _Fix_ writes the new date into the file and has it read again; _Undo_ in the toast puts the
/// old one back.
#[test]
fn req_038_08_req_038_09_fixes_and_undoes() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, &exiftool);
    let photo = dir.join("IMG_20230512_101500.jpg");
    open_for_all(&mut harness);
    harness.get_by_label("Fix 1").click();
    settle(&mut harness);
    assert!(harness.state().date_dialog.is_none(), "closed");
    assert_eq!(taken(&exiftool, &photo).as_deref(), Some("2023:05:12 10:15:00"));
    assert_eq!(taken(&exiftool, &dir.join("b.jpg")), None, "left alone");
    assert!(
        fixture
            .scans
            .paths
            .lock()
            .unwrap()
            .iter()
            .any(|(_, paths)| paths.contains(&"IMG_20230512_101500.jpg".to_string())),
        "read again"
    );
    assert!(harness.query_by_label_contains("Fixed the dates of 1 photo").is_some());

    harness.get_by_label("Undo").click();
    settle(&mut harness);
    assert_eq!(taken(&exiftool, &photo).as_deref(), Some("2020:01:01 08:00:00"));
    assert!(
        harness
            .query_by_label_contains("Put back the dates of 1 photo")
            .is_some()
    );
}

/// REQ-038-09: a photo changed since the fix keeps its changes; _Undo last fix_ is in the dialog until then.
#[test]
fn req_038_09_undo_leaves_files_changed_since() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, &exiftool);
    let photo = dir.join("IMG_20230512_101500.jpg");
    open_for_all(&mut harness);
    assert!(harness.query_by_label("Undo last fix").is_none(), "nothing to undo yet");
    harness.get_by_label("Fix 1").click();
    settle(&mut harness);
    exif(&exiftool, &["-Rating=3"], &photo);

    open_for_all(&mut harness);
    harness.get_by_label("Undo last fix").click();
    settle(&mut harness);
    assert_eq!(taken(&exiftool, &photo).as_deref(), Some("2023:05:12 10:15:00"), "kept");
    assert!(
        harness
            .query_by_label_contains("1 photo changed since were left alone")
            .is_some()
    );
}

/// REQ-038-01: _Fix dates…_ in a tile's context menu opens the dialog for that item.
#[test]
fn req_038_01_the_context_menu_opens_the_dialog() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, &exiftool);
    let at = harness
        .query_by_role_and_label(Role::Image, "b.jpg")
        .unwrap()
        .rect()
        .center();
    harness.hover_at(at);
    harness.run_steps(1);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        harness.run_steps(1);
    }
    harness.run_steps(2);
    harness.get_by_label("Fix dates…").click();
    wait_until_read(&mut harness);
    assert_eq!(harness.state().date_dialog.as_ref().unwrap().count(), 1);
    assert!(harness.query_by_label("Fix the date of 1 item").is_some());
}
