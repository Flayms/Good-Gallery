//! Faces in the settings, the viewer and on the People page (spec 032), on a library of real photos and files written
//! with exiftool. Models run in `media`'s and `indexer`'s tests; here, what they found is in the index.

mod common;

use app::Page;
use common::{Fixture, settle};
use db::faces::{self, FacePhoto};
use db::libraries::LibraryStatus;
use db::writer::{IndexWriter, IndexedFile};
use domain::face_models::Detection;
use domain::faces::FaceRegion;
use domain::media::{MediaKind, MediaMetadata};
use eframe::egui::{self, Key};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

type App = egui_kittest::Harness<'static, app::AppState>;

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

const BEN: [f32; 4] = [0.1, 0.1, 0.2, 0.3];
const STRANGER: [f32; 4] = [0.6, 0.1, 0.2, 0.3];

fn detection(rect: [f32; 4]) -> Detection {
    Detection {
        rect,
        landmarks: [[0.5, 0.5]; 5],
        score: 0.9,
    }
}

/// `a.png` with Ben's named region, a face on it, and a face like his without a name. `b.png` to `d.png` each with an
/// unknown face, alike, but unlike his.
fn library(fixture: &Fixture, models: bool) -> (db::rusqlite::Connection, PathBuf) {
    let dir = PathBuf::from(fixture.folder("photos"));
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, dir.to_str().unwrap(), "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let files: Vec<IndexedFile> = ["a.png", "b.png", "c.png", "d.png"]
        .into_iter()
        .enumerate()
        .map(|(i, name)| {
            image::RgbImage::from_pixel(400, 300, image::Rgb([60, 120, 200]))
                .save(dir.join(name))
                .unwrap();
            let faces = if i == 0 {
                let [x, y, w, h] = BEN;
                vec![FaceRegion {
                    name: "Ben".into(),
                    x,
                    y,
                    w,
                    h,
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
                    // Photo tools write a named face's keyword too (REQ-004-09).
                    tags: if i == 0 {
                        vec![vec!["People".into(), "Ben".into()]]
                    } else {
                        Vec::new()
                    },
                    faces,
                    ..Default::default()
                },
                read_error: None,
            }
        })
        .collect();
    let mut writer = IndexWriter::new(conn);
    writer.write(root, &files).unwrap();
    let mut conn: db::rusqlite::Connection = writer.into();
    if models {
        let set = |key: &str, value: &str| db::settings::set_value(&conn, key, &json!(value)).unwrap();
        set(faces::DETECTOR_KEY, "detector");
        set(faces::RECOGNIZER_KEY, "recognizer");
        set(faces::THRESHOLD_KEY, "0.5");
        let photos: Vec<FacePhoto> = faces::pending_photos(&conn, "detector", 10).unwrap();
        for photo in &photos {
            let found: Vec<Detection> = if photo.rel_path == "a.png" {
                vec![detection(BEN), detection(STRANGER)]
            } else {
                vec![detection(STRANGER)]
            };
            faces::store_scan(&mut conn, photo, "detector", &found).unwrap();
        }
        for waiting in faces::pending_faces(&conn, "detector", "recognizer", 10).unwrap() {
            let vector = if waiting.photo.rel_path == "a.png" {
                [1.0, 0.0]
            } else {
                [0.0, 1.0]
            };
            faces::store_embedding(&conn, waiting.face, "recognizer", &vector).unwrap();
        }
    }
    (conn, dir)
}

fn app(fixture: &Fixture, models: bool, exiftool: Option<&Path>) -> (App, PathBuf) {
    let (conn, dir) = library(fixture, models);
    let mut state = fixture.state(conn).with_faces(&fixture.dir.path().join("data"), None);
    if let Some(exiftool) = exiftool {
        let config = media::edit::exiftool_config(&fixture.dir.path().join("data")).unwrap();
        state = state.with_edit_tools(media::edit::Tools {
            exiftool: exiftool.to_owned(),
            config,
        });
    }
    let mut harness = common::harness_with_step(state, 1.0 / 60.0);
    harness.set_size(egui::vec2(1000.0, 4000.0));
    settle(&mut harness);
    (harness, dir)
}

/// Opens `name` in the viewer with its faces shown.
fn open_with_faces(harness: &mut App, name: &str) {
    common::open_item(harness, name);
    settle(harness);
    if let Some(originals) = harness.state_mut().originals.as_mut() {
        originals.wait();
    }
    harness.run_steps(3);
    harness.key_press(Key::P);
    settle(harness);
}

/// Types at the end of `field`.
fn type_name(harness: &mut App, field: &str, name: &str) {
    harness.get_by_label(field).click();
    harness.run_steps(2);
    harness.key_press(Key::End);
    harness.run_steps(2);
    harness.event(egui::Event::Text(name.into()));
    harness.run_steps(2);
}

fn regions(exiftool: &Path, file: &Path) -> Vec<String> {
    let output = Command::new(exiftool)
        .args(["-json", "-struct", "-XMP-mwg-rs:RegionInfo"])
        .arg(file)
        .output()
        .unwrap();
    let read: Value = serde_json::from_slice(&output.stdout).unwrap();
    read[0]["RegionInfo"]["RegionList"]
        .as_array()
        .map_or(Vec::new(), |list| {
            list.iter()
                .map(|region| region["Name"].as_str().unwrap_or("").to_owned())
                .collect()
        })
}

fn write_ben(exiftool: &Path, file: &Path) {
    let status = Command::new(exiftool)
        .args([
            "-q",
            "-overwrite_original",
            "-XMP-mwg-rs:RegionInfo={RegionList=[{Area={X=0.2,Y=0.25,W=0.2,H=0.3,Unit=normalized},Name=Ben,Type=Face}]}",
            "-XMP-iptcExt:PersonInImage=Ben",
            "-XMP-dc:Subject=Ben",
        ])
        .arg(file)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn req_032_09_shows_unknown_faces_with_suggestions_beside_named_ones() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, true, None);
    open_with_faces(&mut harness, "a.png");
    assert!(harness.query_by_label("Face of Ben").is_some());
    assert!(
        harness.query_by_label("Unknown face: Ben?").is_some(),
        "the one like him"
    );
    // The face on Ben's region has no second box.
    assert_eq!(harness.query_all_by_label_contains("Unknown face:").count(), 1);
}

/// REQ-009-17, REQ-032-09: the button also shows for a photo with only unknown faces, with regions on or off.
#[test]
fn req_032_09_faces_button_shows_when_faces_are_looked_for() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, true, None);
    common::open_item(&mut harness, "b.png");
    settle(&mut harness);
    assert!(harness.query_by_label("Faces").is_some());
    harness.key_press(Key::P);
    settle(&mut harness);
    assert!(harness.query_by_label("Unknown face: Who is this?").is_some());
}

#[test]
fn req_032_09_no_faces_button_while_none_are_looked_for() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, false, None);
    common::open_item(&mut harness, "b.png");
    settle(&mut harness);
    assert!(harness.query_by_label("Faces").is_none());
}

#[test]
fn req_032_10_not_this_person_is_remembered() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, true, None);
    open_with_faces(&mut harness, "a.png");
    harness.get_by_label("Unknown face: Ben?").click();
    settle(&mut harness);
    harness.get_by_label("Not this person").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Unknown face: Who is this?").is_some());
    assert!(harness.query_by_label("Unknown face: Ben?").is_none());
}

#[test]
fn req_032_11_ignored_and_non_faces_go_away() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, true, None);
    open_with_faces(&mut harness, "a.png");
    harness.get_by_label("Unknown face: Ben?").click();
    settle(&mut harness);
    harness.get_by_label("Ignore").click();
    settle(&mut harness);
    assert_eq!(harness.query_all_by_label_contains("Unknown face:").count(), 0);
}

#[test]
fn req_032_11_a_non_face_goes_away() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, true, None);
    open_with_faces(&mut harness, "b.png");
    harness.get_by_label("Unknown face: Who is this?").click();
    settle(&mut harness);
    harness.get_by_label("Not a face").click();
    settle(&mut harness);
    assert_eq!(harness.query_all_by_label_contains("Unknown face:").count(), 0);
}

/// REQ-032-14: only a confirmed name reaches the file; before, the file stays as it was.
#[test]
fn req_032_14_confirming_a_suggestion_writes_it_and_nothing_before_that() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, true, Some(&exiftool));
    let file = dir.join("a.png");
    let before = std::fs::read(&file).unwrap();
    open_with_faces(&mut harness, "a.png");
    harness.get_by_label("Unknown face: Ben?").click();
    settle(&mut harness);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        before,
        "suggestions don't touch the file"
    );

    harness.get_by_label("It's Ben").click();
    settle(&mut harness);
    assert_eq!(regions(&exiftool, &file), ["Ben"]);
}

#[test]
fn req_032_14_rejecting_and_ignoring_leave_the_file_alone() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, true, Some(&exiftool));
    let file = dir.join("a.png");
    let before = std::fs::read(&file).unwrap();
    open_with_faces(&mut harness, "a.png");
    harness.get_by_label("Unknown face: Ben?").click();
    settle(&mut harness);
    harness.get_by_label("Not this person").click();
    settle(&mut harness);
    harness.get_by_label("Unknown face: Who is this?").click();
    settle(&mut harness);
    harness.get_by_label("Ignore").click();
    settle(&mut harness);
    assert_eq!(std::fs::read(&file).unwrap(), before);
}

#[test]
fn req_032_09_naming_an_unknown_face_writes_the_typed_name() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, true, Some(&exiftool));
    open_with_faces(&mut harness, "b.png");
    harness.get_by_label("Unknown face: Who is this?").click();
    settle(&mut harness);
    type_name(&mut harness, "Who is this?", "  Zoe  Lang ");
    harness.get_by_label("Save").click();
    settle(&mut harness);
    assert_eq!(regions(&exiftool, &dir.join("b.png")), ["Zoe Lang"]);
}

/// REQ-032-13: a region of another program can be renamed and removed on the photo.
#[test]
fn req_032_13_renames_and_removes_a_region_on_the_photo() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, true, Some(&exiftool));
    let file = dir.join("a.png");
    write_ben(&exiftool, &file);
    open_with_faces(&mut harness, "a.png");
    harness.get_by_label("Face of Ben").click();
    settle(&mut harness);
    type_name(&mut harness, "Who is this?", " Jr");
    harness.get_by_label("Rename").click();
    settle(&mut harness);
    assert_eq!(regions(&exiftool, &file), ["Ben Jr"]);
}

#[test]
fn req_032_13_removing_on_the_photo_takes_the_region_and_the_keyword() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, true, Some(&exiftool));
    let file = dir.join("a.png");
    write_ben(&exiftool, &file);
    open_with_faces(&mut harness, "a.png");
    harness.get_by_label("Face of Ben").click();
    settle(&mut harness);
    harness.get_by_label("Remove").click();
    settle(&mut harness);
    assert!(regions(&exiftool, &file).is_empty());
    let output = Command::new(&exiftool)
        .args(["-s3", "-XMP-dc:Subject"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Ben"));
}

#[test]
fn req_032_12_draws_a_face_and_saves_it_where_it_was_drawn() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, false, Some(&exiftool));
    // A region so the Faces button shows without models.
    open_with_faces(&mut harness, "a.png");
    harness.get_by_label("Add face").click();
    settle(&mut harness);

    let screen = harness.ctx.content_rect();
    let start = screen.center() + egui::vec2(-40.0, -40.0);
    let end = screen.center() + egui::vec2(40.0, 40.0);
    harness.hover_at(start);
    harness.run_steps(2);
    harness.drag_at(start);
    harness.run_steps(2);
    harness.hover_at(end);
    harness.run_steps(2);
    harness.drop_at(end);
    settle(&mut harness);

    type_name(&mut harness, "Who is this?", "Clara");
    harness.get_by_label("Save").click();
    settle(&mut harness);
    assert_eq!(regions(&exiftool, &dir.join("a.png")), ["Clara"]);
}

#[test]
fn req_032_15_names_a_group_after_asking_and_writes_to_all_its_photos() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, true, Some(&exiftool));
    harness.get_by_label("Unconfirmed faces").click();
    settle(&mut harness);
    assert_eq!(harness.state().page, Page::People);
    harness.get_by_label("3 faces");

    type_name(&mut harness, "Who is this? Group of 3", "Zoe");
    harness.get_by_label("Name them").click();
    settle(&mut harness);
    assert!(regions(&exiftool, &dir.join("b.png")).is_empty(), "it asks first");

    harness.get_by_label("Write Zoe into 3 files?");
    harness.get_all_by_label("Name them").last().unwrap().click();
    settle(&mut harness);
    for name in ["b.png", "c.png", "d.png"] {
        assert_eq!(regions(&exiftool, &dir.join(name)), ["Zoe"], "{name}");
    }
}

#[test]
fn req_032_15_ignoring_a_group_or_taking_a_face_out_leaves_too_few() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, true, None);
    harness.get_by_label("Unconfirmed faces").click();
    settle(&mut harness);
    harness
        .query_all_by_label_contains("out of the group")
        .next()
        .unwrap()
        .click();
    settle(&mut harness);
    assert!(harness.query_by_label("3 faces").is_none());
    assert!(harness.query_by_label("2 faces").is_none(), "too few for a group");
    harness.get_by_label("Ben? · 1 face");
}

/// REQ-032-24: a face like Ben is to confirm, under his name; _Yes, all_ asks, then writes him into the photo.
#[test]
fn req_032_24_confirm_or_reject_a_suggested_group() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, true, Some(&exiftool));
    harness.get_by_label("Unconfirmed faces").click();
    settle(&mut harness);
    harness.get_by_label("To confirm");
    harness.get_by_label("Ben? · 1 face");
    harness.get_by_label("3 faces");

    harness.get_by_label("Yes, all are Ben").click();
    settle(&mut harness);
    assert!(regions(&exiftool, &dir.join("a.png")).is_empty(), "it asks first");
    harness.get_by_label("Write Ben into 1 file?");
    harness.get_all_by_label("Name them").last().unwrap().click();
    settle(&mut harness);
    assert_eq!(regions(&exiftool, &dir.join("a.png")), ["Ben"]);
}

/// REQ-032-24: _Not Ben_ takes the face out of his group, and he isn't suggested for it again.
#[test]
fn req_032_24_a_face_that_isnt_the_person_leaves_the_group() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, true, None);
    harness.get_by_label("Unconfirmed faces").click();
    settle(&mut harness);
    harness.get_by_label_contains("isn't Ben").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Ben? · 1 face").is_none());
    assert!(harness.query_by_label("To confirm").is_none());
    harness.get_by_label("3 faces");

    harness.state_mut().page = Page::Gallery;
    settle(&mut harness);
    open_with_faces(&mut harness, "a.png");
    assert!(harness.query_by_label("Unknown face: Ben?").is_none());
}

/// REQ-032-25: a face taken out of its group stays an unknown face in the viewer.
#[test]
fn req_032_25_taking_a_face_out_keeps_it_in_the_viewer() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, true, None);
    harness.get_by_label("Unconfirmed faces").click();
    settle(&mut harness);
    harness
        .query_all_by_label_contains("out of the group")
        .next()
        .unwrap()
        .click();
    settle(&mut harness);
    assert!(harness.query_by_label("3 faces").is_none());

    harness.state_mut().page = Page::Gallery;
    settle(&mut harness);
    open_with_faces(&mut harness, "b.png");
    harness.get_by_label("Unknown face: Who is this?");
}

/// REQ-032-26: a face opens its photo with the faces shown; the filmstrip holds the group's photos; closing comes
/// back to the page with the search as it was.
#[test]
fn req_032_26_a_face_opens_its_photo_among_its_groups() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, true, None);
    harness.get_by_label("Unconfirmed faces").click();
    settle(&mut harness);
    let group = group_photos(&mut harness);
    // Ben's face first, then the strangers' group: its second face.
    let at = group[1][1];
    harness
        .query_all_by_label_contains("in its photo")
        .nth(2)
        .unwrap()
        .click();
    settle(&mut harness);
    let viewer = harness.state().viewer.as_ref().expect("the viewer opened");
    assert!(viewer.faces_shown());
    let gallery = harness.state().gallery.as_ref().unwrap();
    assert_eq!(gallery.position_of(at), Some(viewer.index()));
    assert_eq!(gallery.count(), Some(3), "only the group's photos");
    harness.get_by_label("Unknown face: Who is this?");

    harness.key_press(Key::Escape);
    std::thread::sleep(std::time::Duration::from_millis(400));
    settle(&mut harness);
    assert!(harness.state().viewer.is_none());
    assert_eq!(harness.state().page, Page::People);
    let gallery = harness.state_mut().gallery.as_mut().unwrap();
    gallery.wait_for_loads();
    assert!(gallery.search().media.is_empty());
    assert_eq!(gallery.count(), Some(4));
}

/// Opens _Unconfirmed faces_ and takes the strangers' first face out of their group, which leaves the two others too
/// few for one: all three are other unknown faces.
fn page_without_groups(fixture: &Fixture, exiftool: Option<&Path>) -> (App, PathBuf) {
    let (mut harness, dir) = app(fixture, true, exiftool);
    harness.get_by_label("Unconfirmed faces").click();
    settle(&mut harness);
    harness
        .query_all_by_label_contains("out of the group")
        .next()
        .unwrap()
        .click();
    settle(&mut harness);
    (harness, dir)
}

fn right_click(harness: &mut App, label: &str) {
    let pos = harness.get_by_label(label).rect().center();
    harness.hover_at(pos);
    harness.run_steps(1);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(1);
    }
    settle(harness);
}

/// The faces in a section of the page, by the label of their crops.
fn crops(harness: &App, contains: &str) -> Vec<String> {
    harness
        .query_all_by_label_contains(contains)
        .filter_map(|node| node.accesskit_node().label())
        .collect()
}

/// REQ-032-28: faces in no group show under _Other unknown faces_ and open their photo among theirs.
#[test]
fn req_032_28_faces_in_no_group_show_and_open_among_theirs() {
    let fixture = Fixture::new();
    let (mut harness, _) = page_without_groups(&fixture, None);
    harness.get_by_label("Other unknown faces");
    let others = crops(&harness, "in its photo");
    // Ben's face to confirm, then the three strangers.
    assert_eq!(others.len(), 4, "{others:?}");
    harness.get_by_label(&others[2]).click();
    settle(&mut harness);
    assert!(harness.state().viewer.is_some(), "the viewer opened");
    assert_eq!(
        harness.state().gallery.as_ref().unwrap().count(),
        Some(3),
        "only theirs"
    );
}

/// REQ-032-28: a long section shows a first part, and the rest on asking.
#[test]
fn req_032_28_shows_more_on_asking() {
    let fixture = Fixture::new();
    // Sixty faces, all taken out of their groups.
    let conn = many_groups(&fixture, 20);
    for face in faces::unknown_faces(&conn, "detector", "recognizer").unwrap() {
        faces::ungroup(&conn, face.id).unwrap();
    }
    let mut harness = people_page_with_thumbnails(&fixture, conn, egui::vec2(1000.0, 4000.0));
    let before = crops(&harness, "in its photo").len();
    assert!(before < 60, "{before}");
    harness.get_by_label("Show more").click();
    settle(&mut harness);
    assert!(crops(&harness, "in its photo").len() > before);
}

/// REQ-032-29: a face's menu ignores it or says it isn't one; the group's buttons do so for all; nothing reaches the
/// file.
#[test]
fn req_032_29_ignore_or_not_a_face_from_the_page() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, true, None);
    harness.get_by_label("Unconfirmed faces").click();
    settle(&mut harness);
    let ben = crops(&harness, "in its photo").remove(0);
    right_click(&mut harness, &ben);
    harness.get_by_label("Not a face").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Ben? · 1 face").is_none());
    harness.get_by_label("Not faces · 1");

    harness.get_by_label("Not faces").click();
    settle(&mut harness);
    harness.get_by_label("Not faces · 4");
    assert!(harness.query_by_label("3 faces").is_none());
    harness.get_by_label("No faces to confirm yet.");

    let (mut harness, _) = page_without_groups(&fixture, None);
    let other = crops(&harness, "in its photo").pop().unwrap();
    right_click(&mut harness, &other);
    harness.get_by_label("Ignore").click();
    settle(&mut harness);
    harness.get_by_label("Ignored · 1");
}

/// REQ-032-30: decided faces are listed, collapsed; one can go back to unknown, to the other decision, or be deleted.
#[test]
fn req_032_30_change_or_delete_a_decision() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, true, None);
    harness.get_by_label("Unconfirmed faces").click();
    settle(&mut harness);
    harness.get_by_label("Ignore").click();
    settle(&mut harness);
    assert!(crops(&harness, "Change face").is_empty(), "collapsed");
    harness.get_by_label("Ignored · 3").click();
    settle(&mut harness);
    let ignored = crops(&harness, "Change face");
    assert_eq!(ignored.len(), 3);

    harness.get_by_label(&ignored[0]).click();
    settle(&mut harness);
    harness.get_by_label("Back to unknown").click();
    settle(&mut harness);
    harness.get_by_label("Ignored · 2");
    harness.get_by_label("Other unknown faces");

    harness.get_by_label(&ignored[1]).click();
    settle(&mut harness);
    harness.get_by_label("Not a face").click();
    settle(&mut harness);
    harness.get_by_label("Ignored · 1");
    harness.get_by_label("Not faces · 1");

    harness.get_by_label(&ignored[2]).click();
    settle(&mut harness);
    harness.get_by_label("Delete").click();
    settle(&mut harness);
    assert!(harness.query_by_label_contains("Ignored ·").is_none());
    let conn = harness.state().db().unwrap();
    let left: i64 = conn
        .lock()
        .unwrap()
        .query_row("SELECT count(*) FROM detected_faces", [], |row| row.get(0))
        .unwrap();
    assert_eq!(left, 4, "Ben's two faces, one back to unknown, one not a face");
}

/// REQ-032-30: a decided face can be named, which writes it into the file and makes it that person.
#[test]
fn req_032_30_naming_a_decided_face_writes_it() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, true, Some(&exiftool));
    harness.get_by_label("Unconfirmed faces").click();
    settle(&mut harness);
    harness.get_by_label("Ignore").click();
    settle(&mut harness);
    harness.get_by_label("Ignored · 3").click();
    settle(&mut harness);
    let face = crops(&harness, "Change face").remove(0);
    harness.get_by_label(&face).click();
    settle(&mut harness);
    harness.get_by_label_contains("Who is this? Face").click();
    harness.run_steps(2);
    harness.event(egui::Event::Text("Zoe".into()));
    harness.run_steps(2);
    harness.get_by_label("Save").click();
    settle(&mut harness);
    let written: Vec<_> = ["b.png", "c.png", "d.png"]
        .into_iter()
        .filter(|name| regions(&exiftool, &dir.join(name)) == ["Zoe"])
        .collect();
    assert_eq!(written.len(), 1, "{written:?}");
    harness.get_by_label("Ignored · 2");
}

/// REQ-032-30: the settings no longer bring back every ignored face at once.
#[test]
fn req_032_30_no_button_brings_back_all_ignored_faces() {
    let fixture = Fixture::new();
    let harness = settings(&fixture, true);
    assert!(harness.query_by_label("Show ignored faces again").is_none());
}

#[test]
fn req_032_15_no_people_page_without_models() {
    let fixture = Fixture::new();
    let (harness, _) = app(&fixture, false, None);
    assert!(harness.query_by_label("Unconfirmed faces").is_none());
}

#[test]
fn req_032_27_unconfirmed_faces_is_the_first_row_of_people() {
    let fixture = Fixture::new();
    let (harness, _) = app(&fixture, true, None);
    let heading = harness.get_by_label("People").rect();
    let unconfirmed = harness.get_by_label("Unconfirmed faces").rect();
    let ben = harness.get_by_label("Ben").rect();
    assert!(heading.bottom() <= unconfirmed.top(), "below the heading");
    assert!(unconfirmed.bottom() <= ben.top(), "above the people's tags");
}

#[test]
fn req_032_27_people_shows_without_tags_and_collapses_with_it() {
    let fixture = Fixture::new();
    let (conn, _) = library(&fixture, true);
    conn.execute_batch("DELETE FROM media_tags; DELETE FROM tags;").unwrap();
    let state = fixture.state(conn).with_faces(&fixture.dir.path().join("data"), None);
    let mut harness = common::harness_with_step(state, 1.0 / 60.0);
    harness.set_size(egui::vec2(1000.0, 4000.0));
    settle(&mut harness);
    assert!(harness.query_by_label("Ben").is_none());
    let heading = harness.get_by_label("People").rect();
    assert!(heading.bottom() <= harness.get_by_label("Unconfirmed faces").rect().top());

    harness.get_by_label("People").click();
    settle(&mut harness);
    assert!(
        harness.query_by_label("Unconfirmed faces").is_none(),
        "collapsed with the group"
    );
    harness.get_by_label("People").click();
    settle(&mut harness);
    harness.get_by_label("Unconfirmed faces");
}

fn settings(fixture: &Fixture, models: bool) -> App {
    let (mut harness, _) = app(fixture, models, None);
    harness.state_mut().page = Page::Settings;
    settle(&mut harness);
    harness
}

#[test]
fn req_032_01_lists_the_models_with_their_licenses() {
    let fixture = Fixture::new();
    let harness = settings(&fixture, false);
    harness.get_by_label("Compare the models");
    for model in domain::face_models::CATALOG {
        assert!(
            harness.query_by_label(&format!("Download {}", model.name)).is_some(),
            "{}",
            model.name
        );
    }
    harness.get_by_label("Not looking for faces");
    harness.get_by_label("Choose a detection model below to start.");
}

fn yunet() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../media/tests/fixtures/faces/face_detection_yunet_2023mar.onnx")
}

#[test]
fn req_032_03_adds_a_file_it_can_run_and_refuses_one_it_cant() {
    let fixture = Fixture::new();
    let mut harness = settings(&fixture, false);
    let junk = fixture.dir.path().join("junk.onnx");
    std::fs::write(&junk, b"no model").unwrap();
    {
        harness.state_mut().add_face_file_at(junk);
    }
    settle(&mut harness);
    assert!(harness.state().faces.models.is_empty());
    harness.get_by_label_contains("Couldn't get the model");

    harness.state_mut().add_face_file_at(yunet());
    settle(&mut harness);
    harness.get_by_label("Use YuNet");
    assert_eq!(harness.state().faces.models.len(), 1);
}

/// A file dropped in a test.
#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|error| error.to_string())
    }
}

/// Drops `files` on the window.
fn drop_files(harness: &mut App, files: &[PathBuf]) {
    harness.input_mut().dropped_files = files
        .iter()
        .map(|file| Arc::new(Dropped(file.clone())) as egui::DroppedFileHandle)
        .collect();
    settle(harness);
}

/// REQ-032-03: a model comes from a file, not from a link.
#[test]
fn req_032_03_has_no_link_field_and_says_a_file_can_be_dropped() {
    let fixture = Fixture::new();
    let harness = settings(&fixture, false);
    assert!(harness.query_by_label("Link to a model").is_none());
    harness.get_by_label("Choose file…");
    harness.get_by_label_contains("drop it here");
}

/// REQ-032-03: a dropped `.onnx` file is added like a chosen one, and a file that can't run is refused.
#[test]
fn req_032_03_adds_a_dropped_file_and_refuses_one_it_cant_run() {
    let fixture = Fixture::new();
    let mut harness = settings(&fixture, false);
    let junk = fixture.dir.path().join("junk.onnx");
    std::fs::write(&junk, b"no model").unwrap();
    drop_files(&mut harness, &[junk]);
    assert!(harness.state().faces.models.is_empty());
    harness.get_by_label_contains("Couldn't get the model");

    drop_files(&mut harness, &[yunet()]);
    harness.get_by_label("Use YuNet");
}

/// REQ-032-03: what isn't an ONNX file is refused with the reason, and of several files the first is added.
#[test]
fn req_032_03_says_why_a_dropped_file_isnt_added() {
    let fixture = Fixture::new();
    let mut harness = settings(&fixture, false);
    let photo = fixture.dir.path().join("photo.jpg");
    std::fs::write(&photo, b"a photo").unwrap();
    drop_files(&mut harness, std::slice::from_ref(&photo));
    assert!(harness.state().faces.models.is_empty());
    harness.get_by_label_contains("photo.jpg isn't an ONNX file");

    drop_files(&mut harness, &[yunet(), photo]);
    harness.get_by_label("Use YuNet");
    harness.get_by_label_contains("one model at a time");
}

/// REQ-032-03: dropping a file on another page does nothing.
#[test]
fn req_032_03_a_file_dropped_on_another_page_is_ignored() {
    let fixture = Fixture::new();
    let mut harness = settings(&fixture, false);
    harness.state_mut().page = app::Page::Gallery;
    settle(&mut harness);
    drop_files(&mut harness, &[yunet()]);
    assert!(harness.state().faces.models.is_empty());
}

/// REQ-032-03: while a file is over the window, the section says where it goes.
#[test]
fn req_032_03_shows_where_to_drop_a_file() {
    let fixture = Fixture::new();
    let mut harness = settings(&fixture, false);
    assert!(harness.query_by_label("Drop the model here").is_none());
    harness.input_mut().hovered_files = vec![egui::HoveredFile {
        path: Some(yunet()),
        ..Default::default()
    }];
    harness.run_steps(2);
    harness.get_by_label("Drop the model here");
}

#[test]
fn req_032_19_says_what_choosing_does_and_in_use_stays() {
    let fixture = Fixture::new();
    let mut harness = settings(&fixture, false);
    harness.get_by_label_contains("Choosing one starts looking through your photos");
    harness.get_by_label_contains("Changing a model looks through your photos again");
    harness.state_mut().add_face_file_at(yunet());
    settle(&mut harness);
    assert_eq!(harness.state().faces.detector, None);

    harness.get_by_label("Use YuNet").click();
    settle(&mut harness);
    let chosen = harness.state().faces.detector.clone();
    assert!(chosen.is_some());
    harness.get_by_label("Using YuNet").click();
    settle(&mut harness);
    assert_eq!(harness.state().faces.detector, chosen, "stopping is Pause");

    harness.get_by_label("Delete YuNet").click();
    settle(&mut harness);
    assert_eq!(harness.state().faces.detector, None);
}

#[test]
fn req_032_18_says_why_a_model_is_recommended() {
    let fixture = Fixture::new();
    let harness = settings(&fixture, false);
    for model in domain::face_models::CATALOG {
        assert!(
            harness.query_all_by_label_contains(model.why).next().is_some(),
            "{}",
            model.name
        );
    }
}

#[test]
fn req_032_20_pause_and_resume() {
    let fixture = Fixture::new();
    let mut harness = settings(&fixture, true);
    harness.get_by_label("Pause").click();
    settle(&mut harness);
    assert!(harness.state().faces.paused);
    harness.get_by_label("Resume").click();
    settle(&mut harness);
    assert!(!harness.state().faces.paused);
}

/// A search stuck on its first photo until the test is over, so it stays "working".
struct Waiting(Arc<AtomicBool>);

impl indexer::faces::FaceWork for Waiting {
    fn detect(&mut self, _: &str, _: &FacePhoto) -> Result<Vec<Detection>, indexer::faces::WorkError> {
        while !self.0.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(Vec::new())
    }

    fn recognize(
        &mut self,
        _: &str,
        _: &FacePhoto,
        faces: &[[[f32; 2]; 5]],
    ) -> Result<Vec<Vec<f32>>, indexer::faces::WorkError> {
        Ok(vec![Vec::new(); faces.len()])
    }
}

#[test]
fn req_032_20_the_icon_shows_on_every_page_and_leads_to_the_settings() {
    let fixture = Fixture::new();
    let (conn, _) = library(&fixture, false);
    let index = fixture.dir.path().join("faces.db");
    let search = db::open(&index).unwrap();
    db::settings::set_value(&search, faces::DETECTOR_KEY, &json!("detector")).unwrap();
    db::libraries::insert(&search, "/nowhere", "Nowhere", LibraryStatus::Online).unwrap();
    search
        .execute(
            "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime, taken_at)
             VALUES (1, 'a.jpg', 'a.jpg', 'image', 1, 1, 1)",
            [],
        )
        .unwrap();
    let release = Arc::new(AtomicBool::new(false));
    let service = Arc::new(indexer::faces::FaceService::start(indexer::faces::FaceDeps {
        conn: search,
        work: Box::new(Waiting(release.clone())),
        is_reachable: Arc::new(|_| true),
        on_change: Box::new(|| {}),
    }));
    let state = fixture
        .state(conn)
        .with_faces(&fixture.dir.path().join("data"), Some(service));
    let mut harness = common::harness_with_step(state, 1.0 / 60.0);
    harness.state_mut().faces.detector = Some("detector".into());
    harness.run_steps(3);
    let open = |harness: &mut App| {
        harness.get_by_label("What's running").click();
        harness.run_steps(3);
    };
    assert!(harness.query_by_label("Finding faces").is_none(), "behind the icon");
    open(&mut harness);
    assert!(harness.query_by_label("Finding faces").is_some(), "in the gallery");
    harness.key_press(Key::Escape);
    harness.run_steps(3);

    harness.state_mut().page = Page::People;
    harness.run_steps(3);
    open(&mut harness);
    assert!(harness.query_by_label("Finding faces").is_some(), "on the People page");

    harness.get_by_label("Faces settings").click();
    harness.run_steps(3);
    assert_eq!(harness.state().page, Page::Settings);
    harness.get_by_label("What's running");
    release.store(true, Ordering::SeqCst);
}

#[test]
fn req_032_21_details_say_whether_a_photo_was_looked_at() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, true, None);
    common::open_item(&mut harness, "a.png");
    settle(&mut harness);
    harness.key_press(Key::I);
    settle(&mut harness);
    harness.get_by_label("Looked at, 2 found");
}

#[test]
fn req_032_22_look_again_from_the_details() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, true, None);
    common::open_item(&mut harness, "a.png");
    settle(&mut harness);
    harness.key_press(Key::I);
    settle(&mut harness);
    harness.get_by_label("Look again").click();
    settle(&mut harness);
    harness.run_steps(3);
    settle(&mut harness);
    harness.get_by_label("Not looked at yet");
    assert!(harness.query_by_label("Look again").is_none());
}

#[test]
fn req_032_22_look_through_all_again_asks_first() {
    let fixture = Fixture::new();
    let mut harness = settings(&fixture, true);
    harness
        .get_all_by_label("Look through all photos again")
        .last()
        .unwrap()
        .click();
    settle(&mut harness);
    harness.get_by_label_contains("can take many hours");
    harness.get_by_label("Look again").click();
    settle(&mut harness);
    assert!(harness.query_by_label_contains("can take many hours").is_none());
}

#[test]
fn req_032_04_delete_face_data_asks_and_stops_the_search() {
    let fixture = Fixture::new();
    let mut harness = settings(&fixture, true);
    harness.get_by_label("Delete face data").click();
    settle(&mut harness);
    assert!(harness.state().faces.detector.is_some(), "it asks first");
    harness.get_by_label("Delete").click();
    settle(&mut harness);
    assert_eq!(harness.state().faces.detector, None);
    assert_eq!(harness.state().faces.recognizer, None);
    harness.state_mut().page = Page::Gallery;
    settle(&mut harness);
    common::open_item(&mut harness, "b.png");
    settle(&mut harness);
    assert!(
        harness.query_by_label("Faces").is_none(),
        "nothing stored, nothing shown"
    );
}

#[test]
fn req_032_17_warns_when_the_model_tells_nobody_apart() {
    let fixture = Fixture::new();
    let (conn, _) = library(&fixture, true);
    // Three named faces each of two people, all with the same vector.
    let extra = [
        ("Ben", 1000),
        ("Ben", 1001),
        ("Ann", 1002),
        ("Ann", 1003),
        ("Ann", 1004),
    ];
    let root: i64 = conn
        .query_row("SELECT id FROM library_roots", [], |row| row.get(0))
        .unwrap();
    let mut conn = conn;
    for (name, id) in extra {
        let rel = format!("{id}.png");
        let media: i64 = conn
            .query_row(
                "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime, taken_at)
                 VALUES (?1, ?2, ?2, 'image', 1, 1000, 1) RETURNING id",
                db::rusqlite::params![root, rel],
                |row| row.get(0),
            )
            .unwrap();
        let photo = faces::pending_photos(&conn, "detector", 10).unwrap().remove(0);
        assert_eq!(photo.media, media);
        faces::store_scan(&mut conn, &photo, "detector", &[detection(BEN)]).unwrap();
        let [x, y, w, h] = BEN;
        conn.execute(
            "INSERT INTO media_faces (media_id, name, x, y, w, h) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            db::rusqlite::params![media, name, x, y, w, h],
        )
        .unwrap();
        let waiting = faces::pending_faces(&conn, "detector", "recognizer", 1)
            .unwrap()
            .remove(0);
        faces::store_embedding(&conn, waiting.face, "recognizer", &[1.0, 0.0]).unwrap();
    }
    let state = fixture.state(conn).with_faces(&fixture.dir.path().join("data"), None);
    let mut harness = common::harness_with_step(state, 1.0 / 60.0);
    harness.set_size(egui::vec2(1000.0, 4000.0));
    harness.state_mut().page = Page::Settings;
    settle(&mut harness);
    settle(&mut harness);
    harness.get_by_label_contains("doesn't tell your named people apart");
}

/// REQ-032-01, REQ-032-09, REQ-032-15: how the section, a face's popup and the People page look.
#[test]
fn req_032_01_settings_section_looks_as_designed() {
    let fixture = Fixture::new();
    // Already on this computer, so no toast fades over the picture.
    let kind = domain::face_models::ModelKind::Detect(domain::face_models::DetectorFormat::YuNet);
    media::face_store::FaceStore::in_data_folder(&fixture.dir.path().join("data"))
        .add_file(&yunet(), &|_| Ok(kind), &mut |_, _| true)
        .unwrap();
    // No library: its path lies in a temporary folder named differently on every run.
    let state = fixture
        .state(db::open_in_memory().unwrap())
        .with_faces(&fixture.dir.path().join("data"), None);
    let mut harness = common::harness_with_step(state, 1.0 / 60.0);
    // Tall enough for the whole section and no further: _About_ further down shows the version, which a release changes.
    harness.set_size(egui::vec2(1000.0, 2420.0));
    harness.state_mut().page = Page::Settings;
    settle(&mut harness);
    harness.get_by_label("Use YuNet").click();
    settle(&mut harness);
    harness.snapshot(common::snapshot_name("settings_faces"));
}

#[test]
fn req_032_09_a_faces_popup_looks_as_designed() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, true, None);
    harness.set_size(egui::vec2(1000.0, 700.0));
    open_with_faces(&mut harness, "a.png");
    harness.get_by_label("Unknown face: Ben?").click();
    settle(&mut harness);
    harness.run_steps(20);
    harness.snapshot(common::snapshot_name("viewer_face_popup"));
}

#[test]
fn req_032_15_people_page_looks_as_designed() {
    let fixture = Fixture::new();
    let (conn, _dir) = library(&fixture, true);
    let mut harness = people_page_with_thumbnails(&fixture, conn, egui::vec2(1000.0, 700.0));
    let shown: Vec<i64> = group_photos(&mut harness).concat();
    wait_for_thumbnails(&mut harness, &shown);
    harness.snapshot(common::snapshot_name("people_page"));
}

/// REQ-032-28, REQ-032-30: the other unknown faces, then the ignored faces and non-faces, opened.
#[test]
fn req_032_30_decided_faces_look_as_designed() {
    let fixture = Fixture::new();
    let (conn, _dir) = library(&fixture, true);
    let mut unknown = faces::unknown_faces(&conn, "detector", "recognizer").unwrap();
    unknown.sort_by_key(|face| face.media);
    // a.png's stranger isn't a face, b.png's is ignored, c.png's taken out, which leaves d.png's alone too.
    faces::set_state(&conn, unknown[0].id, faces::FaceState::NotAFace).unwrap();
    faces::set_state(&conn, unknown[1].id, faces::FaceState::Ignored).unwrap();
    faces::ungroup(&conn, unknown[2].id).unwrap();
    let mut harness = people_page_with_thumbnails(&fixture, conn, egui::vec2(1000.0, 700.0));
    // One at a time: the first opened moves the second.
    for header in ["Not faces · 1", "Ignored · 1"] {
        harness.get_by_label(header).click();
        settle(&mut harness);
    }
    harness.hover_at(egui::pos2(990.0, 690.0));
    let media: Vec<i64> = unknown.iter().map(|face| face.media).collect();
    wait_for_thumbnails(&mut harness, &media);
    harness.snapshot(common::snapshot_name("people_page_decided"));
}

/// REQ-032-15: the faces of a group on screen show, without the gallery having been drawn.
#[test]
fn req_032_15_shows_the_faces_of_a_group() {
    let fixture = Fixture::new();
    let (conn, _dir) = library(&fixture, true);
    let mut harness = people_page_with_thumbnails(&fixture, conn, egui::vec2(1000.0, 700.0));
    let groups = group_photos(&mut harness);
    wait_for_thumbnails(&mut harness, &groups[0]);
}

/// REQ-032-15: only groups on screen or nearly so hold thumbnails, or the page could pass the textures' budget for good.
#[test]
fn req_032_15_loads_only_the_faces_of_groups_near_the_screen() {
    let fixture = Fixture::new();
    let conn = many_groups(&fixture, 8);
    let mut harness = people_page_with_thumbnails(&fixture, conn, egui::vec2(1000.0, 300.0));
    let groups = group_photos(&mut harness);
    assert_eq!(groups.len(), 8);
    wait_for_thumbnails(&mut harness, &groups[0]);
    // Long enough for all of them, had they been asked for.
    for _ in 0..200 {
        harness.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let gallery = harness.state_mut().gallery.as_mut().unwrap();
    for media in &groups[7] {
        assert!(gallery.thumbnail(*media).is_none(), "photo {media} is far off screen");
    }
}

/// A library with `groups` groups of three unknown faces each, all unlike each other.
fn many_groups(fixture: &Fixture, groups: usize) -> db::rusqlite::Connection {
    let dir = fixture.folder("photos");
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, &dir, "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let files: Vec<IndexedFile> = (0..groups * 3)
        .map(|i| IndexedFile {
            rel_path: format!("{i}.png"),
            file_name: format!("{i}.png"),
            kind: MediaKind::Image,
            size: 1,
            mtime: 1_000,
            sidecar_mtime: None,
            metadata: MediaMetadata {
                width: Some(400),
                height: Some(300),
                ..Default::default()
            },
            read_error: None,
        })
        .collect();
    let mut writer = IndexWriter::new(conn);
    writer.write(root, &files).unwrap();
    let mut conn: db::rusqlite::Connection = writer.into();
    let set = |key: &str, value: &str| db::settings::set_value(&conn, key, &json!(value)).unwrap();
    set(faces::DETECTOR_KEY, "detector");
    set(faces::RECOGNIZER_KEY, "recognizer");
    set(faces::THRESHOLD_KEY, "0.5");
    for photo in faces::pending_photos(&conn, "detector", files.len()).unwrap() {
        faces::store_scan(&mut conn, &photo, "detector", &[detection(STRANGER)]).unwrap();
    }
    for waiting in faces::pending_faces(&conn, "detector", "recognizer", files.len()).unwrap() {
        let number: usize = waiting.photo.rel_path.trim_end_matches(".png").parse().unwrap();
        let mut vector = vec![0.0; groups];
        vector[number / 3] = 1.0;
        faces::store_embedding(&conn, waiting.face, "recognizer", &vector).unwrap();
    }
    conn
}

/// The app on the People page, the gallery never drawn, with every photo's thumbnails in a real cache and streamer.
fn people_page_with_thumbnails(fixture: &Fixture, conn: db::rusqlite::Connection, size: egui::Vec2) -> App {
    let cache = indexer::thumb_cache::ThumbCache::open(fixture.dir.path().join("thumbs"), u64::MAX);
    cache.wait_ready();
    let media: Vec<i64> = conn
        .prepare("SELECT id FROM media")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for id in media {
        let key = db::thumbs::thumb_source(&conn, id).unwrap().unwrap().key;
        let shade = (id * 37 % 200) as u8 + 40;
        let pixels = [shade, 90, 255 - shade].repeat(400 * 300);
        let thumbnail = webp::Encoder::from_rgb(&pixels, 400, 300).encode_lossless();
        for width in domain::thumbs::THUMB_WIDTHS {
            let path = cache.path(&key, width);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &*thumbnail).unwrap();
        }
    }
    let render_state = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        eframe::egui_wgpu::RendererOptions::PREDICTABLE,
    );
    let mut state = fixture.state(conn).with_faces(&fixture.dir.path().join("data"), None);
    state.gallery = state.gallery.take().map(|gallery| {
        gallery.with_thumbnails(app::gallery::Thumbnails {
            streamer: app::textures::TextureStreamer::new(
                render_state.clone(),
                cache.clone(),
                app::textures::DEFAULT_BUDGET,
                egui::Context::default(),
            ),
            cache,
            service: None,
            ready: std::sync::mpsc::channel().1,
        })
    });
    state.page = Page::People;
    let mut harness = common::harness_on(state, render_state);
    harness.set_size(size);
    settle(&mut harness);
    harness
}

/// Each group's photos, largest group first.
fn group_photos(harness: &mut App) -> Vec<Vec<i64>> {
    let gallery = harness.state_mut().gallery.as_mut().unwrap();
    gallery.wait_for_loads();
    let groups = gallery.face_groups().expect("the groups have loaded");
    groups
        .suggested
        .iter()
        .map(|(_, group)| group)
        .chain(&groups.unknown)
        .map(|group| group.iter().map(|(face, _)| face.media).collect())
        .collect()
}

/// Draws frames until these photos' thumbnails are there, as they come in on other threads.
fn wait_for_thumbnails(harness: &mut App, media: &[i64]) {
    for _ in 0..500 {
        harness.run_steps(1);
        let gallery = harness.state_mut().gallery.as_mut().unwrap();
        if media.iter().all(|media| gallery.thumbnail(*media).is_some()) {
            harness.run_steps(40);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the thumbnails of {media:?} never showed");
}
