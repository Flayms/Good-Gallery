//! The viewer (spec 009) on a library of real photos; without a GPU texture streamer there are no thumbnails, so
//! previews are the ThumbHash placeholders.

mod common;

use app::originals::OriginalState;
use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::rusqlite::Connection;
use db::writer::{IndexWriter, IndexedFile};
use domain::media::{MediaKind, MediaMetadata};
use eframe::egui::accesskit::{Role, Toggled};
use eframe::egui::{self, Key, Modifiers, PointerButton, pos2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};

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
    harness.snapshot(common::snapshot_name("viewer"));
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

/// Records which files Explorer was asked to show.
#[derive(Default)]
struct RecordedExplorer(std::sync::Mutex<Vec<std::path::PathBuf>>);

impl app::state::FileManager for RecordedExplorer {
    fn show_in_folder(&self, file: &std::path::Path) -> std::io::Result<()> {
        self.0.lock().unwrap().push(file.to_path_buf());
        Ok(())
    }
}

fn indexed(i: i64, name: &str, kind: MediaKind, tags: &[&[&str]], rating: Option<i64>) -> IndexedFile {
    IndexedFile {
        rel_path: name.into(),
        file_name: name.into(),
        kind,
        size: 2048,
        mtime: 1_000,
        sidecar_mtime: None,
        metadata: MediaMetadata {
            width: Some(400),
            height: Some(300),
            taken_at: Some(2_000_000 - i),
            rating,
            tags: tags
                .iter()
                .map(|path| path.iter().map(|s| s.to_string()).collect())
                .collect(),
            ..Default::default()
        },
    }
}

/// `0.png` tagged Beach, People › Anna and Places › Italy › Rome, rated 4, with a reddish ThumbHash; `1.png`;
/// `2.mp4`, a video; `3.png`. Photos are on disk, the video isn't.
fn tagged_app(fixture: &Fixture, explorer: std::sync::Arc<RecordedExplorer>) -> App {
    use base64::Engine as _;
    let dir = fixture.folder("photos");
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, &dir, "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let files = [
        indexed(
            0,
            "0.png",
            MediaKind::Image,
            &[&["Beach"], &["People", "Anna"], &["Places", "Italy", "Rome"]],
            Some(4),
        ),
        indexed(1, "1.png", MediaKind::Image, &[], None),
        indexed(2, "2.mp4", MediaKind::Video, &[], None),
        indexed(3, "3.png", MediaKind::Image, &[], None),
    ];
    for name in ["0.png", "1.png", "3.png"] {
        image::RgbImage::from_pixel(400, 300, image::Rgb([200, 40, 40]))
            .save(std::path::Path::new(&dir).join(name))
            .unwrap();
    }
    let mut writer = IndexWriter::new(conn);
    writer.write(root, &files).unwrap();
    let conn: Connection = writer.into();
    let red = image::RgbaImage::from_pixel(16, 12, image::Rgba([220, 30, 30, 255]));
    let hash = thumbhash::rgba_to_thumb_hash(16, 12, red.as_raw());
    conn.execute(
        "UPDATE media SET thumbhash = ?1 WHERE file_name = '0.png'",
        [base64::engine::general_purpose::STANDARD.encode(hash)],
    )
    .unwrap();
    let state = fixture.state(conn).with_file_manager(explorer);
    let mut harness = common::harness_with_step(state, 1.0 / 60.0);
    // Tall enough for the whole info panel.
    harness.set_size(egui::vec2(900.0, 760.0));
    settle(&mut harness);
    harness
}

fn right_click_at(harness: &mut App, pos: egui::Pos2) {
    harness.hover_at(pos);
    harness.run_steps(1);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        harness.run_steps(1);
    }
    harness.run_steps(2);
}

/// A tag in the info panel, at the right of the window (the sidebar lists tags too).
fn panel_chip<'a>(harness: &'a App, name: &'a str) -> egui_kittest::Node<'a> {
    harness
        .query_all_by_role_and_label(Role::Button, name)
        .find(|node| node.rect().left() > 500.0)
        .unwrap_or_else(|| panic!("{name} in the info panel"))
}

/// Hides the file's location, which lies in a temporary folder named differently on every run and machine. Up to the
/// window's edge: the label is only as wide as the path, which differs between machines.
fn mask_location(harness: &mut App) {
    let location = harness
        .get_by_label_contains(&format!("photos{}", std::path::MAIN_SEPARATOR))
        .rect();
    let right = harness.ctx.content_rect().right();
    harness.mask(egui::Rect::from_min_max(
        location.min,
        egui::pos2(right, location.max.y),
    ));
}

fn search(harness: &App) -> domain::search::GallerySearch {
    harness.state().gallery.as_ref().unwrap().search().clone()
}

/// REQ-009-08, REQ-013-13: `i` and _Info_ toggle the info panel with the file's details and its tags by category; the
/// photo moves aside, so the panel never covers it.
#[test]
fn req_009_08_req_013_13_toggles_the_info_panel_with_the_details() {
    let fixture = Fixture::new();
    let mut harness = tagged_app(&fixture, Default::default());
    open(&mut harness, "0.png");

    harness.key_press(Key::I);
    settle(&mut harness);
    assert!(viewer(&harness).info_open());
    for label in [
        "File",
        "Location",
        "Library",
        "Taken",
        "Modified",
        "Dimensions",
        "Size",
        "Rating",
    ] {
        assert!(harness.query_by_label(label).is_some(), "{label}");
    }
    assert!(harness.query_by_label("400 × 300").is_some());
    assert!(harness.query_by_label("2.0 KB").is_some());
    assert!(harness.query_by_label("Rated 4 out of 5").is_some());
    // The sidebar names People, Places and tags too; the panel is at the right.
    let anna = panel_chip(&harness, "Anna").rect();
    let rome = panel_chip(&harness, "Rome").rect();
    let beach = panel_chip(&harness, "Beach").rect();
    assert!(
        anna.top() < rome.top() && rome.top() < beach.top(),
        "People, Places, Tags"
    );
    let photo = viewer(&harness).photo_rect().unwrap();
    assert!(photo.right() < anna.left(), "the photo stays beside the panel");
    // Past the original's fade-in; the temporary folder's name changes with every run.
    settle_viewer(&mut harness);
    harness.run_steps(20);
    mask_location(&mut harness);
    harness.snapshot(common::snapshot_name("viewer_info"));

    harness.get_by_label("Info").click();
    settle(&mut harness);
    assert!(!viewer(&harness).info_open());
}

/// REQ-009-09: a click on a tag in the info panel closes the viewer and filters the gallery by it, keeping the other
/// filters.
#[test]
fn req_009_09_filters_by_a_tag_from_the_info_panel() {
    let fixture = Fixture::new();
    let mut harness = tagged_app(&fixture, Default::default());
    let mut filtered = search(&harness);
    filtered.name = Some("png".into());
    harness.state_mut().gallery.as_mut().unwrap().set_search(filtered);
    settle(&mut harness);
    open(&mut harness, "0.png");
    harness.key_press(Key::I);
    settle(&mut harness);

    panel_chip(&harness, "Beach").click();
    settle(&mut harness);

    assert!(harness.state().viewer.is_none());
    assert_eq!(search(&harness).tags, ["Beach"]);
    assert_eq!(search(&harness).name.as_deref(), Some("png"));
}

/// REQ-009-10: _Show in Explorer_ asks Explorer to show the file; a file that's gone gets an error toast instead.
#[test]
fn req_009_10_shows_the_file_in_explorer_or_says_it_is_gone() {
    let fixture = Fixture::new();
    let explorer = std::sync::Arc::new(RecordedExplorer::default());
    let mut harness = tagged_app(&fixture, explorer.clone());
    open(&mut harness, "0.png");
    harness.get_by_label(app::state::SHOW_IN_FOLDER).click();
    settle(&mut harness);
    let file = std::path::Path::new(&fixture.folder("photos")).join("0.png");
    assert_eq!(*explorer.0.lock().unwrap(), [file]);
    assert!(harness.state().viewer.is_some(), "the viewer stays");

    // The video's file isn't on disk.
    for _ in 0..2 {
        harness.key_press(Key::ArrowRight);
        harness.run_steps(1);
    }
    settle(&mut harness);
    assert_eq!(shown(&harness).as_deref(), Some("2.png"));
    harness.get_by_label(app::state::SHOW_IN_FOLDER).click();
    settle(&mut harness);
    assert_eq!(explorer.0.lock().unwrap().len(), 1);
    let toasts = harness.state().toasts.current();
    assert!(
        toasts
            .last()
            .is_some_and(|toast| toast.text.contains("no longer exists")),
        "{toasts:?}"
    );
}

/// REQ-009-11: a tile's context menu shows the file in Explorer or opens the info sheet beside the gallery; the
/// viewer's opens its info panel.
#[test]
fn req_009_11_context_menus_show_in_explorer_and_information() {
    let fixture = Fixture::new();
    let explorer = std::sync::Arc::new(RecordedExplorer::default());
    let mut harness = tagged_app(&fixture, explorer.clone());
    let tile = harness.get_by_role_and_label(Role::Image, "1.png").rect().center();

    right_click_at(&mut harness, tile);
    harness.get_by_label(app::state::SHOW_IN_FOLDER).click();
    settle(&mut harness);
    assert_eq!(explorer.0.lock().unwrap().len(), 1);

    right_click_at(&mut harness, tile);
    harness.get_by_label("Information").click();
    settle(&mut harness);
    assert!(harness.state().viewer.is_none(), "without opening the viewer");
    assert!(harness.query_by_label("Close information").is_some());
    assert!(harness.query_by_label("Dimensions").is_some());
    mask_location(&mut harness);
    harness.snapshot(common::snapshot_name("info_sheet"));
    harness.get_by_label("Close information").click();
    settle(&mut harness);
    assert!(harness.state().info_sheet.is_none());

    // The sheet steps aside for the viewer, which has its own panel.
    right_click_at(&mut harness, tile);
    harness.get_by_label("Information").click();
    settle(&mut harness);
    open(&mut harness, "0.png");
    assert!(harness.query_by_label("Close information").is_none());
    right_click_at(&mut harness, CENTER);
    harness.get_by_label("Information").click();
    settle(&mut harness);
    assert!(viewer(&harness).info_open());
    assert!(
        panel_chip(&harness, "Beach").rect().left() > 500.0,
        "details of the shown item"
    );
}

fn strip_item<'a>(harness: &'a App, name: &'a str) -> egui_kittest::Node<'a> {
    harness.get_by_role_and_label(Role::Button, name)
}

fn filmstrip(harness: &App) -> Option<egui::Rect> {
    harness
        .query_by_role_and_label(Role::Navigation, "Filmstrip")
        .map(|node| node.rect())
}

/// REQ-015-01, REQ-015-02, REQ-015-03: `f` and _Filmstrip_ toggle a row of the gallery's thumbnails below the photo,
/// which fits above it; the choice is remembered; the shown item is selected and centered.
#[test]
fn req_015_01_req_015_02_req_015_03_toggles_and_remembers_the_filmstrip() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 40, &[]);
    open(&mut harness, "20.png");
    assert!(filmstrip(&harness).is_none(), "hidden until the user shows it");

    harness.key_press(Key::F);
    settle(&mut harness);
    let strip = filmstrip(&harness).expect("shown");
    assert!(
        viewer(&harness).photo_rect().unwrap().bottom() <= strip.top(),
        "the photo fits above the filmstrip"
    );
    let current = strip_item(&harness, "20.png");
    assert_eq!(current.accesskit_node().toggled(), Some(Toggled::True));
    assert!((current.rect().center().x - strip.center().x).abs() < 1.0, "centered");
    assert_eq!(
        strip_item(&harness, "21.png").accesskit_node().toggled(),
        Some(Toggled::False)
    );
    // Past the original's fade-in.
    settle_viewer(&mut harness);
    harness.run_steps(20);
    harness.snapshot(common::snapshot_name("viewer_filmstrip"));

    harness.key_press(Key::ArrowRight);
    settle(&mut harness);
    assert_eq!(
        strip_item(&harness, "21.png").accesskit_node().toggled(),
        Some(Toggled::True)
    );
    // The info panel narrows the filmstrip: the shown item is centered again.
    harness.key_press(Key::I);
    settle(&mut harness);
    let narrower = filmstrip(&harness).unwrap();
    assert!(narrower.width() < strip.width());
    let center = strip_item(&harness, "21.png").rect().center().x;
    assert!((center - narrower.center().x).abs() < 1.0, "{center} in {narrower:?}");
    harness.key_press(Key::I);
    settle(&mut harness);

    // Remembered for the next viewer.
    harness.key_press(Key::Escape);
    settle(&mut harness);
    open(&mut harness, "3.png");
    assert!(viewer(&harness).filmstrip_shown());
    harness.get_by_role_and_label(Role::Button, "Filmstrip").click();
    settle(&mut harness);
    assert!(filmstrip(&harness).is_none());
    let stored = {
        let db = harness.state().db().unwrap();
        let conn = db.lock().unwrap();
        db::settings::value(&conn, domain::viewer::FILMSTRIP_KEY).unwrap()
    };
    assert_eq!(stored, Some(serde_json::json!("hidden")));
}

/// REQ-015-04, REQ-015-05: a click on a thumbnail shows that item and preloads around it; the wheel scrolls the
/// filmstrip sideways without changing the shown item.
#[test]
fn req_015_04_req_015_05_jumps_to_and_browses_the_filmstrip() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 40, &[]);
    open(&mut harness, "20.png");
    harness.key_press(Key::F);
    settle(&mut harness);

    strip_item(&harness, "23.png").click();
    settle_viewer(&mut harness);
    assert_eq!(shown(&harness).as_deref(), Some("23.png"));
    assert!(
        matches!(original(&harness, 24), OriginalState::Loaded(_)),
        "preloads around it"
    );

    // Focused (by assistive technology) and Enter.
    strip_item(&harness, "25.png").focus();
    harness.run_steps(1);
    harness.key_press(Key::Enter);
    settle_viewer(&mut harness);
    assert_eq!(shown(&harness).as_deref(), Some("25.png"));
    strip_item(&harness, "23.png").click();
    settle_viewer(&mut harness);

    let before = strip_item(&harness, "23.png").rect();
    let strip = filmstrip(&harness).unwrap();
    wheel_at(&mut harness, strip.center(), -2.0);
    let after = strip_item(&harness, "23.png").rect();
    assert!(after.left() < before.left() - 100.0, "{before:?} → {after:?}");
    assert_eq!(shown(&harness).as_deref(), Some("23.png"));
    assert_eq!(viewer(&harness).view(), domain::viewer::FIT, "the photo didn't zoom");
}

/// REQ-015-06: the filmstrip steps aside with the controls; the photo stays where it is.
#[test]
fn req_015_06_the_filmstrip_steps_aside_with_the_controls() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 5, &[]);
    open(&mut harness, "1.png");
    harness.key_press(Key::F);
    harness.hover_at(pos2(450.0, 200.0));
    settle(&mut harness);
    let photo = viewer(&harness).photo_rect();
    assert!(filmstrip(&harness).is_some());

    std::thread::sleep(std::time::Duration::from_millis(2600));
    harness.run_steps(30);
    assert!(filmstrip(&harness).is_none());
    assert_eq!(viewer(&harness).photo_rect(), photo, "the photo doesn't move");
}

/// REQ-015-07: videos in the filmstrip carry a play symbol; photos don't.
#[test]
fn req_015_07_marks_videos_in_the_filmstrip() {
    let fixture = Fixture::new();
    let mut harness = tagged_app(&fixture, Default::default());
    open(&mut harness, "0.png");
    harness.key_press(Key::F);
    settle(&mut harness);
    assert_eq!(harness.query_all_by_label("Video").count(), 1);
    let video = strip_item(&harness, "2.mp4").rect();
    assert!(video.contains(harness.get_by_label("Video").rect().center()));
}

/// REQ-013-11: the chrome takes a dark tint of the item's colors, and the tint follows when stepping.
#[test]
fn req_013_11_tints_the_chrome_with_the_items_colors() {
    let fixture = Fixture::new();
    let mut harness = tagged_app(&fixture, Default::default());
    open(&mut harness, "0.png");
    let red = viewer(&harness).chrome().bar;
    assert!(red.r() > red.g() + 5, "{red:?}");
    harness.key_press(Key::ArrowRight);
    settle(&mut harness);
    let plain = viewer(&harness).chrome().bar;
    assert_eq!(plain.r(), plain.g(), "{plain:?}");
}

/// REQ-013-12: the photo grows from its tile into the viewer and shrinks back on close.
#[test]
fn req_013_12_the_photo_grows_from_its_tile_and_back() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 3, &[]);
    harness.ctx.all_styles_mut(|style| style.animation_time = 0.12);
    let tile = harness.get_by_role_and_label(Role::Image, "1.png").rect();

    harness.get_by_role_and_label(Role::Image, "1.png").click();
    harness.run_steps(2);
    let start = viewer(&harness).photo_rect().unwrap();
    assert!(
        (start.center() - tile.center()).length() < 40.0,
        "{start:?} near {tile:?}"
    );
    harness.run_steps(30);
    let open = viewer(&harness).photo_rect().unwrap();
    assert!((open.center() - CENTER).length() < 1.0, "{open:?}");

    harness.key_press(Key::Escape);
    harness.run_steps(3);
    assert!(viewer(&harness).is_closing());
    assert!(viewer(&harness).photo_rect().unwrap().width() < open.width());
    harness.run_steps(30);
    assert!(harness.state().viewer.is_none());
}

/// REQ-020-05: in the light appearance the viewer stays dark: its control bar, and egui's style inside it.
#[test]
fn req_020_05_the_viewer_stays_dark_in_the_light_appearance() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, 3, &[]);
    let mut settings = harness.state().settings;
    settings.appearance = domain::appearance::Appearance::Light;
    harness.state_mut().update_settings(settings);
    settle(&mut harness);
    assert_eq!(harness.ctx.theme(), egui::Theme::Light);

    open(&mut harness, "1.png");
    // Past the original's fade-in.
    harness.run_steps(20);
    // The control bar's glass, beside the Close button's icon.
    let close = harness.get_by_role_and_label(Role::Button, "Close").rect();
    let image = harness.render().unwrap();
    let at = close.min + egui::vec2(3.0, 3.0);
    let [r, g, b, _] = image.get_pixel(at.x as u32, at.y as u32).0;
    let lightness = (u32::from(r) + u32::from(g) + u32::from(b)) / 3;
    assert!(lightness < 80, "the control bar is light: {r} {g} {b}");
    // The window controls the app draws on Linux stay visible over the viewer: the middle of the close button's ×.
    if cfg!(target_os = "linux") {
        let close = harness.get_by_label("Close window").rect().center();
        let [r, g, b, _] = image.get_pixel(close.x as u32, close.y as u32).0;
        let lightness = (u32::from(r) + u32::from(g) + u32::from(b)) / 3;
        assert!(
            lightness > 150,
            "the close button's × is dark on the dark viewer: {r} {g} {b}"
        );
    }
    harness.snapshot(common::snapshot_name("viewer_light"));
}
