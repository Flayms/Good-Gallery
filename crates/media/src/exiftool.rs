//! A pool of long-running exiftool processes (`-stay_open`), read with the arguments of exiftool-vendored (the library
//! earlier versions read with), so rows indexed by them stay valid.

use crate::thumbnail::PreviewSource;
use base64::Engine as _;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// How long one read may take before its process is killed and replaced.
const TASK_TIMEOUT: Duration = Duration::from_secs(60);

/// Tags exiftool-vendored (v38) asks for as numbers (`-TAG#`): durations in seconds, orientation as 1–8.
const NUMERIC_TAGS: [&str; 7] = [
    "*Duration*",
    "GPSAltitude",
    "GPSLatitude",
    "GPSLongitude",
    "GPSPosition",
    "GeolocationPosition",
    "Orientation",
];

#[derive(Debug, thiserror::Error)]
pub enum ExifToolError {
    #[error("can't start exiftool ({path}): {source}")]
    Start { path: PathBuf, source: std::io::Error },
    #[error("exiftool stopped: {0}")]
    Io(#[from] std::io::Error),
    #[error("exiftool didn't answer within {0:?}")]
    Timeout(Duration),
    #[error("exiftool: {0}")]
    Failed(String),
}

/// The exiftool executable: `GOOD_GALLERY_EXIFTOOL`, `exiftool\exiftool.exe` next to the app, or `exiftool` on `PATH`.
pub fn find_exiftool() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("GOOD_GALLERY_EXIFTOOL") {
        return Some(PathBuf::from(path));
    }
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            let (folder, file) = crate::helpers::EXIFTOOL_BUNDLED;
            Some(exe.parent()?.join(folder).join(file))
        })
        .filter(|path| path.is_file());
    bundled.or_else(|| {
        // On Windows only the executable: an extensionless `exiftool` there is the Perl script, which can't be run.
        let name = if cfg!(windows) { "exiftool.exe" } else { "exiftool" };
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    })
}

/// exiftool-vendored's filter that repairs invalid UTF-8 in values: such a value comes back as an object
/// `{"__etvInvalidUtf8V1": {"replacement": "s:<repaired>", …}}` (see [`unwrap_invalid_utf8`]).
const UTF8_FILTER: &str = r#"Filter=if (Image::ExifTool::IsUTF8(\$_) < 0) { my $binary; { package DB; for (my $i = 0; ; ++$i) { my @caller = caller($i); last unless @caller; next unless $caller[3] eq "Image::ExifTool::Filter"; my $arg = $DB::args[2]; if (ref($arg) && ref($$arg) eq "SCALAR") { $binary = 1; last; } } } unless ($binary) { my $raw = $_; tr/\0//d; Image::ExifTool::XMP::FixUTF8( \$_, Image::ExifTool::PackUTF8(0xfffd) ) if Image::ExifTool::IsUTF8(\$_) < 0; $_ = { "__etvInvalidUtf8V1" => { replacement => "s:" . $_, rawBase64 => "b64:" . Image::ExifTool::XMP::EncodeBase64($raw, 1) } }; } }"#;
const INVALID_UTF8_MARKER: &str = "__etvInvalidUtf8V1";

/// The arguments of one read, without the trailing `-execute`, as exiftool-vendored v38's `ReadTask` with its default
/// options builds them: `-fast`, the UTF-8 filter, `struct=1`, MWG composite tags, `keepUTCTime`, numeric tags before
/// `-all` (the first mention of a tag wins), then `-ignoreMinorErrors`.
fn read_args(file: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-charset".into(),
        "filename=utf8".into(),
        "-json".into(),
        "-fast".into(),
    ];
    args.extend(["-api".into(), UTF8_FILTER.into()]);
    args.extend(["-api", "struct=1", "-use", "MWG", "-api", "keepUTCTime"].map(String::from));
    args.extend(NUMERIC_TAGS.iter().map(|tag| format!("-{tag}#")));
    args.push("-all".into());
    args.push(file.to_string_lossy().into_owned());
    args.push("-ignoreMinorErrors".into());
    args
}

/// Replaces the UTF-8 filter's objects with their repaired strings, at any depth.
fn unwrap_invalid_utf8(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            let repaired = map
                .get(INVALID_UTF8_MARKER)
                .and_then(|marker| marker.get("replacement"))
                .and_then(serde_json::Value::as_str)
                .map(|text| text.strip_prefix("s:").unwrap_or(text).to_string());
            match repaired {
                Some(text) => *value = serde_json::Value::String(text),
                None => map.values_mut().for_each(unwrap_invalid_utf8),
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(unwrap_invalid_utf8),
        _ => {}
    }
}

struct Process {
    child: Child,
    stdin: ChildStdin,
    /// Everything exiftool printed up to each `{ready<n>}` line.
    replies: Receiver<std::io::Result<String>>,
    next_task: u64,
}

impl Process {
    fn start(path: &Path) -> Result<Self, ExifToolError> {
        let mut command = Command::new(path);
        command
            .args(["-stay_open", "True", "-@", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = crate::helpers::spawn_helper(command).map_err(|source| ExifToolError::Start {
            path: path.to_path_buf(),
            source,
        })?;
        let stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let stderr = child.stderr.take().expect("piped");
        let (tx, replies) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut reply = String::new();
            let mut line = Vec::new();
            loop {
                line.clear();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) => return,
                    Ok(_) => {
                        // Lossy: a stray invalid byte must not cost the whole reply (the filter repairs values).
                        let text = String::from_utf8_lossy(&line);
                        let text = text.trim_end_matches(['\r', '\n']);
                        if text.starts_with("{ready") && text.ends_with('}') {
                            if tx.send(Ok(std::mem::take(&mut reply))).is_err() {
                                return;
                            }
                        } else {
                            reply.push_str(text);
                            reply.push('\n');
                        }
                    }
                    Err(error) => {
                        let _ = tx.send(Err(error));
                        return;
                    }
                }
            }
        });
        // exiftool reports per-file errors on stderr; the JSON on stdout says enough, so this only keeps the pipe empty.
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                tracing::debug!("exiftool: {line}");
            }
        });
        Ok(Self {
            child,
            stdin,
            replies,
            next_task: 0,
        })
    }

    /// Runs one command (one argument per line) and returns what it printed.
    fn run(&mut self, args: &[String]) -> Result<String, ExifToolError> {
        self.next_task += 1;
        let mut input = String::new();
        for arg in args {
            input.push_str(arg);
            input.push('\n');
        }
        input.push_str(&format!("-execute{}\n", self.next_task));
        self.stdin.write_all(input.as_bytes())?;
        self.stdin.flush()?;
        match self.replies.recv_timeout(TASK_TIMEOUT) {
            Ok(reply) => Ok(reply?),
            Err(RecvTimeoutError::Timeout) => Err(ExifToolError::Timeout(TASK_TIMEOUT)),
            Err(RecvTimeoutError::Disconnected) => Err(ExifToolError::Failed("process exited".into())),
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.stdin.write_all(b"-stay_open\nFalse\n");
        let _ = self.stdin.flush();
        // Don't wait for a process that may hang (that's often why it is dropped). The Windows exiftool.exe is a
        // launcher whose perl.exe child holds the pipes, so end the whole process tree.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = Command::new("taskkill")
                .args(["/PID", &self.child.id().to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x0800_0000)
                .status();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Pool {
    idle: Vec<Process>,
    /// Processes running or idle.
    count: usize,
    max: usize,
}

/// Up to `max` exiftool processes, started on demand and reused.
pub struct ExifTool {
    path: PathBuf,
    pool: Mutex<Pool>,
    available: Condvar,
}

impl ExifTool {
    pub fn new(path: PathBuf, max_processes: usize) -> Self {
        Self {
            path,
            pool: Mutex::new(Pool {
                idle: Vec::new(),
                count: 0,
                max: max_processes.max(1),
            }),
            available: Condvar::new(),
        }
    }

    /// Raises the pool size to at least `max`; never lowers it.
    pub fn grow_max_processes(&self, max: usize) {
        let mut pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
        if max > pool.max {
            pool.max = max;
            self.available.notify_all();
        }
    }

    /// Changes the pool size; extra idle processes end now, busy ones when they are returned.
    pub fn set_max_processes(&self, max: usize) {
        let mut pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
        pool.max = max.max(1);
        while pool.count > pool.max && pool.idle.pop().is_some() {
            pool.count -= 1;
        }
        self.available.notify_all();
    }

    fn acquire(&self) -> Result<Process, ExifToolError> {
        let mut pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if let Some(process) = pool.idle.pop() {
                return Ok(process);
            }
            if pool.count < pool.max {
                pool.count += 1;
                drop(pool);
                return Process::start(&self.path).inspect_err(|_| self.release(None));
            }
            pool = self.available.wait(pool).unwrap_or_else(|p| p.into_inner());
        }
    }

    /// Returns a healthy process to the pool, or forgets a broken one (`None`).
    fn release(&self, process: Option<Process>) {
        let surplus = {
            let mut pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
            let surplus = match process {
                Some(process) if pool.count <= pool.max => {
                    pool.idle.push(process);
                    None
                }
                other => {
                    pool.count -= 1;
                    other
                }
            };
            self.available.notify_one();
            surplus
        };
        // Ending a process takes a while; not while holding the pool lock.
        drop(surplus);
    }

    /// Process ids of the idle processes.
    #[cfg(test)]
    pub(crate) fn process_ids(&self) -> Vec<u32> {
        let pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
        pool.idle.iter().map(|process| process.child.id()).collect()
    }

    /// All tags of `file` as exiftool-vendored reads them: one JSON object.
    pub fn read(&self, file: &Path) -> Result<serde_json::Map<String, serde_json::Value>, ExifToolError> {
        parse_reply(&self.run(&read_args(file))?)
    }

    /// A binary tag of `file` (e.g. an embedded preview), as exiftool-vendored's `extractBinaryTagToBuffer` reads it;
    /// `None` if the file doesn't have it.
    pub fn read_binary(&self, tag: &str, file: &Path) -> Result<Option<Vec<u8>>, ExifToolError> {
        let tags = parse_reply(&self.run(&binary_args(tag, file))?)?;
        // exiftool may spell the tag in another letter case.
        let value = tags
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(tag))
            .and_then(|(_, value)| value.as_str());
        Ok(value
            .and_then(|value| value.strip_prefix("base64:"))
            .and_then(|data| base64::engine::general_purpose::STANDARD.decode(data).ok()))
    }

    fn run(&self, args: &[String]) -> Result<String, ExifToolError> {
        let mut process = self.acquire()?;
        let result = process.run(args);
        if result.is_ok() {
            self.release(Some(process));
        } else {
            // A timed-out or broken process can't be trusted with the next file.
            drop(process);
            self.release(None);
        }
        result
    }
}

/// Embedded JPEG previews for thumbnails (REQ-005-04). Most files have none; real read errors resurface when the
/// file itself is decoded.
impl PreviewSource for ExifTool {
    fn extract_preview(&self, file: &Path) -> Option<Vec<u8>> {
        self.read_binary("PreviewImage", file)
            .ok()
            .flatten()
            .filter(|data| !data.is_empty())
    }
}

/// The arguments of exiftool-vendored's `BinaryToBufferTask`: the tag as base64 in JSON.
fn binary_args(tag: &str, file: &Path) -> Vec<String> {
    vec![
        "-charset".into(),
        "filename=utf8".into(),
        "-json".into(),
        "-b".into(),
        format!("-{tag}"),
        file.to_string_lossy().into_owned(),
    ]
}

fn parse_reply(reply: &str) -> Result<serde_json::Map<String, serde_json::Value>, ExifToolError> {
    if reply.trim().is_empty() {
        return Err(ExifToolError::Failed("no output (file missing or unreadable)".into()));
    }
    let mut value: serde_json::Value =
        serde_json::from_str(reply).map_err(|e| ExifToolError::Failed(format!("unexpected output: {e}")))?;
    unwrap_invalid_utf8(&mut value);
    match value {
        serde_json::Value::Array(items) => match items.into_iter().next() {
            Some(serde_json::Value::Object(tags)) => Ok(tags),
            _ => Err(ExifToolError::Failed("no tags".into())),
        },
        _ => Err(ExifToolError::Failed("unexpected output".into())),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// exiftool for tests, or `None` (test skipped) when it isn't installed. `GG_REQUIRE_HELPERS=1` makes a missing
    /// exiftool a failure instead (CI).
    pub(crate) fn exiftool() -> Option<ExifTool> {
        match find_exiftool() {
            Some(path) => Some(ExifTool::new(path, 2)),
            None if std::env::var_os("GG_REQUIRE_HELPERS").is_some() => panic!("exiftool not found"),
            None => {
                eprintln!("skipped: exiftool not found (set GOOD_GALLERY_EXIFTOOL or put it on PATH)");
                None
            }
        }
    }

    #[test]
    fn uses_exiftool_vendoreds_arguments() {
        let mut args = read_args(Path::new("D:\\a.jpg"));
        assert_eq!(args.remove(5), UTF8_FILTER);

        assert_eq!(
            args.join(" "),
            "-charset filename=utf8 -json -fast -api -api struct=1 -use MWG -api keepUTCTime -*Duration*# \
             -GPSAltitude# -GPSLatitude# -GPSLongitude# -GPSPosition# -GeolocationPosition# -Orientation# -all \
             D:\\a.jpg -ignoreMinorErrors"
        );
        assert!(
            read_args(Path::new("D:\\a.jpg"))
                .iter()
                .all(|arg| !arg.contains(['\n', '\r', '\0']))
        );
    }

    #[test]
    fn unwraps_values_the_utf8_filter_repaired() {
        let mut value = serde_json::json!([{
            "Keywords": ["ok", {"__etvInvalidUtf8V1": {"replacement": "s:caf\u{fffd}", "rawBase64": "b64:Y2Fm6Q=="}}],
            "Make": {"__etvInvalidUtf8V1": {"replacement": "s:X", "rawBase64": "b64:WA=="}},
        }]);

        unwrap_invalid_utf8(&mut value);

        assert_eq!(
            value,
            serde_json::json!([{ "Keywords": ["ok", "caf\u{fffd}"], "Make": "X" }])
        );
    }

    /// REQ-011-09: names with spaces, apostrophes, dashes, `;` and `&` reach exiftool as one argument each.
    #[test]
    fn req_011_09_reads_files_with_awkward_names() {
        let Some(exiftool) = exiftool() else { return };
        let dir = tempfile::tempdir().unwrap();
        for name in ["-x.png", "a b.png", "it's (x).png", "a;b&c.png", "Ärger ✓.png"] {
            let path = dir.path().join(name);
            image::RgbImage::new(3, 2).save(&path).unwrap();

            let tags = exiftool.read(&path).unwrap();

            assert_eq!(tags["ImageWidth"], 3, "{name}");
            assert_eq!(tags["FileName"], name, "{name}");
        }
    }

    /// REQ-005-04: binary tags come back as the bytes exiftool stored; files without them give `None`.
    #[test]
    fn req_005_04_reads_embedded_images() {
        let Some(exiftool) = exiftool() else { return };
        let dir = tempfile::tempdir().unwrap();
        let (file, thumb) = (dir.path().join("a.jpg"), dir.path().join("t.jpg"));
        image::RgbImage::new(64, 48).save(&file).unwrap();
        image::RgbImage::new(16, 12).save(&thumb).unwrap();
        assert_eq!(exiftool.read_binary("ThumbnailImage", &file).unwrap(), None);
        assert_eq!(exiftool.extract_preview(&file), None);

        let status = Command::new(find_exiftool().unwrap())
            .arg("-overwrite_original")
            .arg(format!("-ThumbnailImage<={}", thumb.display()))
            .arg(&file)
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());

        assert_eq!(
            exiftool.read_binary("ThumbnailImage", &file).unwrap(),
            Some(std::fs::read(&thumb).unwrap())
        );
    }

    #[test]
    fn uses_exiftool_vendoreds_arguments_for_binary_tags() {
        assert_eq!(
            binary_args("PreviewImage", Path::new("D:\\a.jpg")).join(" "),
            "-charset filename=utf8 -json -b -PreviewImage D:\\a.jpg"
        );
    }

    #[test]
    fn a_missing_file_is_an_error_and_the_process_stays_usable() {
        let Some(exiftool) = exiftool() else { return };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        image::RgbImage::new(1, 1).save(&path).unwrap();

        assert!(exiftool.read(&dir.path().join("missing.jpg")).is_err());
        assert_eq!(exiftool.read(&path).unwrap()["ImageHeight"], 1);
    }

    #[test]
    fn reads_in_parallel() {
        let Some(exiftool) = exiftool() else { return };
        let dir = tempfile::tempdir().unwrap();
        let paths: Vec<_> = (1..=6)
            .map(|i| {
                let path = dir.path().join(format!("{i}.png"));
                image::RgbImage::new(i, 1).save(&path).unwrap();
                path
            })
            .collect();

        std::thread::scope(|scope| {
            for (i, path) in paths.iter().enumerate() {
                let exiftool = &exiftool;
                scope.spawn(move || assert_eq!(exiftool.read(path).unwrap()["ImageWidth"], i + 1));
            }
        });
    }
}
