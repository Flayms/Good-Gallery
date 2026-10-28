//! Helper processes (exiftool, later ffmpeg) end with the app. The Windows exiftool.exe is a launcher whose child does
//! the work, and neither a `-stay_open False` nor `taskkill /T` reliably ends both while the app exits; so the app puts
//! itself into a job object that kills every process in it once the app's handle closes, whatever way the app ends.
//! Processes the app starts join the job automatically.

/// Puts the current process into a kill-on-close job. Call once, early, before helpers start.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn end_helpers_with_app() -> std::io::Result<()> {
    use std::sync::OnceLock;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // The job handle stays open for the process's lifetime: closing it is what ends the helpers.
    static JOB: OnceLock<Result<usize, i32>> = OnceLock::new();
    let job = JOB.get_or_init(|| {
        // SAFETY: plain Win32 calls with valid arguments; `info` outlives the call that reads it.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(0));
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            // Breakaway allowed: programs the app opens for the user (Explorer, an editor) must outlive it, so they
            // are started with `CREATE_BREAKAWAY_FROM_JOB`.
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if set == 0 || AssignProcessToJobObject(job, GetCurrentProcess()) == 0 {
                return Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(0));
            }
            Ok(job as usize)
        }
    });
    job.map(|_| ()).map_err(std::io::Error::from_raw_os_error)
}

/// Linux: starts the thread that starts every helper ([`spawn_helper`]) now, on the calling thread (the main thread):
/// threads inherit the nice value of the thread that creates them, so a decode thread (nice 10, see
/// [`lower_thread_priority`]) that happened to start it would slow every helper down for good.
#[cfg(target_os = "linux")]
pub fn end_helpers_with_app() -> std::io::Result<()> {
    linux_spawner::spawner().map(drop)
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn end_helpers_with_app() -> std::io::Result<()> {
    Ok(())
}

/// Starts a helper program so that it ends with the app (REQ-018-07). Windows: the job object of
/// [`end_helpers_with_app`]. Linux: the child asks for `SIGKILL` when its parent goes (`PR_SET_PDEATHSIG`). That signal
/// fires when the *thread* that started the child ends, and helpers are started from short-lived scan threads and kept
/// in pools, so one thread that lives as long as the app starts them all.
pub fn spawn_helper(command: std::process::Command) -> std::io::Result<std::process::Child> {
    #[cfg(target_os = "linux")]
    {
        linux_spawner::spawn(command)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let mut command = command;
        command.spawn()
    }
}

#[cfg(target_os = "linux")]
mod linux_spawner {
    use std::process::{Child, Command};
    use std::sync::{OnceLock, mpsc};

    pub type Request = (Command, mpsc::Sender<std::io::Result<Child>>);

    /// The thread that starts helpers, started on the first call.
    pub fn spawner() -> std::io::Result<&'static mpsc::Sender<Request>> {
        static SPAWNER: OnceLock<std::io::Result<mpsc::Sender<Request>>> = OnceLock::new();
        let spawner = SPAWNER.get_or_init(|| {
            let (tx, rx) = mpsc::channel::<Request>();
            std::thread::Builder::new()
                .name("helper spawner".into())
                .spawn(move || {
                    for (mut command, reply) in rx {
                        let _ = reply.send(start(&mut command));
                    }
                })
                .map(|_| tx)
        });
        spawner
            .as_ref()
            .map_err(|error| std::io::Error::new(error.kind(), error.to_string()))
    }

    pub fn spawn(command: Command) -> std::io::Result<Child> {
        let spawner = spawner()?;
        let (tx, rx) = mpsc::channel();
        spawner
            .send((command, tx))
            .map_err(|_| std::io::Error::other("the helper spawner ended"))?;
        rx.recv()
            .map_err(|_| std::io::Error::other("the helper spawner ended"))?
    }

    #[allow(unsafe_code)]
    fn start(command: &mut Command) -> std::io::Result<Child> {
        use std::os::unix::process::CommandExt;
        let app = std::process::id();
        // SAFETY: the closure runs in the forked child before exec and only makes async-signal-safe syscalls.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                // The app ended between fork and prctl: the signal will never come.
                if libc::getppid() as u32 != app {
                    return Err(std::io::Error::other("the app ended"));
                }
                Ok(())
            });
        }
        command.spawn()
    }
}

/// Runs the calling thread below normal priority, so background work (thumbnail decoding) doesn't take time from the UI
/// thread. Measured: more or higher-priority decode threads cost frame time on an integrated GPU.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn lower_thread_priority() {
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
    // SAFETY: the pseudo handle of the current thread is always valid.
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

/// Linux applies the nice value per thread (REQ-018-08).
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
pub fn lower_thread_priority() {
    // SAFETY: plain syscalls on the calling thread's own id.
    unsafe {
        let thread = libc::gettid();
        libc::setpriority(libc::PRIO_PROCESS, thread as libc::id_t, 10);
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn lower_thread_priority() {}

/// Opens Explorer on the folder of `file` with the file selected (REQ-009-10). Started outside the app's job object,
/// so the window stays when the app ends.
#[cfg(windows)]
pub fn show_in_explorer(file: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    // Explorer wants `/select,"path"` as one argument with its own quotes; Windows paths can't contain `"`.
    let start = |flags: u32| {
        std::process::Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", file.display()))
            .creation_flags(flags)
            .spawn()
            .map(drop)
    };
    // A job above the app's own (a launcher, a terminal) may forbid breaking away; explorer.exe hands the window to
    // the shell and exits anyway.
    start(CREATE_BREAKAWAY_FROM_JOB).or_else(|error| match error.kind() {
        std::io::ErrorKind::PermissionDenied => start(0),
        _ => Err(error),
    })
}

/// Opens the desktop's file manager on the folder of `file` with the file selected (REQ-018-05): the freedesktop
/// `FileManager1.ShowItems` D-Bus call (Nautilus, Dolphin, Nemo, …) through `gdbus` or `dbus-send`, else `xdg-open` on
/// the folder.
#[cfg(target_os = "linux")]
pub fn show_in_explorer(file: &std::path::Path) -> std::io::Result<()> {
    let mut last = None;
    for (program, args) in show_in_folder_commands(file) {
        match std::process::Command::new(program)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => last = Some(std::io::Error::other(format!("{program}: {status}"))),
            Err(error) => last = Some(error),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no file manager")))
}

/// The commands that show `file` in a file manager, best first: `(program, arguments)`.
#[cfg(target_os = "linux")]
fn show_in_folder_commands(file: &std::path::Path) -> Vec<(&'static str, Vec<std::ffi::OsString>)> {
    let uri = file_uri(file);
    let args = |args: &[&str]| args.iter().map(Into::into).collect::<Vec<std::ffi::OsString>>();
    let mut commands = vec![
        (
            "gdbus",
            args(&[
                "call",
                "--session",
                "--dest=org.freedesktop.FileManager1",
                "--object-path=/org/freedesktop/FileManager1",
                "--method=org.freedesktop.FileManager1.ShowItems",
                // GVariant text: an array of URIs and an empty startup id. The URI holds no quotes (`file_uri`).
                &format!("['{uri}']"),
                "",
            ]),
        ),
        (
            "dbus-send",
            args(&[
                "--session",
                "--print-reply",
                "--dest=org.freedesktop.FileManager1",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
                &format!("array:string:{uri}"),
                "string:",
            ]),
        ),
    ];
    if let Some(folder) = file.parent() {
        commands.push(("xdg-open", vec![folder.as_os_str().to_owned()]));
    }
    commands
}

/// `file://` and the absolute path, percent-encoded except for unreserved characters and `/` (RFC 3986), so the URI
/// holds no commas, quotes or spaces that `dbus-send` or GVariant text would split on.
#[cfg(target_os = "linux")]
fn file_uri(file: &std::path::Path) -> String {
    use std::fmt::Write;
    let mut uri = String::from("file://");
    for &byte in file.as_os_str().as_encoded_bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    uri
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn show_in_explorer(_file: &std::path::Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "not on this platform",
    ))
}

/// Where a bundled helper lies next to the app's executable: `(folder, file)`. The package (`cargo xtask dist`)
/// puts the helpers there.
pub type Bundled = (&'static str, &'static str);
pub const EXIFTOOL_BUNDLED: Bundled = ("exiftool", if cfg!(windows) { "exiftool.exe" } else { "exiftool" });
pub const FFMPEG_BUNDLED: Bundled = ("ffmpeg", if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" });
pub const MPV_BUNDLED: Bundled = ("mpv", if cfg!(windows) { "libmpv-2.dll" } else { "libmpv.so.2" });

/// libmpv for video playback: `GOOD_GALLERY_MPV`, `mpv/libmpv-2.dll` next to the app, or on `PATH`. On Linux the
/// system's library: the bare name, which the dynamic loader looks up in its own paths (REQ-018-03).
pub fn find_mpv() -> Option<std::path::PathBuf> {
    let (folder, name) = MPV_BUNDLED;
    if cfg!(target_os = "linux") {
        return find_bundled("GOOD_GALLERY_MPV", folder, name).or_else(|| Some(name.into()));
    }
    find_helper("GOOD_GALLERY_MPV", folder, name)
}

/// A bundled helper file: the path in the environment variable `env`, else `<folder>/<name>` next to the app, else
/// `name` on `PATH`.
pub fn find_helper(env: &str, folder: &str, name: &str) -> Option<std::path::PathBuf> {
    find_bundled(env, folder, name).or_else(|| {
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    })
}

/// The path in the environment variable `env`, else `<folder>/<name>` next to the app if it is there.
fn find_bundled(env: &str, folder: &str, name: &str) -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os(env) {
        return Some(path.into());
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(folder).join(name)))
        .filter(|path| path.is_file())
}

/// The refresh rate of the primary display in Hz, if Windows reports one: video frames are timed to it.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn display_refresh_rate() -> Option<f64> {
    use windows_sys::Win32::Graphics::Gdi::{DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW};
    // SAFETY: DEVMODEW is plain data; zeroed with its size set is the documented way to ask.
    let mut mode: DEVMODEW = unsafe { std::mem::zeroed() };
    mode.dmSize = size_of::<DEVMODEW>() as u16;
    // SAFETY: a null device name means the display the calling thread runs on; `mode` lives for the call.
    let ok = unsafe { EnumDisplaySettingsW(std::ptr::null(), ENUM_CURRENT_SETTINGS, &raw mut mode) };
    // 0 and 1 mean "the hardware's default".
    (ok != 0 && mode.dmDisplayFrequency > 1).then_some(f64::from(mode.dmDisplayFrequency))
}

#[cfg(not(windows))]
pub fn display_refresh_rate() -> Option<f64> {
    None
}

/// Whether Windows' _Animation effects_ are off: animations become instant (`docs/design.md`).
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn reduced_motion() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW};
    let mut on: i32 = 1;
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL to the pointer, which lives for the call.
    let ok = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut on).cast(), 0) };
    ok != 0 && on == 0
}

/// Whether the desktop's animations are off (REQ-018-06): GNOME's `enable-animations` or KDE's animation speed at
/// _instant_. Reading them starts a process, so a background thread reads them every 2 s and this returns the last
/// answer.
#[cfg(target_os = "linux")]
pub fn reduced_motion() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static REDUCED: AtomicBool = AtomicBool::new(false);
    static FOLLOW: std::sync::Once = std::sync::Once::new();
    FOLLOW.call_once(|| {
        let kde = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktop| desktop.split(':').any(|d| d == "KDE"));
        // The first answer at once, so the first frames already follow the setting.
        REDUCED.store(read_reduced_motion(kde), Ordering::Relaxed);
        let spawned = std::thread::Builder::new()
            .name("desktop preferences".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    REDUCED.store(read_reduced_motion(kde), Ordering::Relaxed);
                }
            });
        if let Err(error) = spawned {
            tracing::warn!("can't follow the desktop's animation setting: {error}");
        }
    });
    REDUCED.load(Ordering::Relaxed)
}

#[cfg(target_os = "linux")]
fn read_reduced_motion(kde: bool) -> bool {
    let output = |program: &str, args: &[&str]| {
        std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
    };
    if kde {
        let args = [
            "--file",
            "kdeglobals",
            "--group",
            "KDE",
            "--key",
            "AnimationDurationFactor",
        ];
        let factor = output("kreadconfig6", &args).or_else(|| output("kreadconfig5", &args));
        if let Some(factor) = factor {
            return kde_animations_off(&factor);
        }
    }
    output(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "enable-animations"],
    )
    .is_some_and(|value| gnome_animations_off(&value))
}

/// `gsettings get org.gnome.desktop.interface enable-animations` printed `false`.
#[cfg(target_os = "linux")]
fn gnome_animations_off(value: &str) -> bool {
    value.trim() == "false"
}

/// KDE's `AnimationDurationFactor` is 0 (_Animation speed: Instant_); empty is the default, 1.
#[cfg(target_os = "linux")]
fn kde_animations_off(value: &str) -> bool {
    value.trim().parse::<f64>().is_ok_and(|factor| factor == 0.0)
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn reduced_motion() -> bool {
    false
}

/// Whether Windows' _Transparency effects_ are off: glass becomes opaque (REQ-013-06).
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn reduced_transparency() -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let wide = |text: &str| text.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let name = wide("EnableTransparency");
    let mut value: u32 = 1;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: both strings are NUL-terminated and live for the call; `value` has room for the DWORD asked for.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut value).cast(),
            &raw mut size,
        )
    };
    // Missing means the default: transparency on.
    status == 0 && value == 0
}

#[cfg(not(windows))]
pub fn reduced_transparency() -> bool {
    false
}

#[cfg(any(windows, target_os = "linux"))]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::exiftool::{ExifTool, find_exiftool};
    use std::process::Command;
    use std::time::{Duration, Instant};

    const CHILD: &str = "GG_HELPERS_TEST_CHILD";

    /// Runs only as the child of `exiftool_ends_with_an_app_that_exits_without_cleaning_up`.
    #[test]
    fn child_starts_exiftool_and_exits_without_cleaning_up() {
        let Some(file) = std::env::var_os(CHILD) else { return };
        end_helpers_with_app().unwrap();
        let exiftool = ExifTool::new(find_exiftool().unwrap(), 1);
        let _ = exiftool.read(std::path::Path::new(&file));
        for pid in exiftool.process_ids() {
            println!("pid {pid}");
        }
        // Like the app's exit while the indexer thread still holds the pool: no drop runs.
        std::mem::forget(exiftool);
        std::process::exit(0);
    }

    /// A process that ended but whose parent hasn't reaped it yet (a zombie) counts as ended.
    #[cfg(target_os = "linux")]
    fn is_running(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            stat.rsplit(')')
                .next()
                .is_some_and(|rest| !rest.trim_start().starts_with('Z'))
        })
    }

    #[cfg(windows)]
    fn is_running(pid: u32) -> bool {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
    }

    /// REQ-018-07 (Linux): exiftool ends with the app, however the app ends.
    #[test]
    fn exiftool_ends_with_an_app_that_exits_without_cleaning_up() {
        if find_exiftool().is_none() {
            assert!(std::env::var_os("GG_REQUIRE_HELPERS").is_none(), "exiftool not found");
            eprintln!("skipped: exiftool not found");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.png");
        image::RgbImage::new(2, 2).save(&file).unwrap();

        // Output to a file: surviving exiftool processes would keep a pipe open.
        let out = dir.path().join("out.txt");
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "helpers::tests::child_starts_exiftool_and_exits_without_cleaning_up",
            ])
            .args(["--nocapture", "--test-threads", "1"])
            .env(CHILD, &file)
            .stdout(std::fs::File::create(&out).unwrap())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());

        let stdout = std::fs::read_to_string(&out).unwrap();
        let pids: Vec<u32> = stdout
            .lines()
            // libtest prints the test name on the same line.
            .filter_map(|line| line.split("pid ").nth(1)?.trim().parse().ok())
            .collect();
        assert!(!pids.is_empty(), "the child started no exiftool: {stdout}");
        let deadline = Instant::now() + Duration::from_secs(10);
        while pids.iter().any(|&pid| is_running(pid)) {
            assert!(Instant::now() < deadline, "exiftool {pids:?} outlived the app");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(target_os = "linux")]
#[cfg(test)]
mod linux_tests {
    use super::*;

    /// REQ-018-05: the file manager gets the file as an escaped `file://` URI; `xdg-open` on the folder is the fallback.
    #[test]
    fn req_018_05_asks_the_file_manager_to_select_the_file() {
        let file = std::path::Path::new("/media/Photos/Ana's trip, 2024/IMG 1.jpg");
        let uri = "file:///media/Photos/Ana%27s%20trip%2C%202024/IMG%201.jpg";
        assert_eq!(file_uri(file), uri);
        let commands = show_in_folder_commands(file);
        let programs: Vec<_> = commands.iter().map(|(program, _)| *program).collect();
        assert_eq!(programs, ["gdbus", "dbus-send", "xdg-open"]);
        assert!(commands[0].1.contains(&format!("['{uri}']").into()));
        assert!(commands[1].1.contains(&format!("array:string:{uri}").into()));
        assert_eq!(commands[2].1, ["/media/Photos/Ana's trip, 2024"]);
        assert_eq!(file_uri(std::path::Path::new("/ä")), "file:///%C3%A4");
    }

    /// REQ-018-06: GNOME's and KDE's ways of turning animations off.
    #[test]
    fn req_018_06_reads_the_desktops_animation_setting() {
        assert!(gnome_animations_off("false\n"));
        assert!(!gnome_animations_off("true\n"));
        assert!(!gnome_animations_off(""));
        assert!(kde_animations_off("0\n"));
        assert!(!kde_animations_off("0.5\n"));
        assert!(!kde_animations_off("\n"));
    }

    /// REQ-018-08: decode threads run below the UI thread; other threads keep their priority.
    #[test]
    fn req_018_08_lowers_the_calling_threads_priority_only() {
        // Field 19 of the thread's stat line; the fields after the command name (in parentheses) start at field 3.
        let nice = || {
            let stat = std::fs::read_to_string("/proc/thread-self/stat").unwrap();
            let fields = &stat[stat.rfind(')').unwrap() + 2..];
            fields.split(' ').nth(16).unwrap().parse::<i32>().unwrap()
        };
        let before = nice();
        let lowered = std::thread::spawn(move || {
            lower_thread_priority();
            nice()
        })
        .join()
        .unwrap();
        assert_eq!(lowered, (before + 10).min(19));
        assert_eq!(nice(), before);
    }

    /// REQ-018-03: libmpv is the system's, found by the dynamic loader, and exiftool the distribution's Perl script.
    #[test]
    fn req_018_03_finds_the_systems_helpers() {
        if std::env::var_os("GOOD_GALLERY_MPV").is_none() {
            assert_eq!(find_mpv(), Some("libmpv.so.2".into()));
        }
        assert_eq!(EXIFTOOL_BUNDLED.1, "exiftool");
        match crate::exiftool::find_exiftool() {
            Some(path) => assert_eq!(path.file_name().unwrap(), "exiftool"),
            None => assert!(std::env::var_os("GG_REQUIRE_HELPERS").is_none(), "exiftool not found"),
        }
    }
}
