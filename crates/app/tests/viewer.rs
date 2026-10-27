//! The viewer (spec 009) on a library of real photos; without a GPU texture streamer there are no thumbnails, so
//! previews are the ThumbHash placeholders.

mod common;

use app::originals::OriginalState;
use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::rusqlite::Connection;
use db::writer::{IndexWriter, IndexedFile};
use domain::media::{MediaKind, MediaMetadata};
use eframe::egui::{self, Key, Modifiers, PointerButton, pos2};
use egui_kittest::kittest::Queryable as _;

type App = egui_kittest::Harness<'static, app::AppState>;

/// Center of the media area in the 900 × 600 window: between 64 px at the sides, 48 px above and 80 px below.
const CENTER: egui::Pos2 = pos2(450.0, 284.0);

/// A library in `dir` with `count` photos `0.png`, `1.png`, … (newest first), 400 × 300 px each; photos named in
/// `missing` are indexed but not on disk.
fn library(fixture: &Fixture, count: usize, missing: &[&str]) -> Connection {
    let dir = fixture.folder("photos");
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, &dir, "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let files: Vec<IndexedFile> = (0..count)
        .map(|i| {
            let name = format!("{i}.png");
            if !missing.contains(&name.as_str()) {
                image::RgbImage::from_pixel(400, 300, image::Rgb([40 * (i % 6) as u8, 120, 200]))
                    .save(std::path::Path::new(&dir).join(&name))
                    .unwrap();
            }
            IndexedFile {
                rel_path: name.clone(),
                file_name: name,
                kind: MediaKind::Image,
                size: 1,
                mtime: 1_000,
                sidecar_mtime: None,
                metadata: MediaMetadata {
                    width: Some(400),
                    height: Some(300),
                    taken_at: Some(2_000_000 - i as i64),
                    ..Default::default()
                },
            }
        })
        .collect();
    let mut writer = IndexWriter::new(conn);
    writer.write(root, &files).unwrap();
    writer.into()
}

fn app(fixture: &Fixture, count: usize, missing: &[&str]) -> App {
    // Frames at 60 fps, so two clicks a few frames apart make a double-click.
    let mut harness = common::harness_with_step(fixture.state(library(fixture, count, missing)), 1.0 / 60.0);
    settle(&mut harness);
    harness
}

/// Runs frames until the wanted originals have loaded or failed and are on screen.
fn settle_viewer(harness: &mut App) {
    settle(harness);
    if let Some(originals) = harness.state_mut().originals.as_mut() {
        originals.wait();
    }
    harness.run_steps(3);
}

fn open(harness: &mut App, name: &str) {
    harness
        .get_by_role_and_label(egui::accesskit::Role::Image, name)
        .click();
    settle_viewer(harness);
}

fn viewer(harness: &App) -> &app::viewer::Viewer {
    harness.state().viewer.as_ref().expect("the viewer is open")
}

/// The shown photo's name: photos are named after their gallery position.
fn shown(harness: &App) -> Option<String> {
    Some(format!("{}.png", harness.state().viewer.as_ref()?.index()))
}

fn original(harness: &App, index: usize) -> OriginalState {
    let id = harness.state().gallery.as_ref().unwrap().id_at(index).unwrap();
    harness.state().originals.as_ref().unwrap().get(id)
}

fn press(harness: &App, pos: egui::Pos2, pressed: bool) {
    harness.event(egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
}

fn click_at(harness: &mut App, pos: egui::Pos2) {
    harness.hover_at(pos);
    harness.run_steps(1);
    press(harness, pos, true);
    harness.run_steps(1);
    press(harness, pos, false);
    // Half a second of frames, so the next click isn't taken as the second of a double-click.
    harness.run_steps(30);
}

fn double_click_at(harness: &mut App, pos: egui::Pos2) {
    harness.hover_at(pos);
    harness.run_steps(1);
    for _ in 0..2 {
        press(harness, pos, true);
        harness.run_steps(1);
        press(harness, pos, false);
        harness.run_steps(1);
    }
    harness.run_steps(30);
}

fn wheel_at(harness: &mut App, pos: egui::Pos2, lines: f32) {
    harness.hover_at(pos);
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, lines),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
    harness.run_steps(2);
}

/// REQ-009-01, REQ-009-04, REQ-013-14: a click on a tile opens the viewer over the darkened window with the item's
/// name, its original loaded from the library at full size and fitted into the window, never enlarged.
#[test]
fn req_009_01_req_009_04_opens_the_clicked_item_with_its_original() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 3, &[]);

    open(&mut harness, "1.png");

    assert_eq!(shown(&harness).as_deref(), Some("1.png"));
    assert!(
        harness
            .query_by_role_and_label(egui::accesskit::Role::Dialog, "1.png")
            .is_some()
    );
    let OriginalState::Loaded(loaded) = original(&harness, 1) else {
        panic!("original not loaded")
    };
    assert_eq!(loaded.size, [400, 300]);
    assert!(
        harness
            .get_by_role_and_label(egui::accesskit::Role::Button, "Close")
            .rect()
            .bottom()
            > 500.0
    );
    // Past the original's fade-in.
    harness.run_steps(20);
    harness.snapshot("viewer");
}

/// REQ-009-01: Enter on a focused tile opens it too.
#[test]
fn req_009_01_opens_the_focused_tile_with_enter() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 3, &[]);
    harness
        .get_by_role_and_label(egui::accesskit::Role::Image, "2.png")
        .focus();
    harness.run_steps(1);
    harness.key_press(Key::Enter);
    settle_viewer(&mut harness);
    assert_eq!(shown(&harness).as_deref(), Some("2.png"));
}

/// REQ-009-02: ← / → and the buttons step through the gallery's order; no _Previous_ on the first item, no _Next_ on
/// the last.
#[test]
fn req_009_02_steps_through_the_gallery() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 3, &[]);
    open(&mut harness, "0.png");
    assert!(harness.query_by_label("Previous").is_none());

    harness.key_press(Key::ArrowRight);
    settle_viewer(&mut harness);
    assert_eq!(shown(&harness).as_deref(), Some("1.png"));
    harness.get_by_label("Next").click();
    settle_viewer(&mut harness);
    assert_eq!(shown(&harness).as_deref(), Some("2.png"));
    assert!(harness.query_by_label("Next").is_none());
    harness.key_press(Key::ArrowRight);
    settle_viewer(&mut harness);
    assert_eq!(shown(&harness).as_deref(), Some("2.png"), "stays on the last item");

    harness.get_by_label("Previous").click();
    settle_viewer(&mut harness);
    harness.key_press(Key::ArrowLeft);
    settle_viewer(&mut harness);
    assert_eq!(shown(&harness).as_deref(), Some("0.png"));
}

/// REQ-009-03, REQ-007-13: Escape, _Close_, Alt + ←, the mouse's back button and a click on the scrim close the
/// viewer; the search stays.
#[test]
fn req_009_03_req_007_13_closes_in_every_way_and_keeps_the_search() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 3, &[]);
    let mut search = harness.state().gallery.as_ref().unwrap().search().clone();
    search.name = Some("png".into());
    harness.state_mut().gallery.as_mut().unwrap().set_search(search.clone());
    settle(&mut harness);

    type Close = fn(&mut App);
    let ways: [(&str, Close); 5] = [
        ("Escape", |h| h.key_press(Key::Escape)),
        ("Close", |h| h.get_by_label("Close").click()),
        ("Alt + ←", |h| h.key_press_modifiers(Modifiers::ALT, Key::ArrowLeft)),
        ("back button", |h| {
            h.event(egui::Event::PointerButton {
                pos: pos2(450.0, 300.0),
                button: PointerButton::Extra1,
                pressed: true,
                modifiers: Modifiers::NONE,
            })
        }),
        ("scrim", |h| {
            click_at(h, pos2(30.0, 150.0));
            std::thread::sleep(std::time::Duration::from_millis(400));
        }),
    ];
    for (way, close) in ways {
        open(&mut harness, "1.png");
        close(&mut harness);
        harness.run_steps(3);
        assert!(harness.state().viewer.is_none(), "{way} closes");
        assert_eq!(harness.state().gallery.as_ref().unwrap().search(), &search);
    }
}

/// REQ-009-03, REQ-009-16: a click on the photo zooms instead of closing, the end of a pan and a double-click on the
/// scrim don't close.
#[test]
fn req_009_03_req_009_16_clicks_on_the_photo_and_double_clicks_dont_close() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 3, &[]);
    open(&mut harness, "1.png");

    click_at(&mut harness, CENTER);
    std::thread::sleep(std::time::Duration::from_millis(400));
    harness.run_steps(2);
    assert!(viewer(&harness).view().is_zoomed(), "a click on the photo zooms");

    // Panning the zoomed photo, released over it.
    harness.drag_at(CENTER);
    harness.run_steps(1);
    harness.hover_at(CENTER + egui::vec2(60.0, 0.0));
    harness.run_steps(1);
    harness.drop_at(CENTER + egui::vec2(60.0, 0.0));
    harness.run_steps(1);
    std::thread::sleep(std::time::Duration::from_millis(400));
    harness.run_steps(2);
    assert!(harness.state().viewer.is_some(), "the end of a pan doesn't close");

    harness.key_press(Key::Num0);
    harness.run_steps(1);
    double_click_at(&mut harness, pos2(30.0, 150.0));
    std::thread::sleep(std::time::Duration::from_millis(400));
    harness.run_steps(2);
    assert!(
        harness.state().viewer.is_some(),
        "a double-click on the scrim doesn't close"
    );
    assert!(viewer(&harness).view().is_zoomed(), "it zooms");
}

/// REQ-009-16: a click on a fitted photo zooms to actual pixels (at least 2×) around the pointer, another fits it
/// again; a double-click switches only once.
#[test]
fn req_009_16_click_toggles_between_fitted_and_actual_pixels() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 3, &[]);
    open(&mut harness, "1.png");

    click_at(&mut harness, CENTER + egui::vec2(10.0, 20.0));
    let view = viewer(&harness).view();
    // 400 px shown 400 px wide: actual pixels are 1×, so 2×; the point under the pointer stays in place.
    assert_eq!(view.scale, 2.0);
    assert_eq!((view.x, view.y), (-10.0, -20.0));
    click_at(&mut harness, CENTER);
    assert!(!viewer(&harness).view().is_zoomed());

    double_click_at(&mut harness, CENTER);
    assert_eq!(viewer(&harness).view().scale, 2.0, "switched once");
}

/// REQ-009-05: the wheel zooms around the pointer, `+` / `-` around the center, `0` fits; dragging pans a zoomed photo
/// but never into empty space.
#[test]
fn req_009_05_zooms_and_pans() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 3, &[]);
    open(&mut harness, "1.png");

    wheel_at(&mut harness, CENTER, 3.0);
    let view = viewer(&harness).view();
    assert!((view.scale - (300.0f32 * 0.002).exp()).abs() < 1e-3, "{view:?}");
    wheel_at(&mut harness, CENTER, -30.0);
    assert!(!viewer(&harness).view().is_zoomed(), "no smaller than fitted");

    harness.key_press(Key::Plus);
    harness.run_steps(1);
    assert!((viewer(&harness).view().scale - 1.25).abs() < 1e-4);
    for _ in 0..12 {
        harness.key_press(Key::Plus);
        harness.run_steps(1);
    }
    assert_eq!(viewer(&harness).view().scale, 8.0, "at most 8×");
    harness.key_press(Key::Minus);
    harness.run_steps(1);
    assert!((viewer(&harness).view().scale - 6.4).abs() < 1e-4);

    // At 6.4× the photo is 2560 × 1920 in a 772 × 472 area: it pans by at most 894 / 724 px.
    harness.drag_at(CENTER);
    harness.run_steps(1);
    harness.hover_at(CENTER + egui::vec2(-300.0, 0.0));
    harness.run_steps(1);
    harness.drop_at(CENTER + egui::vec2(-300.0, 0.0));
    harness.run_steps(1);
    let view = viewer(&harness).view();
    assert!((view.x + 300.0).abs() < 1.0, "{view:?}");
    for _ in 0..4 {
        harness.drag_at(CENTER);
        harness.run_steps(1);
        harness.hover_at(CENTER + egui::vec2(-300.0, 0.0));
        harness.run_steps(1);
        harness.drop_at(CENTER + egui::vec2(-300.0, 0.0));
        harness.run_steps(1);
    }
    assert!(
        (viewer(&harness).view().x + 894.0).abs() < 1.0,
        "stops at the photo's edge"
    );

    harness.key_press(Key::Num0);
    harness.run_steps(1);
    assert!(!viewer(&harness).view().is_zoomed());
}

/// REQ-009-13: photos up to two places around the shown one load ahead; stepping to one shows its original at once;
/// those further away stop loading.
#[test]
fn req_009_13_req_009_14_preloads_two_places_around() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 8, &[]);
    open(&mut harness, "3.png");

    for (index, loaded) in [
        (0, false),
        (1, true),
        (2, true),
        (3, true),
        (4, true),
        (5, true),
        (6, false),
    ] {
        assert_eq!(
            matches!(original(&harness, index), OriginalState::Loaded(_)),
            loaded,
            "{index}.png"
        );
    }
    harness.key_press(Key::ArrowRight);
    harness.run_steps(1);
    assert!(
        viewer(&harness).shows_original_at_once(),
        "a preloaded original doesn't fade in"
    );
    settle_viewer(&mut harness);
    assert!(!matches!(original(&harness, 1), OriginalState::Loaded(_)), "freed");
    assert!(matches!(original(&harness, 6), OriginalState::Loaded(_)));
}

/// REQ-009-15: an original that can't be loaded says so.
#[test]
fn req_009_15_says_when_the_original_fails() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 3, &["1.png"]);
    open(&mut harness, "1.png");
    assert!(matches!(original(&harness, 1), OriginalState::Failed(_)));
    assert!(harness.query_by_label_contains("Can't load the original").is_some());
}

/// REQ-009-12: an item no longer in the result after a reload shows "Media not found".
#[test]
fn req_009_12_shows_media_not_found_for_a_removed_item() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 3, &[]);
    open(&mut harness, "1.png");
    let id = harness.state().gallery.as_ref().unwrap().id_at(1).unwrap();
    {
        let db = harness.state().db().unwrap();
        db.lock()
            .unwrap()
            .execute("DELETE FROM media WHERE id = ?1", [id])
            .unwrap();
    }
    harness.state_mut().gallery.as_mut().unwrap().reload();
    settle_viewer(&mut harness);
    assert!(harness.query_by_label_contains("Media not found").is_some());
}

/// REQ-013-10: the controls step aside after 2.5 s without pointer movement and come back when it moves.
#[test]
fn req_013_10_controls_step_aside_while_idle() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 3, &[]);
    open(&mut harness, "1.png");
    harness.hover_at(pos2(450.0, 200.0));
    harness.run_steps(1);
    assert!(harness.query_by_label("Close").is_some());

    std::thread::sleep(std::time::Duration::from_millis(2600));
    harness.run_steps(20);
    std::thread::sleep(std::time::Duration::from_millis(250));
    harness.run_steps(2);
    assert!(harness.query_by_label("Close").is_none(), "hidden");

    harness.hover_at(pos2(460.0, 210.0));
    harness.run_steps(3);
    assert!(harness.query_by_label("Close").is_some(), "back on pointer movement");
}

/// REQ-016-06: stepping keeps the shown item in view in the gallery, so closing lands on it.
#[test]
fn req_016_06_scrolls_the_viewed_item_into_view() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 80, &[]);
    open(&mut harness, "0.png");
    for _ in 0..60 {
        harness.key_press(Key::ArrowRight);
        harness.run_steps(1);
    }
    harness.key_press(Key::Escape);
    settle(&mut harness);
    assert!(harness.state().viewer.is_none());
    let tile = harness
        .get_by_role_and_label(egui::accesskit::Role::Image, "60.png")
        .rect();
    assert!(tile.top() >= 48.0 && tile.bottom() <= 600.0, "{tile:?}");
}

/// REQ-009-03: around a zoomed photo, where it is clipped away and the scrim shows, a click closes.
#[test]
fn req_009_03_a_click_beside_a_zoomed_photo_closes() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 3, &[]);
    open(&mut harness, "1.png");
    for _ in 0..6 {
        harness.key_press(Key::Plus);
        harness.run_steps(1);
    }
    // The zoomed photo reaches past the media area into the 48 px strip at the top.
    click_at(&mut harness, pos2(450.0, 20.0));
    std::thread::sleep(std::time::Duration::from_millis(400));
    harness.run_steps(2);
    assert!(harness.state().viewer.is_none());
}

/// REQ-009-01: while the viewer is open, Tab, Enter and Space don't reach the gallery, toolbar or sidebar below it.
#[test]
fn req_009_01_keys_dont_reach_the_window_below() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 3, &[]);
    open(&mut harness, "1.png");
    for key in [Key::Tab, Key::Tab, Key::Enter, Key::Tab, Key::Space, Key::Enter] {
        harness.key_press(key);
        harness.run_steps(1);
    }
    settle(&mut harness);
    assert_eq!(shown(&harness).as_deref(), Some("1.png"));
    assert_eq!(harness.state().page, app::Page::Gallery);
    assert_eq!(viewer(&harness).view(), domain::viewer::FIT);
}
