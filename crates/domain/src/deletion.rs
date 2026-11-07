//! Deleting media items (REQ-009-23): where their files go, and how the app asks first.

/// Where deleted files go on this system.
pub const TRASH: &str = if cfg!(windows) { "Recycle Bin" } else { "Trash" };

/// Whether `path` is a network path (`\\server\share`, `//server/share`, `\\?\UNC\…`), which has no Recycle Bin. A
/// mapped network drive (`Z:`) is found by asking the system (`media::delete`).
pub fn is_network_path(path: &str) -> bool {
    let unified = path.replace('/', r"\");
    match unified.strip_prefix(r"\\?\") {
        Some(verbatim) => verbatim.get(..4).is_some_and(|unc| unc.eq_ignore_ascii_case(r"UNC\")),
        None => unified.starts_with(r"\\"),
    }
}

/// The dialog asking before deleting `count` (formatted) items: its title, its text and its button.
pub fn question(count: &str, permanently: bool) -> (String, String, &'static str) {
    let items = if count == "1" { "item" } else { "items" };
    if permanently {
        (
            format!("Delete {count} {items} permanently?"),
            "Some of them are on a network drive, which has no Recycle Bin. Deleting them can't be undone.".to_owned(),
            "Delete permanently",
        )
    } else {
        (
            format!("Delete {count} {items}?"),
            format!("Their files go to the {TRASH}, with their sidecars."),
            "Delete",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ-009-23: network paths have no Recycle Bin; local ones and drive letters are asked about separately.
    #[test]
    fn req_009_23_network_paths_have_no_recycle_bin() {
        for network in [
            r"\\nas\photos",
            "//nas/photos",
            r"\\?\UNC\nas\photos",
            r"\\nas\photos\2024",
        ] {
            assert!(is_network_path(network), "{network}");
        }
        for local in [r"C:\Photos", "/home/me/Photos", r"\\?\C:\Photos", "relative", ""] {
            assert!(!is_network_path(local), "{local}");
        }
    }

    /// REQ-009-23: the dialog names how many items go and where, and says when it is permanent.
    #[test]
    fn req_009_23_the_question_says_where_the_files_go() {
        let (title, text, button) = question("3", false);
        assert_eq!(title, "Delete 3 items?");
        assert!(text.contains(TRASH), "{text}");
        assert_eq!(button, "Delete");

        let (title, text, button) = question("1", true);
        assert_eq!(title, "Delete 1 item permanently?");
        assert!(text.contains("can't be undone"), "{text}");
        assert!(!text.contains(TRASH) || text.contains("no"), "{text}");
        assert_eq!(button, "Delete permanently");
    }
}
