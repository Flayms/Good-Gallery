//! Asks GitHub for the latest release and fetches its installer (spec 031). The connection is the system's `curl`'s, so
//! the app itself keeps no network code and no network crate (REQ-031-10); without Authenticode nothing else vouches
//! for what it downloads, so the installer has to carry a signature made with the key below (REQ-031-06).

use domain::update::{LATEST_RELEASE_API, Version, latest_version, signed_message};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The public half of the key that signs releases, in hex (`packaging/new-update-key.sh`).
pub const PUBLIC_KEY: &str = include_str!("../../../packaging/update-signing-key.txt");

const UNINSTALLER: &str = "Uninstall Good Gallery.exe";
/// More than GitHub's answer can be, so a hostile answer can't fill the memory.
const MAX_ANSWER: u64 = 1024 * 1024;
const MAX_SIGNATURE: u64 = 1024;
const MAX_INSTALLER: u64 = 1024 * 1024 * 1024;
const ANSWER_SECONDS: u32 = 30;
const INSTALLER_SECONDS: u32 = 30 * 60;

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("can't start curl: {0}")]
    Start(std::io::Error),
    #[error("curl: {0}")]
    Curl(String),
    #[error("GitHub's answer can't be read")]
    Unreadable,
    /// The signature is missing, or doesn't match the installer (REQ-031-06).
    #[error("the update can't be verified")]
    NotVerified,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Windows' own `curl.exe` by its full path, so a `curl.exe` elsewhere on `PATH` is never run; the distribution's
/// elsewhere.
fn curl() -> PathBuf {
    match std::env::var_os("SystemRoot") {
        Some(root) if cfg!(windows) => Path::new(&root).join(r"System32\curl.exe"),
        _ => PathBuf::from("curl"),
    }
}

/// HTTPS only, also after a redirect, and bounded in time and size.
fn curl_args(url: &str, seconds: u32, max_bytes: u64) -> Vec<String> {
    [
        "--silent",
        "--show-error",
        "--fail",
        "--location",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--max-time",
        &seconds.to_string(),
        "--max-filesize",
        &max_bytes.to_string(),
        url,
    ]
    .map(str::to_owned)
    .to_vec()
}

fn run_curl(args: &[String], output: Option<&Path>) -> Result<Vec<u8>, UpdateError> {
    let mut command = Command::new(curl());
    command.args(args);
    if let Some(output) = output {
        command.arg("--output").arg(output);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let child = crate::helpers::spawn_helper(command).map_err(UpdateError::Start)?;
    let output = child.wait_with_output()?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(UpdateError::Curl(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }
}

/// The version of the latest published release (REQ-031-01).
pub fn latest() -> Result<Version, UpdateError> {
    let answer = run_curl(&curl_args(LATEST_RELEASE_API, ANSWER_SECONDS, MAX_ANSWER), None)?;
    latest_version(&answer).ok_or(UpdateError::Unreadable)
}

/// Whether `signature` is `key`'s (in hex) over the installer called `name` with these `contents` (REQ-031-06).
pub fn verify(name: &str, contents: &[u8], signature: &[u8], key: &str) -> bool {
    let Some(key) = hex(key.trim()).and_then(|key| ed25519_compact::PublicKey::from_slice(&key).ok()) else {
        return false;
    };
    let Ok(signature) = ed25519_compact::Signature::from_slice(signature) else {
        return false;
    };
    key.verify(signed_message(name, contents), &signature).is_ok()
}

fn hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok())
        .collect()
}

/// Downloads `version`'s installer and its signature into `folder` (emptied first). A file that can't be verified is
/// deleted before anything could run it (REQ-031-06).
pub fn download(version: &Version, folder: &Path) -> Result<PathBuf, UpdateError> {
    let _ = std::fs::remove_dir_all(folder);
    std::fs::create_dir_all(folder)?;
    let name = version.installer_name();
    let installer = folder.join(&name);
    let signature = folder.join(format!("{name}.sig"));
    run_curl(
        &curl_args(&version.installer_url(), INSTALLER_SECONDS, MAX_INSTALLER),
        Some(&installer),
    )?;
    let verified = run_curl(
        &curl_args(&version.signature_url(), ANSWER_SECONDS, MAX_SIGNATURE),
        Some(&signature),
    )
    .is_ok()
        && verify_files(&name, &installer, &signature, PUBLIC_KEY)?;
    if verified {
        Ok(installer)
    } else {
        std::fs::remove_dir_all(folder)?;
        Err(UpdateError::NotVerified)
    }
}

fn verify_files(name: &str, installer: &Path, signature: &Path, key: &str) -> Result<bool, UpdateError> {
    Ok(verify(
        name,
        &std::fs::read(installer)?,
        &std::fs::read(signature)?,
        key,
    ))
}

/// The folder this app was installed into by the Windows installer, if it was: only that one can install an update
/// (REQ-031-07).
pub fn installed_folder() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    installed_in(&std::env::current_exe().ok()?)
}

fn installed_in(exe: &Path) -> Option<PathBuf> {
    let folder = exe.parent()?;
    folder.join(UNINSTALLER).is_file().then(|| folder.to_path_buf())
}

/// Starts the verified `installer`, which waits for the app to end, replaces it in `folder` and starts it again
/// (REQ-031-08). It has to outlive the app, so it leaves the job that ends helpers with the app.
#[cfg(windows)]
pub fn start_installer(installer: &Path, folder: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    Command::new(installer)
        .args(["/S", "/UPDATE"])
        // NSIS wants /D last and unquoted, whatever spaces the folder has.
        .raw_arg(format!("/D={}", folder.display()))
        .creation_flags(CREATE_BREAKAWAY_FROM_JOB)
        .spawn()
        .map(drop)
}

#[cfg(not(windows))]
pub fn start_installer(_installer: &Path, _folder: &Path) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_compact::{KeyPair, Seed};

    const INSTALLER: &str = "good-gallery-1.2.3-windows-x64-setup.exe";

    fn key_pair() -> (KeyPair, String) {
        let pair = KeyPair::from_seed(Seed::new([7; 32]));
        let key = pair.pk.iter().map(|byte| format!("{byte:02x}")).collect();
        (pair, key)
    }

    fn signature(pair: &KeyPair, name: &str, contents: &[u8]) -> Vec<u8> {
        pair.sk.sign(signed_message(name, contents), None).to_vec()
    }

    #[test]
    fn req_031_06_accepts_only_the_signature_of_this_file_and_this_version() {
        let (pair, key) = key_pair();
        let signed = signature(&pair, INSTALLER, b"installer");
        assert!(verify(INSTALLER, b"installer", &signed, &key));
        assert!(verify(INSTALLER, b"installer", &signed, &format!("{key}\n")));

        // Other contents: damaged or replaced on the way.
        assert!(!verify(INSTALLER, b"installes", &signed, &key));
        // Another name: an older signed installer offered as this version.
        let older = "good-gallery-1.2.2-windows-x64-setup.exe";
        assert!(!verify(older, b"installer", &signed, &key));
        // Another key.
        let other = KeyPair::from_seed(Seed::new([8; 32]));
        let by_other = signature(&other, INSTALLER, b"installer");
        assert!(!verify(INSTALLER, b"installer", &by_other, &key));
        // No signature, or not one.
        assert!(!verify(INSTALLER, b"installer", b"", &key));
        assert!(!verify(INSTALLER, b"installer", &signed[..63], &key));
    }

    #[test]
    fn req_031_06_without_a_valid_key_nothing_verifies() {
        let (pair, _) = key_pair();
        let signed = signature(&pair, INSTALLER, b"installer");
        for key in ["", "abc", "zz", "é", &"00".repeat(31)] {
            assert!(!verify(INSTALLER, b"installer", &signed, key), "{key}");
        }
    }

    /// The real key has to be in place, or no release could ever be installed: `packaging/new-update-key.sh`.
    #[test]
    fn req_031_09_the_app_has_the_public_key_built_in() {
        assert!(
            hex(PUBLIC_KEY.trim()).is_some_and(|key| ed25519_compact::PublicKey::from_slice(&key).is_ok()),
            "packaging/update-signing-key.txt holds no public key; run packaging/new-update-key.sh"
        );
    }

    #[test]
    fn req_031_06_a_file_on_disk_is_read_for_verifying() {
        let (pair, key) = key_pair();
        let dir = tempfile::tempdir().unwrap();
        let (installer, sig) = (dir.path().join(INSTALLER), dir.path().join("sig"));
        std::fs::write(&installer, b"installer").unwrap();
        std::fs::write(&sig, signature(&pair, INSTALLER, b"installer")).unwrap();
        assert!(verify_files(INSTALLER, &installer, &sig, &key).unwrap());
        std::fs::write(&installer, b"replaced").unwrap();
        assert!(!verify_files(INSTALLER, &installer, &sig, &key).unwrap());
    }

    #[test]
    fn req_031_10_curl_stays_on_https_and_within_its_bounds() {
        let args = curl_args("https://example.org/x", 30, 1024);
        let joined = args.join(" ");
        for part in [
            "--proto =https",
            "--proto-redir =https",
            "--fail",
            "--max-time 30",
            "--max-filesize 1024",
        ] {
            assert!(joined.contains(part), "{joined}");
        }
        assert_eq!(args.last().unwrap(), "https://example.org/x");
    }

    #[test]
    fn req_031_07_only_a_folder_with_the_uninstaller_can_install() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("good-gallery.exe");
        assert_eq!(installed_in(&exe), None, "portable");
        std::fs::write(dir.path().join(UNINSTALLER), b"").unwrap();
        assert_eq!(installed_in(&exe).as_deref(), Some(dir.path()));
    }
}
