//! The world map (spec 044): clusters of the photos with a position, opened in the gallery.

use crate::common;

use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::writer::{IndexWriter, IndexedFile};
use domain::media::{Location, MediaKind, MediaMetadata};
use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::Queryable as _;

type App = egui_kittest::Harness<'static, app::AppState>;

fn photo(rel_path: &str, taken_at: i64, at: Option<(f64, f64)>) -> IndexedFile {
    IndexedFile {
        rel_path: rel_path.into(),
        file_name: rel_path.into(),
        kind: MediaKind::Image,
        size: 1,
        mtime: 1_000,
        sidecar_mtime: None,
        metadata: MediaMetadata {
            width: Some(400),
            height: Some(300),
            taken_at: Some(taken_at),
            location: at.and_then(|(latitude, longitude)| Location::new(latitude, longitude)),
            ..Default::default()
        },
        read_error: None,
    }
}

/// Two photos in Munich, one in Santiago de Chile, one without a position.
fn app(fixture: &Fixture) -> App {
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let mut writer = IndexWriter::new(conn);
    writer
        .write(
            root,
            &[
                photo("munich1.jpg", 1_000, Some((48.137, 11.575))),
                photo("munich2.jpg", 2_000, Some((48.140, 11.560))),
                photo("santiago.jpg", 3_000, Some((-33.45, -70.66))),
                photo("home.jpg", 4_000, None),
            ],
        )
        .unwrap();
    let mut harness = common::harness(fixture.state(writer.into()));
    harness.set_size(egui::vec2(1200.0, 800.0));
    settle(&mut harness);
    harness
}

fn search(harness: &App) -> domain::search::GallerySearch {
    harness.state().gallery.as_ref().unwrap().search().clone()
}

/// REQ-044-01, REQ-044-03: _Map_ in Places shows the photos with a position as clusters named by their count and
/// town; a click shows a cluster's photos in the gallery, with a chip that takes them away again.
#[test]
fn req_044_01_req_044_03_shows_clusters_and_opens_their_photos() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);
    harness.get_by_role_and_label(Role::Button, "Map").click();
    settle(&mut harness);
    assert_eq!(harness.state().page, app::Page::Map);
    harness.get_by_role_and_label(Role::Button, "2 photos near Munich");
    harness.get_by_role_and_label(Role::Button, "1 photo near Santiago");
    harness.snapshot(common::snapshot_name("map"));

    harness
        .get_by_role_and_label(Role::Button, "2 photos near Munich")
        .click();
    settle(&mut harness);
    assert_eq!(harness.state().page, app::Page::Gallery);
    assert_eq!(harness.state().gallery.as_ref().unwrap().count(), Some(2));
    assert_eq!(search(&harness).media.len(), 2);
    harness
        .get_by_role_and_label(Role::Button, "Remove 2 photos from the map")
        .click();
    settle(&mut harness);
    assert!(search(&harness).media.is_empty());
    assert_eq!(harness.state().gallery.as_ref().unwrap().count(), Some(4));
}

/// REQ-044-02: zooming in splits a cluster; the buttons zoom around the map's center.
#[test]
fn req_044_02_zooming_in_splits_clusters() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);
    harness.state_mut().page = app::Page::Map;
    settle(&mut harness);
    let munich = domain::map::project(11.567, 48.138);
    {
        let gallery = harness.state_mut().gallery.as_mut().unwrap();
        gallery.map_view = domain::map::MapView {
            center: munich,
            zoom: 10.0,
        };
    }
    harness.run_steps(2);
    harness.get_by_role_and_label(Role::Button, "2 photos near Munich");
    for _ in 0..3 {
        harness.get_by_role_and_label(Role::Button, "Zoom in").click();
        harness.run_steps(2);
    }
    assert_eq!(harness.state().gallery.as_ref().unwrap().map_view.zoom, 13.0);
    assert_eq!(
        harness.query_all_by_label("1 photo near Munich").count(),
        2,
        "apart up close"
    );
}
