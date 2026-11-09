//! The context menu's rename, rating, rotate and delete items (REQ-009-21 … REQ-009-24).

use crate::common;

use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::writer::{IndexWriter, IndexedFile};
use domain::media::{MediaKind, MediaMetadata};
use eframe::egui::{self, Modifiers, PointerButton, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use std::path::{Path, PathBuf};
use std::process::Command;

type App = Harness<'static, app::AppState>;

const NAMES: [&str; 4] = ["a.jpg", "b.jpg", "c.gif", "d.mp4"];

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

/// Two JPEGs (8 × 4, left half red), a GIF and a video, newest first; with exiftool and renaming.
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
            let kind = if name.ends_with(".mp4") {
                MediaKind::Video
            } else {
                image::RgbImage::from_fn(8, 4, |x, _| {
                    image::Rgb(if x < 4 { [220, 30, 30] } else { [30, 30, 220] })
                })
                .save(dir.join(name))
                .unwrap();
                MediaKind::Image
            };
            IndexedFile {
                rel_path: name.to_string(),
                file_name: name.to_string(),
                kind,
                size: 1,
                mtime: 1_000,
                sidecar_mtime: None,
                metadata: MediaMetadata {
                    width: Some(8),
                    height: Some(4),
                    taken_at: Some(2_000_000 - i as i64),
                    ..Default::default()
                },
                read_error: None,
            }
        })
        .collect();
    let mut writer = IndexWriter::new(conn);
    writer.write(root, &files).unwrap();
    let config = media::edit::exiftool_config(&fixture.dir.path().join("data")).unwrap();
    let journal = fixture.dir.path().join("data").join("rename-journal.json");
    let state = fixture
        .state(writer.into())
        .with_edit_tools(media::edit::Tools {
            exiftool: exiftool.to_owned(),
            config,
        })
        .with_rename_journal(journal)
        // Deleted files go for good: the user's Recycle Bin stays as it was.
        .with_trash(|path| std::fs::remove_file(path));
    let mut harness = common::harness_with_step(state, 1.0 / 60.0);
    harness.set_size(egui::vec2(1000.0, 700.0));
    settle(&mut harness);
    (harness, dir)
}

fn right_click_at(harness: &mut App, pos: egui::Pos2) {
    harness.hover_at(pos);
    harness.run_steps(1);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        harness.run_steps(1);
    }
    harness.run_steps(2);
}

fn tile(harness: &App, name: &str) -> egui::Pos2 {
    let labels = [name.to_string(), format!("{name}, selected"), format!("{name}, video")];
    labels
        .iter()
        .find_map(|label| harness.query_by_role_and_label(Role::Image, label))
        .unwrap_or_else(|| panic!("no tile {name}"))
        .rect()
        .center()
}

/// The item `label` of the menu opened at `at`, which opens below it: the selection bar has a _Rename…_ too.
fn menu_item<'a>(harness: &'a App, at: egui::Pos2, label: &'a str) -> egui_kittest::Node<'a> {
    harness
        .get_all_by_label(label)
        .filter(|node| node.rect().top() >= at.y)
        .min_by(|a, b| {
            let distance = |node: &egui_kittest::Node<'_>| (node.rect().center() - at).length();
            distance(a).total_cmp(&distance(b))
        })
        .unwrap()
}

/// Chooses `stars` (0 removes the rating) under _Rating_.
fn rate(harness: &mut App, label: &str) {
    harness.get_by_label("Rating").hover();
    harness.run_steps(3);
    harness.get_by_label(label).click();
    settle(harness);
}

fn read(exiftool: &Path, file: &Path, tag: &str) -> Option<i64> {
    let output = Command::new(exiftool)
        .args(["-j", "-n", &format!("-{tag}")])
        .arg(file)
        .output()
        .unwrap();
    let read: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    read[0][tag.rsplit(':').next().unwrap()].as_i64()
}

fn rename_ids(harness: &App) -> Option<usize> {
    harness.state().rename.dialog.as_ref().map(|dialog| dialog.count())
}

/// REQ-009-21, REQ-009-22: on a selected item the menu rates, rotates and renames the whole selection; on another
/// item, that item alone.
#[test]
fn req_009_21_the_menu_renames_rates_and_rotates_the_selection() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, &exiftool);
    let gallery = harness.state_mut().gallery.as_mut().unwrap();
    gallery.toggle_selected_at(0);
    gallery.toggle_selected_at(1);
    harness.run_steps(2);

    let at = tile(&harness, "a.jpg");
    right_click_at(&mut harness, at);
    rate(&mut harness, "3 stars");
    for name in ["a.jpg", "b.jpg"] {
        assert_eq!(read(&exiftool, &dir.join(name), "XMP-xmp:Rating"), Some(3), "{name}");
    }
    assert_eq!(read(&exiftool, &dir.join("c.gif"), "XMP-xmp:Rating"), None);

    let at = tile(&harness, "b.jpg");
    right_click_at(&mut harness, at);
    harness.get_by_label("Rotate right").click();
    settle(&mut harness);
    for name in ["a.jpg", "b.jpg"] {
        assert_eq!(read(&exiftool, &dir.join(name), "Orientation"), Some(6), "{name}");
    }
    assert!(
        fixture
            .scans
            .paths
            .lock()
            .unwrap()
            .iter()
            .any(|(_, paths)| paths.contains(&"a.jpg".to_string())),
        "read again, with a new thumbnail"
    );

    let at = tile(&harness, "a.jpg");
    right_click_at(&mut harness, at);
    menu_item(&harness, at, "Rename…").click();
    settle(&mut harness);
    assert_eq!(rename_ids(&harness), Some(2));
    harness.state_mut().rename.dialog = None;
    harness.run_steps(2);

    // Not in the selection: that item alone.
    let at = tile(&harness, "c.gif");
    right_click_at(&mut harness, at);
    rate(&mut harness, "1 star");
    assert_eq!(read(&exiftool, &dir.join("c.gif"), "XMP-xmp:Rating"), Some(1));
    assert_eq!(read(&exiftool, &dir.join("a.jpg"), "XMP-xmp:Rating"), Some(3));
    let at = tile(&harness, "c.gif");
    right_click_at(&mut harness, at);
    menu_item(&harness, at, "Rename…").click();
    settle(&mut harness);
    assert_eq!(rename_ids(&harness), Some(1));
}

/// REQ-009-21: a video or a GIF can't be rotated.
#[test]
fn req_009_21_rotate_is_off_for_videos_and_gifs() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, &exiftool);
    for name in ["c.gif", "d.mp4"] {
        let at = tile(&harness, name);
        right_click_at(&mut harness, at);
        for item in ["Rotate left", "Rotate right"] {
            assert!(
                harness.get_by_label(item).accesskit_node().is_disabled(),
                "{name}: {item}"
            );
        }
        harness.key_press(egui::Key::Escape);
        harness.run_steps(2);
    }
    let at = tile(&harness, "a.jpg");
    right_click_at(&mut harness, at);
    assert!(!harness.get_by_label("Rotate left").accesskit_node().is_disabled());
    harness.snapshot(common::snapshot_name("context_menu"));
}

/// REQ-009-21: the viewer's menu rates and rotates the photo it shows, which is then shown turned.
#[test]
fn req_009_21_the_viewers_menu_rates_and_rotates() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, &exiftool);
    common::open_item(&mut harness, "a.jpg");
    settle(&mut harness);
    let center = egui::pos2(500.0, 350.0);

    right_click_at(&mut harness, center);
    rate(&mut harness, "5 stars");
    assert_eq!(read(&exiftool, &dir.join("a.jpg"), "XMP-xmp:Rating"), Some(5));

    right_click_at(&mut harness, center);
    harness.get_by_label("Rotate left").click();
    settle(&mut harness);
    assert_eq!(read(&exiftool, &dir.join("a.jpg"), "Orientation"), Some(8));
    assert!(harness.state().viewer.is_some(), "the viewer stays");
}

fn ids(harness: &App) -> Vec<i64> {
    let gallery = harness.state().gallery.as_ref().unwrap();
    (0..gallery.count().unwrap_or(0))
        .filter_map(|at| gallery.id_at(at))
        .collect()
}

fn is_indexed(harness: &App, name: &str) -> bool {
    let db = harness.state().db().unwrap();
    let conn = db.lock().unwrap();
    conn.query_row("SELECT count(*) FROM media WHERE rel_path = ?1", [name], |row| {
        row.get::<_, i64>(0)
    })
    .unwrap()
        > 0
}

/// Answers the delete dialog, which must ask `title`.
fn answer(harness: &mut App, title: &str, button: &str) {
    harness.run_steps(2);
    assert!(harness.query_by_label(title).is_some(), "asks {title}");
    harness.get_by_label(button).click();
    settle(harness);
}

/// REQ-009-23, REQ-009-24: _Delete…_ on a selected item asks, then deletes the whole selection with its sidecars;
/// the items leave the gallery, the selection and the index, and their libraries are scanned again.
#[test]
fn req_009_23_req_009_24_the_menu_deletes_the_selection_after_asking() {
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, &exiftool().unwrap_or_default());
    std::fs::write(dir.join("a.jpg.xmp"), "<x/>").unwrap();
    let gallery = harness.state_mut().gallery.as_mut().unwrap();
    gallery.toggle_selected_at(0);
    gallery.toggle_selected_at(1);
    harness.run_steps(2);

    let at = tile(&harness, "a.jpg");
    right_click_at(&mut harness, at);
    menu_item(&harness, at, "Delete…").click();
    answer(&mut harness, "Delete 2 items?", "Cancel");
    assert!(dir.join("a.jpg").exists() && dir.join("b.jpg").exists(), "cancelled");

    let at = tile(&harness, "b.jpg");
    right_click_at(&mut harness, at);
    menu_item(&harness, at, "Delete…").click();
    answer(&mut harness, "Delete 2 items?", "Delete");
    for name in ["a.jpg", "a.jpg.xmp", "b.jpg"] {
        assert!(!dir.join(name).exists(), "{name} deleted");
    }
    assert!(dir.join("c.gif").exists());
    assert!(!is_indexed(&harness, "a.jpg") && !is_indexed(&harness, "b.jpg"));
    assert_eq!(harness.state().gallery.as_ref().unwrap().count(), Some(2));
    assert_eq!(harness.state().gallery.as_ref().unwrap().selected_count(), 0);
    assert!(
        fixture
            .scans
            .paths
            .lock()
            .unwrap()
            .iter()
            .any(|(_, paths)| paths.contains(&"a.jpg".to_string())),
        "scanned again"
    );
}

/// REQ-009-23: Delete deletes the selection; the menu on an item outside it deletes that item alone.
#[test]
fn req_009_23_the_delete_key_and_an_item_outside_the_selection() {
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, &exiftool().unwrap_or_default());
    harness.state_mut().gallery.as_mut().unwrap().toggle_selected_at(2);
    harness.run_steps(2);
    harness.key_press(egui::Key::Delete);
    answer(&mut harness, "Delete 1 item?", "Delete");
    assert!(!dir.join("c.gif").exists());

    harness.state_mut().gallery.as_mut().unwrap().toggle_selected_at(0);
    harness.run_steps(2);
    let at = tile(&harness, "d.mp4");
    right_click_at(&mut harness, at);
    menu_item(&harness, at, "Delete…").click();
    answer(&mut harness, "Delete 1 item?", "Delete");
    assert!(!dir.join("d.mp4").exists());
    assert!(dir.join("a.jpg").exists(), "the selection stays");
    assert_eq!(harness.state().gallery.as_ref().unwrap().count(), Some(2));
}

/// REQ-009-24: the viewer moves on from a deleted item to the one taking its place, back at the end, and closes
/// when none is left.
#[test]
fn req_009_24_the_viewer_moves_on_from_a_deleted_item() {
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, &exiftool().unwrap_or_default());
    let [_, b, c, _] = ids(&harness)[..] else { panic!() };
    common::open_item(&mut harness, "a.jpg");
    settle(&mut harness);

    harness.key_press(egui::Key::Delete);
    answer(&mut harness, "Delete 1 item?", "Delete");
    assert!(!dir.join("a.jpg").exists());
    assert_eq!(harness.state().viewer.as_ref().map(app::viewer::Viewer::id), Some(b));

    // From the menu, at the end: the one before.
    harness.key_press(egui::Key::ArrowRight);
    harness.key_press(egui::Key::ArrowRight);
    settle(&mut harness);
    let center = egui::pos2(500.0, 350.0);
    right_click_at(&mut harness, center);
    harness.get_by_label("Delete…").click();
    answer(&mut harness, "Delete 1 item?", "Delete");
    assert!(!dir.join("d.mp4").exists());
    assert_eq!(harness.state().viewer.as_ref().map(app::viewer::Viewer::id), Some(c));

    for _ in 0..2 {
        harness.key_press(egui::Key::Delete);
        answer(&mut harness, "Delete 1 item?", "Delete");
    }
    harness.run_steps(60);
    assert!(harness.state().viewer.is_none(), "closed with nothing left");
    assert!(ids(&harness).is_empty());
}

/// Steps frames from a click on _Copy_ until one puts an image on the clipboard; its size and first pixel. Every frame
/// is looked at: the decode may finish while the click is still being handled, and the next frame then copies it.
fn copied_image(harness: &mut App) -> Option<([usize; 2], egui::Color32)> {
    for _ in 0..10 {
        harness.run_steps(1);
        let copied = harness
            .output()
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyImage(image) => Some((image.size, image.pixels[0])),
                _ => None,
            });
        if copied.is_some() {
            return copied;
        }
        harness.state_mut().wait_for_jobs();
    }
    None
}

/// REQ-009-25: _Copy_ puts a photo on the clipboard from its tile and from the viewer; a video has no _Copy_.
#[test]
fn req_009_25_copies_a_photo_from_its_tile_and_the_viewer() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, &exiftool);
    let at = tile(&harness, "d.mp4");
    right_click_at(&mut harness, at);
    assert!(harness.query_by_label("Copy").is_none(), "no Copy for a video");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);

    let at = tile(&harness, "a.jpg");
    right_click_at(&mut harness, at);
    menu_item(&harness, at, "Copy").click();
    let (size, first) = copied_image(&mut harness).expect("a.jpg was copied");
    assert_eq!(size, [8, 4]);
    assert!(first.r() > 150 && first.b() < 100, "the left half is red: {first:?}");
    assert!(
        harness
            .state()
            .toasts
            .current()
            .iter()
            .any(|toast| toast.text == "Copied"),
        "a notice says so"
    );

    common::open_item(&mut harness, "b.jpg");
    settle(&mut harness);
    let center = harness.ctx.content_rect().center();
    right_click_at(&mut harness, center);
    harness.get_by_label("Copy").click();
    assert_eq!(copied_image(&mut harness).map(|(size, _)| size), Some([8, 4]));
}
