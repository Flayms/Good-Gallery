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
use egui_kittest::kittest::{NodeT as _, Queryable as _};

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
        read_error: None,
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
    harness.snapshot(common::snapshot_name("search_suggestions"));
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
    harness.snapshot(common::snapshot_name("filters_menu"));

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
    let today = domain::time_zone::today();
    harness
        .get_by_role_and_label(Role::Button, &today.format("%-d %B %Y").to_string())
        .click();
    settle(&mut harness);
    assert_eq!((search(&harness).from, search(&harness).to), (Some(today), Some(today)));
    harness.get_by_role_and_label(Role::Button, "Clear date range").click();
    settle(&mut harness);
    assert_eq!(search(&harness).from, None);
}

fn day(text: &str) -> chrono::NaiveDate {
    text.parse().unwrap()
}

/// REQ-041-01, REQ-041-02: the calendar opens at the range's month and names its days with their photos and videos,
/// which the range itself doesn't change; two clicks make a range either way round, and the next starts anew. Days
/// without photos can be picked.
#[test]
fn req_041_01_req_041_02_picks_a_range_on_a_calendar_of_the_photos() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);
    let mut dated = search(&harness);
    dated.from = Some(day("1970-01-05"));
    harness.state_mut().gallery.as_mut().unwrap().set_search(dated);
    settle(&mut harness);

    harness.get_by_label("Filters, 1 active").click();
    settle(&mut harness);
    harness.get_by_role_and_label(Role::Button, "January 1970");
    let photos = "1 January 1970, 3 photos · 1 video";
    harness.get_by_role_and_label(Role::Button, photos).click();
    settle(&mut harness);
    assert_eq!(
        (search(&harness).from, search(&harness).to),
        (Some(day("1970-01-01")), Some(day("1970-01-01")))
    );
    harness.get_by_role_and_label(Role::Button, "10 January 1970").click();
    settle(&mut harness);
    assert_eq!(
        (search(&harness).from, search(&harness).to),
        (Some(day("1970-01-01")), Some(day("1970-01-10")))
    );
    harness.get_by_role_and_label(Role::Button, "3 January 1970").click();
    settle(&mut harness);
    assert_eq!(
        (search(&harness).from, search(&harness).to),
        (Some(day("1970-01-03")), Some(day("1970-01-03"))),
        "a new range"
    );
    assert!(
        harness.query_by_label(photos).is_some(),
        "the 1st keeps its photos outside the range"
    );
    harness.snapshot(common::snapshot_name("date_calendar"));
}

/// REQ-041-03, REQ-041-04: the month's name shows its year's months with their counts, arrows step years and a month
/// opens it; a quick range sets its days and shows its first month.
#[test]
fn req_041_03_req_041_04_steps_through_years_and_quick_ranges() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);
    let mut dated = search(&harness);
    dated.from = Some(day("1970-01-05"));
    harness.state_mut().gallery.as_mut().unwrap().set_search(dated);
    settle(&mut harness);
    harness.get_by_label("Filters, 1 active").click();
    settle(&mut harness);

    harness.get_by_role_and_label(Role::Button, "Next month").click();
    settle(&mut harness);
    harness.get_by_role_and_label(Role::Button, "February 1970").click();
    settle(&mut harness);
    harness.get_by_role_and_label(Role::Button, "January 1970, 3 photos · 1 video");
    harness.get_by_role_and_label(Role::Button, "Next year").click();
    settle(&mut harness);
    harness.get_by_role_and_label(Role::Button, "1971");
    harness.get_by_role_and_label(Role::Button, "Previous year").click();
    settle(&mut harness);
    harness.get_by_role_and_label(Role::Button, "March 1970").click();
    settle(&mut harness);
    harness.get_by_role_and_label(Role::Button, "1 March 1970");

    harness.get_by_role_and_label(Role::Button, "Last year").click();
    settle(&mut harness);
    let (from, to) = domain::calendar::Quick::LastYear.range(domain::time_zone::today());
    assert_eq!((search(&harness).from, search(&harness).to), (Some(from), Some(to)));
    harness.get_by_role_and_label(Role::Button, &app::format::month_year(from));
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
    harness.snapshot(common::snapshot_name("sidebar"));

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

#[test]
fn req_008_08_req_008_09_hides_and_shows_the_sidebar_and_remembers_it() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);
    let gallery_left = |harness: &App| {
        harness
            .get_by_role_and_label(Role::TextInput, app::search_bar::LABEL)
            .rect()
            .left()
    };
    let shown_left = gallery_left(&harness);
    assert!(harness.query_by_role_and_label(Role::Button, "Show sidebar").is_none());

    harness.get_by_role_and_label(Role::Button, "Hide sidebar").click();
    settle(&mut harness);
    assert!(
        harness.query_by_role_and_label(Role::Button, "Beach").is_none(),
        "the sidebar still shows"
    );
    assert!(
        gallery_left(&harness) < shown_left - 100.0,
        "the gallery doesn't take the sidebar's width"
    );
    let db = harness.state().db().unwrap();
    let stored = db::settings::value(&db.lock().unwrap(), "sidebar.hidden").unwrap();
    assert_eq!(stored, Some(true.into()));

    // The Settings page has the button too.
    harness.state_mut().page = app::Page::Settings;
    settle(&mut harness);
    harness.get_by_role_and_label(Role::Button, "Show sidebar").click();
    settle(&mut harness);
    assert!(harness.query_by_role_and_label(Role::Button, "Beach").is_some());
    harness.state_mut().page = app::Page::Gallery;
    settle(&mut harness);

    // Ctrl + B hides and shows it again.
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
    settle(&mut harness);
    assert!(harness.query_by_role_and_label(Role::Button, "Beach").is_none());
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
    settle(&mut harness);
    assert!(harness.query_by_role_and_label(Role::Button, "Beach").is_some());
    assert_eq!(gallery_left(&harness), shown_left);

    // The next start keeps it hidden.
    let conn = index();
    db::settings::set_value(&conn, "sidebar.hidden", &true.into()).unwrap();
    let mut harness = fixture.harness(conn);
    settle(&mut harness);
    assert!(harness.query_by_role_and_label(Role::Button, "Beach").is_none());
    assert!(harness.query_by_role_and_label(Role::Button, "Show sidebar").is_some());
}

/// REQ-013-02, REQ-013-08: at the minimum window size every toolbar control is visible, none overlaps another, and
/// none lies under the window controls.
#[test]
fn req_013_02_req_013_08_the_toolbar_fits_the_minimum_window_beside_the_window_controls() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);
    harness.set_size(egui::vec2(800.0, 600.0));
    harness.state_mut().search_bar = Default::default();
    // Filters show their count, the widest the button gets.
    let mut filtered = search(&harness);
    filtered.ratings = vec![5];
    filtered.kind = Some(MediaKind::Image);
    harness.state_mut().gallery.as_mut().unwrap().set_search(filtered);
    settle(&mut harness);

    let screen = harness.ctx.content_rect();
    let controls = [
        harness
            .get_by_role_and_label(Role::TextInput, app::search_bar::LABEL)
            .rect(),
        harness.get_by_role_and_label(Role::Button, "Filters, 2 active").rect(),
        harness.get_by_role_and_label(Role::Button, "View options").rect(),
    ];
    let window_controls = app::titlebar::button_rects(screen, &app::titlebar::ButtonLayout::default());
    for (i, control) in controls.iter().enumerate() {
        assert!(screen.contains_rect(*control), "{control:?} is cut off");
        assert!(control.width() >= 28.0, "{control:?} is too small");
        for other in &controls[i + 1..] {
            assert!(!control.intersects(*other), "{control:?} overlaps {other:?}");
        }
        for &(_, button) in &window_controls {
            assert!(
                !control.intersects(button),
                "{control:?} lies under the window control {button:?}"
            );
        }
    }
}

/// REQ-013-08: _Filters_ sit right beside the search bar, also with a count; _View_ stays at the right end.
#[test]
fn req_013_08_filters_sit_beside_the_search_bar() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);
    harness.set_size(egui::vec2(1400.0, 800.0));
    for kind in [None, Some(MediaKind::Image)] {
        let mut filtered = search(&harness);
        filtered.kind = kind;
        harness.state_mut().gallery.as_mut().unwrap().set_search(filtered);
        settle(&mut harness);
        let label = if kind.is_some() { "Filters, 1 active" } else { "Filters" };
        let field = harness
            .get_by_role_and_label(Role::TextInput, app::search_bar::LABEL)
            .rect();
        let filters = harness.get_by_role_and_label(Role::Button, label).rect();
        let view = harness.get_by_role_and_label(Role::Button, "View options").rect();
        let gap = filters.left() - field.right();
        assert!((0.0..=24.0).contains(&gap), "{label} is {gap} from the search bar");
        assert!(
            view.left() - filters.right() > 200.0,
            "{label} isn't by the search bar but by View"
        );
    }
}

/// The y of a sidebar row, to compare the groups' order.
fn row_top(harness: &App, label: &str) -> f32 {
    harness.get_by_role_and_label(Role::Button, label).rect().top()
}

#[test]
fn req_008_10_req_008_11_orders_and_hides_groups_from_the_settings_and_remembers_them() {
    let fixture = Fixture::new();
    let open_settings = |conn: Connection| {
        let mut harness = fixture.harness(conn);
        harness.set_size(egui::vec2(1000.0, 1000.0));
        harness.state_mut().page = app::Page::Settings;
        settle(&mut harness);
        harness
    };
    let disabled = |harness: &App, label: &str| {
        harness
            .get_by_role_and_label(Role::Button, label)
            .accesskit_node()
            .is_disabled()
    };
    let mut harness = open_settings(index());
    // Default: Smart albums (none, so hidden), People (Anna), Places (Italy), Ratings (5 stars), Tags (Beach).
    assert!(row_top(&harness, "Anna") < row_top(&harness, "Italy"));
    assert!(row_top(&harness, "Italy") < row_top(&harness, "5 stars"));
    assert!(row_top(&harness, "5 stars") < row_top(&harness, "Beach"));
    assert!(disabled(&harness, "Reset to default"), "nothing to reset");
    assert!(disabled(&harness, "Move Smart albums up"));
    assert!(disabled(&harness, "Move Tags down"));
    harness.snapshot(common::snapshot_name("settings_sidebar"));

    // Tags to the top, Ratings off.
    for _ in 0..4 {
        harness.get_by_role_and_label(Role::Button, "Move Tags up").click();
        // Two frames: the rows show where they moved to.
        harness.run_steps(2);
    }
    harness.get_by_role_and_label(Role::CheckBox, "Ratings").click();
    settle(&mut harness);
    assert!(row_top(&harness, "Beach") < row_top(&harness, "Anna"));
    assert!(row_top(&harness, "Anna") < row_top(&harness, "Italy"));
    assert!(harness.query_by_role_and_label(Role::Button, "5 stars").is_none());
    // The libraries stay first.
    assert!(row_top(&harness, "All libraries") < row_top(&harness, "Beach"));
    assert!(!disabled(&harness, "Reset to default"));
    let db = harness.state().db().unwrap();
    let stored = db::settings::value(&db.lock().unwrap(), "sidebar.groups")
        .unwrap()
        .unwrap();
    drop(harness);

    // The next start shows the same.
    let conn = index();
    db::settings::set_value(&conn, "sidebar.groups", &stored).unwrap();
    let mut harness = open_settings(conn);
    assert!(row_top(&harness, "Beach") < row_top(&harness, "Anna"));
    assert!(harness.query_by_role_and_label(Role::Button, "5 stars").is_none());
    let checkbox = harness.get_by_role_and_label(Role::CheckBox, "Ratings");
    assert_eq!(
        checkbox.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::False)
    );

    harness.get_by_role_and_label(Role::Button, "Reset to default").click();
    settle(&mut harness);
    assert!(row_top(&harness, "Anna") < row_top(&harness, "Beach"));
    assert!(row_top(&harness, "5 stars") < row_top(&harness, "Beach"));
    assert!(disabled(&harness, "Reset to default"));
}

/// Whether the sidebar shows: its _Hide sidebar_ button is in it.
fn sidebar_shows(harness: &App) -> bool {
    harness.query_by_role_and_label(Role::Button, "Hide sidebar").is_some()
}

#[test]
fn req_019_15_a_narrow_window_opens_the_sidebar_over_the_gallery() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);
    harness.set_size(egui::vec2(400.0, 800.0));
    settle(&mut harness);
    // A resized gallery lays out again once the width has stayed the same for a moment (real time).
    std::thread::sleep(std::time::Duration::from_millis(400));
    settle(&mut harness);
    let search_left = |harness: &App| {
        harness
            .get_by_role_and_label(Role::TextInput, app::search_bar::LABEL)
            .rect()
            .left()
    };
    assert!(!sidebar_shows(&harness), "the sidebar takes room on a narrow screen");
    let gallery_left = search_left(&harness);
    assert!(gallery_left < 100.0, "the gallery doesn't use the whole width");
    harness.snapshot(common::snapshot_name("narrow_gallery"));

    // Opened, it lies over the gallery; picking something closes it.
    harness.get_by_role_and_label(Role::Button, "Show sidebar").click();
    settle(&mut harness);
    assert!(sidebar_shows(&harness));
    assert_eq!(search_left(&harness), gallery_left, "the sidebar narrows the gallery");
    harness.snapshot(common::snapshot_name("narrow_sidebar"));
    harness.get_by_role_and_label(Role::Button, "Beach").click();
    settle(&mut harness);
    assert_eq!(count(&harness), Some(3));
    assert!(!sidebar_shows(&harness), "picking a tag leaves the sidebar open");

    // A click beside it closes it, and so does Ctrl + B.
    harness.get_by_role_and_label(Role::Button, "Show sidebar").click();
    settle(&mut harness);
    harness.hover_at(egui::pos2(380.0, 500.0));
    harness.event(egui::Event::PointerButton {
        pos: egui::pos2(380.0, 500.0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.event(egui::Event::PointerButton {
        pos: egui::pos2(380.0, 500.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    settle(&mut harness);
    assert!(!sidebar_shows(&harness), "a click beside the sidebar leaves it open");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
    settle(&mut harness);
    assert!(sidebar_shows(&harness));
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::B);
    settle(&mut harness);
    assert!(!sidebar_shows(&harness));

    // None of it is stored: a wide window shows the sidebar as before (REQ-008-09).
    let db = harness.state().db().unwrap();
    assert_eq!(
        db::settings::value(&db.lock().unwrap(), "sidebar.hidden").unwrap(),
        None
    );
    harness.set_size(egui::vec2(900.0, 800.0));
    settle(&mut harness);
    assert!(sidebar_shows(&harness));
}

#[test]
fn req_019_15_the_info_sheet_fits_a_narrow_window() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);
    harness.set_size(egui::vec2(300.0, 800.0));
    settle(&mut harness);
    let gallery = harness.state().gallery.as_ref().unwrap();
    let id = gallery.id_at(0);
    harness.state_mut().info_sheet = id;
    settle(&mut harness);
    let title = harness.get_by_label("Information").rect();
    assert!(title.left() >= 0.0, "the info sheet starts left of the window");
    assert!(title.right() <= 300.0, "the info sheet starts right of the window");
}

/// The color drawn at `pos`.
fn pixel(harness: &mut egui_kittest::Harness<'static, app::AppState>, pos: egui::Pos2) -> egui::Color32 {
    let image = harness.render().unwrap();
    let [r, g, b, _] = image.get_pixel(pos.x as u32, pos.y as u32).0;
    egui::Color32::from_rgb(r, g, b)
}

/// REQ-013-19: a frameless button shows the hover state under the pointer; _Filters_ and _View_ are icons named by
/// their tips.
#[test]
fn req_013_19_buttons_answer_the_pointer_and_filters_and_view_are_icons() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture);
    for (label, tip) in [("Filters", "Filters"), ("View options", "View")] {
        let rect = harness.get_by_label(label).rect();
        assert!(rect.width() < 72.0, "{label} is an icon: {rect:?}");
        // In the button's padding, beside its icon.
        let inside = egui::pos2(rect.left() + 3.0, rect.center().y);
        harness.hover_at(egui::pos2(500.0, 500.0));
        harness.run_steps(20);
        let before = pixel(&mut harness, inside);
        harness.hover_at(rect.center());
        harness.run_steps(20);
        assert_ne!(
            pixel(&mut harness, inside),
            before,
            "{label} shows the pointer is on it"
        );
        harness.run_steps(60);
        assert!(
            harness.query_all_by_label(tip).count() >= if tip == label { 2 } else { 1 },
            "{label} has the tip {tip}"
        );
    }
}

/// REQ-041-02: a day shows the thumbnail of its first item, asked for by the calendar itself: with the range on a day
/// without photos, the gallery shows no tiles that would ask for them.
#[test]
fn req_041_02_shows_the_first_photo_of_each_day() {
    const DAY: i64 = 86_400_000;
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let mut writer = IndexWriter::new(conn);
    writer
        .write(
            root,
            &[
                file("a.jpg", MediaKind::Image, DAY / 2, None, &[]),
                file("b.jpg", MediaKind::Image, DAY / 2 + 60_000, None, &[]),
                file("c.mp4", MediaKind::Video, DAY + DAY / 2, None, &[]),
                file("d.jpg", MediaKind::Image, 2 * DAY + DAY / 2, None, &[]),
            ],
        )
        .unwrap();
    let conn: Connection = writer.into();
    let id = |name: &str| db::media::id_of(&conn, root, name).unwrap().unwrap();
    let firsts = [id("a.jpg"), id("c.mp4"), id("d.jpg")];
    let second = id("b.jpg");
    let cache = indexer::thumb_cache::ThumbCache::open(tempfile::tempdir().unwrap().keep(), u64::MAX);
    cache.wait_ready();
    for (n, media) in [id("a.jpg"), id("b.jpg"), id("c.mp4"), id("d.jpg")]
        .into_iter()
        .enumerate()
    {
        let key = db::thumbs::thumb_source(&conn, media).unwrap().unwrap().key;
        let colors = [[220, 120, 60], [60, 160, 220], [80, 200, 120], [200, 80, 180]][n];
        let pixels = colors.repeat(400 * 300);
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
    let fixture = Fixture::new();
    let mut state = fixture.state(conn);
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
    // From the first frame on, so no tile ever asks for a thumbnail.
    let gallery = state.gallery.as_mut().unwrap();
    let mut empty_day = gallery.search().clone();
    (empty_day.from, empty_day.to) = (Some(day("1970-01-05")), Some(day("1970-01-05")));
    gallery.set_search(empty_day);
    let mut harness = common::harness_on(state, render_state);
    settle(&mut harness);
    assert_eq!(count(&harness), Some(0), "no tiles");

    harness.get_by_label("Filters, 1 active").click();
    settle(&mut harness);
    harness.get_by_role_and_label(Role::Button, "1 January 1970, 2 photos");
    harness.get_by_role_and_label(Role::Button, "2 January 1970, 1 video");
    for _ in 0..500 {
        harness.run_steps(1);
        let gallery = harness.state_mut().gallery.as_mut().unwrap();
        if firsts.iter().all(|media| gallery.thumbnail(*media).is_some()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let gallery = harness.state_mut().gallery.as_mut().unwrap();
    assert!(
        firsts.iter().all(|media| gallery.thumbnail(*media).is_some()),
        "each day's first thumbnail"
    );
    assert!(gallery.thumbnail(second).is_none(), "only the first of a day");
    harness.run_steps(40);
    harness.snapshot(common::snapshot_name("date_calendar_photos"));
}
