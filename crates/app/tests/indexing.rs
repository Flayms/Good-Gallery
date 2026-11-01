//! What the app shows while the indexer runs (specs 002 and 006).

mod common;

use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::writer::{IndexWriter, IndexedFile};
use domain::media::{MediaKind, MediaMetadata};
use egui_kittest::kittest::Queryable as _;
use indexer::{CurrentScan, IndexerEvent, IndexerStatus, ScanKind, ScanResult};
use std::sync::mpsc::channel;

fn with_library() -> (Fixture, db::rusqlite::Connection) {
    let conn = db::open_in_memory().unwrap();
    db::libraries::insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Online).unwrap();
    (Fixture::new(), conn)
}

#[test]
fn req_002_08_shows_which_library_is_scanned_with_counts_and_waiting_scans() {
    let (fixture, conn) = with_library();
    let (tx, rx) = channel();
    let state = fixture.state(conn).with_indexer(fixture.scans.clone(), rx);
    let library = state.libraries[0].id;
    let mut harness = common::harness(state);
    let status = |kind| IndexerStatus {
        current: Some(CurrentScan {
            library,
            kind,
            scanned: 1204,
            indexed: 310,
        }),
        queue: vec![7],
    };

    tx.send(IndexerEvent::Status(status(ScanKind::Full))).unwrap();
    settle(&mut harness);
    harness.get_by_label("Indexing Photos");
    harness.get_by_label("1,204 files · 310 updated · 1 queued");

    tx.send(IndexerEvent::Status(status(ScanKind::Quick))).unwrap();
    settle(&mut harness);
    harness.get_by_label("Checking Photos");

    tx.send(IndexerEvent::Status(IndexerStatus::default())).unwrap();
    settle(&mut harness);
    assert!(harness.query_by_label("Checking Photos").is_none());
}

#[test]
fn req_006_11_refreshes_after_the_first_written_files_and_when_the_scan_ends() {
    let fixture = Fixture::new();
    let path = fixture.dir.path().join("gallery.db");
    let (tx, rx) = channel();
    let mut state = fixture
        .state(db::open(&path).unwrap())
        .with_indexer(fixture.scans.clone(), rx);
    // Another connection plays the indexer, which writes on its own connection.
    let indexer = db::open(&path).unwrap();
    let write = |n: i64| {
        let path = format!("D:\\{n}");
        db::libraries::insert(&indexer, &path, "L", LibraryStatus::Online).unwrap();
    };

    write(1);
    tx.send(IndexerEvent::Written(1)).unwrap();
    state.poll();
    assert_eq!(state.libraries.len(), 1, "first written files show right away");

    write(2);
    tx.send(IndexerEvent::Written(1)).unwrap();
    state.poll();
    assert_eq!(state.libraries.len(), 1, "then at most every 5 seconds");

    tx.send(IndexerEvent::Done(1, ScanResult::Completed)).unwrap();
    state.poll();
    assert_eq!(state.libraries.len(), 2, "and once more when the scan ends");
}

/// A library _Photos_ with `trip/a.jpg` and `b.jpg` unreadable (unless not `unreadable`), and `c.jpg` read.
fn with_unreadable_files(unreadable: bool) -> (Fixture, db::rusqlite::Connection, i64) {
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let file = |rel_path: &str, read_error: Option<&str>| IndexedFile {
        rel_path: rel_path.into(),
        file_name: rel_path.rsplit('/').next().unwrap().into(),
        kind: MediaKind::Image,
        size: 1,
        mtime: 1_000,
        sidecar_mtime: None,
        metadata: MediaMetadata::default(),
        read_error: read_error.filter(|_| unreadable).map(String::from),
    };
    let mut writer = IndexWriter::new(conn);
    writer
        .write(
            root,
            &[
                file("trip/a.jpg", Some("exiftool didn't answer within 60s")),
                file("b.jpg", Some("exiftool: broken")),
                file("c.jpg", None),
            ],
        )
        .unwrap();
    (Fixture::new(), writer.into(), root)
}

#[test]
fn req_002_16_lists_unreadable_files_with_their_library_folder_and_reason() {
    let (fixture, conn, _) = with_unreadable_files(true);
    let mut harness = fixture.harness(conn);
    settle(&mut harness);
    // The gallery's tiles are named after their files too.
    let named =
        |harness: &egui_kittest::Harness<'_, app::AppState>, name: &str| harness.query_all_by_label(name).count();
    let tiles = (named(&harness, "a.jpg"), named(&harness, "b.jpg"));

    harness.get_by_label("2 files couldn't be read").click();
    settle(&mut harness);

    assert_eq!(
        (named(&harness, "a.jpg"), named(&harness, "b.jpg")),
        (tiles.0 + 1, tiles.1 + 1)
    );
    assert_eq!(named(&harness, "c.jpg"), 1, "only its tile");
    harness.get_by_label("Photos › trip");
    harness.get_by_label("exiftool didn't answer within 60s");
    harness.get_by_label("exiftool: broken");
    harness.get_by_label("Read a.jpg again");
    harness.get_by_label("Read b.jpg again");
    harness.get_by_label("Read all again");
    harness.snapshot(common::snapshot_name("unreadable_files"));
}

#[test]
fn req_002_16_shows_nothing_without_unreadable_files() {
    let (fixture, conn, _) = with_unreadable_files(false);
    let mut harness = fixture.harness(conn);
    settle(&mut harness);

    assert!(harness.query_by_label_contains("couldn't be read").is_none());
}

#[test]
fn req_002_17_reads_files_again_and_drops_those_read_successfully() {
    let (fixture, conn, root) = with_unreadable_files(true);
    let (tx, rx) = channel();
    let state = fixture.state(conn).with_indexer(fixture.scans.clone(), rx);
    let mut harness = common::harness(state);
    settle(&mut harness);
    harness.get_by_label("2 files couldn't be read").click();
    settle(&mut harness);

    harness.get_by_label("Read a.jpg again").click();
    settle(&mut harness);
    assert_eq!(
        fixture.scans.paths.lock().unwrap().as_slice(),
        [(root, vec!["trip/a.jpg".to_string()])]
    );
    harness.get_by_label("Reading a.jpg…");

    harness.get_by_label("Read all again").click();
    settle(&mut harness);
    assert_eq!(
        fixture.scans.paths.lock().unwrap().last(),
        Some(&(root, vec!["b.jpg".to_string(), "trip/a.jpg".to_string()]))
    );

    // The indexer reads a.jpg this time; b.jpg fails again.
    let db = harness.state().db().unwrap();
    db.lock()
        .unwrap()
        .execute("UPDATE media SET meta_error = NULL WHERE rel_path = 'trip/a.jpg'", [])
        .unwrap();
    tx.send(IndexerEvent::Done(root, ScanResult::Completed)).unwrap();
    settle(&mut harness);

    harness.get_by_label("1 file couldn't be read");
    assert!(harness.query_by_label_contains("a.jpg again").is_none());
    assert!(harness.query_by_label("Reading a.jpg…").is_none());
    harness.get_by_label("Read b.jpg again");
}
