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

/// Other systems end a process's pipes with it, which ends exiftool.
#[cfg(not(windows))]
pub fn end_helpers_with_app() -> std::io::Result<()> {
    Ok(())
}

/// Runs the calling thread below normal priority, so background work (thumbnail decoding) doesn't take time from the
/// UI thread. Measured in M0: more or higher-priority decode threads cost frame time on an integrated GPU.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn lower_thread_priority() {
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
    // SAFETY: the pseudo handle of the current thread is always valid.
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[cfg(not(windows))]
pub fn lower_thread_priority() {}

#[cfg(all(test, windows))]
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

    fn is_running(pid: u32) -> bool {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\""))
    }

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
