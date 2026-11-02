//! Rating by hand (spec 024) on a library of real photos, written with the real exiftool.

mod common;

use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::writer::{IndexWriter, IndexedFile};
use domain::media::{MediaKind, MediaMetadata};
use eframe::egui::accesskit::Role;
use eframe::egui::{self, Key};
use egui_kittest::kittest::Queryable as _;
use std::path::{Path, PathBuf};
use std::process::Command;

type App = egui_kittest::Harness<'static, app::AppState>;

/// exiftool, or `None` (the test is skipped) when it isn't installed; `GG_REQUIRE_HELPERS=1` fails then.
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

/// `a.png`, rated 2 in the index and the file, and `b.png`, unrated; with exiftool for writing if given.
fn app(fixture: &Fixture, exiftool: Option<&Path>) -> (App, PathBuf) {
    let dir = PathBuf::from(fixture.folder("photos"));
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, dir.to_str().unwrap(), "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let files: Vec<IndexedFile> = [("a.png", Some(2)), ("b.png", None)]
        .into_iter()
        .enumerate()
        .map(|(i, (name, rating))| {
            image::RgbImage::from_pixel(400, 300, image::Rgb([200, 120, 40]))
                .save(dir.join(name))
                .unwrap();
            IndexedFile {
                rel_path: name.into(),
                file_name: name.into(),
                kind: MediaKind::Image,
                size: 1,
                mtime: 1_000,
                sidecar_mtime: None,
                metadata: MediaMetadata {
                    width: Some(400),
                    height: Some(300),
                    taken_at: Some(2_000_000 - i as i64),
                    rating,
                    ..Default::default()
                },
                read_error: None,
            }
        })
        .collect();
    let mut writer = IndexWriter::new(conn);
    writer.write(root, &files).unwrap();
    let mut state = fixture.state(writer.into());
    if let Some(exiftool) = exiftool {
        let status = Command::new(exiftool)
            .args(["-q", "-overwrite_original", "-XMP-xmp:Rating=2"])
            .arg(dir.join("a.png"))
            .status()
            .unwrap();
        assert!(status.success());
        let config = media::edit::exiftool_config(&fixture.dir.path().join("data")).unwrap();
        state = state.with_edit_tools(media::edit::Tools {
            exiftool: exiftool.to_owned(),
            config,
        });
    }
    let mut harness = common::harness_with_step(state, 1.0 / 60.0);
    harness.set_size(egui::vec2(900.0, 760.0));
    settle(&mut harness);
    (harness, dir)
}

fn open(harness: &mut App, name: &str) {
    harness.get_by_role_and_label(Role::Image, name).click();
    settle(harness);
}

/// The index's rating of `name`.
fn indexed(harness: &App, name: &str) -> Option<i64> {
    let db = harness.state().db().unwrap();
    let conn = db.lock().unwrap();
    conn.query_row("SELECT rating FROM media WHERE file_name = ?1", [name], |row| {
        row.get(0)
    })
    .unwrap()
}

/// The XMP rating in the file, if it has one.
fn in_file(exiftool: &Path, file: &Path) -> Option<i64> {
    let output = Command::new(exiftool)
        .args(["-j", "-n", "-XMP-xmp:Rating"])
        .arg(file)
        .output()
        .unwrap();
    let read: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    read[0]["Rating"].as_i64()
}

fn mtime(file: &Path) -> std::time::SystemTime {
    std::fs::metadata(file).unwrap().modified().unwrap()
}

/// REQ-024-01, REQ-024-03: `1`–`5` in the viewer set the rating in the file and the index, the gallery has it, and
/// the same key again removes it; the date modified stays.
#[test]
fn req_024_01_req_024_03_rates_with_the_keys() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, Some(&exiftool));
    let file = dir.join("b.png");
    let before = mtime(&file);
    open(&mut harness, "b.png");

    harness.key_press(Key::Num3);
    settle(&mut harness);
    assert_eq!(indexed(&harness, "b.png"), Some(3));
    assert_eq!(in_file(&exiftool, &file), Some(3));
    assert_eq!(mtime(&file), before);
    assert!(harness.state().viewer.is_some(), "the viewer stays");
    assert!(
        fixture
            .scans
            .paths
            .lock()
            .unwrap()
            .iter()
            .any(|(_, paths)| paths == &["b.png"]),
        "the file is read again"
    );

    harness.key_press(Key::Num3);
    settle(&mut harness);
    assert_eq!(indexed(&harness, "b.png"), None);
    assert_eq!(in_file(&exiftool, &file), Some(0));

    harness.key_press(Key::Num5);
    settle(&mut harness);
    harness.key_press(Key::Escape);
    settle(&mut harness);
    let gallery = harness.state_mut().gallery.as_mut().unwrap();
    let item = gallery.item(1).expect("b.png is the second item");
    assert_eq!((item.file_name.as_str(), item.rating), ("b.png", Some(5)));
}

/// REQ-024-02: the stars in the info panel set the rating; the current one's star removes it.
#[test]
fn req_024_02_rates_with_the_stars() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, Some(&exiftool));
    open(&mut harness, "a.png");
    harness.key_press(Key::I);
    settle(&mut harness);
    assert!(harness.query_by_label("Rated 2 out of 5").is_some());

    harness.get_by_role_and_label(Role::Button, "Rate 4 stars").click();
    settle(&mut harness);
    assert_eq!(indexed(&harness, "a.png"), Some(4));
    assert_eq!(in_file(&exiftool, &dir.join("a.png")), Some(4));
    assert!(harness.query_by_label("Rated 4 out of 5").is_some());

    harness.get_by_role_and_label(Role::Button, "Rate 4 stars").click();
    settle(&mut harness);
    assert_eq!(indexed(&harness, "a.png"), None);
    assert!(harness.query_by_label("Not rated").is_some());
    harness.get_by_role_and_label(Role::Button, "Rate 1 star");
}

/// REQ-024-04: a file that can't be written keeps its rating, in the file and the index, and a toast says why.
#[test]
fn req_024_04_keeps_the_rating_of_a_file_it_cant_write() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, Some(&exiftool));
    let file = dir.join("a.png");
    let mut permissions = std::fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&file, permissions).unwrap();
    open(&mut harness, "a.png");

    harness.key_press(Key::Num5);
    settle(&mut harness);
    assert_eq!(indexed(&harness, "a.png"), Some(2));
    assert_eq!(in_file(&exiftool, &file), Some(2));
    let toasts = harness.state().toasts.current();
    assert!(
        toasts
            .last()
            .is_some_and(|toast| toast.text == "Couldn't save the rating: the file is read-only"),
        "{toasts:?}"
    );
}

/// REQ-024-04: without exiftool nothing is written, and the user is told.
#[test]
fn req_024_04_says_so_without_exiftool() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, None);
    open(&mut harness, "b.png");

    harness.key_press(Key::Num4);
    settle(&mut harness);
    assert_eq!(indexed(&harness, "b.png"), None);
    let toasts = harness.state().toasts.current();
    assert!(
        toasts.last().is_some_and(|toast| toast.text.contains("exiftool")),
        "{toasts:?}"
    );
}

fn media_id(harness: &App, name: &str) -> i64 {
    let db = harness.state().db().unwrap();
    let conn = db.lock().unwrap();
    conn.query_row("SELECT id FROM media WHERE file_name = ?1", [name], |row| row.get(0))
        .unwrap()
}

/// REQ-024-07: ratings set in quick succession are written one after another, in order: the last one is in the file
/// and the index, and none of them failed.
#[test]
fn req_024_07_writes_quick_changes_one_after_another() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, Some(&exiftool));
    let media = media_id(&harness, "a.png");

    for rating in [1, 2, 3, 4, 5, 1, 2, 3, 4, 5, 1, 2, 3, 4] {
        harness.state_mut().rate(app::state::Rate {
            media,
            root_id: 1,
            rel_path: "a.png".into(),
            kind: domain::media::MediaKind::Image,
            rating: Some(rating),
        });
    }
    settle(&mut harness);

    let toasts = harness.state().toasts.current();
    assert!(toasts.is_empty(), "{toasts:?}");
    assert_eq!(in_file(&exiftool, &dir.join("a.png")), Some(4));
    assert_eq!(indexed(&harness, "a.png"), Some(4));
}
