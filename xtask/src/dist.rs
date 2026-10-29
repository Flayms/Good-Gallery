//! `cargo xtask dist [--helpers <dir>] [--out <dir>] [--no-build] [--no-installer]`: the Windows downloads of a
//! release (REQ-014-08). Builds the app in release, stages it with the helper programs (from
//! `packaging/fetch-helpers.ps1`, default `target/helpers`) in the layout the app looks for, then writes
//! `good-gallery-<version>-windows-x64-portable.zip` and, with NSIS, `good-gallery-<version>-windows-x64-setup.exe` into `target/dist`.
//!
//! On Linux it writes `good-gallery-<version>-linux-x64.tar.gz` instead (REQ-018-09): the app, its menu entry, icon,
//! install script and license files, without helpers (the app uses the system's). `--helpers` and `--no-installer`
//! don't apply there.

use anyhow::{Context as _, bail};
use media::helpers::{Bundled, EXIFTOOL_BUNDLED, FFMPEG_BUNDLED, MPV_BUNDLED};
use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "usage: cargo xtask dist [--helpers <dir>] [--out <dir>] [--no-build] [--no-installer]";
/// The executable's name in the package (Cargo builds `app.exe`).
pub const EXE: &str = "good-gallery.exe";
/// The staged package's folder in the output folder.
const STAGE: &str = "good-gallery";
/// Helpers in the package: what the app looks for, and the helper folder to copy (exiftool needs all of its
/// folder: `exiftool_files` holds its Perl).
const HELPERS: [(Bundled, bool); 3] = [(EXIFTOOL_BUNDLED, true), (FFMPEG_BUNDLED, false), (MPV_BUNDLED, false)];

/// The installer's and the portable zip's file names.
pub fn artifact_names(version: &str) -> (String, String) {
    (
        format!("good-gallery-{version}-windows-x64-setup.exe"),
        format!("good-gallery-{version}-windows-x64-portable.zip"),
    )
}

/// The Linux download's name.
pub fn linux_artifact_name(version: &str) -> String {
    format!("good-gallery-{version}-linux-x64.tar.gz")
}

/// The files of the Linux package besides the executable: `(source below the workspace, name in the package)`.
const LINUX_FILES: [(&str, &str); 6] = [
    ("LICENSE", "LICENSE.txt"),
    ("packaging/linux/THIRD-PARTY-NOTICES.txt", "THIRD-PARTY-NOTICES.txt"),
    ("packaging/linux/README.txt", "README.txt"),
    ("packaging/linux/install.sh", "install.sh"),
    ("packaging/linux/good-gallery.desktop", "good-gallery.desktop"),
    ("crates/app/assets/app-icon.png", "good-gallery.png"),
];

/// Stages the Linux package into `stage` (emptied first): the executable as `good-gallery` and [`LINUX_FILES`] from
/// `root`. Copies keep the sources' permissions, so the app and `install.sh` stay executable.
pub fn stage_linux(exe: &Path, root: &Path, stage: &Path) -> anyhow::Result<()> {
    if stage.exists() {
        std::fs::remove_dir_all(stage).with_context(|| format!("can't empty {}", stage.display()))?;
    }
    std::fs::create_dir_all(stage)?;
    std::fs::copy(exe, stage.join("good-gallery")).with_context(|| format!("can't copy {}", exe.display()))?;
    for (source, name) in LINUX_FILES {
        let source = root.join(source);
        std::fs::copy(&source, stage.join(name)).with_context(|| format!("can't copy {}", source.display()))?;
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from).with_context(|| format!("can't read {}", from.display()))? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).with_context(|| format!("can't copy {}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Stages the package into `stage` (emptied first): the executable, the helpers below `helpers` and the license
/// files at the top.
pub fn stage(exe: &Path, helpers: &Path, licenses: &[(&Path, &str)], stage: &Path) -> anyhow::Result<()> {
    for ((folder, file), _) in HELPERS {
        let source = helpers.join(folder).join(file);
        if !source.is_file() {
            bail!(
                "{} is missing; run packaging/fetch-helpers.ps1 (or pass --helpers)",
                source.display()
            );
        }
    }
    if stage.exists() {
        std::fs::remove_dir_all(stage).with_context(|| format!("can't empty {}", stage.display()))?;
    }
    std::fs::create_dir_all(stage)?;
    std::fs::copy(exe, stage.join(EXE)).with_context(|| format!("can't copy {}", exe.display()))?;
    for ((folder, file), whole_folder) in HELPERS {
        if whole_folder {
            copy_dir(&helpers.join(folder), &stage.join(folder))?;
        } else {
            std::fs::create_dir_all(stage.join(folder))?;
            std::fs::copy(helpers.join(folder).join(file), stage.join(folder).join(file))?;
        }
    }
    for (source, name) in licenses {
        std::fs::copy(source, stage.join(name)).with_context(|| format!("can't copy {}", source.display()))?;
    }
    Ok(())
}

/// Zips the staged package's contents (no top folder) with Windows' own tar, which writes zip files.
fn zip(stage: &Path, out: &Path) -> anyhow::Result<()> {
    let tar = PathBuf::from(std::env::var_os("SystemRoot").context("SystemRoot isn't set")?).join(r"System32\tar.exe");
    let mut entries: Vec<_> = std::fs::read_dir(stage)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<_, _>>()?;
    entries.sort();
    if out.exists() {
        std::fs::remove_file(out)?;
    }
    crate::process::run_command(
        Command::new(tar)
            .arg("-a")
            .arg("-c")
            .arg("-f")
            .arg(out)
            .arg("-C")
            .arg(stage)
            .args(entries),
    )
}

/// makensis: `MAKENSIS`, on `PATH`, or where the NSIS installer (and Chocolatey) put it.
fn find_makensis() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("MAKENSIS") {
        return Some(path.into());
    }
    let on_path = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("makensis.exe"))
            .find(|path| path.is_file())
    });
    on_path.or_else(|| {
        ["ProgramFiles(x86)", "ProgramFiles"]
            .iter()
            .filter_map(std::env::var_os)
            .map(|dir| PathBuf::from(dir).join(r"NSIS\makensis.exe"))
            .find(|path| path.is_file())
    })
}

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let root = std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join(".."))?;
    let mut helpers = root.join("target").join("helpers");
    let mut out = root.join("target").join("dist");
    let (mut build, mut installer) = (true, true);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--helpers" => helpers = std::path::absolute(args.next().context(USAGE)?)?,
            "--out" => out = std::path::absolute(args.next().context(USAGE)?)?,
            "--no-build" => build = false,
            "--no-installer" => installer = false,
            other => bail!("unknown argument {other} ({USAGE})"),
        }
    }
    // xtask shares the workspace version with the app.
    let version = env!("CARGO_PKG_VERSION");
    if build {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let status = Command::new(cargo)
            .args(["build", "--release", "--locked", "-p", "app", "--features", "packaged"])
            .current_dir(&root)
            .status()?;
        if !status.success() {
            bail!("the release build failed");
        }
    }
    if cfg!(target_os = "linux") {
        let name = linux_artifact_name(version);
        let folder = name.trim_end_matches(".tar.gz").to_owned();
        let stage_dir = out.join(&folder);
        stage_linux(&root.join("target/release/app"), &root, &stage_dir)?;
        println!("staged {}", stage_dir.display());
        // One top folder in the archive, as Linux archives have.
        crate::process::run_command(
            Command::new("tar")
                .arg("-czf")
                .arg(out.join(&name))
                .arg("-C")
                .arg(&out)
                .arg(&folder),
        )?;
        println!("{}", out.join(&name).display());
        return Ok(());
    }
    let exe = root.join(r"target\release\app.exe");
    let stage_dir = out.join(STAGE);
    stage(
        &exe,
        &helpers,
        &[
            (&root.join("LICENSE"), "LICENSE.txt"),
            (
                &root.join(r"packaging\windows\THIRD-PARTY-NOTICES.txt"),
                "THIRD-PARTY-NOTICES.txt",
            ),
        ],
        &stage_dir,
    )?;
    println!("staged {}", stage_dir.display());

    let (setup_name, zip_name) = artifact_names(version);
    zip(&stage_dir, &out.join(&zip_name))?;
    println!("{}", out.join(&zip_name).display());

    if installer {
        let makensis = find_makensis().context("makensis wasn't found: install NSIS or set MAKENSIS")?;
        crate::process::run_command(
            Command::new(makensis)
                .arg("/V2")
                .arg(format!("/DVERSION={version}"))
                .arg(format!("/DSOURCE={}", stage_dir.display()))
                .arg(format!("/DOUTFILE={}", out.join(&setup_name).display()))
                .arg(format!(
                    "/DICON={}",
                    root.join(r"crates\app\assets\app-icon.ico").display()
                ))
                .arg(root.join(r"packaging\windows\installer.nsi")),
        )?;
        println!("{}", out.join(&setup_name).display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, path.to_string_lossy().as_bytes()).unwrap();
    }

    /// REQ-014-08, REQ-014-09: the package holds the app and every helper where the app looks for it
    /// (`media::helpers`), so it runs without anything else installed.
    #[test]
    fn stages_the_helpers_where_the_app_finds_them() {
        let dir = tempfile::tempdir().unwrap();
        let (exe, helpers, license) = (
            dir.path().join("app.exe"),
            dir.path().join("helpers"),
            dir.path().join("LICENSE"),
        );
        touch(&exe);
        touch(&license);
        for ((folder, file), _) in HELPERS {
            touch(&helpers.join(folder).join(file));
        }
        touch(&helpers.join("exiftool/exiftool_files/perl.dll"));
        touch(&helpers.join("mpv/include/mpv/client.h"));
        let out = dir.path().join("stage");
        touch(&out.join("left-over.txt"));

        stage(&exe, &helpers, &[(&license, "LICENSE.txt")], &out).unwrap();

        assert!(out.join(EXE).is_file());
        assert!(out.join("LICENSE.txt").is_file());
        for (folder, file) in [EXIFTOOL_BUNDLED, FFMPEG_BUNDLED, MPV_BUNDLED] {
            assert!(out.join(folder).join(file).is_file(), "{folder}/{file}");
        }
        assert!(out.join("exiftool/exiftool_files/perl.dll").is_file());
        assert!(!out.join("mpv/include").exists(), "only the DLL of the mpv package");
        assert!(!out.join("left-over.txt").exists());
    }

    #[test]
    fn says_how_to_get_missing_helpers() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("app.exe"));
        let error = stage(
            &dir.path().join("app.exe"),
            &dir.path().join("helpers"),
            &[],
            &dir.path().join("out"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("fetch-helpers.ps1"), "{error}");
    }

    /// REQ-014-08, REQ-014-10: the release attaches setup.exe and the portable zip named after the version, the system
    /// and the architecture.
    #[test]
    fn req_014_10_names_say_the_system_and_the_architecture() {
        assert_eq!(
            artifact_names("1.2.3"),
            (
                "good-gallery-1.2.3-windows-x64-setup.exe".to_owned(),
                "good-gallery-1.2.3-windows-x64-portable.zip".to_owned()
            )
        );
        assert_eq!(linux_artifact_name("1.2.3"), "good-gallery-1.2.3-linux-x64.tar.gz");
    }

    /// REQ-018-09: the Linux package holds the app, its menu entry, icon, install script and licenses, and the app and
    /// the script stay executable.
    #[cfg(unix)]
    #[test]
    fn req_018_09_stages_the_linux_package() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("..")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("app");
        touch(&exe);
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let out = dir.path().join("good-gallery-1.2.3-linux-x64");
        touch(&out.join("left-over.txt"));

        stage_linux(&exe, &root, &out).unwrap();

        let mut names: Vec<_> = std::fs::read_dir(&out)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        let expected = [
            "LICENSE.txt",
            "README.txt",
            "THIRD-PARTY-NOTICES.txt",
            "good-gallery",
            "good-gallery.desktop",
            "good-gallery.png",
            "install.sh",
        ];
        assert_eq!(names, expected);
        for executable in ["good-gallery", "install.sh"] {
            let mode = std::fs::metadata(out.join(executable)).unwrap().permissions().mode();
            assert!(mode & 0o111 != 0, "{executable} isn't executable");
        }
        let entry = std::fs::read_to_string(out.join("good-gallery.desktop")).unwrap();
        // The window's app id (`crates/app/src/main.rs`) and the icon's name, so docks match the window to the entry.
        for line in ["Exec=good-gallery", "Icon=good-gallery", "StartupWMClass=good-gallery"] {
            assert!(entry.lines().any(|l| l == line), "{line}");
        }
    }
}
