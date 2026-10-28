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
    /// Tag paths, outermost first; flat keywords are single-element paths.
    pub tags: Vec<Vec<String>>,
}

/// Bump when metadata extraction changes, so the indexer re-reads files already marked unchanged. Existing indexes were
/// written at version 1.
pub const METADATA_VERSION: i64 = 1;

fn extension(file_name: &str) -> Option<String> {
    let dot = file_name.rfind('.')?;
    // `.jpg` alone is a dotfile, not an extension (as Node's `extname`).
    (dot > 0).then(|| file_name[dot..].to_ascii_lowercase())
}

/// The kind of a supported media file, by extension in any letter case. Dotfiles (macOS `._IMG_1.jpg`) aren't media.
pub fn media_kind(file_name: &str) -> Option<MediaKind> {
    if file_name.starts_with('.') {
        return None;
    }
    match extension(file_name)?.as_str() {
        ".jpg" | ".jpeg" | ".png" | ".webp" | ".gif" | ".avif" => Some(MediaKind::Image),
        ".mp4" | ".m4v" | ".mov" | ".webm" | ".mkv" => Some(MediaKind::Video),
        _ => None,
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

/// The sidecar of `file_name` among a folder's sidecars (lower-cased name → actual name): `IMG_1.jpg.xmp`
/// (darktable, digiKam) before `IMG_1.xmp` (Lightroom), in any letter case.
pub fn find_sidecar<'a>(sidecars: &'a std::collections::HashMap<String, String>, file_name: &str) -> Option<&'a str> {
    let lower = file_name.to_lowercase();
    let stem = match lower.rfind('.') {
        Some(dot) if dot > 0 => &lower[..dot],
        _ => lower.as_str(),
    };
    sidecars
        .get(&format!("{lower}.xmp"))
        .or_else(|| sidecars.get(&format!("{stem}.xmp")))
        .map(String::as_str)
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let sidecars = |names: &[&str]| -> std::collections::HashMap<String, String> {
            names.iter().map(|n| (n.to_lowercase(), n.to_string())).collect()
        };
        let both = sidecars(&["IMG_1.JPG.xmp", "img_1.XMP"]);
        assert_eq!(find_sidecar(&both, "IMG_1.jpg"), Some("IMG_1.JPG.xmp"));
        assert_eq!(find_sidecar(&sidecars(&["img_1.XMP"]), "IMG_1.jpg"), Some("img_1.XMP"));
        assert_eq!(find_sidecar(&sidecars(&["IMG_2.xmp"]), "IMG_1.jpg"), None);
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
}
