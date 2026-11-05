//! Tells the user about a newer version, and installs it when they ask (spec 031).

use crate::icons::Icon;
use crate::theme;
use domain::update::Version;
use eframe::egui::{self, RichText, Stroke, vec2};
use std::sync::Arc;

/// The running version.
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the workspace's version is plain")
}

/// GitHub, or a fake in tests.
pub trait Updater: Send + Sync {
    /// The latest published version. Blocks; runs on a job thread.
    fn latest(&self) -> Result<Version, String>;
    /// Whether this build can install an update itself (REQ-031-07).
    fn can_install(&self) -> bool;
    /// Downloads and verifies `version` and starts its installer. The app closes when this succeeds. Blocks.
    fn install(&self, version: &Version) -> Result<(), String>;
}

/// The update's state, as the toolbar and the settings show it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Unknown,
    Available(Version),
    Installing(Version),
}

/// What the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked {
    Install,
    SeeWhatsNew,
}

/// How a check the user asked for went (REQ-031-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskedCheck {
    Running,
    /// Nothing newer than the running version.
    Latest,
    Failed,
}

#[derive(Default)]
pub struct Updates {
    updater: Option<Arc<dyn Updater>>,
    /// The check at the app's start, or after turning checking on, has been made.
    asked: bool,
    checking: bool,
    /// The check the user asked for, until they leave the app or change the setting.
    pub asked_check: Option<AskedCheck>,
    pub phase: Phase,
    /// The installer runs: the app should close (REQ-031-05).
    pub close: bool,
}

impl Updates {
    pub fn new(updater: Arc<dyn Updater>) -> Self {
        Self {
            updater: Some(updater),
            ..Default::default()
        }
    }

    pub fn updater(&self) -> Option<Arc<dyn Updater>> {
        self.updater.clone()
    }

    /// Whether a check is due: once, when the app starts (REQ-031-01).
    pub fn start_check(&mut self, enabled: bool) -> bool {
        let due = self.updater.is_some() && enabled && !self.asked && !self.busy();
        if due {
            self.checking = true;
            self.asked = true;
        }
        due
    }

    /// The user asked for a check, also with checking turned off (REQ-031-11). Whether it can start.
    pub fn ask_check(&mut self) -> bool {
        let can = self.updater.is_some() && !self.busy();
        if can {
            self.checking = true;
            self.asked_check = Some(AskedCheck::Running);
        }
        can
    }

    /// A check is running or an update is being installed, so no check can start.
    pub fn busy(&self) -> bool {
        self.checking || matches!(self.phase, Phase::Installing(_))
    }

    /// The check answered. An older or equal version, or a failed check, shows nothing unless the user asked for this
    /// check (REQ-031-03, REQ-031-04, REQ-031-11).
    pub fn checked(&mut self, latest: Option<Version>, enabled: bool) {
        self.checking = false;
        let asked = self.asked_check == Some(AskedCheck::Running);
        if asked {
            self.asked_check = Some(if latest.is_some() {
                AskedCheck::Latest
            } else {
                AskedCheck::Failed
            });
        }
        if (!enabled && !asked) || matches!(self.phase, Phase::Installing(_)) {
            return;
        }
        if let Some(latest) = latest.filter(|latest| *latest > current_version()) {
            self.phase = Phase::Available(latest);
            self.asked_check = None;
        }
    }

    /// _Check for updates_ was turned off, or on again, which checks right away (REQ-031-02).
    pub fn enabled_changed(&mut self) {
        self.asked = false;
        self.asked_check = None;
        if matches!(self.phase, Phase::Available(_)) {
            self.phase = Phase::Unknown;
        }
    }

    fn can_install(&self) -> bool {
        self.updater.as_ref().is_some_and(|updater| updater.can_install())
    }
}

/// The toolbar's quiet notice and its menu (REQ-031-04). Nothing while there is no update.
pub fn notice(ui: &mut egui::Ui, updates: &Updates) -> Option<Asked> {
    let label = match updates.phase {
        Phase::Unknown => return None,
        Phase::Available(_) => "Update available",
        Phase::Installing(_) => "Downloading update",
    };
    let button = egui::Button::new((Icon::Download.image(theme::ICON_SIZE, theme::muted()), label))
        .frame(false)
        .min_size(vec2(0.0, theme::CONTROL_HEIGHT));
    let response = ui.add(button);
    let mut asked = None;
    crate::toolbar::menu(&response, "update", 280.0, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        asked = offer(ui, updates);
    });
    asked
}

/// Which version is available and what can be done about it; in the toolbar's menu and the settings' _About_.
pub fn offer(ui: &mut egui::Ui, updates: &Updates) -> Option<Asked> {
    let mut asked = None;
    match updates.phase {
        Phase::Unknown => {}
        Phase::Available(version) => {
            ui.label(RichText::new(format!("Version {version} is available")).family(theme::semibold()));
            ui.horizontal(|ui| {
                if updates.can_install() && action(ui, "Update and restart") {
                    asked = Some(Asked::Install);
                }
                if action(ui, "See what's new") {
                    asked = Some(Asked::SeeWhatsNew);
                }
            });
        }
        Phase::Installing(version) => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(theme::ICON_SIZE));
                ui.label(format!("Downloading version {version}…"));
            });
        }
    }
    asked
}

fn action(ui: &mut egui::Ui, label: &str) -> bool {
    ui.add(egui::Button::new(label).stroke(Stroke::new(1.0, theme::rim())))
        .clicked()
}

/// The row of _About_ that checks when the user asks; whether they did. Nothing in a build that can't check, or while an
/// update is being installed (REQ-031-11).
pub fn check_row(state: &Updates) -> Option<(String, bool)> {
    state.updater.as_ref()?;
    if matches!(state.phase, Phase::Installing(_)) {
        return None;
    }
    let says = match state.asked_check {
        Some(AskedCheck::Running) => "Checking…".to_owned(),
        Some(AskedCheck::Latest) => format!("{} is the latest version.", current_version()),
        Some(AskedCheck::Failed) => "Couldn't reach GitHub. Check your connection and try again.".to_owned(),
        None => "Asks GitHub right away, even with automatic checks turned off.".to_owned(),
    };
    Some((says, !state.busy()))
}

/// The real thing: GitHub through `media::update`. Keeps the download in `folder`.
#[cfg(not(target_arch = "wasm32"))]
pub struct GitHub {
    folder: std::path::PathBuf,
    installed: Option<std::path::PathBuf>,
}

#[cfg(not(target_arch = "wasm32"))]
impl GitHub {
    /// Also removes what an earlier update left in `folder`.
    pub fn new(folder: std::path::PathBuf) -> Self {
        let _ = std::fs::remove_dir_all(&folder);
        Self {
            folder,
            installed: media::update::installed_folder(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Updater for GitHub {
    fn latest(&self) -> Result<Version, String> {
        media::update::latest().map_err(|error| error.to_string())
    }

    fn can_install(&self) -> bool {
        self.installed.is_some()
    }

    fn install(&self, version: &Version) -> Result<(), String> {
        let folder = self.installed.as_ref().ok_or("this build can't install updates")?;
        let installer = media::update::download(version, &self.folder).map_err(|error| error.to_string())?;
        media::update::start_installer(&installer, folder).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;

    impl Updater for Fake {
        fn latest(&self) -> Result<Version, String> {
            unreachable!()
        }
        fn can_install(&self) -> bool {
            false
        }
        fn install(&self, _version: &Version) -> Result<(), String> {
            unreachable!()
        }
    }

    #[test]
    fn req_031_01_checks_at_the_start_and_only_then() {
        let mut updates = Updates::new(Arc::new(Fake));
        assert!(updates.start_check(true), "at the start");
        assert!(!updates.start_check(true), "not while it runs");
        updates.checked(None, true);
        assert!(!updates.start_check(true), "not again until the next start");
    }

    #[test]
    fn req_031_01_a_build_without_an_updater_never_checks() {
        let mut updates = Updates::default();
        assert!(!updates.start_check(true));
        assert!(!updates.ask_check());
        assert_eq!(check_row(&updates), None);
    }

    #[test]
    fn req_031_02_turned_off_it_doesnt_check_and_turned_on_it_checks_at_once() {
        let mut updates = Updates::new(Arc::new(Fake));
        assert!(!updates.start_check(false));
        assert!(updates.start_check(true));
        updates.checked(None, true);
        updates.enabled_changed();
        assert!(!updates.start_check(false));
        updates.enabled_changed();
        assert!(updates.start_check(true));
    }

    /// REQ-031-11: an answer to a check the user asked for always shows; only one check runs at a time.
    #[test]
    fn req_031_11_says_how_a_check_the_user_asked_for_went() {
        let mut updates = Updates::new(Arc::new(Fake));
        assert!(updates.ask_check());
        assert!(!updates.ask_check(), "not while one runs");
        assert_eq!(check_row(&updates).map(|(_, can)| can), Some(false));
        updates.checked(None, false);
        assert_eq!(updates.asked_check, Some(AskedCheck::Failed));

        assert!(updates.ask_check());
        updates.checked(Some(current_version()), false);
        assert_eq!(updates.asked_check, Some(AskedCheck::Latest));

        assert!(updates.ask_check());
        let newer = Version::parse("99.0.0").unwrap();
        updates.checked(Some(newer), false);
        assert_eq!(updates.phase, Phase::Available(newer), "also with checking turned off");
        assert_eq!(updates.asked_check, None);
    }

    #[test]
    fn req_031_02_an_answer_after_turning_it_off_shows_nothing() {
        let mut updates = Updates::new(Arc::new(Fake));
        updates.checked(Version::parse("99.0.0"), false);
        assert_eq!(updates.phase, Phase::Unknown);
    }

    #[test]
    fn req_031_04_a_later_check_doesnt_disturb_a_running_update() {
        let mut updates = Updates::new(Arc::new(Fake));
        let version = Version::parse("99.0.0").unwrap();
        updates.phase = Phase::Installing(version);
        updates.checked(Version::parse("100.0.0"), true);
        assert_eq!(updates.phase, Phase::Installing(version));
    }
}
