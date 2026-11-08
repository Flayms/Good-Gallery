//! The update's notice and button (spec 031), with a fake GitHub.

use crate::common;

use app::state::FileManager;
use app::updates::{Phase, Updater, current_version};
use app::{AppState, Page};
use common::{Fixture, settle};
use domain::update::Version;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const NEWER: &str = "99.0.0";

struct FakeGitHub {
    latest: Result<String, String>,
    can_install: bool,
    install: Result<(), String>,
    checks: AtomicUsize,
    installed: Mutex<Vec<Version>>,
}

impl FakeGitHub {
    fn latest(version: &str) -> Self {
        Self {
            latest: Ok(version.to_owned()),
            can_install: true,
            install: Ok(()),
            checks: AtomicUsize::new(0),
            installed: Mutex::new(Vec::new()),
        }
    }
}

impl Updater for FakeGitHub {
    fn latest(&self) -> Result<Version, String> {
        self.checks.fetch_add(1, Ordering::SeqCst);
        self.latest.clone().map(|version| Version::parse(&version).unwrap())
    }

    fn can_install(&self) -> bool {
        self.can_install
    }

    fn install(&self, version: &Version) -> Result<(), String> {
        self.installed.lock().unwrap().push(*version);
        self.install.clone()
    }
}

#[derive(Default)]
struct RecordedBrowser {
    opened: Mutex<Vec<String>>,
}

impl FileManager for RecordedBrowser {
    fn show_in_folder(&self, _file: &std::path::Path) -> std::io::Result<()> {
        Ok(())
    }

    fn open_in_browser(&self, url: &str) -> std::io::Result<()> {
        self.opened.lock().unwrap().push(url.to_owned());
        Ok(())
    }
}

fn with_github(fixture: &Fixture, github: Arc<FakeGitHub>) -> Harness<'static, AppState> {
    let state = fixture.state(db::open_in_memory().unwrap()).with_updater(github);
    let mut harness = common::harness(state);
    settle(&mut harness);
    harness
}

fn no_notice(harness: &Harness<'_, AppState>) {
    assert!(harness.query_by_label("Update available").is_none());
}

/// Opens the toolbar's menu of the update.
fn open_notice(harness: &mut Harness<'_, AppState>) {
    harness.get_by_label("Update available").click();
    settle(harness);
}

#[test]
fn req_031_01_checks_when_the_app_starts_and_only_once() {
    let fixture = Fixture::new();
    let github = Arc::new(FakeGitHub::latest(NEWER));
    let mut harness = with_github(&fixture, github.clone());
    settle(&mut harness);
    assert_eq!(github.checks.load(Ordering::SeqCst), 1);
}

#[test]
fn req_031_01_is_on_by_default_and_without_an_updater_nothing_shows() {
    assert!(db::settings::Settings::default().check_for_updates);
    let fixture = Fixture::new();
    let mut harness = fixture.harness(db::open_in_memory().unwrap());
    settle(&mut harness);
    no_notice(&harness);
}

#[test]
fn req_031_02_turned_off_it_asks_nothing_and_hides_the_notice() {
    let fixture = Fixture::new();
    let conn = db::open_in_memory().unwrap();
    let settings = db::settings::Settings {
        check_for_updates: false,
        ..Default::default()
    };
    db::settings::save(&conn, &settings).unwrap();
    let github = Arc::new(FakeGitHub::latest(NEWER));
    let mut harness = common::harness(fixture.state(conn).with_updater(github.clone()));
    settle(&mut harness);
    assert_eq!(github.checks.load(Ordering::SeqCst), 0);
    no_notice(&harness);
}

#[test]
fn req_031_02_the_settings_turn_it_off_and_on() {
    let fixture = Fixture::new();
    let github = Arc::new(FakeGitHub::latest(NEWER));
    let mut harness = with_github(&fixture, github.clone());
    harness.get_by_label("Update available");

    harness.state_mut().page = Page::Settings;
    harness.set_size(eframe::egui::vec2(900.0, 2600.0));
    settle(&mut harness);
    harness.get_by_label(&format!("Version {NEWER} is available"));
    harness
        .get_by_role_and_label(Role::CheckBox, "Check for updates")
        .click();
    settle(&mut harness);
    assert!(!harness.state().settings.check_for_updates);
    assert_eq!(harness.state().updates.phase, Phase::Unknown);
    assert!(
        harness
            .query_by_label(&format!("Version {NEWER} is available"))
            .is_none()
    );

    harness
        .get_by_role_and_label(Role::CheckBox, "Check for updates")
        .click();
    settle(&mut harness);
    assert_eq!(github.checks.load(Ordering::SeqCst), 2, "turning it on checks at once");
    harness.get_by_label(&format!("Version {NEWER} is available"));
}

/// Opens the settings, tall enough for _About_, with `github` asked for nothing since they opened.
fn settings_with(fixture: &Fixture, github: Arc<FakeGitHub>, automatic: bool) -> Harness<'static, AppState> {
    let conn = db::open_in_memory().unwrap();
    let settings = db::settings::Settings {
        check_for_updates: automatic,
        ..Default::default()
    };
    db::settings::save(&conn, &settings).unwrap();
    let mut harness = common::harness(fixture.state(conn).with_updater(github));
    harness.state_mut().page = Page::Settings;
    harness.set_size(eframe::egui::vec2(900.0, 2600.0));
    settle(&mut harness);
    harness
}

#[test]
fn req_031_11_check_now_says_it_is_the_latest_version() {
    let fixture = Fixture::new();
    let github = Arc::new(FakeGitHub::latest(&current_version().to_string()));
    let mut harness = settings_with(&fixture, github.clone(), true);
    assert_eq!(github.checks.load(Ordering::SeqCst), 1, "at the start");
    harness.get_by_label("Check now").click();
    settle(&mut harness);
    assert_eq!(github.checks.load(Ordering::SeqCst), 2);
    harness.get_by_label_contains("is the latest version");
    no_notice(&harness);
}

#[test]
fn req_031_11_check_now_works_with_automatic_checks_off_and_offers_the_update() {
    let fixture = Fixture::new();
    let github = Arc::new(FakeGitHub::latest(NEWER));
    let mut harness = settings_with(&fixture, github.clone(), false);
    assert_eq!(github.checks.load(Ordering::SeqCst), 0, "not on its own");
    harness.get_by_label("Check now").click();
    settle(&mut harness);
    assert_eq!(github.checks.load(Ordering::SeqCst), 1);
    harness.get_by_label(&format!("Version {NEWER} is available"));
}

#[test]
fn req_031_11_a_check_the_user_asked_for_says_when_it_failed() {
    let fixture = Fixture::new();
    let github = Arc::new(FakeGitHub {
        latest: Err("no network".into()),
        ..FakeGitHub::latest(NEWER)
    });
    let mut harness = settings_with(&fixture, github, true);
    assert!(
        harness.query_by_label_contains("Couldn't reach GitHub").is_none(),
        "quiet on its own"
    );
    harness.get_by_label("Check now").click();
    settle(&mut harness);
    harness.get_by_label_contains("Couldn't reach GitHub");
}

#[test]
fn req_031_11_no_button_without_an_updater() {
    let fixture = Fixture::new();
    let mut harness = fixture.harness(db::open_in_memory().unwrap());
    harness.state_mut().page = Page::Settings;
    harness.set_size(eframe::egui::vec2(900.0, 2600.0));
    settle(&mut harness);
    assert!(harness.query_by_label("Check now").is_none());
}

#[test]
fn req_031_03_a_failed_check_shows_nothing() {
    let fixture = Fixture::new();
    let github = Arc::new(FakeGitHub {
        latest: Err("no network".into()),
        ..FakeGitHub::latest(NEWER)
    });
    let harness = with_github(&fixture, github.clone());
    assert_eq!(github.checks.load(Ordering::SeqCst), 1);
    no_notice(&harness);
    assert!(harness.query_by_label_contains("no network").is_none());
}

#[test]
fn req_031_04_a_newer_version_shows_the_notice_and_its_release() {
    let fixture = Fixture::new();
    let browser = Arc::new(RecordedBrowser::default());
    let github = Arc::new(FakeGitHub::latest(NEWER));
    let state = fixture
        .state(db::open_in_memory().unwrap())
        .with_file_manager(browser.clone())
        .with_updater(github);
    let mut harness = common::harness(state);
    settle(&mut harness);

    open_notice(&mut harness);
    harness.get_by_label(&format!("Version {NEWER} is available"));
    harness.get_by_label("See what's new").click();
    settle(&mut harness);

    assert_eq!(
        *browser.opened.lock().unwrap(),
        [format!("https://github.com/Flayms/Good-Gallery/releases/tag/v{NEWER}")]
    );
}

/// REQ-031-04: the notice is quiet, and its menu names the version and what can be done.
#[test]
fn req_031_04_the_notice_and_its_menu_look_right() {
    let fixture = Fixture::new();
    let mut harness = with_github(&fixture, Arc::new(FakeGitHub::latest(NEWER)));
    open_notice(&mut harness);
    harness.snapshot(common::snapshot_name("update_notice"));
}

#[test]
fn req_031_04_the_same_or_an_older_version_shows_nothing() {
    for latest in [current_version().to_string(), "0.0.1".to_owned()] {
        let fixture = Fixture::new();
        let github = Arc::new(FakeGitHub::latest(&latest));
        let harness = with_github(&fixture, github.clone());
        assert_eq!(github.checks.load(Ordering::SeqCst), 1);
        no_notice(&harness);
    }
}

#[test]
fn req_031_05_nothing_is_installed_until_the_button_is_pressed() {
    let fixture = Fixture::new();
    let github = Arc::new(FakeGitHub::latest(NEWER));
    let mut harness = with_github(&fixture, github.clone());
    open_notice(&mut harness);
    assert!(github.installed.lock().unwrap().is_empty());

    harness.get_by_label("Update and restart").click();
    settle(&mut harness);

    assert_eq!(*github.installed.lock().unwrap(), [Version::parse(NEWER).unwrap()]);
}

#[test]
fn req_031_05_the_app_shows_the_download_while_it_runs() {
    let fixture = Fixture::new();
    let mut harness = with_github(&fixture, Arc::new(FakeGitHub::latest(NEWER)));
    let version = Version::parse(NEWER).unwrap();
    harness.state_mut().updates.phase = Phase::Installing(version);
    harness.run_steps(2);
    harness.get_by_label("Downloading update").click();
    harness.run_steps(2);
    harness.get_by_label(&format!("Downloading version {NEWER}…"));
    assert!(harness.query_by_label("Update and restart").is_none());
}

#[test]
fn req_031_06_a_failed_install_says_so_and_keeps_the_app_and_the_offer() {
    let fixture = Fixture::new();
    let github = Arc::new(FakeGitHub {
        install: Err("the update can't be verified".into()),
        ..FakeGitHub::latest(NEWER)
    });
    let mut harness = with_github(&fixture, github);
    open_notice(&mut harness);
    harness.get_by_label("Update and restart").click();
    settle(&mut harness);

    harness.get_by_label("Couldn't install the update: the update can't be verified");
    assert_eq!(
        harness.state().updates.phase,
        Phase::Available(Version::parse(NEWER).unwrap())
    );
}

#[test]
fn req_031_07_a_build_that_cant_install_only_links_to_the_release() {
    let fixture = Fixture::new();
    let github = Arc::new(FakeGitHub {
        can_install: false,
        ..FakeGitHub::latest(NEWER)
    });
    let mut harness = with_github(&fixture, github);
    open_notice(&mut harness);
    harness.get_by_label("See what's new");
    assert!(harness.query_by_label("Update and restart").is_none());
}
