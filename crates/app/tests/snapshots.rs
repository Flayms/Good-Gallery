//! Snapshot tests of whole screens. `UPDATE_SNAPSHOTS=1 cargo test -p app` rewrites `tests/snapshots/*.png`.

mod common;

use app::Page;
use common::{Fixture, settle};
use egui_kittest::kittest::Queryable as _;

/// REQ-006-10: without libraries, the gallery invites the user to add one.
#[test]
fn req_006_10_first_run_shows_the_no_libraries_empty_state() {
    let fixture = Fixture::new();
    let mut harness = fixture.harness(db::open_in_memory().unwrap());
    settle(&mut harness);

    harness.get_by_label("No libraries yet");
    harness.snapshot("first_run");
    harness.get_by_label("Add a library").click();
    settle(&mut harness);
    assert_eq!(harness.state().page, Page::Settings);
}

/// REQ-006-10: with libraries but no media items, the gallery says so.
#[test]
fn req_006_10_shows_no_media_found_while_nothing_is_indexed() {
    let fixture = Fixture::new();
    let conn = db::open_in_memory().unwrap();
    db::libraries::insert(&conn, "D:\\Photos", "Photos", db::libraries::LibraryStatus::Online).unwrap();
    let mut harness = fixture.harness(conn);
    settle(&mut harness);

    harness.get_by_label("No media found");
}
