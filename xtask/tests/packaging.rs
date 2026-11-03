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

/// REQ-031-08: the installer waits for the app before it touches anything, and starts the new version.
#[test]
fn req_031_08_an_update_waits_for_the_app_and_restarts_it() {
    let installer = read("packaging/windows/installer.nsi");
    let section = &installer[installer.find("Section \"-Install\"").unwrap()..];
    let wait = section.find("Call WaitForApp").expect("the installer doesn't wait");
    assert!(
        wait < section.find("RMDir").unwrap(),
        "it removes files before the app ended"
    );
    assert!(section.contains("Call RestartApp"));
    assert!(
        installer.contains("SetErrorLevel 2"),
        "giving up has to look like a failure"
    );
    let smoke = read("packaging/smoke-test.ps1");
    assert!(
        smoke.contains("'/UPDATE'"),
        "the smoke test doesn't update over a running app"
    );
}

/// REQ-031-01: the Linux packages recommend what the update check needs.
#[test]
fn req_031_01_the_linux_packages_recommend_curl() {
    assert_eq!(read("packaging/linux/nfpm.yaml").matches("- curl").count(), 2);
}

/// REQ-031-09: what the release's script signs, the app verifies. `openssl` makes a key like `new-update-key.sh` does.
#[cfg(unix)]
#[test]
fn req_031_09_the_app_verifies_what_the_release_signs() {
    use std::process::Command;
    if Command::new("openssl").arg("version").output().is_err() {
        assert!(std::env::var_os("GG_REQUIRE_HELPERS").is_none(), "openssl is missing");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let key = Command::new("openssl")
        .args(["genpkey", "-algorithm", "ed25519"])
        .output()
        .unwrap();
    assert!(key.status.success());
    let key = String::from_utf8(key.stdout).unwrap();
    let pem = dir.path().join("key.pem");
    std::fs::write(&pem, &key).unwrap();
    let public = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "openssl pkey -in {} -pubout -outform DER | tail -c 32 | od -An -tx1 | tr -d ' \\n'",
            pem.display()
        ))
        .output()
        .unwrap();
    let public = String::from_utf8(public.stdout).unwrap();
    let public_file = dir.path().join("public.txt");
    std::fs::write(&public_file, &public).unwrap();

    let name = "good-gallery-1.2.3-windows-x64-setup.exe";
    let installer = dir.path().join(name);
    std::fs::write(&installer, b"installer").unwrap();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../packaging/sign-update.sh");
    let run = |key: &str, public_file: &std::path::Path| {
        Command::new("bash")
            .arg(&script)
            .arg(&installer)
            .arg(public_file)
            .env("UPDATE_SIGNING_KEY", key)
            .output()
            .unwrap()
    };
    let signed = run(&key, &public_file);
    assert!(signed.status.success(), "{}", String::from_utf8_lossy(&signed.stderr));
    let signature = std::fs::read(dir.path().join(format!("{name}.sig"))).unwrap();
    assert!(media::update::verify(name, b"installer", &signature, &public));
    assert!(!media::update::verify(name, b"replaced", &signature, &public));

    // Without the key, or with another one, nothing gets signed.
    assert!(!run("", &public_file).status.success());
    let other = dir.path().join("other.txt");
    std::fs::write(&other, "00".repeat(32)).unwrap();
    assert!(!run(&key, &other).status.success());
}

/// REQ-014-15: the installer offers the shortcuts and _Open with_ on a page of options, remembers them for the next
/// install or update, and removes what was turned off.
#[test]
fn req_014_15_the_installer_offers_and_remembers_its_options() {
    let nsi = read("packaging/windows/installer.nsi");
    assert!(nsi.contains("!insertmacro MUI_PAGE_COMPONENTS"), "no page of options");
    for (section, value) in [
        (r#"Section "Start menu shortcut" START_MENU"#, "StartMenuShortcut"),
        (r#"Section /o "Desktop shortcut" DESKTOP"#, "DesktopShortcut"),
        (r#"Section "Show under $\"Open with$\"" OPEN_WITH"#, "OpenWith"),
    ] {
        assert!(nsi.contains(section), "installer.nsi lacks {section}");
        // Read back on the next run, and written for it.
        assert!(
            nsi.contains(&format!(r#"!insertmacro Remembered "{value}""#)),
            "{value} isn't read back"
        );
        assert!(
            nsi.contains(&format!(r#"!insertmacro Remember "{value}""#)),
            "{value} isn't remembered"
        );
    }
    // What was turned off goes, also on uninstall.
    assert!(nsi.contains(r#"Delete "$DESKTOP\${PRODUCT}.lnk""#));
    assert!(nsi.matches(r#"Delete "$SMPROGRAMS\${PRODUCT}.lnk""#).count() >= 2);
    assert!(nsi.matches("!insertmacro ForEachExtension NoOpenWith").count() >= 2);
}
