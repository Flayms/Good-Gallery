//! Media items: kinds, supported files, housekeeping folders (glossary: _Media kind_, _Housekeeping folder_).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

impl MediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "image" => Some(Self::Image),
            "video" => Some(Self::Video),
            _ => None,
        }
    }
}

/// What the indexer reads from a media file and its sidecar (spec 004).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaMetadata {
    /// As displayed (swapped for rotated media); `None` when unknown.
    pub width: Option<i64>,
    pub height: Option<i64>,
    /// Seconds.
    pub duration: Option<f64>,
    /// Milliseconds since the Unix epoch.
    pub taken_at: Option<i64>,
    /// 1–5, `None` when unrated or rejected.
    pub rating: Option<i64>,
    /// The sidecar's orientation (1–8, as EXIF numbers it): decoders turn a photo by it when the file has none of its
    /// own (REQ-004-12).
    pub sidecar_orientation: Option<u8>,
    /// Tag paths, outermost first; flat keywords are single-element paths.
    pub tags: Vec<Vec<String>>,
    /// The edit the file was saved with (spec 021), from the file itself.
    /// Boxed: most files have none, and scans move many of these around.
    pub edit: Option<Box<crate::edit::SavedEdit>>,
    /// Named face regions on the upright photo (REQ-004-13).
    pub faces: Vec<crate::faces::FaceRegion>,
    /// The camera, as [`camera_name`] names it (REQ-040-01).
    pub camera: Option<String>,
    /// The lens (REQ-040-01).
    pub lens: Option<String>,
    /// Where it was taken (REQ-040-02).
    pub location: Option<Location>,
}

/// A GPS position in degrees, south and west negative (REQ-040-02).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
}

impl Location {
    /// A position on Earth, or `None` for one out of range or at exactly 0° / 0°, which cameras write without a fix.
    pub fn new(latitude: f64, longitude: f64) -> Option<Self> {
        let on_earth =
            latitude.is_finite() && longitude.is_finite() && latitude.abs() <= 90.0 && longitude.abs() <= 180.0;
        (on_earth && (latitude, longitude) != (0.0, 0.0)).then_some(Self { latitude, longitude })
    }
}

/// The camera as a person names it: the make, then the model, but the make only once when the model starts with its
/// first word ("Canon EOS R5", "NIKON D750" from "NIKON CORPORATION"), or whichever of the two is there (REQ-040-01).
pub fn camera_name(make: Option<&str>, model: Option<&str>) -> Option<String> {
    let clean = |text: Option<&str>| {
        let text = text?.split_whitespace().collect::<Vec<_>>().join(" ");
        (!text.is_empty()).then_some(text)
    };
    match (clean(make), clean(model)) {
        (Some(make), Some(model)) => {
            let brand = make.split(' ').next().unwrap_or_default().to_lowercase();
            let first = model.split(' ').next().unwrap_or_default().to_lowercase();
            Some(
                if first == brand || model.to_lowercase().starts_with(&make.to_lowercase()) {
                    model
                } else {
                    format!("{make} {model}")
                },
            )
        }
        (make, model) => model.or(make),
    }
}

/// Bump when metadata extraction changes, so the indexer re-reads files already marked unchanged. Existing indexes were
/// written at version 1; version 2 reads orientations from sidecars (REQ-004-12), version 3 face regions (REQ-004-13),
/// version 4 the camera, lens and location (REQ-040-03), version 5 places from positions (REQ-042-04).
pub const METADATA_VERSION: i64 = 5;

fn extension(file_name: &str) -> Option<String> {
    let dot = file_name.rfind('.')?;
    // `.jpg` alone is a dotfile, not an extension (as Node's `extname`).
    (dot > 0).then(|| file_name[dot..].to_ascii_lowercase())
}

/// Extensions of the photos the app indexes (REQ-002-01), lower case.
pub const IMAGE_EXTENSIONS: [&str; 6] = [".jpg", ".jpeg", ".png", ".webp", ".gif", ".avif"];
/// Extensions of the videos the app indexes (REQ-002-01), lower case.
pub const VIDEO_EXTENSIONS: [&str; 5] = [".mp4", ".m4v", ".mov", ".webm", ".mkv"];

/// The kind of a supported media file, by extension in any letter case. Dotfiles (macOS `._IMG_1.jpg`) aren't media.
pub fn media_kind(file_name: &str) -> Option<MediaKind> {
    if file_name.starts_with('.') {
        return None;
    }
    let extension = extension(file_name)?;
    if IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        Some(MediaKind::Image)
    } else if VIDEO_EXTENSIONS.contains(&extension.as_str()) {
        Some(MediaKind::Video)
    } else {
        None
    }
}

/// Folders an OS or NAS creates: dot and `$` folders, Synology, QNAP and Windows housekeeping.
pub fn is_ignored_dir(name: &str) -> bool {
    const IGNORED: [&str; 5] = [
        "@eadir",
        "#recycle",
        "#snapshot",
        ".@__thumb",
        "system volume information",
    ];
    name.starts_with('.') || name.starts_with('$') || IGNORED.contains(&name.to_lowercase().as_str())
}

pub fn is_sidecar(file_name: &str) -> bool {
    extension(file_name).as_deref() == Some(".xmp")
}

/// The sidecar of `file_name` among a folder's sidecars (lower-cased name → actual names): `IMG_1.jpg.xmp`
/// (darktable, digiKam) before `IMG_1.xmp` (Lightroom), in any letter case. Where letter case tells files apart (Linux)
/// and several sidecars differ only in it, the one whose name matches `file_name` exactly wins, else the first by name.
pub fn find_sidecar<'a>(
    sidecars: &'a std::collections::HashMap<String, Vec<String>>,
    file_name: &str,
) -> Option<&'a str> {
    fn stem(name: &str) -> &str {
        match name.rfind('.') {
            Some(dot) if dot > 0 => &name[..dot],
            _ => name,
        }
    }
    let pick = |base: &str| {
        let names = sidecars.get(&format!("{}.xmp", base.to_lowercase()))?;
        names
            .iter()
            .find(|name| stem(name) == base)
            .or_else(|| names.iter().min())
            .map(String::as_str)
    };
    pick(file_name).or_else(|| pick(stem(file_name)))
}

/// Whether a relative path from the index stays inside its library (REQ-011-08): `/`-separated components, none of
/// them empty, `.`, `..` or with a NUL, and on Windows none with a `\` or `:` (which would make a drive, UNC or stream
/// prefix there; elsewhere they are ordinary file name characters).
pub fn is_safe_rel_path(rel_path: &str) -> bool {
    let forbidden: &[char] = if cfg!(windows) { &['\\', ':', '\0'] } else { &['\0'] };
    !rel_path.is_empty()
        && rel_path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && !part.contains(forbidden))
}

/// The path of `file` relative to the library folder `root`, `/`-separated, if the file lies inside it
/// (REQ-027-01). With `windows`, `\` separates too and letter case doesn't matter, as on Windows' file systems.
pub fn rel_path_in(root: &str, file: &str, windows: bool) -> Option<String> {
    let normalize = |path: &str| {
        if windows {
            path.replace('\\', "/")
        } else {
            path.to_owned()
        }
    };
    let (root, file) = (normalize(root), normalize(file));
    let root = root.trim_end_matches('/');
    let mut rest = file.as_str();
    for wanted in root.chars() {
        let mut chars = rest.chars();
        let found = chars.next()?;
        let same = if windows {
            found.to_lowercase().eq(wanted.to_lowercase())
        } else {
            found == wanted
        };
        if !same {
            return None;
        }
        rest = chars.as_str();
    }
    let rel_path = rest.strip_prefix('/')?;
    is_safe_rel_path(rel_path).then(|| rel_path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_040_01_names_the_camera_with_its_make_once() {
        let name = |make, model| camera_name(make, model);
        assert_eq!(
            name(Some("Canon"), Some("Canon EOS R5")).as_deref(),
            Some("Canon EOS R5")
        );
        assert_eq!(
            name(Some("NIKON CORPORATION"), Some("NIKON D750")).as_deref(),
            Some("NIKON D750")
        );
        assert_eq!(
            name(Some("Apple"), Some("iPhone 15 Pro")).as_deref(),
            Some("Apple iPhone 15 Pro")
        );
        assert_eq!(name(Some("FUJIFILM"), Some("X-T5")).as_deref(), Some("FUJIFILM X-T5"));
        assert_eq!(
            name(Some("  SONY "), Some("ILCE-7M4  ")).as_deref(),
            Some("SONY ILCE-7M4")
        );
        assert_eq!(name(None, Some("GoPro HERO12")).as_deref(), Some("GoPro HERO12"));
        assert_eq!(name(Some("DJI"), None).as_deref(), Some("DJI"));
        assert_eq!(name(Some(" "), Some("")), None);
    }

    #[test]
    fn req_040_02_keeps_only_positions_on_earth_with_a_fix() {
        assert_eq!(
            Location::new(-33.45, -70.66),
            Some(Location {
                latitude: -33.45,
                longitude: -70.66
            })
        );
        assert!(Location::new(90.0, 180.0).is_some());
        assert!(Location::new(0.0, 12.5).is_some(), "on the equator");
        for (latitude, longitude) in [(0.0, 0.0), (91.0, 10.0), (10.0, -180.5), (f64::NAN, 1.0)] {
            assert_eq!(Location::new(latitude, longitude), None, "{latitude}, {longitude}");
        }
    }

    #[test]
    fn req_011_08_refuses_relative_paths_that_leave_the_library() {
        for safe in [
            "a.jpg",
            "2024/trip/IMG 0001.jpg",
            "..hidden/x.jpg",
            "a..b.png",
            "Ünïcødé/ä.jpg",
        ] {
            assert!(is_safe_rel_path(safe), "{safe}");
        }
        for unsafe_path in [
            "",
            "/etc/passwd",
            "//server/share/x.jpg",
            "../x.jpg",
            "a/../../x.jpg",
            "a/./b.jpg",
            "a//b.jpg",
            "a/",
            "a\0b",
        ] {
            assert!(!is_safe_rel_path(unsafe_path), "{unsafe_path:?}");
        }
        // Drive, UNC and stream prefixes on Windows, where `\` and `:` aren't file name characters.
        #[cfg(windows)]
        for unsafe_path in [
            "C:/Windows/win.ini",
            "C:Windows",
            "\\\\server\\share\\x.jpg",
            "..\\..\\x.jpg",
            "x.jpg:stream",
        ] {
            assert!(!is_safe_rel_path(unsafe_path), "{unsafe_path:?}");
        }
    }

    #[test]
    fn req_004_10_prefers_the_full_name_sidecar_in_any_letter_case() {
        let sidecars = |names: &[&str]| -> std::collections::HashMap<String, Vec<String>> {
            let mut map = std::collections::HashMap::<String, Vec<String>>::new();
            for name in names {
                map.entry(name.to_lowercase()).or_default().push(name.to_string());
            }
            map
        };
        let both = sidecars(&["IMG_1.JPG.xmp", "img_1.XMP"]);
        assert_eq!(find_sidecar(&both, "IMG_1.jpg"), Some("IMG_1.JPG.xmp"));
        assert_eq!(find_sidecar(&sidecars(&["img_1.XMP"]), "IMG_1.jpg"), Some("img_1.XMP"));
        assert_eq!(find_sidecar(&sidecars(&["IMG_2.xmp"]), "IMG_1.jpg"), None);
        // Sidecars that differ only in letter case (possible on Linux) each belong to the file named the same way.
        let twins = sidecars(&["img.xmp", "IMG.xmp"]);
        assert_eq!(find_sidecar(&twins, "IMG.jpg"), Some("IMG.xmp"));
        assert_eq!(find_sidecar(&twins, "img.jpg"), Some("img.xmp"));
        assert_eq!(
            find_sidecar(&twins, "Img.jpg"),
            Some("IMG.xmp"),
            "else the first by name"
        );
    }

    #[test]
    fn req_002_01_recognizes_supported_files_in_any_letter_case() {
        for name in ["a.jpg", "a.JPEG", "a.png", "a.webp", "a.gif", "a.AviF"] {
            assert_eq!(media_kind(name), Some(MediaKind::Image), "{name}");
        }
        for name in ["a.mp4", "a.M4V", "a.mov", "a.webm", "a.mkv"] {
            assert_eq!(media_kind(name), Some(MediaKind::Video), "{name}");
        }
        for name in ["a.txt", "a.xmp", "a.heic", "jpg", "a.jpg.bak"] {
            assert_eq!(media_kind(name), None, "{name}");
        }
    }

    #[test]
    fn req_002_02_skips_dotfiles_and_housekeeping_folders() {
        assert_eq!(media_kind("._IMG_1.jpg"), None);
        assert_eq!(media_kind(".jpg"), None);
        for dir in [
            ".git",
            "$RECYCLE.BIN",
            "@eaDir",
            "#recycle",
            "#snapshot",
            ".@__thumb",
            "System Volume Information",
        ] {
            assert!(is_ignored_dir(dir), "{dir}");
        }
        assert!(!is_ignored_dir("Holidays"));
    }

    #[test]
    fn sidecars_are_xmp_files() {
        assert!(is_sidecar("IMG_1.XMP"));
        assert!(!is_sidecar("IMG_1.jpg"));
    }

    #[test]
    fn req_027_01_finds_a_files_path_inside_its_library() {
        assert_eq!(
            rel_path_in("/home/a/Photos", "/home/a/Photos/2021/beach.jpg", false).as_deref(),
            Some("2021/beach.jpg")
        );
        assert_eq!(
            rel_path_in("/home/a/Photos/", "/home/a/Photos/beach.jpg", false).as_deref(),
            Some("beach.jpg")
        );
        // Another folder that starts with the same name, the folder itself, and letter case on Linux.
        assert_eq!(rel_path_in("/home/a/Photos", "/home/a/Photos2/beach.jpg", false), None);
        assert_eq!(rel_path_in("/home/a/Photos", "/home/a/Photos", false), None);
        assert_eq!(rel_path_in("/home/a/Photos", "/home/a/photos/beach.jpg", false), None);
        assert_eq!(
            rel_path_in("/home/a/Photos", "/home/a/Photos/../b/beach.jpg", false),
            None
        );
    }

    #[test]
    fn req_027_01_matches_windows_paths_in_any_letter_case() {
        assert_eq!(
            rel_path_in("D:\\Photos", "d:\\photos\\Trip\\IMG_1.JPG", true).as_deref(),
            Some("Trip/IMG_1.JPG")
        );
        assert_eq!(
            rel_path_in("\\\\nas\\Fotos\\", "\\\\NAS\\fotos\\Ä.jpg", true).as_deref(),
            Some("Ä.jpg")
        );
        assert_eq!(rel_path_in("D:\\Photos", "D:\\Photos2\\a.jpg", true), None);
    }
}
