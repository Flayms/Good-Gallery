//! Smart albums (specs 034 and 039): saving a search from the Filters menu, the sidebar group, renaming and deleting,
//! and rules matching all or any of their conditions.

mod common;

use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::rusqlite::Connection;
use db::writer::{IndexWriter, IndexedFile};
use domain::media::{MediaKind, MediaMetadata};
use domain::search::GallerySearch;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};

type App = Harness<'static, app::AppState>;

fn file(rel_path: &str, rating: Option<i64>, tags: &[&str]) -> IndexedFile {
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
            taken_at: Some(1_000),
            rating,
            tags: tags.iter().map(|tag| vec![tag.to_string()]).collect(),
            ..Default::default()
        },
        read_error: None,
    }
}

/// Beach (2, one rated 5), Berlin (1).
fn index() -> Connection {
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let mut writer = IndexWriter::new(conn);
    writer
        .write(
            root,
            &[
                file("beach.jpg", Some(5), &["Beach"]),
                file("dunes.jpg", None, &["Beach"]),
                file("berlin.jpg", None, &["Berlin"]),
            ],
        )
        .unwrap();
    writer.into()
}

fn search(harness: &App) -> GallerySearch {
    harness.state().gallery.as_ref().unwrap().search().clone()
}

fn set_search(harness: &mut App, search: GallerySearch) {
    harness.state_mut().gallery.as_mut().unwrap().set_search(search);
    settle(harness);
}

fn beach() -> GallerySearch {
    GallerySearch {
        tags: vec!["Beach".into()],
        ..GallerySearch::default()
    }
}

/// Whether the sidebar marks a row as active.
fn active(harness: &App, label: &str) -> bool {
    let row = harness.get_by_role_and_label(Role::Button, label);
    row.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

/// The dialog's name field: the search bar's comes first, then the name, then the conditions' fields.
fn name_field<'a>(harness: &'a App) -> egui_kittest::Node<'a> {
    harness
        .query_all_by_role(Role::TextInput)
        .nth(1)
        .expect("the dialog is open")
}

/// Saves the current search under `name` through the Filters menu.
fn save_as(harness: &mut App, name: &str) {
    harness.get_by_label_contains("Filters").click();
    harness.run();
    harness.get_by_label("Save as smart album…").click();
    harness.run();
    let field = name_field(harness);
    field.set_text_value_accesskit(name);
    harness.run();
    harness.get_by_label("Save").click();
    settle(harness);
}

/// REQ-034-01, REQ-034-02: a search that isn't the default one is saved under a suggested name and listed with its
/// count; the default search can't be saved, and without albums there is no group.
#[test]
fn req_034_01_req_034_02_saves_a_search_and_lists_it_with_its_count() {
    let fixture = Fixture::new();
    let mut harness = fixture.harness(index());
    settle(&mut harness);
    assert!(harness.query_by_label("Smart albums").is_none());
    harness.get_by_label_contains("Filters").click();
    harness.run();
    assert!(
        harness
            .get_by_label("Save as smart album…")
            .accesskit_node()
            .is_disabled()
    );
    harness.key_press(egui::Key::Escape);
    harness.run();

    set_search(&mut harness, beach());
    harness.get_by_label_contains("Filters").click();
    harness.run();
    harness.get_by_label("Save as smart album…").click();
    harness.run();
    // The name is suggested from the tags.
    assert_eq!(name_field(&harness).value().as_deref(), Some("Beach"));
    harness.snapshot(common::snapshot_name("smart_album_save"));
    name_field(&harness).set_text_value_accesskit("Seaside");
    harness.run();
    harness.get_by_label("Save").click();
    settle(&mut harness);

    harness.get_by_label("Smart albums");
    assert!(active(&harness, "Seaside"), "the search is the album's");
    harness.snapshot(common::snapshot_name("smart_album_sidebar"));
}

/// REQ-034-01: an empty name can't be saved, and _Cancel_ saves nothing.
#[test]
fn req_034_01_needs_a_name() {
    let fixture = Fixture::new();
    let mut harness = fixture.harness(index());
    settle(&mut harness);
    set_search(&mut harness, beach());
    harness.get_by_label_contains("Filters").click();
    harness.run();
    harness.get_by_label("Save as smart album…").click();
    harness.run();
    let field = name_field(&harness);
    field.set_text_value_accesskit("   ");
    harness.run();
    assert!(harness.get_by_label("Save").accesskit_node().is_disabled());
    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Smart albums").is_none());
}

/// REQ-034-03: clicking a smart album shows its search; changing the search leaves the album as it was.
#[test]
fn req_034_03_opens_a_smart_album() {
    let fixture = Fixture::new();
    let mut harness = fixture.harness(index());
    settle(&mut harness);
    let five = GallerySearch {
        ratings: vec![5],
        descending: false,
        ..GallerySearch::default()
    };
    set_search(&mut harness, five.clone());
    save_as(&mut harness, "Best");
    set_search(&mut harness, GallerySearch::default());
    assert!(!active(&harness, "Best"));

    harness.get_by_role_and_label(Role::Button, "Best").click();
    settle(&mut harness);
    assert_eq!(search(&harness), five);
    assert_eq!(harness.state().gallery.as_ref().unwrap().count(), Some(1));
    assert!(active(&harness, "Best"));

    let mut changed = five.clone();
    changed.ratings.push(0);
    set_search(&mut harness, changed);
    assert!(!active(&harness, "Best"));
    harness.get_by_role_and_label(Role::Button, "Best").click();
    settle(&mut harness);
    assert_eq!(search(&harness), five);
}

/// REQ-034-05: _Rename…_ and _Delete_ in a smart album's context menu.
#[test]
fn req_034_05_renames_and_deletes_a_smart_album() {
    let fixture = Fixture::new();
    let mut harness = fixture.harness(index());
    settle(&mut harness);
    set_search(&mut harness, beach());
    save_as(&mut harness, "Sea");

    harness.get_by_role_and_label(Role::Button, "Sea").click_secondary();
    harness.run();
    harness.get_by_label("Rename…").click();
    harness.run();
    let field = name_field(&harness);
    assert_eq!(field.value().as_deref(), Some("Sea"));
    field.set_text_value_accesskit("Coast");
    harness.run();
    harness.get_by_label("Rename").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Sea").is_none());

    harness.get_by_role_and_label(Role::Button, "Coast").click_secondary();
    harness.run();
    harness.get_by_label("Delete").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Coast").is_none());
    assert!(harness.query_by_label("Smart albums").is_none());
    // Only the album: the search shown stays.
    assert_eq!(search(&harness), beach());
}

fn editor(harness: &mut App) -> &mut app::gallery::AlbumEditor {
    harness
        .state_mut()
        .gallery
        .as_mut()
        .unwrap()
        .album_editor()
        .expect("the editor is open")
}

/// The count the sidebar shows for `album`.
fn count_of(harness: &App, album: &str) -> Option<String> {
    harness
        .state()
        .gallery
        .as_ref()
        .unwrap()
        .sidebar()
        .and_then(|data| data.albums.iter().find(|row| row.album.name == album))
        .map(|row| row.count.to_string())
}

/// REQ-039-01, REQ-039-02, REQ-039-03, REQ-039-05: _Save as smart album…_ opens the editor with the search's filters;
/// matching any of two tags keeps both, the album opens with its name as one chip, and removing the chip leaves it.
#[test]
fn req_039_01_req_039_03_req_039_05_saves_rules_matching_any() {
    use app::gallery::album_editor::{Kind, Row};
    let fixture = Fixture::new();
    let mut harness = fixture.harness(index());
    settle(&mut harness);
    set_search(&mut harness, beach());
    harness.get_by_label_contains("Filters").click();
    harness.run();
    harness.get_by_label("Save as smart album…").click();
    harness.run();
    assert_eq!(
        editor(&mut harness).rows,
        [Row {
            text: "Beach".into(),
            ..Row::new(Kind::Tag)
        }]
    );
    let edit = editor(&mut harness);
    edit.name = "Sea".into();
    edit.any = true;
    edit.rows.push(Row {
        text: "Berlin".into(),
        ..Row::new(Kind::Tag)
    });
    harness.run();
    harness.snapshot(common::snapshot_name("album_editor"));
    harness.get_by_label("Save").click();
    settle(&mut harness);

    assert_eq!(count_of(&harness, "Sea").as_deref(), Some("3"));
    assert_eq!(
        harness.state().gallery.as_ref().unwrap().count(),
        Some(3),
        "the saved album is open"
    );
    assert!(active(&harness, "Sea"));
    assert!(
        harness.query_by_label("Remove smart album Sea").is_some(),
        "one chip with its name"
    );
    assert!(harness.query_by_label("Remove tag Beach").is_none());
    harness.get_by_label("Remove smart album Sea").click();
    settle(&mut harness);
    assert!(search(&harness).rules.is_none());
    assert!(!active(&harness, "Sea"));
    assert_eq!(count_of(&harness, "Sea").as_deref(), Some("3"), "the album stays");
}

/// REQ-039-01, REQ-039-06: _Edit…_ opens an album's rules, and an album the filters bar can show is kept as a plain
/// search; an incomplete condition keeps _Save_ off.
#[test]
fn req_039_01_req_039_06_edits_an_album() {
    use app::gallery::album_editor::{Kind, Row};
    let fixture = Fixture::new();
    let mut harness = fixture.harness(index());
    settle(&mut harness);
    set_search(&mut harness, beach());
    save_as(&mut harness, "Seaside");
    let rows = |harness: &App| {
        harness
            .state()
            .gallery
            .as_ref()
            .unwrap()
            .sidebar()
            .unwrap()
            .albums
            .clone()
    };
    assert_eq!(rows(&harness)[0].album.search, beach(), "stored as a search");

    harness.get_by_role_and_label(Role::Button, "Seaside").click_secondary();
    harness.run();
    harness.get_by_label("Edit…").click();
    harness.run();
    let edit = editor(&mut harness);
    assert_eq!(edit.rows.len(), 1);
    edit.rows.push(Row::new(Kind::RatingAtLeast));
    edit.rows.push(Row::new(Kind::Tag));
    harness.run();
    assert!(
        harness.get_by_label("Save").accesskit_node().is_disabled(),
        "an empty tag"
    );
    editor(&mut harness).rows.pop();
    harness.run();
    harness.get_by_label("Save").click();
    settle(&mut harness);
    let album = rows(&harness)[0].clone();
    assert_eq!(album.count, 1, "beach.jpg: Beach and 5 stars");
    let rules = album.album.search.rules.expect("at least 3 stars needs rules");
    assert_eq!(
        (rules.name.as_str(), rules.any, rules.conditions.len()),
        ("Seaside", false, 2)
    );
}

/// REQ-040-05: _Camera is_ offers the cameras in the index, most used first with their counts, and needs one chosen;
/// the album then holds what that camera took.
#[test]
fn req_040_05_the_editor_offers_the_cameras_in_the_index() {
    use app::gallery::album_editor::{Kind, Row};
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let shot = |rel: &str, camera: &str| {
        let mut file = file(rel, None, &["Trip"]);
        file.metadata.camera = Some(camera.into());
        file.metadata.lens = Some("RF50mm F1.8 STM".into());
        file
    };
    let mut writer = IndexWriter::new(conn);
    writer
        .write(
            root,
            &[
                shot("a.jpg", "Canon EOS R5"),
                shot("b.jpg", "Canon EOS R5"),
                shot("c.jpg", "Apple iPhone 15"),
            ],
        )
        .unwrap();
    let fixture = Fixture::new();
    let mut harness = fixture.harness(writer.into());
    harness.set_size(egui::vec2(1000.0, 700.0));
    settle(&mut harness);
    set_search(
        &mut harness,
        GallerySearch {
            tags: vec!["Trip".into()],
            ..GallerySearch::default()
        },
    );
    harness.get_by_label_contains("Filters").click();
    harness.run();
    harness.get_by_label("Save as smart album…").click();
    harness.run();
    let edit = editor(&mut harness);
    edit.name = "Canon".into();
    edit.rows = vec![Row::new(Kind::Camera)];
    harness.run();
    assert!(
        harness.get_by_label("Save").accesskit_node().is_disabled(),
        "no camera chosen"
    );

    harness.get_by_role_and_label(Role::ComboBox, "Camera").click();
    harness.run();
    let canon = harness.get_by_label("Canon EOS R5 (2)").rect();
    let apple = harness.get_by_label("Apple iPhone 15 (1)").rect();
    assert!(canon.top() < apple.top(), "most used first");
    harness.get_by_label("Canon EOS R5 (2)").click();
    harness.run();
    harness.get_by_label("Save").click();
    settle(&mut harness);
    assert_eq!(count_of(&harness, "Canon").as_deref(), Some("2"));
}
