//! Selecting several items in the gallery and rating them at once (spec 025).

mod common;

use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::writer::{IndexWriter, IndexedFile};
use domain::media::{MediaKind, MediaMetadata};
use eframe::egui::accesskit::Role;
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::kittest::Queryable as _;
use std::path::{Path, PathBuf};

type App = egui_kittest::Harness<'static, app::AppState>;

/// exiftool, or `None` (the test is skipped) when it isn't installed; `GG_REQUIRE_HELPERS=1` fails then.
fn exiftool() -> Option<PathBuf> {
    match media::exiftool::find_exiftool() {
        Some(exiftool) => Some(exiftool),
        None if std::env::var_os("GG_REQUIRE_HELPERS").is_some() => panic!("exiftool not found"),
        None => {
            eprintln!("skipped: exiftool not found");
            None
        }
    }
}

/// `count` photos `0.png`, `1.png`, … (newest first, so in this order), on disk if `on_disk`; with exiftool for
/// writing if given. `0.png` is tagged Beach.
fn app(fixture: &Fixture, count: usize, on_disk: bool, exiftool: Option<&Path>) -> (App, PathBuf) {
    app_in_folders(fixture, count, on_disk, exiftool, |_| "")
}

/// [`app`] with each photo in the folder `folder(i)` (relative, `""` for the library's top).
fn app_in_folders(
    fixture: &Fixture,
    count: usize,
    on_disk: bool,
    exiftool: Option<&Path>,
    folder: fn(usize) -> &'static str,
) -> (App, PathBuf) {
    let dir = PathBuf::from(fixture.folder("photos"));
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, dir.to_str().unwrap(), "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let files: Vec<IndexedFile> = (0..count)
        .map(|i| {
            let name = format!("{i}.png");
            let rel_path = match folder(i) {
                "" => name.clone(),
                folder => format!("{folder}/{name}"),
            };
            if on_disk {
                std::fs::create_dir_all(dir.join(folder(i))).unwrap();
                image::RgbImage::from_pixel(64, 48, image::Rgb([40 * (i % 6) as u8, 120, 200]))
                    .save(dir.join(&rel_path))
                    .unwrap();
            }
            IndexedFile {
                rel_path,
                file_name: name,
                kind: MediaKind::Image,
                size: 1,
                mtime: 1_000,
                sidecar_mtime: None,
                metadata: MediaMetadata {
                    width: Some(400),
                    height: Some(300),
                    taken_at: Some(2_000_000 - i as i64),
                    tags: if i == 0 { vec![vec!["Beach".into()]] } else { Vec::new() },
                    ..Default::default()
                },
                read_error: None,
            }
        })
        .collect();
    let mut writer = IndexWriter::new(conn);
    writer.write(root, &files).unwrap();
    let mut state = fixture.state(writer.into());
    if let Some(exiftool) = exiftool {
        let config = media::edit::exiftool_config(&fixture.dir.path().join("data")).unwrap();
        state = state.with_edit_tools(media::edit::Tools {
            exiftool: exiftool.to_owned(),
            config,
        });
    }
    let mut harness = common::harness_with_step(state, 1.0 / 60.0);
    harness.set_size(egui::vec2(1000.0, 700.0));
    settle(&mut harness);
    (harness, dir)
}

/// Clicks the tile of `name`, selected or not.
fn click(harness: &mut App, name: &str, modifiers: Modifiers) {
    let selected = format!("{name}, selected");
    harness
        .query_by_role_and_label(Role::Image, name)
        .unwrap_or_else(|| harness.get_by_role_and_label(Role::Image, &selected))
        .click_modifiers(modifiers);
    settle(harness);
}

/// Double-clicks the tile of `name`, selected or not.
fn double_click(harness: &mut App, name: &str) {
    let selected = format!("{name}, selected");
    let pos = harness
        .query_by_role_and_label(Role::Image, name)
        .unwrap_or_else(|| harness.get_by_role_and_label(Role::Image, &selected))
        .rect()
        .center();
    double_click_at(harness, pos);
    settle(harness);
}

/// Two clicks a frame apart, as a double-click is; half a second after any earlier click, which would make it a
/// triple-click.
fn double_click_at(harness: &mut App, pos: egui::Pos2) {
    harness.run_steps(30);
    harness.hover_at(pos);
    harness.run_steps(1);
    for _ in 0..2 {
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            harness.run_steps(1);
        }
    }
}

fn selected(harness: &App) -> Vec<i64> {
    harness.state().gallery.as_ref().unwrap().selected_ids()
}

fn selected_names(harness: &App) -> Vec<String> {
    let mut names: Vec<String> = harness
        .query_all_by_label_contains(", selected")
        .filter_map(|node| {
            use egui_kittest::kittest::NodeT as _;
            Some(node.accesskit_node().label()?.trim_end_matches(", selected").to_owned())
        })
        .collect();
    names.sort();
    names
}

fn indexed(harness: &App, name: &str) -> Option<i64> {
    let db = harness.state().db().unwrap();
    let conn = db.lock().unwrap();
    conn.query_row("SELECT rating FROM media WHERE file_name = ?1", [name], |row| {
        row.get(0)
    })
    .unwrap()
}

/// REQ-025-01, REQ-025-06: Ctrl-click selects and deselects an item, keeping the others; the toolbar shows how many
/// are selected next to the search bar.
#[test]
fn req_025_01_req_025_06_ctrl_click_selects() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, 4, false, None);

    click(&mut harness, "1.png", Modifiers::COMMAND);
    click(&mut harness, "3.png", Modifiers::COMMAND);
    assert_eq!(selected_names(&harness), ["1.png", "3.png"]);
    assert!(harness.query_by_label("2 selected").is_some());
    assert!(harness.query_by_label("Clear selection").is_some());
    assert!(
        harness.query_by_label(app::search_bar::LABEL).is_some(),
        "the search bar stays"
    );
    assert!(harness.state().viewer.is_none(), "Ctrl-click doesn't open the viewer");
    harness.snapshot(common::snapshot_name("selection"));

    click(&mut harness, "1.png", Modifiers::COMMAND);
    assert_eq!(selected_names(&harness), ["3.png"]);
    harness.get_by_label("Clear selection").click();
    settle(&mut harness);
    assert!(selected(&harness).is_empty());
    assert!(harness.query_by_label("Clear selection").is_none());
}

/// REQ-025-02: Shift-click adds the range from the anchor, in gallery order.
#[test]
fn req_025_02_shift_click_adds_a_range() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, 6, false, None);
    click(&mut harness, "4.png", Modifiers::COMMAND);
    click(&mut harness, "1.png", Modifiers::SHIFT);
    assert_eq!(selected_names(&harness), ["1.png", "2.png", "3.png", "4.png"]);
}

/// REQ-025-01, REQ-025-08: a click selects only that item and doesn't open it; a double-click opens it.
#[test]
fn req_025_01_req_025_08_a_click_selects_and_a_double_click_opens() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, 4, false, None);
    click(&mut harness, "0.png", Modifiers::NONE);
    assert_eq!(selected_names(&harness), ["0.png"]);
    assert!(harness.state().viewer.is_none(), "a click doesn't open the viewer");
    click(&mut harness, "2.png", Modifiers::NONE);
    assert_eq!(selected_names(&harness), ["2.png"], "a click selects only that item");
    click(&mut harness, "0.png", Modifiers::COMMAND);
    assert_eq!(selected_names(&harness), ["0.png", "2.png"]);

    double_click(&mut harness, "3.png");
    let viewer = harness
        .state()
        .viewer
        .as_ref()
        .expect("a double-click opens the viewer");
    assert_eq!(viewer.index(), 3);
}

/// REQ-025-04: _Select all_ in the toolbar selects the whole search; a click beside the items clears it.
#[test]
fn req_025_04_select_all_button_and_a_click_on_the_background() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, 5, false, None);
    harness.get_by_label("Select all").click();
    settle(&mut harness);
    assert_eq!(selected(&harness).len(), 5);

    // Below the tiles: the gallery's background.
    let below = egui::pos2(500.0, 650.0);
    harness.hover_at(below);
    harness.run_steps(1);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: below,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    settle(&mut harness);
    assert!(selected(&harness).is_empty());
}

/// REQ-025-09: _Select all in library_ and _Select all in folder_ show it and select its items.
#[test]
fn req_025_09_selects_a_library_or_folder_from_the_sidebar() {
    let fixture = Fixture::new();
    let (mut harness, _) = app_in_folders(&fixture, 6, false, None, |i| if i % 2 == 1 { "odd" } else { "" });
    harness.get_by_role_and_label(Role::Button, "Photos").click_secondary();
    settle(&mut harness);
    harness.get_by_label("Select all in library").click();
    settle(&mut harness);
    assert_eq!(selected(&harness).len(), 6);
    let library = harness.state().gallery.as_ref().unwrap().search().library;
    assert!(library.is_some(), "the library is shown");

    harness.get_by_role_and_label(Role::Button, "odd").click_secondary();
    settle(&mut harness);
    harness.get_by_label("Select all in folder").click();
    settle(&mut harness);
    assert_eq!(selected_names(&harness), ["1.png", "3.png", "5.png"]);
    let search = harness.state().gallery.as_ref().unwrap().search().clone();
    assert_eq!(search.folder.as_deref(), Some("odd"));
}

/// REQ-025-04, REQ-025-05: Ctrl+A selects the whole search, Escape clears it, and a new search clears it too.
#[test]
fn req_025_04_req_025_05_selects_all_and_clears() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, 5, false, None);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    settle(&mut harness);
    assert_eq!(selected(&harness).len(), 5);
    assert!(harness.query_by_label("5 selected").is_some());
    harness.key_press(Key::Escape);
    settle(&mut harness);
    assert!(selected(&harness).is_empty());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    settle(&mut harness);
    let gallery = harness.state_mut().gallery.as_mut().unwrap();
    let mut search = gallery.search().clone();
    search.with_tag("Beach", domain::search::TagMode::Include);
    gallery.set_search(search);
    settle(&mut harness);
    assert!(selected(&harness).is_empty(), "no hidden item stays selected");
}

/// REQ-025-07: `1`–`5` and _Rate_ rate every selected item, in the files and the index.
#[test]
fn req_025_07_rates_the_selection() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, 4, true, Some(&exiftool));
    click(&mut harness, "1.png", Modifiers::COMMAND);
    click(&mut harness, "2.png", Modifiers::COMMAND);

    harness.key_press(Key::Num4);
    settle(&mut harness);
    assert_eq!(
        ["0.png", "1.png", "2.png", "3.png"].map(|name| indexed(&harness, name)),
        [None, Some(4), Some(4), None]
    );
    let toasts = harness.state().toasts.current();
    assert!(
        toasts.last().is_some_and(|toast| toast.text == "Rated 2 items"),
        "{toasts:?}"
    );

    harness.get_by_label("Rate the selection").click();
    harness.run_steps(2);
    harness.get_by_label("Remove rating").click();
    settle(&mut harness);
    assert_eq!(indexed(&harness, "1.png"), None);
    assert_eq!(indexed(&harness, "2.png"), None);
}

/// REQ-025-07: files that can't be written keep their rating, are counted, and the first one is named.
#[test]
fn req_025_07_counts_the_files_it_cant_rate() {
    let Some(exiftool) = exiftool() else { return };
    let fixture = Fixture::new();
    let (mut harness, dir) = app(&fixture, 3, true, Some(&exiftool));
    let locked = dir.join("1.png");
    let mut permissions = std::fs::metadata(&locked).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&locked, permissions).unwrap();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    settle(&mut harness);

    harness.key_press(Key::Num2);
    settle(&mut harness);
    assert_eq!(
        ["0.png", "1.png", "2.png"].map(|name| indexed(&harness, name)),
        [Some(2), None, Some(2)]
    );
    let toasts = harness.state().toasts.current();
    assert!(
        toasts
            .last()
            .is_some_and(|toast| toast.text == "Couldn't rate 1 of 3 items. 1.png: the file is read-only"),
        "{toasts:?}"
    );
}

/// REQ-025-07: more than 100 items are rated only after the user confirms.
#[test]
fn req_025_07_asks_before_rating_many() {
    let fixture = Fixture::new();
    let (mut harness, _) = app(&fixture, 101, false, None);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    settle(&mut harness);

    harness.key_press(Key::Num5);
    settle(&mut harness);
    assert!(harness.query_by_label("Rate 101 items with 5 stars?").is_some());
    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Rate 101 items with 5 stars?").is_none());
    assert_eq!(indexed(&harness, "0.png"), None);
    assert_eq!(selected(&harness).len(), 101, "the selection stays");
}
