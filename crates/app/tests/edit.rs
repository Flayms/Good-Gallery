//! Editing photos in the viewer (spec 021) on a library of real files. Without a wgpu renderer for the app, the
//! preview is rendered on the CPU; saving runs exiftool and skips itself without it.

mod common;

use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::rusqlite::Connection;
use db::writer::{IndexWriter, IndexedFile};
use domain::edit::{Adjustment, BlurKind, Channel, MaskShape, Part, Recipe, SavedEdit};
use domain::look::{Look, LookChoice};
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
        read_error: None,
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
    common::open_item(harness, name);
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

    // A double-click on the slider, in view, resets it.
    harness.set_size(egui::vec2(900.0, 1600.0));
    harness.run_steps(2);
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

/// REQ-021-18: with _Curves_ open, the curve editor shows the histogram of what the curve receives; exposure moves it,
/// the curve doesn't.
#[test]
fn req_021_18_the_curve_editor_shows_the_histogram() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, None, None);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    assert!(
        session(&harness).unwrap().histogram().is_none(),
        "only with Curves open"
    );
    // What lies above folded away, so the curve is in view.
    for section in ["Crop and rotate", "Look", "Light"] {
        harness.get_by_role_and_label(Role::Button, section).click();
        harness.run_steps(2);
    }
    harness.get_by_role_and_label(Role::Button, "Curves").click();
    harness.run_steps(2);
    let shown = session(&harness).unwrap().histogram().expect("a histogram").clone();
    assert!(shown.get(Channel::All).iter().sum::<u32>() > 0);
    harness.snapshot(common::snapshot_name("edit_curves"));

    set_recipe(&mut harness, brighter());
    let brighter = session(&harness).unwrap().histogram().unwrap().clone();
    assert_ne!(brighter, shown, "exposure moves it");
    let mut curved = self::brighter();
    curved.curves.all.add_point(0.3, 0.6);
    set_recipe(&mut harness, curved);
    assert_eq!(
        session(&harness).unwrap().histogram().unwrap(),
        &brighter,
        "the curve doesn't"
    );
}

/// REQ-021-20: _Color_ has _Hue_ after _Tint_; turning it turns the photo's colors in the preview.
#[test]
fn req_021_20_the_color_section_has_a_hue_slider() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, None, None);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    // Light folded away, so Color is in view.
    harness.get_by_role_and_label(Role::Button, "Light").click();
    harness.run_steps(2);
    let top = |harness: &App, label: &str| harness.get_by_role_and_label(Role::Slider, label).rect().top();
    let (tint, hue, vibrance) = (top(&harness, "Tint"), top(&harness, "Hue"), top(&harness, "Vibrance"));
    assert!(tint < hue && hue < vibrance, "Tint, Hue, Vibrance");
    let mut recipe = Recipe::default();
    Adjustment::Hue.set(&mut recipe, 120.0);
    set_recipe(&mut harness, recipe);
    assert_eq!(session(&harness).unwrap().recipe().hue, 120.0);
    harness.snapshot(common::snapshot_name("edit_hue"));
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

/// REQ-029-01, REQ-029-05: the buttons turn the photo a quarter at a time, the viewer fits the turned photo, _Compare_
/// shows the whole original, and undo and reset take the turn back.
#[test]
fn req_029_01_req_029_05_turns_the_photo_and_takes_it_back() {
    let fixture = Fixture::new();
    let (mut harness, _dir) = app(&fixture, None, None);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    settle_all(&mut harness);
    let turns = |harness: &App| session(harness).unwrap().recipe().quarter_turns;
    assert_eq!(session(&harness).unwrap().shown_size(false), Some([120.0, 80.0]));

    harness.get_by_role_and_label(Role::Button, "Rotate right").click();
    harness.run_steps(2);
    assert_eq!(turns(&harness), 1);
    assert_eq!(session(&harness).unwrap().shown_size(false), Some([80.0, 120.0]));
    assert_eq!(session(&harness).unwrap().shown_size(true), Some([120.0, 80.0]));

    harness.get_by_role_and_label(Role::Button, "Rotate left").click();
    harness.run_steps(2);
    harness.get_by_role_and_label(Role::Button, "Rotate left").click();
    harness.run_steps(2);
    assert_eq!(turns(&harness), 3, "left of no turn wraps around");

    harness.get_by_role_and_label(Role::Button, "Undo").click();
    harness.run_steps(2);
    assert_eq!(turns(&harness), 0);

    harness.get_by_role_and_label(Role::Button, "Rotate right").click();
    harness.run_steps(2);
    harness.get_by_role_and_label(Role::Button, "Reset all").click();
    harness.run_steps(2);
    assert_eq!(turns(&harness), 0);
}

/// REQ-029-04: the white balance picker and the blur's focus land on the spot clicked on the turned, cropped photo.
#[test]
fn req_029_04_a_pick_lands_on_the_same_spot_of_the_photo() {
    let fixture = Fixture::new();
    let (mut harness, _dir) = app(&fixture, None, None);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    settle_all(&mut harness);
    let recipe = Recipe {
        crop: Some([0.5, 0.0, 0.5, 0.5]),
        quarter_turns: 1,
        ..Default::default()
    };
    set_recipe(&mut harness, recipe);

    let session = harness.state_mut().viewer.as_mut().unwrap().editing_mut().unwrap();
    session.place_focus();
    // The output's top left: turned clockwise, that's the crop's bottom left.
    session.pick(eframe::egui::pos2(0.0, 0.0));
    let focus = session.recipe().blur_focus;
    assert!(
        (focus[0] - 0.5).abs() < 1e-5 && (focus[1] - 0.5).abs() < 1e-5,
        "{focus:?}"
    );
}

/// REQ-029-02, REQ-029-05: _Crop_ shows the whole photo with a frame; a drag resizes it, one undo step each; leaving
/// shows only the cropped part.
#[test]
fn req_029_02_req_029_05_crops_by_dragging_the_frame() {
    let fixture = Fixture::new();
    let (mut harness, _dir) = app(&fixture, None, None);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    settle_all(&mut harness);

    harness.get_by_role_and_label(Role::Button, "Crop").click();
    harness.run_steps(2);
    assert!(session(&harness).unwrap().is_cropping());
    harness.snapshot(common::snapshot_name("edit_crop"));
    let area = session(&harness).unwrap().crop_area().expect("the photo is drawn");

    // The bottom right corner to the photo's middle.
    harness.drag_at(area.right_bottom());
    harness.run_steps(2);
    harness.hover_at(area.center());
    harness.run_steps(2);
    harness.drop_at(area.center());
    harness.run_steps(3);
    let crop = session(&harness).unwrap().recipe().crop.expect("a crop");
    for (value, expected) in crop.iter().zip([0.0, 0.0, 0.5, 0.5]) {
        assert!((value - expected).abs() < 0.02, "{crop:?}");
    }
    assert_eq!(
        session(&harness).unwrap().shown_size(false),
        Some([120.0, 80.0]),
        "the whole photo"
    );

    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    let editing = session(&harness).expect("Escape leaves the crop, not the editor");
    assert!(!editing.is_cropping());
    let [width, height] = editing.shown_size(false).unwrap();
    assert!(
        (width - 60.0).abs() < 2.5 && (height - 40.0).abs() < 2.5,
        "{width} {height}"
    );
    assert_eq!(editing.shown_size(true), Some([120.0, 80.0]), "Compare shows all of it");

    harness.get_by_role_and_label(Role::Button, "Undo").click();
    harness.run_steps(2);
    assert_eq!(session(&harness).unwrap().recipe().crop, None);
}

/// REQ-029-03: a shape gives a centered frame of it; _Swap_ turns 4:3 upright.
#[test]
fn req_029_03_picks_the_frames_shape() {
    let fixture = Fixture::new();
    let (mut harness, _dir) = app(&fixture, None, None);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    settle_all(&mut harness);
    harness.get_by_role_and_label(Role::Button, "Crop").click();
    harness.run_steps(2);
    let shape = |harness: &App| {
        let [_, _, w, h] = session(harness).unwrap().recipe().crop.expect("a crop");
        w * 120.0 / (h * 80.0)
    };

    harness.get_by_label("1:1").click();
    harness.run_steps(2);
    assert!((shape(&harness) - 1.0).abs() < 1e-3);

    // From the whole photo again, for the widest 4:3.
    harness.get_by_role_and_label(Role::Button, "Reset all").click();
    harness.run_steps(2);
    harness.get_by_label("4:3").click();
    harness.run_steps(2);
    assert!((shape(&harness) - 4.0 / 3.0).abs() < 1e-3);
    harness.get_by_label("Swap").click();
    harness.run_steps(2);
    assert!((shape(&harness) - 3.0 / 4.0).abs() < 1e-3);

    harness.get_by_label("Done").click();
    harness.run_steps(2);
    assert!(!session(&harness).unwrap().is_cropping());
}

/// REQ-029-06: saving ends the crop and writes only what the frame held.
#[test]
fn req_029_06_saves_what_the_frame_holds() {
    let fixture = Fixture::new();
    let Some(tools) = tools(&fixture) else { return };
    let (mut harness, dir) = app(&fixture, None, Some(tools));
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    settle_all(&mut harness);
    let recipe = Recipe {
        crop: Some([0.5, 0.0, 0.5, 0.5]),
        quarter_turns: 1,
        ..Default::default()
    };
    set_recipe(&mut harness, recipe);
    harness.get_by_role_and_label(Role::Button, "Save as copy").click();
    harness.run_steps(1);
    settle_all(&mut harness);
    assert_eq!(image::image_dimensions(dir.join("a (edited).jpg")).unwrap(), (40, 60));
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
    harness.set_size(egui::vec2(900.0, 1600.0));
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

/// Replaces `a.jpg` with a red disc on a grey-blue wall, the same size, so it has a subject.
fn disc_photo(dir: &Path) {
    let disc = image::RgbImage::from_fn(120, 80, |x, y| {
        let (dx, dy) = (x as f32 - 60.0, y as f32 - 40.0);
        if dx * dx + dy * dy < 22.0 * 22.0 {
            image::Rgb([220, 70, 40])
        } else {
            image::Rgb([70, 90, 110])
        }
    });
    disc.save(dir.join("a.jpg")).unwrap();
}

/// REQ-022-01, REQ-022-02, REQ-022-04: _Subject_ looks for the subject, then offers light and color only for it, and
/// _Show selection_ tints it; _Photo_ has everything again.
#[test]
fn req_022_01_req_022_02_req_022_04_edits_the_subject_found_in_the_photo() {
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, None, None);
    disc_photo(&dir);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    assert_eq!(session(&harness).unwrap().part(), Part::Photo);
    assert!(
        session(&harness).unwrap().subject().is_none(),
        "only looked for when needed"
    );

    harness.get_by_role_and_label(Role::Button, "Subject").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("Finding the subject…").is_some());
    settle_all(&mut harness);
    let found = session(&harness).unwrap().subject().expect("a subject");
    assert!(found.sample([0.5, 0.5]) > 0.8 && found.sample([0.03, 0.05]) < 0.2);
    // Light and color only.
    assert!(harness.query_by_label("Exposure").is_some() && harness.query_by_label("Saturation").is_some());
    assert!(harness.query_by_label("Curves").is_none() && harness.query_by_label("Effects").is_none());

    let viewer = harness.state_mut().viewer.as_mut().unwrap();
    let session = viewer.editing_mut().unwrap();
    let mut recipe = session.recipe().clone();
    Adjustment::Exposure.set(recipe.part_mut(Part::Subject), 1.5);
    session.commit(recipe);
    harness.run_steps(2);
    assert!(self::session(&harness).unwrap().tints_selection(), "tinted at once");
    harness.snapshot(common::snapshot_name("edit_subject"));
    assert_eq!(
        self::session(&harness)
            .unwrap()
            .recipe()
            .part(Part::Subject)
            .unwrap()
            .exposure,
        1.5
    );
    assert_eq!(
        self::session(&harness).unwrap().recipe().exposure,
        0.0,
        "the photo's stays"
    );

    harness.get_by_role_and_label(Role::Button, "Photo").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("Curves").is_some() && harness.query_by_label("Effects").is_some());
}

/// REQ-022-05: a photo saved with subject adjustments is edited again with them, and its subject is looked for again.
#[test]
fn req_022_05_edits_again_with_the_saved_parts() {
    let fixture = Fixture::new();
    let mut recipe = Recipe::default();
    Adjustment::Shadows.set(recipe.part_mut(Part::Background), -40.0);
    let edit = SavedEdit {
        recipe: recipe.to_json(),
        source: "a.jpg".into(),
    };
    let (mut harness, dir) = app(&fixture, Some(edit), None);
    disc_photo(&dir);
    open(&mut harness, "a.jpg");
    edit_and_check(&mut harness, Some("a.jpg"), 0.0);
    let session = session(&harness).unwrap();
    assert_eq!(session.recipe().part(Part::Background).unwrap().shadows, -40.0);
    assert!(session.subject().is_some(), "found again without choosing a part");
}

/// REQ-023-01, REQ-023-02, REQ-023-03: _Effects_ offers the blurs; a focus blur has a size, a transition and a focus
/// to place, _Background_ looks for the subject.
#[test]
fn req_023_01_req_023_02_req_023_03_chooses_a_blur_and_places_its_focus() {
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, None, None);
    disc_photo(&dir);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    harness.set_size(egui::vec2(900.0, 800.0));
    harness.run_steps(2);
    // Crop and rotate, Look, Light and Color folded away, Effects open.
    for section in ["Crop and rotate", "Look", "Light", "Color", "Effects"] {
        harness.get_by_role_and_label(Role::Button, section).click();
        harness.run_steps(2);
    }
    assert!(harness.query_by_label("Focus size").is_none(), "no blur yet");
    harness.get_by_role_and_label(Role::Button, "Tilt-shift").click();
    harness.run_steps(2);
    let recipe = session(&harness).unwrap().recipe().clone();
    assert_eq!(recipe.blur_kind, BlurKind::TiltShift);
    assert!(recipe.blur > 0.0 && !recipe.is_neutral());
    assert!(harness.query_by_label("Focus size").is_some() && harness.query_by_label("Transition").is_some());

    // The panel scrolls; the button has to be in view to take the click.
    let size = harness.ctx.content_rect().size();
    harness.set_size(egui::vec2(size.x, size.y + 100.0));
    harness.run_steps(2);
    harness.get_by_role_and_label(Role::Button, "Place focus").click();
    harness.run_steps(2);
    harness.set_size(size);
    harness.run_steps(2);
    assert!(session(&harness).unwrap().is_picking());
    let viewer = harness.state_mut().viewer.as_mut().unwrap();
    viewer.editing_mut().unwrap().pick(egui::pos2(0.5, 0.25));
    harness.run_steps(2);
    let editing = session(&harness).unwrap();
    assert_eq!(editing.recipe().blur_focus, [0.5, 0.25]);
    assert!(!editing.is_picking());
    harness.snapshot(common::snapshot_name("edit_blur"));

    // Behind the subject: the subject is looked for.
    assert!(session(&harness).unwrap().subject().is_none());
    harness.get_by_role_and_label(Role::Button, "Behind subject").click();
    harness.run_steps(2);
    settle_all(&mut harness);
    assert_eq!(session(&harness).unwrap().recipe().blur_kind, BlurKind::Background);
    assert!(session(&harness).unwrap().subject().is_some());
    assert!(harness.query_by_label("Place focus").is_none());
}

/// REQ-028-01, REQ-028-02, REQ-028-03: _Look_ offers the film looks; a picked one shows _Strength_ and _Grain_, each
/// pick is one undo step, and _None_ takes it away.
#[test]
fn req_028_01_req_028_02_req_028_03_picks_a_look_and_sets_its_strength_and_grain() {
    let fixture = Fixture::new();
    let (mut harness, _dir) = app(&fixture, None, None);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    assert!(harness.query_by_label("Strength").is_none(), "no look yet");

    harness.get_by_role_and_label(Role::Button, "Portra 400").click();
    harness.run_steps(2);
    let look = |harness: &App| session(harness).unwrap().recipe().look;
    assert_eq!(look(&harness), Some(LookChoice::new(Look::Portra400)));
    assert!(harness.query_by_label("Strength").is_some() && harness.query_by_label("Grain").is_some());
    harness.snapshot(common::snapshot_name("edit_look"));

    // Strength and grain carry over to another look.
    harness.get_by_role_and_label(Role::Slider, "Strength").focus();
    harness.run_steps(1);
    harness.key_press(egui::Key::ArrowLeft);
    harness.run_steps(1);
    harness.get_by_role_and_label(Role::Slider, "Grain").focus();
    harness.run_steps(1);
    harness.key_press(egui::Key::ArrowRight);
    harness.run_steps(2);
    let tuned = look(&harness).unwrap();
    assert_eq!((tuned.strength, tuned.grain), (99.0, 101.0));
    harness.get_by_role_and_label(Role::Button, "HP5").click();
    harness.run_steps(2);
    let hp5 = look(&harness).unwrap();
    assert_eq!((hp5.look, hp5.strength, hp5.grain), (Look::Hp5, 99.0, 101.0));

    // Undo goes back a pick, None takes the look away.
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.run_steps(2);
    assert_eq!(look(&harness).map(|choice| choice.look), Some(Look::Portra400));
    harness.get_by_role_and_label(Role::Button, "None").click();
    harness.run_steps(2);
    assert_eq!(look(&harness), None);
    assert!(harness.query_by_label("Strength").is_none());
}

/// Room for the panel's sections.
fn open_masks(harness: &mut App) {
    harness.set_size(egui::vec2(900.0, 1600.0));
    harness.run_steps(2);
}

/// REQ-030-01, REQ-030-06, REQ-030-07: masks are added, chosen, changed and removed, each as an undo step.
#[test]
fn req_030_01_req_030_06_req_030_07_adds_changes_and_removes_masks() {
    let fixture = Fixture::new();
    let (mut harness, _dir) = app(&fixture, None, None);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    settle_all(&mut harness);
    open_masks(&mut harness);

    harness.get_by_role_and_label(Role::Button, "Add luminance").click();
    harness.run_steps(2);
    let masks = |harness: &App| session(harness).unwrap().recipe().masks.clone();
    assert_eq!(masks(&harness)[0].shape, MaskShape::LUMINANCE);
    assert_eq!(session(&harness).unwrap().part(), Part::Mask(0));
    assert!(harness.query_by_label("Curves").is_none(), "light and color only");
    harness.snapshot(common::snapshot_name("edit_mask"));

    harness.get_by_role_and_label(Role::Slider, "From").focus();
    harness.run_steps(1);
    harness.key_press(Key::ArrowLeft);
    harness.run_steps(2);
    match masks(&harness)[0].shape {
        MaskShape::Luminance { low, .. } => assert_eq!(low, 59.0),
        other => panic!("{other:?}"),
    }

    harness.get_by_role_and_label(Role::Button, "Invert").click();
    harness.run_steps(2);
    assert!(masks(&harness)[0].invert);
    harness.get_by_role_and_label(Role::Button, "Show selection").click();
    harness.run_steps(2);
    assert!(masks(&harness)[0].invert, "showing it changes nothing");

    harness.get_by_role_and_label(Role::Button, "Add color").click();
    harness.run_steps(2);
    assert_eq!(masks(&harness).len(), 2);
    assert_eq!(session(&harness).unwrap().part(), Part::Mask(1));
    harness.get_by_role_and_label(Role::Button, "Remove mask").click();
    harness.run_steps(2);
    assert_eq!(masks(&harness).len(), 1);
    assert_eq!(session(&harness).unwrap().part(), Part::Photo);

    // Undo brings it back; once more takes it away, and with it the choice of it.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run_steps(2);
    assert_eq!(masks(&harness).len(), 2);
    harness.get_by_role_and_label(Role::Button, "Color 2").click();
    harness.run_steps(2);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run_steps(3);
    assert_eq!(masks(&harness).len(), 1);
    assert_eq!(
        session(&harness).unwrap().part(),
        Part::Photo,
        "the chosen mask is gone"
    );
}

/// REQ-030-15: the parts, the masks and the buttons to add one come before all else, not folded away.
#[test]
fn req_030_15_what_to_edit_comes_first() {
    let fixture = Fixture::new();
    let (mut harness, _dir) = app(&fixture, None, None);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    settle_all(&mut harness);
    harness.set_size(egui::vec2(900.0, 1600.0));
    harness.run_steps(2);

    assert!(
        harness.query_by_role_and_label(Role::Button, "Masks").is_none(),
        "no section"
    );
    let top = |harness: &App, label: &str| harness.get_by_role_and_label(Role::Button, label).rect().top();
    let adds = [
        "Add gradient",
        "Add luminance",
        "Add color",
        "Add brush",
        "Add freeform",
    ];
    for label in adds {
        assert!(top(&harness, "Photo") < top(&harness, label), "{label}");
        assert!(top(&harness, label) < top(&harness, "Crop and rotate"), "{label}");
    }

    // A mask joins the list, and its place in it is before its controls.
    harness.get_by_role_and_label(Role::Button, "Add color").click();
    harness.run_steps(2);
    assert_eq!(top(&harness, "Photo"), top(&harness, "Subject"));
    assert_eq!(top(&harness, "Photo"), top(&harness, "Background"));
    assert!(top(&harness, "Color 1") < top(&harness, "Add color"));
    assert!(top(&harness, "Add color") < top(&harness, "Remove mask"));

    // Only masks can be removed.
    for part in ["Photo", "Subject", "Background"] {
        harness.get_by_role_and_label(Role::Button, part).click();
        harness.run_steps(2);
        assert!(harness.query_by_label("Remove mask").is_none(), "{part}");
    }
}

/// REQ-030-07, REQ-022-04: a chosen mask or part is tinted at once; _Show selection_ turns it off for all of them, while
/// _Photo_ has nothing to show.
#[test]
fn req_030_07_tints_by_default_until_turned_off() {
    let fixture = Fixture::new();
    let (mut harness, _dir) = app(&fixture, None, None);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    settle_all(&mut harness);
    harness.set_size(egui::vec2(900.0, 1600.0));
    harness.run_steps(2);
    let tints = |harness: &App| session(harness).unwrap().tints_selection();

    assert!(!tints(&harness), "the photo");
    assert!(harness.query_by_label("Show selection").is_none());
    harness.get_by_role_and_label(Role::Button, "Add gradient").click();
    harness.run_steps(2);
    assert!(tints(&harness), "on when added");
    harness.get_by_role_and_label(Role::Button, "Photo").click();
    harness.run_steps(2);
    assert!(!tints(&harness));
    harness.get_by_role_and_label(Role::Button, "Gradient 1").click();
    harness.run_steps(2);
    assert!(tints(&harness), "on when chosen");

    harness.get_by_role_and_label(Role::Button, "Show selection").click();
    harness.run_steps(2);
    assert!(!tints(&harness), "off");
    harness.get_by_role_and_label(Role::Button, "Add color").click();
    harness.run_steps(2);
    assert!(!tints(&harness), "stays off for the next mask");
    harness.get_by_role_and_label(Role::Button, "Show selection").click();
    harness.run_steps(2);
    assert!(tints(&harness), "on again");
}

/// REQ-030-16: holding an adjustment's slider hides the tint, holding one of the selection's own doesn't.
#[test]
fn req_030_16_the_tint_steps_aside_while_adjusting() {
    let fixture = Fixture::new();
    let (mut harness, _dir) = app(&fixture, None, None);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    settle_all(&mut harness);
    harness.set_size(egui::vec2(900.0, 1600.0));
    harness.run_steps(2);
    harness.get_by_role_and_label(Role::Button, "Add luminance").click();
    harness.run_steps(2);
    let tints = |harness: &App| session(harness).unwrap().tints_selection();
    let middle = |harness: &App, label: &str| harness.get_by_role_and_label(Role::Slider, label).rect().center();

    let exposure = middle(&harness, "Exposure");
    harness.drag_at(exposure);
    harness.run_steps(2);
    assert!(!tints(&harness), "hidden while held");
    harness.drop_at(exposure);
    harness.run_steps(2);
    assert!(tints(&harness), "back when let go");

    let from = middle(&harness, "From");
    harness.drag_at(from);
    harness.run_steps(2);
    assert!(tints(&harness), "it changes the selection");
    harness.drop_at(from);
    harness.run_steps(2);
    assert!(tints(&harness));
}

/// REQ-030-03: a handle follows the drag, in one undo step.
#[test]
fn req_030_03_drags_a_gradients_handle() {
    let fixture = Fixture::new();
    let (mut harness, _dir) = app(&fixture, None, None);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    settle_all(&mut harness);
    open_masks(&mut harness);

    harness.get_by_role_and_label(Role::Button, "Add gradient").click();
    harness.run_steps(3);
    harness.snapshot(common::snapshot_name("edit_gradient"));
    let area = session(&harness).unwrap().gradient_area().expect("the photo is drawn");
    let at = |u: f32, v: f32| area.min + egui::vec2(u * area.width(), v * area.height());

    harness.drag_at(at(0.5, 0.1));
    harness.run_steps(2);
    harness.hover_at(at(0.25, 0.3));
    harness.run_steps(2);
    harness.drop_at(at(0.25, 0.3));
    harness.run_steps(3);
    match session(&harness).unwrap().recipe().masks[0].shape {
        MaskShape::Linear { from, to } => {
            assert!(
                (from[0] - 0.25).abs() < 0.03 && (from[1] - 0.3).abs() < 0.03,
                "{from:?}"
            );
            assert_eq!(to, [0.5, 0.5]);
        }
        other => panic!("{other:?}"),
    }
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run_steps(3);
    assert_eq!(session(&harness).unwrap().recipe().masks[0].shape, MaskShape::LINEAR);
}

/// REQ-030-12: a drag paints a stroke, thinned, as one undo step; a click a dot; _Erase_ and _Clear_ do as they say.
#[test]
fn req_030_12_paints_and_erases_on_the_photo() {
    let fixture = Fixture::new();
    let (mut harness, _dir) = app(&fixture, None, None);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    settle_all(&mut harness);
    open_masks(&mut harness);

    harness.get_by_role_and_label(Role::Button, "Add brush").click();
    harness.run_steps(3);
    let strokes = |harness: &App| session(harness).unwrap().recipe().paintings[0].strokes.clone();
    assert!(strokes(&harness).is_empty());
    assert!(session(&harness).unwrap().has_photo_tool());
    harness.snapshot(common::snapshot_name("edit_brush"));
    let area = session(&harness).unwrap().gradient_area().expect("the photo is drawn");
    let at = |u: f32, v: f32| area.min + egui::vec2(u * area.width(), v * area.height());

    harness.drag_at(at(0.2, 0.5));
    harness.run_steps(2);
    for u in [0.3, 0.4, 0.5, 0.6] {
        harness.hover_at(at(u, 0.5));
        harness.run_steps(2);
    }
    harness.drop_at(at(0.7, 0.5));
    harness.run_steps(3);
    let drawn = strokes(&harness);
    assert_eq!(drawn.len(), 1);
    assert!(!drawn[0].erase && !drawn[0].closed);
    assert_eq!(
        drawn[0].points.len(),
        2,
        "a straight line is two points: {:?}",
        drawn[0].points
    );
    let first = drawn[0].points[0];
    assert!(
        (first[0] - 0.2).abs() < 0.03 && (first[1] - 0.5).abs() < 0.03,
        "{first:?}"
    );

    harness.get_by_role_and_label(Role::Button, "Erase").click();
    harness.run_steps(2);
    harness.drag_at(at(0.5, 0.5));
    harness.run_steps(2);
    harness.drop_at(at(0.5, 0.5));
    harness.run_steps(3);
    let drawn = strokes(&harness);
    assert_eq!(drawn.len(), 2, "a click is a dot");
    assert!(drawn[1].erase);
    assert_eq!(drawn[1].points.len(), 1);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run_steps(2);
    assert_eq!(strokes(&harness).len(), 1, "one undo step");
    harness.get_by_role_and_label(Role::Button, "Clear").click();
    harness.run_steps(2);
    assert!(strokes(&harness).is_empty());

    // Removing the mask takes its painting along.
    harness.get_by_role_and_label(Role::Button, "Remove mask").click();
    harness.run_steps(2);
    assert!(session(&harness).unwrap().recipe().paintings.is_empty());
}

/// REQ-030-13: a drag draws an outline; one too short to enclose anything is dropped.
#[test]
fn req_030_13_draws_an_outline_on_the_photo() {
    let fixture = Fixture::new();
    let (mut harness, _dir) = app(&fixture, None, None);
    open(&mut harness, "a.jpg");
    edit(&mut harness);
    settle_all(&mut harness);
    open_masks(&mut harness);

    harness.get_by_role_and_label(Role::Button, "Add freeform").click();
    harness.run_steps(3);
    assert_eq!(
        session(&harness).unwrap().recipe().masks[0].shape,
        MaskShape::Painted {
            layer: 0,
            freeform: true
        }
    );
    assert!(harness.query_by_label("Size").is_none(), "an outline has no brush size");
    let area = session(&harness).unwrap().gradient_area().expect("the photo is drawn");
    let at = |u: f32, v: f32| area.min + egui::vec2(u * area.width(), v * area.height());

    harness.drag_at(at(0.2, 0.2));
    harness.run_steps(2);
    for (u, v) in [(0.8, 0.2), (0.8, 0.8), (0.2, 0.8)] {
        harness.hover_at(at(u, v));
        harness.run_steps(2);
    }
    harness.drop_at(at(0.2, 0.8));
    harness.run_steps(3);
    let strokes = session(&harness).unwrap().recipe().paintings[0].strokes.clone();
    assert_eq!(strokes.len(), 1);
    assert!(strokes[0].closed);
    assert_eq!(strokes[0].points.len(), 4, "{:?}", strokes[0].points);

    // A straight line encloses nothing.
    harness.drag_at(at(0.3, 0.3));
    harness.run_steps(2);
    harness.hover_at(at(0.5, 0.3));
    harness.run_steps(2);
    harness.drop_at(at(0.7, 0.3));
    harness.run_steps(3);
    assert_eq!(session(&harness).unwrap().recipe().paintings[0].strokes.len(), 1);
}
