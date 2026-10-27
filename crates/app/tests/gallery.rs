//! The gallery (spec 006) on an index with media items; without a GPU texture streamer, tiles show placeholders.

mod common;

use app::gallery::Gallery;
use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::rusqlite::Connection;
use db::writer::{IndexWriter, IndexedFile};
use domain::media::{MediaKind, MediaMetadata};
use eframe::egui;
use egui_kittest::kittest::Queryable as _;
use std::sync::{Arc, Mutex};

fn file(name: &str, size: (i64, i64), taken_at: i64, duration: Option<f64>) -> IndexedFile {
    IndexedFile {
        rel_path: name.into(),
        file_name: name.into(),
        kind: if duration.is_some() {
            MediaKind::Video
        } else {
            MediaKind::Image
        },
        size: 1,
        mtime: 1_000,
        sidecar_mtime: None,
        metadata: MediaMetadata {
            width: Some(size.0),
            height: Some(size.1),
            taken_at: Some(taken_at),
            duration,
            ..Default::default()
        },
    }
}

/// An index with one library holding `files`.
fn index(files: &[IndexedFile]) -> Connection {
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let mut writer = IndexWriter::new(conn);
    writer.write(root, files).unwrap();
    writer.into()
}

/// `count` photos named `0.jpg`, `1.jpg`, … newest first, of varying shapes.
fn photos(count: usize) -> Vec<IndexedFile> {
    (0..count)
        .map(|i| {
            file(
                &format!("{i}.jpg"),
                (400, [300, 400, 600][i % 3]),
                2_000_000 - i as i64,
                None,
            )
        })
        .collect()
}

fn gallery<'a>(harness: &'a egui_kittest::Harness<'_, app::AppState>) -> &'a Gallery {
    harness.state().gallery.as_ref().unwrap()
}

fn wheel(harness: &egui_kittest::Harness<'_, app::AppState>, lines: f32, ctrl: bool) {
    harness.hover_at(egui::pos2(450.0, 300.0));
    let modifiers = if ctrl {
        egui::Modifiers::CTRL
    } else {
        egui::Modifiers::NONE
    };
    harness.event_modifiers(
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, lines),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers,
        },
        modifiers,
    );
}

/// REQ-006-08, REQ-006-09: every media item gets a tile at its position; videos are named with their duration.
#[test]
fn req_006_08_req_006_09_shows_a_tile_per_media_item_and_video_durations() {
    let fixture = Fixture::new();
    let conn = index(&[
        file("beach.jpg", (400, 300), 3_000, None),
        file("clip.mp4", (1920, 1080), 2_000, Some(65.0)),
        file("portrait.jpg", (300, 400), 1_000, None),
    ]);
    let mut harness = fixture.harness(conn);
    settle(&mut harness);

    assert_eq!(gallery(&harness).count(), Some(3));
    let beach = harness.get_by_label("beach.jpg").rect();
    let clip = harness.get_by_label("clip.mp4, video, 1:05").rect();
    let portrait = harness.get_by_label("portrait.jpg").rect();
    // Masonry, 5 columns, newest first: side by side in the first row, each at its aspect ratio.
    assert!(beach.left() < clip.left() && clip.left() < portrait.left());
    assert!((beach.height() / beach.width() - 0.75).abs() < 0.02);
    assert!((portrait.height() / portrait.width() - 4.0 / 3.0).abs() < 0.02);
    harness.snapshot("gallery");
}

#[test]
fn req_006_06_ctrl_wheel_zooms_in_steps_and_remembers_the_level() {
    let fixture = Fixture::new();
    let mut harness = fixture.harness(index(&photos(20)));
    settle(&mut harness);
    assert_eq!(gallery(&harness).columns, 5);

    // Scrolling down zooms out (more columns), scrolling up zooms in.
    wheel(&harness, -1.0, true);
    harness.run_steps(2);
    assert_eq!(gallery(&harness).columns, 6);
    wheel(&harness, 3.0, true);
    harness.run_steps(2);
    assert_eq!(gallery(&harness).columns, 3);

    let db = harness.state().db().unwrap();
    let again = Gallery::new(db.clone(), db, None);
    assert_eq!(again.columns, 3, "the zoom level is remembered across sessions");
}

#[test]
fn req_006_07_keeps_the_first_visible_item_in_view_when_zooming() {
    let fixture = Fixture::new();
    let mut harness = fixture.harness(index(&photos(300)));
    settle(&mut harness);
    wheel(&harness, -20.0, false);
    settle(&mut harness);
    let first = gallery(&harness).first_visible();
    assert!(first > 10, "scrolled down to item {first}");

    wheel(&harness, -2.0, true);
    settle(&mut harness);

    assert_eq!(gallery(&harness).columns, 7);
    // Right below the toolbar, where it was (another item may start at the same height).
    let tile = harness.get_by_label(&format!("{first}.jpg")).rect();
    assert!((tile.top() - gallery(&harness).content_top()).abs() < 2.0, "{tile:?}");
}

/// REQ-006-10: gray tiles while the layout loads, and the error when it can't be loaded.
#[test]
fn req_006_10_shows_skeleton_tiles_while_loading_and_the_error_when_loading_fails() {
    let fixture = Fixture::new();
    let reader = Arc::new(Mutex::new(index(&photos(3))));
    let state = fixture.state(index(&photos(3)));
    let settings = state.db().unwrap();
    // The loader waits for the connection while the test holds it.
    let held = reader.lock().unwrap();
    let mut harness = common::harness(state.with_gallery(Gallery::new(reader.clone(), settings.clone(), None)));
    harness.run_steps(3);
    assert!(gallery(&harness).count().is_none());
    harness.snapshot("gallery_loading");
    drop(held);

    let broken = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
    harness.state_mut().gallery = Some(Gallery::new(broken, settings, None));
    settle(&mut harness);
    harness.get_by_label("Couldn't load media");
}

/// REQ-006-11: media items written by a running scan show up in the gallery.
#[test]
fn req_006_11_shows_media_items_written_while_indexing() {
    let fixture = Fixture::new();
    let path = fixture.dir.path().join("gallery.db");
    let conn = db::open(&path).unwrap();
    let root = db::libraries::insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let (tx, rx) = std::sync::mpsc::channel();
    let mut state = fixture.state(conn).with_indexer(fixture.scans.clone(), rx);
    state.wait_for_jobs();
    assert_eq!(state.gallery.as_ref().unwrap().count(), Some(0));

    let mut writer = IndexWriter::new(db::open(&path).unwrap());
    writer.write(root, &photos(2)).unwrap();
    tx.send(indexer::IndexerEvent::Written(root)).unwrap();
    state.poll();
    state.wait_for_jobs();

    assert_eq!(state.gallery.as_ref().unwrap().count(), Some(2));
}

/// REQ-006-07, REQ-006-11: newer media items written above the view while indexing don't move what the user looks
/// at, and the tiles at their new positions get their data.
#[test]
fn req_006_07_req_006_11_keeps_the_first_visible_item_in_view_when_items_are_added_above() {
    let fixture = Fixture::new();
    let mut harness = fixture.harness(index(&photos(300)));
    settle(&mut harness);
    wheel(&harness, -20.0, false);
    settle(&mut harness);
    let first = gallery(&harness).first_visible();
    let name = format!("{first}.jpg");

    {
        let db = harness.state().db().unwrap();
        let conn = db.lock().unwrap();
        for i in 0..37 {
            conn.execute(
                "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime, width, height, taken_at)
                 VALUES (1, ?1, ?1, 'image', 1, 1000, 400, 300, ?2)",
                (format!("new{i}.jpg"), 3_000_000 + i),
            )
            .unwrap();
        }
    }
    harness.state_mut().gallery.as_mut().unwrap().reload();
    settle(&mut harness);
    settle(&mut harness);

    assert_eq!(gallery(&harness).count(), Some(337));
    let tile = harness.get_by_label(&name).rect();
    assert!((tile.top() - gallery(&harness).content_top()).abs() < 2.0, "{tile:?}");
}
