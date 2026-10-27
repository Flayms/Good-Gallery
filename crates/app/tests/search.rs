//! Search bar, filters, sort and sidebar (specs 007, 008) on an index with tagged, rated media items.

mod common;

use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::rusqlite::Connection;
use db::writer::{IndexWriter, IndexedFile};
use domain::media::{MediaKind, MediaMetadata};
use domain::search::SortBy;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;

type App = Harness<'static, app::AppState>;

fn file(rel_path: &str, kind: MediaKind, taken_at: i64, rating: Option<i64>, tags: &[&[&str]]) -> IndexedFile {
    IndexedFile {
        rel_path: rel_path.into(),
        file_name: rel_path.rsplit('/').next().unwrap().into(),
        kind,
        size: 1,
        mtime: 1_000,
        sidecar_mtime: None,
        metadata: MediaMetadata {
            width: Some(400),
            height: Some(300),
            taken_at: Some(taken_at),
            rating,
            tags: tags.iter().map(|p| p.iter().map(|s| s.to_string()).collect()).collect(),
            ..Default::default()
        },
    }
}

/// Beach (3), Berlin (1); People › Anna; Places › Italy › Rome; ratings 5 and 3; one video; two folders.
fn index() -> Connection {
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    db::libraries::insert(&conn, "E:\\Away", "Away", LibraryStatus::Offline).unwrap();
    let mut writer = IndexWriter::new(conn);
    writer
        .write(
            root,
            &[
                file(
                    "2021/beach.jpg",
                    MediaKind::Image,
                    5_000,
                    Some(5),
                    &[&["Beach"], &["People", "Anna"]],
                ),
                file("2021/IMG_dunes.jpg", MediaKind::Image, 4_000, Some(3), &[&["Beach"]]),
                file(
                    "trip/berlin.jpg",
                    MediaKind::Image,
                    3_000,
                    None,
                    &[&["Berlin"], &["Places", "Italy", "Rome"]],
                ),
                file("trip/clip.mp4", MediaKind::Video, 2_000, None, &[&["Beach"]]),
            ],
        )
        .unwrap();
    writer.into()
}

fn app(fixture: &Fixture) -> App {
    let mut harness = fixture.harness(index());
    settle(&mut harness);
    harness
}

fn search(harness: &App) -> domain::search::GallerySearch {
    harness.state().gallery.as_ref().unwrap().search().clone()
}

fn count(harness: &App) -> Option<usize> {
    harness.state().gallery.as_ref().unwrap().count()
}

fn type_into_search(harness: &mut App, text: &str) {
    harness
        .get_by_role_and_label(Role::TextInput, app::search_bar::LABEL)
        .click();
    harness.run_steps(1);
    harness
        .get_by_role_and_label(Role::TextInput, app::search_bar::LABEL)
        .type_text(text);
    settle(harness);
}

#[test]
fn req_007_04_req_007_05_suggests_tags_and_adds_them_as_included_or_excluded() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);

    type_into_search(&mut harness, "be");
    // Most used first, with counts.
    let beach = harness.get_by_role_and_label(Role::ListItem, "Beach").rect();
    let berlin = harness.get_by_role_and_label(Role::ListItem, "Berlin").rect();
    assert!(beach.top() < berlin.top());
    harness.snapshot("search_suggestions");
    harness.key_press(egui::Key::Enter);
    settle(&mut harness);
    assert_eq!(search(&harness).tags, ["Beach"]);
    assert_eq!(count(&harness), Some(3));

    // A leading `-` excludes; the suggestions stay open for the next tag.
    harness
        .get_by_role_and_label(Role::TextInput, app::search_bar::LABEL)
        .type_text("-ber");
    settle(&mut harness);
    harness.get_by_role_and_label(Role::ListItem, "Berlin").click();
    settle(&mut harness);
    assert_eq!(
        (search(&harness).tags, search(&harness).exclude),
        (vec!["Beach".into()], vec!["Berlin".into()])
    );
    assert!(
        harness.query_by_role_and_label(Role::ListItem, "Beach").is_none(),
        "tags in the search aren't suggested"
    );

    // A chip switches between included and excluded; × removes it; Backspace removes the last tag.
    harness.get_by_label("Beach, included").click();
    settle(&mut harness);
    assert_eq!(search(&harness).exclude, ["Berlin", "Beach"]);
    harness.get_by_label("Remove Berlin").click();
    settle(&mut harness);
    assert_eq!(search(&harness).exclude, ["Beach"]);
    harness
        .get_by_role_and_label(Role::TextInput, app::search_bar::LABEL)
        .click();
    harness.run_steps(1);
    harness.key_press(egui::Key::Backspace);
    settle(&mut harness);
    assert!(search(&harness).exclude.is_empty());
    assert_eq!(count(&harness), Some(4));
}

#[test]
fn req_007_06_filters_by_file_name_from_the_search_bar() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);

    type_into_search(&mut harness, "img");
    harness
        .get_by_role_and_label(Role::ListItem, "File name contains “img”")
        .click();
    settle(&mut harness);
    assert_eq!(search(&harness).name.as_deref(), Some("img"));
    assert_eq!(count(&harness), Some(1));

    harness.get_by_label("Remove file name filter img").click();
    settle(&mut harness);
    assert_eq!(search(&harness).name, None);
}

#[test]
fn req_007_07_req_007_09_req_013_08_filters_by_kind_and_rating_and_counts_active_filters() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);

    harness.get_by_label("Filters").click();
    harness.run_steps(2);
    harness.get_by_role_and_label(Role::Button, "Videos").click();
    settle(&mut harness);
    assert_eq!(search(&harness).kind, Some(MediaKind::Video));
    assert_eq!(count(&harness), Some(1));
    harness.snapshot("filters_menu");

    harness.get_by_role_and_label(Role::Button, "All").click();
    // The sidebar has rating rows of the same names; the menu's come last.
    harness
        .query_all_by_role_and_label(Role::Button, "5 stars")
        .last()
        .unwrap()
        .click();
    harness
        .query_all_by_role_and_label(Role::Button, "Unrated")
        .last()
        .unwrap()
        .click();
    settle(&mut harness);
    assert_eq!(search(&harness).ratings, [0, 5]);
    assert_eq!(count(&harness), Some(3));
    harness.get_by_role_and_label(Role::Label, "Unrated and 5 stars");
    harness.get_by_label("Filters, 1 active");

    harness.get_by_role_and_label(Role::Button, "Clear filters").click();
    settle(&mut harness);
    assert_eq!(search(&harness).active_filter_count(), 0);
}

#[test]
fn req_007_08_sets_a_date_range_from_the_filters() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);

    harness.get_by_label("Filters").click();
    harness.run_steps(2);
    harness.get_by_label("Set from day").click();
    settle(&mut harness);
    let from = search(&harness).from.expect("a start day");
    assert_eq!(from, chrono::Local::now().date_naive());
    harness.get_by_label(&format!("From {}", from.format("%-d %b %Y")));
    harness.get_by_role_and_label(Role::Button, "Clear date range").click();
    settle(&mut harness);
    assert_eq!(search(&harness).from, None);
}

#[test]
fn req_007_12_sorts_from_the_view_menu() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);

    harness.get_by_label("View options").click();
    harness.run_steps(2);
    harness.get_by_role_and_label(Role::Button, "Descending").click();
    settle(&mut harness);
    assert!(!search(&harness).descending);
    harness.get_by_role_and_label(Role::Button, "Name").click();
    settle(&mut harness);
    assert_eq!(search(&harness).sort_by, SortBy::Name);
    // REQ-016-04: grouping by day only for date sorts.
    harness.get_by_label("Only when sorted by date");
}

#[test]
fn req_008_01_req_008_02_req_007_10_limits_the_gallery_to_a_library_and_its_folders() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);
    harness.get_by_role_and_label(Role::Button, "Away, offline");

    harness.get_by_role_and_label(Role::Button, "Photos").click();
    settle(&mut harness);
    let library = search(&harness).library.expect("a library");
    harness.get_by_role_and_label(Role::Button, "trip").click();
    settle(&mut harness);
    assert_eq!(
        (search(&harness).library, search(&harness).folder.as_deref()),
        (Some(library), Some("trip"))
    );
    assert_eq!(count(&harness), Some(2));

    harness.get_by_role_and_label(Role::Button, "All libraries").click();
    settle(&mut harness);
    assert_eq!(
        (search(&harness).library, search(&harness).folder.as_deref()),
        (None, None)
    );
    assert_eq!(count(&harness), Some(4));
}

#[test]
fn req_008_03_req_008_06_filters_by_clicking_people_places_and_tags() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);
    harness.snapshot("sidebar");

    harness.get_by_role_and_label(Role::Button, "Anna").click();
    settle(&mut harness);
    assert_eq!(search(&harness).tags, ["Anna"]);
    assert_eq!(count(&harness), Some(1));
    // Clicking it again removes it.
    harness.get_by_role_and_label(Role::Button, "Anna").click();
    settle(&mut harness);
    assert!(search(&harness).tags.is_empty());

    // Places is a tree: Italy › Rome.
    harness.get_by_label("Expand Italy").click();
    settle(&mut harness);
    harness.get_by_role_and_label(Role::Button, "Rome").click();
    settle(&mut harness);
    assert_eq!(search(&harness).tags, ["Rome"]);

    // Tags are below the sidebar's fold.
    harness.hover_at(egui::pos2(100.0, 400.0));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, -20.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    });
    settle(&mut harness);
    harness.get_by_role_and_label(Role::Button, "Berlin").click();
    settle(&mut harness);
    assert_eq!(search(&harness).tags, ["Rome", "Berlin"]);
}

#[test]
fn req_008_04_req_008_07_toggles_ratings_and_remembers_collapsed_groups() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);

    harness.get_by_role_and_label(Role::Button, "5 stars").click();
    settle(&mut harness);
    assert_eq!(search(&harness).ratings, [5]);
    harness.get_by_role_and_label(Role::Button, "5 stars").click();
    settle(&mut harness);
    assert!(search(&harness).ratings.is_empty());

    harness.get_by_role_and_label(Role::Button, "People").click();
    settle(&mut harness);
    assert!(harness.query_by_role_and_label(Role::Button, "Anna").is_none());
    let db = harness.state().db().unwrap();
    let stored = db::settings::value(&db.lock().unwrap(), "sidebar.group.people").unwrap();
    assert_eq!(stored, Some("closed".into()));
}
