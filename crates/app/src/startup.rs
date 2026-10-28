//! What the executable needs before the window opens: the data folder, the log file and the window icon.

use eframe::egui;
use std::path::{Path, PathBuf};

/// The data folder's name in `%APPDATA%` (Linux: the XDG data home) for the packaged app (the `packaged` feature, set by `cargo xtask dist`): the
/// folder every earlier version used, so an update opens the existing index and thumbnail cache as they are
/// (REQ-001-08, REQ-001-09).
pub const PACKAGED_DATA_FOLDER: &str = "good-gallery";
/// Other builds (`cargo run`, also `--release`) keep their own, so development never touches the user's real index.
pub const DEVELOPMENT_DATA_FOLDER: &str = "good-gallery-rust";

/// The data folder below `base` ([`data_home`]) for the packaged app or a development build.
pub fn default_data_dir(base: &Path, packaged: bool) -> PathBuf {
    base.join(if packaged {
        PACKAGED_DATA_FOLDER
    } else {
        DEVELOPMENT_DATA_FOLDER
    })
}

/// The folder the data folder lies in, from the environment variable `get` returns: `%APPDATA%` on Windows; on Linux
/// `$XDG_DATA_HOME` if it is absolute, else `$HOME/.local/share`, as the XDG Base Directory spec says (REQ-018-02).
pub fn data_home(get: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    if cfg!(windows) {
        return get("APPDATA").map(PathBuf::from);
    }
    get("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| get("HOME").map(|home| Path::new(&home).join(".local/share")))
}

/// The log file's folder in the data folder.
pub const LOG_FOLDER: &str = "logs";
const LOG_FILE: &str = "app.log";
const OLD_LOG_FILE: &str = "app.old.log";

/// Opens a fresh `logs/app.log` in `data_dir`; the previous run's log is kept as `app.old.log`.
pub fn open_log(data_dir: &Path) -> std::io::Result<std::fs::File> {
    let folder = data_dir.join(LOG_FOLDER);
    std::fs::create_dir_all(&folder)?;
    let log = folder.join(LOG_FILE);
    if log.is_file() {
        // `rename` replaces the older log on Windows too. When it fails (the file is held open), this run appends.
        if std::fs::rename(&log, folder.join(OLD_LOG_FILE)).is_err() {
            return std::fs::OpenOptions::new().append(true).open(log);
        }
    }
    std::fs::File::create(log)
}

/// The app icon for the window's title and the taskbar (eframe shows its own logo otherwise).
pub fn window_icon() -> egui::IconData {
    let image = image::load_from_memory_with_format(include_bytes!("../assets/app-icon.png"), image::ImageFormat::Png)
        .expect("the bundled icon is a valid PNG")
        .into_rgba8();
    egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ-001-09: the packaged app uses `good-gallery`; development builds don't.
    #[test]
    fn packaged_builds_use_good_gallery_and_development_builds_dont() {
        let appdata = Path::new(r"C:\Users\someone\AppData\Roaming");
        assert_eq!(default_data_dir(appdata, true), appdata.join("good-gallery"));
        assert_eq!(default_data_dir(appdata, false), appdata.join("good-gallery-rust"));
    }

    /// REQ-018-02: Linux keeps the data folder in the XDG data home.
    #[cfg(target_os = "linux")]
    #[test]
    fn req_018_02_the_data_folder_lies_in_the_xdg_data_home() {
        let env = |vars: &'static [(&'static str, &'static str)]| {
            move |name: &str| vars.iter().find(|(key, _)| *key == name).map(|(_, value)| value.into())
        };
        let home = env(&[("HOME", "/home/someone")]);
        assert_eq!(data_home(home), Some(PathBuf::from("/home/someone/.local/share")));
        let xdg = env(&[("HOME", "/home/someone"), ("XDG_DATA_HOME", "/data")]);
        assert_eq!(data_home(xdg), Some(PathBuf::from("/data")));
        // The spec says relative paths are invalid and to be ignored.
        let relative = env(&[("HOME", "/home/someone"), ("XDG_DATA_HOME", "data")]);
        assert_eq!(data_home(relative), Some(PathBuf::from("/home/someone/.local/share")));
        assert_eq!(data_home(env(&[])), None);
        let base = data_home(home).unwrap();
        assert_eq!(
            default_data_dir(&base, true),
            Path::new("/home/someone/.local/share/good-gallery")
        );
        assert_eq!(
            default_data_dir(&base, false),
            Path::new("/home/someone/.local/share/good-gallery-rust")
        );
    }

    #[test]
    fn keeps_the_previous_log() {
        let dir = tempfile::tempdir().unwrap();
        std::io::Write::write_all(&mut open_log(dir.path()).unwrap(), b"first").unwrap();
        std::io::Write::write_all(&mut open_log(dir.path()).unwrap(), b"second").unwrap();
        open_log(dir.path()).unwrap();
        let logs = dir.path().join("logs");
        assert_eq!(std::fs::read_to_string(logs.join("app.log")).unwrap(), "");
        assert_eq!(std::fs::read_to_string(logs.join("app.old.log")).unwrap(), "second");
    }

    #[test]
    fn the_window_icon_is_square() {
        let icon = window_icon();
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
    }
}
