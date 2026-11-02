//! The packages register the app for the formats it indexes, so a new format can't be left out unnoticed.

use domain::media::{IMAGE_EXTENSIONS, VIDEO_EXTENSIONS};
use std::path::Path;

fn read(rel_path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(rel_path);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("can't read {}: {e}", path.display()))
}

/// The MIME type a Linux file manager gives files with the extension.
fn mime_type(extension: &str) -> &'static str {
    match extension {
        ".jpg" | ".jpeg" => "image/jpeg",
        ".png" => "image/png",
        ".webp" => "image/webp",
        ".gif" => "image/gif",
        ".avif" => "image/avif",
        ".mp4" => "video/mp4",
        ".m4v" => "video/x-m4v",
        ".mov" => "video/quicktime",
        ".webm" => "video/webm",
        ".mkv" => "video/x-matroska",
        other => panic!("no MIME type known for {other}: add it here and to good-gallery.desktop"),
    }
}

/// REQ-027-05: the Windows installer offers the app under _Open with_ for every indexed format, and takes it back on
/// uninstall.
#[test]
fn req_027_05_the_installer_registers_every_indexed_format() {
    let nsi = read("packaging/windows/installer.nsi");
    for extension in IMAGE_EXTENSIONS.iter().chain(&VIDEO_EXTENSIONS) {
        let line = format!("!insertmacro ${{MACRO}} \"{extension}\"");
        assert!(nsi.contains(&line), "installer.nsi doesn't register {extension}");
    }
    for registration in [
        "!insertmacro ForEachExtension OpenWith",
        "!insertmacro ForEachExtension NoOpenWith",
        r#"DeleteRegKey HKCU "Software\Classes\${PROGID}""#,
        r#"DeleteRegKey HKCU "${APP_KEY}""#,
        r#"'"$INSTDIR\${EXE}" "%1"'"#,
    ] {
        assert!(nsi.contains(registration), "installer.nsi lacks {registration}");
    }
}

/// REQ-027-05: the Linux menu entry lists every indexed format's MIME type and passes the opened file.
#[test]
fn req_027_05_the_desktop_file_lists_every_indexed_format() {
    let desktop = read("packaging/linux/good-gallery.desktop");
    let mime_types: Vec<&str> = desktop
        .lines()
        .find_map(|line| line.strip_prefix("MimeType="))
        .expect("good-gallery.desktop has a MimeType line")
        .split(';')
        .collect();
    for extension in IMAGE_EXTENSIONS.iter().chain(&VIDEO_EXTENSIONS) {
        let mime_type = mime_type(extension);
        assert!(
            mime_types.contains(&mime_type),
            "good-gallery.desktop lacks {mime_type}"
        );
    }
    assert!(
        desktop.contains("Exec=good-gallery %f"),
        "the menu entry doesn't pass the file"
    );
    assert!(
        read("packaging/linux/install.sh").contains(r#"Exec=\"$bin/good-gallery\" %f"#),
        "install.sh drops the file argument"
    );
}
