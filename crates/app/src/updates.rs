//! Tells the user about a newer version, and installs it when they ask (spec 031).

use crate::icons::Icon;
use crate::theme;
use domain::update::Version;
use eframe::egui::{self, RichText, Stroke, vec2};
use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

/// How often the app asks while it stays open (REQ-031-01).
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

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

#[derive(Default)]
pub struct Updates {
    updater: Option<Arc<dyn Updater>>,
    /// When to ask next; `None` until the first check.
    next_check: Option<Instant>,
    checking: bool,
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

    /// Whether a check is due at `now`; if so, the next one is a day later.
    pub fn start_check(&mut self, enabled: bool, now: Instant) -> bool {
        let due = self.updater.is_some()
            && enabled
            && !self.checking
            && !matches!(self.phase, Phase::Installing(_))
            && self.next_check.is_none_or(|next| now >= next);
        if due {
            self.checking = true;
            self.next_check = Some(now + CHECK_EVERY);
        }
        due
    }

    /// How long until the next check, so that an idle window wakes up for it.
    pub fn wake_in(&self, enabled: bool, now: Instant) -> Option<Duration> {
        let next = self.next_check.filter(|_| enabled && self.updater.is_some())?;
        Some(next.saturating_duration_since(now))
    }

    /// The check answered. An older or equal version, or a failed check, shows nothing (REQ-031-03, REQ-031-04).
    pub fn checked(&mut self, latest: Option<Version>, enabled: bool) {
        self.checking = false;
        if !enabled || matches!(self.phase, Phase::Installing(_)) {
            return;
        }
        if let Some(latest) = latest.filter(|latest| *latest > current_version()) {
            self.phase = Phase::Available(latest);
        }
    }

    /// _Check for updates_ was turned off, or on again, which checks right away (REQ-031-02).
    pub fn enabled_changed(&mut self) {
        self.next_check = None;
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
    fn req_031_01_checks_at_the_start_and_then_daily() {
        let mut updates = Updates::new(Arc::new(Fake));
        let start = Instant::now();
        assert!(updates.start_check(true, start), "at the start");
        assert!(!updates.start_check(true, start), "not while it runs");
        updates.checked(None, true);
        assert!(!updates.start_check(true, start + Duration::from_secs(60)));
        assert_eq!(updates.wake_in(true, start), Some(CHECK_EVERY));
        assert!(updates.start_check(true, start + CHECK_EVERY), "a day later");
    }

    #[test]
    fn req_031_01_a_build_without_an_updater_never_checks() {
        let mut updates = Updates::default();
        assert!(!updates.start_check(true, Instant::now()));
        assert_eq!(updates.wake_in(true, Instant::now()), None);
    }

    #[test]
    fn req_031_02_turned_off_it_doesnt_check_and_turned_on_it_checks_at_once() {
        let mut updates = Updates::new(Arc::new(Fake));
        let start = Instant::now();
        assert!(!updates.start_check(false, start));
        assert!(updates.start_check(true, start));
        updates.checked(None, true);
        updates.enabled_changed();
        assert!(!updates.start_check(false, start));
        updates.enabled_changed();
        assert!(updates.start_check(true, start));
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
