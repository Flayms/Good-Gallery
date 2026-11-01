//! Adding and removing tags in the info panel (spec 026) on a library of real photos, written with the real exiftool.

mod common;

use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::writer::{IndexWriter, IndexedFile};
use domain::media::{MediaKind, MediaMetadata};
use eframe::egui::accesskit::Role;
use eframe::egui::{self, Key};
use egui_kittest::kittest::Queryable as _;
use serde_json::{Value, json};
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

fn path(levels: &[&str]) -> Vec<String> {
    levels.iter().map(|level| level.to_string()).collect()
}

/// `a.png` tagged Beach, People › Anna and Places › Italy › Rome, in the index and the file, with a face region of
/// Ben; `b.png` without tags. With exiftool for writing if given.
fn app(fixture: &Fixture, exiftool: Option<&Path>) -> (App, PathBuf) {
    let dir = PathBuf::from(fixture.folder("photos"));
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, dir.to_str().unwrap(), "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let tags = vec![
        path(&["Beach"]),
        path(&["People", "Anna"]),
        path(&["Places", "Italy", "Rome"]),
    ];
    let files: Vec<IndexedFile> = [("a.png", tags), ("b.png", Vec::new())]
        .into_iter()
        .enumerate()
        .map(|(i, (name, tags))| {
            image::RgbImage::from_pixel(400, 300, image::Rgb([60, 120, 200]))
                .save(dir.join(name))
                .unwrap();
            let faces = if i == 0 {
                vec![domain::faces::FaceRegion {
                    name: "Ben".into(),
                    x: 0.1,
                    y: 0.1,
                    w: 0.2,
                    h: 0.3,
                }]
            } else {
                Vec::new()
            };
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
                    tags,
                    faces,
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
            .args([
                "-q",
                "-overwrite_original",
                "-XMP-lr:HierarchicalSubject=People|Anna",
                "-XMP-lr:HierarchicalSubject=Places|Italy|Rome",
                "-XMP-dc:Subject=Beach",
                "-XMP-dc:Subject=Anna",
                "-XMP-dc:Subject=Rome",
            ])
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
    harness.set_size(egui::vec2(900.0, 900.0));
    settle(&mut harness);
    (harness, dir)
}

/// Opens `name` in the viewer with its info panel.
fn open_info(harness: &mut App, name: &str) {
    harness.get_by_role_and_label(Role::Image, name).click();
    settle(harness);
    harness.key_press(Key::I);
    settle(harness);
    // The original on screen, not its placeholder.
    if let Some(originals) = harness.state_mut().originals.as_mut() {
        originals.wait();
    }
    harness.run_steps(3);
}

/// Hides the file's location, which lies in a temporary folder named differently on every run.
fn mask_location(harness: &mut App, dir: &Path) {
    let location = harness.get_by_label_contains(&dir.to_string_lossy()).rect();
    let right = harness.ctx.content_rect().right();
    harness.mask(egui::Rect::from_min_max(
        location.min,
        egui::pos2(right, location.max.y),
    ));
}

fn type_text(harness: &mut App, text: &str) {
    harness.event(egui::Event::Text(text.into()));
    harness.run_steps(2);
}

/// The keyword fields of `file`.
fn keywords(exiftool: &Path, file: &Path) -> Value {
    let output = Command::new(exiftool)
        .args(["-j", "-HierarchicalSubject", "-Subject"])
        .arg(file)
        .output()
        .unwrap();
    let mut read: Value = serde_json::from_slice(&output.stdout).unwrap();
    read[0].as_object_mut().unwrap().remove("SourceFile");
    read[0].take()
}

fn reread(fixture: &Fixture, name: &str) -> bool {
    fixture
        .scans
        .paths
        .lock()
        .unwrap()
        .iter()
        .any(|(_, paths)| paths.iter().any(|path| path == name))
}

/// REQ-026-01, REQ-026-03: a suggested place is added with its hierarchy, a typed new person under People; the file
/// gets them and is read again. Typing doesn't trigger the viewer's keys.
#[test]
fn req_026_01_req_026_03_adds_suggested_and_new_tags() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, Some(&exiftool));
    open_info(&mut harness, "b.png");

    harness.get_by_label("Add to Places").click();
    harness.run_steps(3);
    type_text(&mut harness, "rom");
    harness.get_by_label("Places › Italy › Rome").click();
    settle(&mut harness);
    assert_eq!(
        keywords(&exiftool, &dir.join("b.png")),
        json!({ "HierarchicalSubject": "Places|Italy|Rome", "Subject": "Rome" })
    );
    assert!(reread(&fixture, "b.png"));

    harness.get_by_label("Add to People").click();
    harness.run_steps(3);
    // `i` and `f` would toggle the info panel and the filmstrip, `e` start editing.
    type_text(&mut harness, "Fie");
    harness.key_press(Key::Enter);
    settle(&mut harness);
    let viewer = harness.state().viewer.as_ref().unwrap();
    assert!(viewer.info_open() && viewer.editing().is_none());
    assert_eq!(
        keywords(&exiftool, &dir.join("b.png")),
        json!({ "HierarchicalSubject": ["Places|Italy|Rome", "People|Fie"], "Subject": ["Rome", "Fie"] })
    );
}

/// REQ-026-01, REQ-026-05: the info sheet beside the gallery adds tags too, with the group's suggestions under the
/// field.
#[test]
fn req_026_01_suggests_in_the_info_sheet() {
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, None);
    let tile = harness.get_by_role_and_label(Role::Image, "b.png").rect().center();
    harness.hover_at(tile);
    harness.run_steps(1);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: tile,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(1);
    }
    harness.run_steps(2);
    harness.get_by_label("Information").click();
    settle(&mut harness);

    harness.get_by_label("Add to Places").click();
    harness.run_steps(3);
    type_text(&mut harness, "rom");
    assert!(harness.query_by_label("Places › Italy › Rome").is_some());
    assert!(
        harness.query_by_label("People › Anna").is_none(),
        "only the group's tags"
    );
    mask_location(&mut harness, &dir);
    harness.snapshot(common::snapshot_name("tag_suggestions"));
}

/// REQ-026-02, REQ-026-03: a tag's remove button takes it out of the file; a person named only by a face region has
/// none.
#[test]
fn req_026_02_removes_a_tag_but_not_a_face_region() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, Some(&exiftool));
    open_info(&mut harness, "a.png");
    assert!(harness.query_by_label("Remove Ben").is_none());
    assert!(harness.query_by_label("Remove Anna").is_some());

    harness.get_by_label("Remove Rome").click();
    settle(&mut harness);
    assert_eq!(
        keywords(&exiftool, &dir.join("a.png")),
        json!({ "HierarchicalSubject": "People|Anna", "Subject": ["Beach", "Anna"] })
    );
    assert!(reread(&fixture, "a.png"));
}

/// REQ-026-01: Escape closes the field without a change, and keeps the viewer open.
#[test]
fn req_026_01_escape_closes_the_field() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, None);
    open_info(&mut harness, "b.png");
    harness.get_by_label("Add to Tags").click();
    harness.run_steps(3);
    type_text(&mut harness, "Sea");
    harness.key_press(Key::Escape);
    settle(&mut harness);
    assert!(harness.query_by_label("New tag for Tags").is_none());
    assert!(harness.state().viewer.is_some());
    assert!(fixture.scans.paths.lock().unwrap().is_empty());
}

/// REQ-026-04: a read-only file keeps its tags, and a toast says why.
#[test]
fn req_026_04_keeps_the_tags_of_a_file_it_cant_write() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, Some(&exiftool));
    let file = dir.join("a.png");
    let mut permissions = std::fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&file, permissions).unwrap();
    open_info(&mut harness, "a.png");

    harness.get_by_label("Remove Beach").click();
    settle(&mut harness);
    assert_eq!(keywords(&exiftool, &file)["Subject"], json!(["Beach", "Anna", "Rome"]));
    let toasts = harness.state().toasts.current();
    assert!(
        toasts
            .last()
            .is_some_and(|toast| toast.text == "Couldn't save the tag: the file is read-only"),
        "{toasts:?}"
    );
    assert!(!reread(&fixture, "a.png"));
}
