//! Renaming a selection (spec 035) in the app: the dialog, its preview, renaming, undo and an interrupted rename.

use crate::common;

use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::writer::{IndexWriter, IndexedFile};
use domain::media::{MediaKind, MediaMetadata};
use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use std::path::PathBuf;

type App = Harness<'static, app::AppState>;

/// `IMG_1.jpg`, `IMG_2.jpg` with a sidecar, and `other.jpg`, on disk and in the index; all selected.
fn app(fixture: &Fixture) -> (App, PathBuf) {
    let dir = PathBuf::from(fixture.folder("photos"));
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, dir.to_str().unwrap(), "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let names = ["IMG_1.jpg", "IMG_2.jpg", "other.jpg"];
    let files: Vec<IndexedFile> = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            image::RgbImage::from_pixel(64, 48, image::Rgb([60, 120, 200]))
                .save(dir.join(name))
                .unwrap();
            IndexedFile {
                rel_path: name.to_string(),
                file_name: name.to_string(),
                kind: MediaKind::Image,
                size: 1,
                mtime: 1_000,
                sidecar_mtime: None,
                metadata: MediaMetadata {
                    width: Some(64),
                    height: Some(48),
                    taken_at: Some(1_683_886_500_000 + i as i64),
                    ..Default::default()
                },
                read_error: None,
            }
        })
        .collect();
    std::fs::write(dir.join("IMG_2.xmp"), "<x/>").unwrap();
    let mut writer = IndexWriter::new(conn);
    writer.write(root, &files).unwrap();
    let journal = fixture.dir.path().join("data").join("rename-journal.json");
    let state = fixture.state(writer.into()).with_rename_journal(journal);
    let mut harness = common::harness_with_step(state, 1.0 / 60.0);
    harness.set_size(egui::vec2(1000.0, 700.0));
    settle(&mut harness);
    harness.state_mut().gallery.as_mut().unwrap().select_all();
    harness.run_steps(2);
    (harness, dir)
}

fn id_of(harness: &App, rel_path: &str) -> Option<i64> {
    let db = harness.state().db().unwrap();
    db::media::id_of(&db.lock().unwrap(), 1, rel_path).unwrap()
}

/// Opens the dialog from the selection bar and waits for its preview.
fn open_dialog(harness: &mut App) {
    harness.get_by_role_and_label(Role::Button, "Rename…").click();
    harness.run_steps(2);
    for _ in 0..200 {
        let loaded = harness
            .state()
            .rename
            .dialog
            .as_ref()
            .is_some_and(|dialog| !dialog.rows().is_empty());
        if loaded {
            // The rows can arrive in a frame after it was drawn: draw them before anything asks for them.
            harness.run_steps(2);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        harness.run_steps(1);
    }
    panic!("the preview didn't load");
}

/// The text field after `label`: Find, then Replace with.
fn field(harness: &App, index: usize) -> egui_kittest::Node<'_> {
    harness
        .query_all_by_role(Role::TextInput)
        .nth(index)
        .expect("the dialog's fields")
}

/// REQ-035-01, REQ-035-02, REQ-035-04: _Rename…_ opens the dialog, and the preview follows what is typed.
#[test]
fn req_035_01_req_035_02_req_035_04_opens_for_the_selection_and_previews_the_names() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture);
    open_dialog(&mut harness);
    harness.get_by_label("Rename 3 items");
    harness.get_by_label("0 renamed · 3 unchanged");
    // The search bar's field comes first.
    field(&harness, 1).set_text_value_accesskit("IMG_");
    field(&harness, 2).set_text_value_accesskit("trip ");
    harness.run_steps(2);
    harness.get_by_label("2 renamed · 1 unchanged");
    harness.get_by_label("trip 1.jpg");
    harness.snapshot(common::snapshot_name("rename_dialog"));
}

/// REQ-035-06, REQ-035-07, REQ-035-08: renaming keeps each media item and its sidecar, and _Undo_ takes it back.
#[test]
fn req_035_06_req_035_07_req_035_08_renames_keeps_the_media_items_and_undoes() {
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture);
    let (one, two) = (id_of(&harness, "IMG_1.jpg"), id_of(&harness, "IMG_2.jpg"));
    open_dialog(&mut harness);
    field(&harness, 1).set_text_value_accesskit("IMG_");
    field(&harness, 2).set_text_value_accesskit("{taken:yyyy}-");
    harness.run_steps(2);
    harness.get_by_role_and_label(Role::Button, "Rename 2").click();
    harness.run_steps(1);
    settle(&mut harness);

    assert!(dir.join("2023-1.jpg").is_file() && dir.join("2023-2.jpg").is_file());
    assert!(!dir.join("IMG_1.jpg").exists());
    assert!(dir.join("2023-2.xmp").is_file(), "the sidecar came along");
    assert_eq!(
        (id_of(&harness, "2023-1.jpg"), id_of(&harness, "2023-2.jpg")),
        (one, two)
    );
    assert_eq!(
        harness.state().gallery.as_ref().unwrap().selected_count(),
        3,
        "still selected"
    );
    harness.get_by_label("Renamed 2 files");

    harness.get_by_role_and_label(Role::Button, "Undo").click();
    harness.run_steps(1);
    settle(&mut harness);
    assert!(dir.join("IMG_1.jpg").is_file() && dir.join("IMG_2.xmp").is_file());
    assert_eq!(id_of(&harness, "IMG_1.jpg"), one);
}

/// REQ-035-09: a rename the app was ended in is finished when it starts again.
#[test]
fn req_035_09_finishes_an_interrupted_rename() {
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture);
    let one = id_of(&harness, "IMG_1.jpg");
    // The journal and the index say IMG_1 → a; the file didn't move yet.
    let journal = fixture.dir.path().join("data").join("rename-journal.json");
    let batch = media::rename::Batch {
        root_id: 1,
        library: dir.clone(),
        renames: vec![("IMG_1.jpg".into(), "a.jpg".into())],
    };
    let mut writing = media::rename::Journal::start(&journal, std::slice::from_ref(&batch)).unwrap();
    {
        let db = harness.state().db().unwrap();
        db::rename::rename(&db.lock().unwrap(), 1, &batch.renames).unwrap();
    }
    writing.indexed().unwrap();
    drop(writing);

    harness.state_mut().recover_rename();
    settle(&mut harness);
    assert!(dir.join("a.jpg").is_file() && !dir.join("IMG_1.jpg").exists());
    assert_eq!(id_of(&harness, "a.jpg"), one);
    harness.get_by_label("Finished renaming 1 file");
    assert!(media::rename::Journal::read(&journal).unwrap().unwrap().finished);
}
