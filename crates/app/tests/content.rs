//! Content search in the app (spec 036): the settings section, the search bar's suggestion and chip, the gallery's
//! matches and empty state, and smart albums. Words are matched by a fake: the real model runs in `media`'s tests.

mod common;

use app::Page;
use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::rusqlite::Connection;
use db::writer::{IndexWriter, IndexedFile};
use domain::content::CATALOG;
use domain::media::{MediaKind, MediaMetadata};
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use std::sync::Arc;

type App = Harness<'static, app::AppState>;

/// `dog.jpg` and `beach.jpg` match the words they contain; nothing else matches anything.
struct Fake;

impl app::gallery::ContentMatches for Fake {
    fn matches(&self, conn: &Connection, words: &str) -> Result<Vec<i64>, String> {
        let mut ids = Vec::new();
        for name in ["dog.jpg", "beach.jpg"] {
            if words.contains(name.trim_end_matches(".jpg")) {
                ids.extend(db::media::id_of(conn, 1, name).map_err(|error| error.to_string())?);
            }
        }
        Ok(ids)
    }
}

fn index() -> Connection {
    let conn = db::open_in_memory().unwrap();
    db::libraries::insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Online).unwrap();
    let files: Vec<IndexedFile> = ["dog.jpg", "beach.jpg", "city.jpg"]
        .iter()
        .enumerate()
        .map(|(i, name)| IndexedFile {
            rel_path: name.to_string(),
            file_name: name.to_string(),
            kind: MediaKind::Image,
            size: 1,
            mtime: 1_000,
            sidecar_mtime: None,
            metadata: MediaMetadata {
                width: Some(400),
                height: Some(300),
                taken_at: Some(1_000 + i as i64),
                rating: Some(5),
                ..Default::default()
            },
            read_error: None,
        })
        .collect();
    let mut writer = IndexWriter::new(conn);
    writer.write(1, &files).unwrap();
    writer.into()
}

/// The app with content search set up in the fixture's data folder; `chosen` picks the recommended model, whose
/// files are faked into the store.
fn app(fixture: &Fixture, chosen: bool) -> App {
    let data = fixture.dir.path().join("data");
    if chosen {
        let dir = data.join("content-models").join(CATALOG[0].id);
        std::fs::create_dir_all(&dir).unwrap();
        for file in &CATALOG[0].files {
            std::fs::write(dir.join(file.name), b"fake").unwrap();
        }
    }
    let conn = index();
    if chosen {
        db::settings::set_value(&conn, db::content::MODEL_KEY, &serde_json::json!(CATALOG[0].id)).unwrap();
    }
    let mut state = fixture.state(conn).with_content(&data, None);
    if let Some(gallery) = &mut state.gallery {
        gallery.set_content_matcher(Arc::new(Fake));
    }
    let mut harness = common::harness(state);
    // Tall enough that the settings' content search section is on screen.
    harness.set_size(eframe::egui::vec2(1000.0, 4000.0));
    settle(&mut harness);
    harness
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

fn count(harness: &mut App) -> Option<usize> {
    let gallery = harness.state_mut().gallery.as_mut().unwrap();
    gallery.wait_for_loads();
    gallery.count()
}

/// REQ-036-01, REQ-036-03: the section explains itself, lists the models with size and license, and offers each for
/// download; with none chosen, the pass is off.
#[test]
fn req_036_01_req_036_03_lists_the_models_and_starts_off() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, false);
    harness.state_mut().page = Page::Settings;
    settle(&mut harness);
    harness.get_by_label("Content search");
    harness.get_by_label_contains("Nothing leaves this computer");
    harness.get_by_label("Off");
    harness.get_by_label("MobileCLIP-S0 (recommended)");
    harness.get_by_label_contains("218 MB. Apple research license");
    harness.get_by_label_contains("608 MB. MIT");
    harness.get_by_role_and_label(Role::Button, "Download MobileCLIP-S0");
    harness.get_by_role_and_label(Role::Button, "Download CLIP ViT-B/32");
    assert!(harness.query_by_label("Pause content search").is_none());
    harness.set_size(eframe::egui::vec2(1000.0, 1500.0));
    harness.run_steps(2);
    harness.snapshot(common::snapshot_name("content_settings"));
}

/// REQ-036-01, REQ-036-03, REQ-036-08: a model on this computer can be used and is then in use; deleting the search
/// data asks, then turns content search off and removes the vectors.
#[test]
fn req_036_01_req_036_03_req_036_08_uses_a_model_and_deletes_the_data() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, true);
    let chosen = |harness: &App| {
        let db = harness.state().db().unwrap();
        db::content::chosen(&db.lock().unwrap(), db::content::MODEL_KEY).unwrap()
    };
    harness.state_mut().choose_content_model(None);
    harness.state_mut().page = Page::Settings;
    settle(&mut harness);
    harness.get_by_role_and_label(Role::Button, "Use MobileCLIP-S0").click();
    settle(&mut harness);
    assert_eq!(chosen(&harness).as_deref(), Some(CATALOG[0].id));
    harness.get_by_role_and_label(Role::Button, "Using MobileCLIP-S0");
    harness.get_by_role_and_label(Role::Button, "Pause content search");

    {
        let db = harness.state().db().unwrap();
        let conn = db.lock().unwrap();
        let item = db::content::pending(&conn, CATALOG[0].id, 1, 0).unwrap().remove(0);
        db::content::store(&conn, &item, CATALOG[0].id, &[1, 2]).unwrap();
    }
    harness
        .get_by_role_and_label(Role::Button, "Delete search data")
        .click();
    settle(&mut harness);
    harness.get_by_label_contains("Delete what the models found");
    harness.get_by_role_and_label(Role::Button, "Delete").click();
    settle(&mut harness);
    assert_eq!(chosen(&harness), None);
    let db = harness.state().db().unwrap();
    assert_eq!(db::content::count(&db.lock().unwrap(), CATALOG[0].id).unwrap(), 0);
}

/// REQ-036-03, REQ-036-05: with a model chosen, typed words are first offered as what the photos show; choosing them
/// shows the matches, combined with the other filters, until the chip is removed. Without one, no such suggestion.
#[test]
fn req_036_05_searches_by_what_the_photos_show() {
    {
        // One fixture at a time: it holds the tests' lock.
        let fixture = Fixture::new();
        let mut off = app(&fixture, false);
        type_into_search(&mut off, "dog");
        assert!(off.query_by_label("Photos showing “dog”").is_none());
    }

    let fixture = Fixture::new();
    let mut harness = app(&fixture, true);
    assert_eq!(count(&mut harness), Some(3));
    type_into_search(&mut harness, "a dog and a beach");
    harness.set_size(eframe::egui::vec2(1000.0, 700.0));
    harness.run_steps(2);
    harness.snapshot(common::snapshot_name("content_suggestion"));
    let first = harness.query_all_by_role(Role::ListItem).next().unwrap();
    assert_eq!(
        first.accesskit_node().label().as_deref(),
        Some("Photos showing “a dog and a beach”")
    );
    harness.key_press(eframe::egui::Key::Enter);
    settle(&mut harness);
    assert_eq!(
        harness.state().gallery.as_ref().unwrap().search().content.as_deref(),
        Some("a dog and a beach")
    );
    harness.get_by_label("Photos showing a dog and a beach");
    assert_eq!(count(&mut harness), Some(2));

    // Another replaces it.
    type_into_search(&mut harness, "dog");
    harness.key_press(eframe::egui::Key::Enter);
    settle(&mut harness);
    assert_eq!(count(&mut harness), Some(1));
    assert!(harness.query_by_label("Photos showing a dog and a beach").is_none());

    harness
        .get_by_role_and_label(Role::Button, "Remove photos showing dog")
        .click();
    settle(&mut harness);
    assert_eq!(harness.state().gallery.as_ref().unwrap().search().content, None);
    assert_eq!(count(&mut harness), Some(3));
}

/// REQ-036-06: words that match nothing say so, with how many photos are still to be looked at.
#[test]
fn req_036_06_says_when_nothing_shows_it() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, true);
    let gallery = harness.state_mut().gallery.as_mut().unwrap();
    gallery.set_content_left(1234);
    gallery.set_content(Some("skyscrapers".into()));
    settle(&mut harness);
    assert_eq!(count(&mut harness), Some(0));
    harness.get_by_label("No photos showing “skyscrapers”");
    harness.get_by_label_contains("1,234 photos haven't been looked at yet");
    harness.set_size(eframe::egui::vec2(1000.0, 700.0));
    harness.run_steps(2);
    harness.snapshot(common::snapshot_name("content_no_matches"));
}

/// REQ-036-07: a smart album keeps the words and counts their matches in the sidebar.
#[test]
fn req_036_07_a_smart_album_keeps_the_words() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, true);
    {
        let db = harness.state().db().unwrap();
        let search = domain::search::GallerySearch {
            content: Some("dog".into()),
            ..Default::default()
        };
        db::smart_albums::insert(&db.lock().unwrap(), "Dogs", &search).unwrap();
    }
    harness.state_mut().gallery.as_mut().unwrap().reload();
    settle(&mut harness);
    harness.get_by_label("Dogs");
    let gallery = harness.state().gallery.as_ref().unwrap();
    let dogs = gallery
        .sidebar()
        .unwrap()
        .albums
        .iter()
        .find(|row| row.album.name == "Dogs")
        .unwrap();
    assert_eq!(dogs.count, 1);
}
