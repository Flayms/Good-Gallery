//! Editing photos in the viewer (spec 021) on a library of real files. Without a wgpu renderer for the app, the
//! preview is rendered on the CPU; saving runs exiftool and skips itself without it.

mod common;

use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::rusqlite::Connection;
use db::writer::{IndexWriter, IndexedFile};
use domain::edit::{Adjustment, Recipe, SavedEdit};
use domain::media::{MediaKind, MediaMetadata};
use eframe::egui::accesskit::Role;
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::kittest::Queryable as _;
use std::path::{Path, PathBuf};

type App = egui_kittest::Harness<'static, app::AppState>;

/// Files of the test library: `a.jpg` (a gradient), `b.gif`, `c.mp4` (indexed, not on disk), newest first.
fn library(fixture: &Fixture, edit: Option<SavedEdit>) -> (Connection, PathBuf) {
    let dir = PathBuf::from(fixture.folder("photos"));
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, dir.to_str().unwrap(), "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let gradient = image::RgbImage::from_fn(120, 80, |x, y| image::Rgb([(x * 2) as u8, (y * 3) as u8, 120]));
    gradient.save(dir.join("a.jpg")).unwrap();
    image::RgbImage::from_pixel(120, 80, image::Rgb([90, 90, 90]))
        .save(dir.join("b.gif"))
        .unwrap();
    let file = |i: i64, name: &str, kind: MediaKind, edit: Option<SavedEdit>| IndexedFile {
        rel_path: name.into(),
        file_name: name.into(),
        kind,
        size: 1,
        mtime: 1_000,
        sidecar_mtime: None,
        metadata: MediaMetadata {
            width: Some(120),
            height: Some(80),
            taken_at: Some(2_000_000 - i),
            edit: edit.map(Box::new),
            ..Default::default()
        },
    };
    let files = [
        file(0, "a.jpg", MediaKind::Image, edit),
        file(1, "b.gif", MediaKind::Image, None),
        file(2, "c.mp4", MediaKind::Video, None),
    ];
    let mut writer = IndexWriter::new(conn);
    writer.write(root, &files).unwrap();
    (writer.into(), dir)
}

/// exiftool for saving, or `None` (the test is skipped) when it isn't installed; `GG_REQUIRE_HELPERS=1` fails then.
fn tools(fixture: &Fixture) -> Option<media::edit::Tools> {
    match media::exiftool::find_exiftool() {
        Some(exiftool) => Some(media::edit::Tools {
            exiftool,
            config: media::edit::exiftool_config(&fixture.dir.path().join("data")).unwrap(),
        }),
        None if std::env::var_os("GG_REQUIRE_HELPERS").is_some() => panic!("exiftool not found"),
        None => {
            eprintln!("skipped: exiftool not found");
            None
        }
    }
}

fn app(fixture: &Fixture, edit: Option<SavedEdit>, tools: Option<media::edit::Tools>) -> (App, PathBuf) {
    let (conn, dir) = library(fixture, edit);
    let mut state = fixture.state(conn);
    if let Some(tools) = tools {
        state = state.with_edit_tools(tools);
    }
    let mut harness = common::harness_with_step(state, 1.0 / 60.0);
    settle(&mut harness);
    (harness, dir)
}

fn settle_all(harness: &mut App) {
    settle(harness);
    if let Some(originals) = harness.state_mut().originals.as_mut() {
        originals.wait();
    }
    if let Some(session) = harness
        .state_mut()
        .viewer
        .as_mut()
        .and_then(|viewer| viewer.editing_mut())
    {
        session.wait();
    }
    harness.run_steps(3);
}

fn open(harness: &mut App, name: &str) {
    harness.get_by_role_and_label(Role::Image, name).click();
    settle_all(harness);
}

fn session(harness: &App) -> Option<&app::edit::EditSession> {
    harness.state().viewer.as_ref()?.editing()
}

/// Starts editing the shown photo with `e` and waits for its original.
fn edit(harness: &mut App) {
    harness.key_press(Key::E);
    harness.run_steps(2);
    settle_all(harness);
    // The details come from the gallery's loader first.
    harness.run_steps(2);
    settle_all(harness);
    assert!(
        session(harness).is_some_and(app::edit::EditSession::is_loaded),
        "editing with the original loaded"
    );
}

fn brighter() -> Recipe {
    let mut recipe = Recipe::default();
    Adjustment::Exposure.set(&mut recipe, 1.0);
    recipe
}

fn set_recipe(harness: &mut App, recipe: Recipe) {
    let session = harness.state_mut().viewer.as_mut().unwrap().editing_mut().unwrap();
    session.commit(recipe);
    harness.run_steps(2);
}

fn mean_red(path: &Path) -> f64 {
    let image = image::open(path).unwrap().into_rgb8();
    image.pixels().map(|p| f64::from(p[0])).sum::<f64>() / f64::from(image.width() * image.height())
}

/// REQ-021-01: a JPEG offers _Edit_ (the bar's button, `e`), which swaps the info panel for the adjustments; a GIF,
/// a video and photos of an offline library don't.
#[test]
fn req_021_01_edit_is_offered_for_jpeg_png_and_webp_photos_in_online_libraries() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, None, None);
    open(&mut harness, "a.jpg");
    harness.get_by_role_and_label(Role::Button, "Edit").click();
    harness.run_steps(2);
    settle_all(&mut harness);
    assert!(session(&harness).is_some());
    harness.get_by_label("Exposure");
    harness.get_by_label("Temperature");
    harness.get_by_role_and_label(Role::Button, "Save as copy");
    // `e` again stops (nothing changed, so nothing to ask).
    harness.key_press(Key::E);
    harness.run_steps(2);
    assert!(session(&harness).is_none());
    assert!(harness.query_by_label("Exposure").is_none());

    // The GIF and the video have no Edit; `e` does nothing.
    for _ in 0..2 {
        harness.key_press(Key::ArrowRight);
        settle_all(&mut harness);
        assert!(harness.query_by_role_and_label(Role::Button, "Edit").is_none());
        harness.key_press(Key::E);
        harness.run_steps(2);
        assert!(session(&harness).is_none());
    }

    // Offline: no Edit.
    let db = harness.state().db().unwrap();
    db.lock()
        .unwrap()
        .execute("UPDATE library_roots SET status = 'offline'", [])
        .unwrap();
    harness.state_mut().refresh();
    for _ in 0..2 {
        harness.key_press(Key::ArrowLeft);
        settle_all(&mut harness);
    }
    assert_eq!(harness.state().viewer.as_ref().unwrap().index(), 0);
    assert!(harness.query_by_role_and_label(Role::Button, "Edit").is_none());
}

/// REQ-021-02, REQ-021-07: the preview follows the recipe at once and the file stays untouched; held `\` shows the
/// original; undo, redo and a double-click on a slider work; the panel as it looks.
#[test]
fn req_021_02_req_021_07_previews_compares_undoes_and_resets() {
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, None, None);
    let before = std::fs::read(dir.join("a.jpg")).unwrap();
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    set_recipe(&mut harness, brighter());
    harness.snapshot(common::snapshot_name("edit_panel"));
    assert_eq!(
        std::fs::read(dir.join("a.jpg")).unwrap(),
        before,
        "the file isn't touched while editing"
    );

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run_steps(2);
    assert!(session(&harness).unwrap().recipe().is_neutral());
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    harness.run_steps(2);
    assert_eq!(session(&harness).unwrap().recipe().exposure, 1.0);

    // A double-click on the slider resets it.
    let slider = harness.get_by_role_and_label(Role::Slider, "Exposure").rect().center();
    for _ in 0..2 {
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: slider,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            harness.run_steps(1);
        }
    }
    harness.run_steps(2);
    assert_eq!(session(&harness).unwrap().recipe().exposure, 0.0);

    // _Reset all_.
    set_recipe(&mut harness, brighter());
    harness.get_by_role_and_label(Role::Button, "Reset all").click();
    harness.run_steps(2);
    assert!(session(&harness).unwrap().recipe().is_neutral());
}

/// REQ-021-08: with unsaved changes, `Esc` and stepping ask; _Cancel_ stays, _Discard_ goes on. Without changes,
/// editing just ends.
#[test]
fn req_021_08_asks_before_leaving_with_unsaved_changes() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, None, None);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    set_recipe(&mut harness, brighter());

    harness.key_press(Key::Escape);
    harness.run_steps(2);
    harness.get_by_label("Save your changes?");
    // The dialog's, not the panel's.
    harness
        .get_all_by_role_and_label(Role::Button, "Cancel")
        .last()
        .unwrap()
        .click();
    harness.run_steps(2);
    assert!(harness.query_by_label("Save your changes?").is_none());
    assert!(session(&harness).is_some());

    harness.key_press(Key::ArrowRight);
    harness.run_steps(2);
    harness.get_by_label("Save your changes?");
    assert_eq!(harness.state().viewer.as_ref().unwrap().index(), 0, "not stepped yet");
    harness.get_by_role_and_label(Role::Button, "Discard").click();
    harness.run_steps(2);
    assert!(session(&harness).is_none());
    assert_eq!(harness.state().viewer.as_ref().unwrap().index(), 1);

    // Unchanged: Esc ends editing without asking, a second Esc closes the viewer.
    harness.key_press(Key::ArrowLeft);
    settle_all(&mut harness);
    edit(&mut harness);
    harness.key_press(Key::Escape);
    harness.run_steps(2);
    assert!(session(&harness).is_none() && harness.state().viewer.is_some());
    assert!(harness.query_by_label("Save your changes?").is_none());
}

/// REQ-021-09: _Save as copy_ writes `a (edited).jpg` beside the untouched original, asks the index to read it and
/// ends editing.
#[test]
fn req_021_09_saves_a_copy() {
    let fixture = Fixture::new();
    let Some(tools) = tools(&fixture) else { return };
    let (mut harness, dir) = app(&fixture, None, Some(tools));
    let before = std::fs::read(dir.join("a.jpg")).unwrap();
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    set_recipe(&mut harness, brighter());
    fixture.scans.scans.lock().unwrap().clear();

    harness.get_by_role_and_label(Role::Button, "Save as copy").click();
    harness.run_steps(1);
    settle_all(&mut harness);

    assert_eq!(std::fs::read(dir.join("a.jpg")).unwrap(), before);
    assert!(mean_red(&dir.join("a (edited).jpg")) > mean_red(&dir.join("a.jpg")) + 10.0);
    harness.get_by_label("Saved a copy");
    assert!(session(&harness).is_none());
    assert!(
        !fixture.scans.scans.lock().unwrap().is_empty(),
        "the index reads the new file"
    );
}

/// REQ-021-10, REQ-021-12: _Save_ asks the first time, keeps the original as a backup and replaces the file; _Revert
/// to original_ in the info panel (confirmed) puts it back.
#[test]
fn req_021_10_req_021_12_saves_over_the_original_and_reverts() {
    let fixture = Fixture::new();
    let Some(tools) = tools(&fixture) else { return };
    let (mut harness, dir) = app(&fixture, None, Some(tools));
    let before = std::fs::read(dir.join("a.jpg")).unwrap();
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    set_recipe(&mut harness, brighter());

    harness.get_by_role_and_label(Role::Button, "Save").click();
    harness.run_steps(2);
    harness.get_by_label("Replace the original?");
    harness.get_by_role_and_label(Role::Button, "Replace").click();
    harness.run_steps(1);
    settle_all(&mut harness);

    let backup = dir.join(".good-gallery").join("originals").join("a.jpg");
    assert_eq!(std::fs::read(&backup).unwrap(), before);
    assert_ne!(std::fs::read(dir.join("a.jpg")).unwrap(), before);
    harness.get_by_label("Saved");
    assert!(session(&harness).is_none());

    // The index learns of the edit by reading the file again; without an indexer here, as it would write it.
    let db = harness.state().db().unwrap();
    let edit = SavedEdit {
        recipe: brighter().to_json(),
        source: ".good-gallery/originals/a.jpg".into(),
    };
    db.lock()
        .unwrap()
        .execute(
            "INSERT INTO media_edits (media_id, recipe, source) SELECT id, ?1, ?2 FROM media WHERE rel_path = 'a.jpg'",
            (&edit.recipe, &edit.source),
        )
        .unwrap();
    harness.state_mut().refresh();
    harness.key_press(Key::I);
    settle_all(&mut harness);
    harness.get_by_label("Saved over the original, which is kept");
    harness
        .get_by_role_and_label(Role::Button, "Revert to original")
        .click();
    harness.run_steps(2);
    harness.get_by_label("Revert to the original?");
    harness.get_by_role_and_label(Role::Button, "Revert").click();
    harness.run_steps(1);
    settle_all(&mut harness);

    assert_eq!(std::fs::read(dir.join("a.jpg")).unwrap(), before);
    assert!(!backup.exists());
    harness.get_by_label("Reverted to the original");
}

/// REQ-021-11: an item saved over its original edits again from the backup, with the saved recipe.
#[test]
fn req_021_11_edits_again_from_the_original_with_the_saved_recipe() {
    let fixture = Fixture::new();
    let edit = SavedEdit {
        recipe: brighter().to_json(),
        source: ".good-gallery/originals/a.jpg".into(),
    };
    let (mut harness, dir) = app(&fixture, Some(edit), None);
    let backup = dir.join(".good-gallery").join("originals");
    std::fs::create_dir_all(&backup).unwrap();
    std::fs::copy(dir.join("a.jpg"), backup.join("a.jpg")).unwrap();
    open(&mut harness, "a.jpg");
    edit_and_check(&mut harness, Some(".good-gallery/originals/a.jpg"), 1.0);

    // Without the backup, the file itself is the original and nothing is applied yet.
    std::fs::remove_file(backup.join("a.jpg")).unwrap();
    harness.key_press(Key::Escape);
    harness.run_steps(2);
    edit_and_check(&mut harness, Some("a.jpg"), 0.0);
}

fn edit_and_check(harness: &mut App, source: Option<&str>, exposure: f32) {
    edit(harness);
    let session = session(harness).unwrap();
    assert_eq!(session.source(), source);
    assert_eq!(session.recipe().exposure, exposure);
    assert!(!session.has_changes());
}

/// REQ-021-15: _When saving edits_ on the Settings page keeps or updates the date modified, stored at once.
#[test]
fn req_021_15_the_date_modified_setting_is_on_the_settings_page() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, None, None);
    // Tall enough for the whole page.
    harness.set_size(egui::vec2(900.0, 1400.0));
    harness.state_mut().page = app::Page::Settings;
    settle(&mut harness);
    harness
        .get_by_role_and_label(Role::ComboBox, "When saving edits")
        .click();
    harness.run_steps(2);
    harness.get_by_label("Update date modified").click();
    settle(&mut harness);
    assert_eq!(
        harness.state().settings.mtime_on_save,
        domain::edit::MtimeOnSave::Update
    );
    let db = harness.state().db().unwrap();
    assert_eq!(
        db::settings::load(&db.lock().unwrap()).unwrap().mtime_on_save,
        domain::edit::MtimeOnSave::Update
    );
}

/// REQ-021-16: a failed save says why, leaves the file and keeps editing open.
#[test]
fn req_021_16_a_failed_save_keeps_editing_open() {
    let fixture = Fixture::new();
    let broken = media::edit::Tools {
        exiftool: fixture.dir.path().join("no-exiftool"),
        config: fixture.dir.path().join("none.config"),
    };
    let (mut harness, dir) = app(&fixture, None, Some(broken));
    let before = std::fs::read(dir.join("a.jpg")).unwrap();
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    set_recipe(&mut harness, brighter());
    harness.get_by_role_and_label(Role::Button, "Save as copy").click();
    harness.run_steps(1);
    settle_all(&mut harness);

    assert!(
        harness
            .state()
            .toasts
            .current()
            .iter()
            .any(|toast| toast.text.starts_with("Couldn't save"))
    );
    let session = session(&harness).expect("still editing");
    assert!(!session.saving && session.has_changes());
    assert_eq!(std::fs::read(dir.join("a.jpg")).unwrap(), before);
    assert!(!dir.join("a (edited).jpg").exists());
}
