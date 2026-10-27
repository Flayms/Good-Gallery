//! What the app shows while the indexer runs (specs 002 and 006).

mod common;

use common::{Fixture, settle};
use db::libraries::LibraryStatus;
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
