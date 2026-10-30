//! Light mode (spec 020): the appearance setting, following the system, and the light surfaces.

mod common;

use app::theme::{DARK, LIGHT};
use app::{AppState, Page};
use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::writer::{IndexWriter, IndexedFile};
use domain::appearance::Appearance;
use domain::media::{MediaKind, MediaMetadata};
use eframe::egui::{self, Color32, Theme, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};

type App = Harness<'static, AppState>;

/// A few photos, one rated and tagged, so the sidebar has every group.
fn index() -> db::rusqlite::Connection {
    let conn = db::open_in_memory().unwrap();
    let root = db::libraries::insert(&conn, "D:\\Photos", "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let files: Vec<IndexedFile> = (0..6)
        .map(|i| IndexedFile {
            rel_path: format!("2024/{i}.jpg"),
            file_name: format!("{i}.jpg"),
            kind: MediaKind::Image,
            size: 1,
            mtime: 1_000,
            sidecar_mtime: None,
            metadata: MediaMetadata {
                width: Some(400),
                height: Some([300, 400, 600][i % 3]),
                taken_at: Some(5_000 - i as i64),
                rating: (i == 0).then_some(5),
                tags: if i == 0 {
                    vec![vec!["Beach".into()], vec!["People".into(), "Anna".into()]]
                } else {
                    Vec::new()
                },
                ..Default::default()
            },
        })
        .collect();
    let mut writer = IndexWriter::new(conn);
    writer.write(root, &files).unwrap();
    writer.into()
}

fn app(fixture: &Fixture, appearance: Appearance) -> App {
    let mut harness = fixture.harness(index());
    let mut settings = harness.state().settings;
    settings.appearance = appearance;
    harness.state_mut().update_settings(settings);
    settle(&mut harness);
    harness
}

/// The color drawn at `pos`.
fn pixel(harness: &mut App, pos: egui::Pos2) -> Color32 {
    let image = harness.render().unwrap();
    let [r, g, b, _] = image.get_pixel(pos.x as u32, pos.y as u32).0;
    Color32::from_rgb(r, g, b)
}

const COMBO: &str = "Appearance";

fn combo(harness: &App) -> String {
    let node = harness.get_by_role_and_label(Role::ComboBox, COMBO);
    node.accesskit_node().value().unwrap_or_default().to_string()
}

#[test]
fn req_020_01_picks_the_appearance_in_the_settings_and_keeps_it_across_restarts() {
    let fixture = Fixture::new();
    let db = fixture.dir.path().join("gallery.db");
    let state = fixture.state(db::open(&db).unwrap());
    let mut harness = common::harness(state);
    harness.set_size(egui::vec2(900.0, 1100.0));
    harness.state_mut().page = Page::Settings;
    settle(&mut harness);
    assert_eq!(combo(&harness), "System");
    // Without a system preference, System is dark (REQ-020-02).
    assert_eq!(harness.ctx.theme(), Theme::Dark);

    harness.get_by_role_and_label(Role::ComboBox, COMBO).click();
    harness.run_steps(2);
    harness.get_by_label("Light").click();
    settle(&mut harness);
    assert_eq!(harness.ctx.theme(), Theme::Light);
    assert_eq!(combo(&harness), "Light");
    assert_eq!(
        pixel(&mut harness, egui::pos2(880.0, 1080.0)),
        LIGHT.base,
        "the page is light"
    );
    harness.snapshot(common::snapshot_name("settings_light"));
    drop(harness);

    let mut restarted = common::harness(fixture.state(db::open(&db).unwrap()));
    settle(&mut restarted);
    assert_eq!(restarted.state().settings.appearance, Appearance::Light);
    assert_eq!(restarted.ctx.theme(), Theme::Light);
}

#[test]
fn req_020_02_follows_the_system_and_is_dark_where_it_says_nothing() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, Appearance::System);
    let gallery = egui::pos2(880.0, 580.0);
    for (system, expected) in [
        (Some(Theme::Light), Theme::Light),
        (Some(Theme::Dark), Theme::Dark),
        (Some(Theme::Light), Theme::Light),
        (None, Theme::Dark),
    ] {
        harness.input_mut().system_theme = system;
        harness.run_steps(2);
        assert_eq!(harness.ctx.theme(), expected, "system {system:?}");
        let page = if expected == Theme::Light {
            LIGHT.base
        } else {
            DARK.base
        };
        assert_eq!(pixel(&mut harness, gallery), page, "system {system:?}");
    }

    // A chosen appearance wins over the system's.
    let mut settings = harness.state().settings;
    settings.appearance = Appearance::Dark;
    harness.state_mut().update_settings(settings);
    harness.input_mut().system_theme = Some(Theme::Light);
    harness.run_steps(2);
    assert_eq!(harness.ctx.theme(), Theme::Dark);
}

#[test]
fn req_020_03_draws_the_gallery_sidebar_and_menus_in_the_light_tokens() {
    let fixture = Fixture::new();
    let mut harness = app(&fixture, Appearance::Light);
    assert_eq!(pixel(&mut harness, egui::pos2(880.0, 580.0)), LIGHT.base, "the gallery");
    assert_eq!(
        pixel(&mut harness, egui::pos2(200.0, 580.0)),
        LIGHT.recess,
        "the sidebar"
    );
    harness.snapshot(common::snapshot_name("gallery_light"));

    harness.get_by_role_and_label(Role::Button, "View options").click();
    settle(&mut harness);
    harness.snapshot(common::snapshot_name("view_menu_light"));
}
