//! The thumbnail cache: WebP files in the layout earlier versions wrote, so an existing cache is read as is
//! (REQ-005-15). Size-capped with LRU eviction by file mtime; reading a thumbnail bumps its mtime.

use domain::thumbs::ThumbKey;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime};

const TMP_SUFFIX: &str = ".tmp";
/// Evicting below the cap leaves headroom, so eviction doesn't run on every write.
const EVICT_TO: f64 = 0.9;
/// Reading a thumbnail refreshes its LRU timestamp at most this often (REQ-005-11).
const TOUCH_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// `<dir>/ab/cd/<sha1>.webp` ([`domain::thumbs::thumb_hash`]), fanned out so no folder grows huge.
pub fn thumb_path(dir: &Path, key: &ThumbKey, width: u32) -> PathBuf {
    let hash = domain::thumbs::thumb_hash(key, width);
    dir.join(&hash[0..2]).join(&hash[2..4]).join(format!("{hash}.webp"))
}

struct CachedFile {
    path: PathBuf,
    size: u64,
    mtime: SystemTime,
}

struct Inner {
    dir: PathBuf,
    max_bytes: AtomicU64,
    bytes: AtomicU64,
    /// Bytes added by writes since the cache was opened, so a count from a folder walk can add the writes made while
    /// it walked.
    written: AtomicU64,
    measured: Mutex<bool>,
    measured_changed: Condvar,
    /// One eviction (or clear) at a time.
    evicting: Mutex<()>,
    eviction_queued: AtomicBool,
}

/// Size-capped thumbnail store. Cheap to clone; all clones share one cache.
#[derive(Clone)]
pub struct ThumbCache {
    inner: Arc<Inner>,
}

impl ThumbCache {
    /// Opens the cache in `dir` and measures its size on a thread, removing temp files a crash left behind
    /// (REQ-005-13).
    pub fn open(dir: PathBuf, max_bytes: u64) -> Self {
        let cache = Self {
            inner: Arc::new(Inner {
                dir,
                max_bytes: AtomicU64::new(max_bytes),
                bytes: AtomicU64::new(0),
                written: AtomicU64::new(0),
                measured: Mutex::new(false),
                measured_changed: Condvar::new(),
                evicting: Mutex::new(()),
                eviction_queued: AtomicBool::new(false),
            }),
        };
        let measuring = cache.clone();
        std::thread::Builder::new()
            .name("thumb cache".into())
            .spawn(move || measuring.measure())
            .expect("can't start a thread");
        cache
    }

    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }

    pub fn path(&self, key: &ThumbKey, width: u32) -> PathBuf {
        thumb_path(&self.inner.dir, key, width)
    }

    /// Blocks until the cache's size is known.
    pub fn wait_ready(&self) {
        let measured = self.inner.measured.lock().unwrap_or_else(|p| p.into_inner());
        drop(
            self.inner
                .measured_changed
                .wait_while(measured, |measured| !*measured)
                .unwrap_or_else(|p| p.into_inner()),
        );
    }

    /// Whether the size is known yet, without waiting for it.
    pub fn is_measured(&self) -> bool {
        *self.inner.measured.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Whether background rendering may store thumbnails: only once the size is known and below the cap.
    pub fn has_room(&self) -> bool {
        self.is_measured() && self.inner.bytes.load(Ordering::Relaxed) < self.inner.max_bytes.load(Ordering::Relaxed)
    }

    /// Total size of the cached thumbnails in bytes.
    pub fn usage(&self) -> u64 {
        self.wait_ready();
        self.inner.bytes.load(Ordering::Relaxed)
    }

    /// A lower cap evicts right away (REQ-005-11, REQ-010-05).
    pub fn set_max_bytes(&self, max_bytes: u64) {
        self.inner.max_bytes.store(max_bytes, Ordering::Relaxed);
        if self.is_measured() && self.inner.bytes.load(Ordering::Relaxed) > max_bytes {
            self.evict_in_background();
        }
    }

    /// Writes atomically (temp file, then rename), so a partial file is never read (REQ-005-13).
    pub fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = temp_path(path);
        std::fs::write(&tmp, data)?;
        // A thumbnail rendered again replaces its file: only the difference counts.
        let replaced = std::fs::metadata(path).map_or(0, |meta| meta.len());
        if let Err(error) = rename_replacing(&tmp, path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(error);
        }
        let added = (data.len() as u64).saturating_sub(replaced);
        let removed = replaced.saturating_sub(data.len() as u64);
        self.inner.written.fetch_add(added, Ordering::Relaxed);
        self.inner.bytes.fetch_add(added, Ordering::Relaxed);
        let bytes = self
            .inner
            .bytes
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |bytes| {
                Some(bytes.saturating_sub(removed))
            })
            .unwrap_or_default()
            .saturating_sub(removed);
        if bytes > self.inner.max_bytes.load(Ordering::Relaxed) {
            self.evict_in_background();
        }
        Ok(())
    }

    /// A cached thumbnail, or `None` if it isn't cached. Counts as a view for eviction, recorded at most once an hour
    /// (REQ-005-11).
    pub fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        let result = std::fs::metadata(path).and_then(|meta| Ok((meta.modified()?, std::fs::read(path)?)));
        let (mtime, data) = match result {
            Ok(found) => found,
            // Not rendered yet, or evicted between the stat and the read.
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let now = SystemTime::now();
        if now.duration_since(mtime).is_ok_and(|age| age > TOUCH_INTERVAL)
            && let Err(error) = std::fs::File::options()
                .write(true)
                .open(path)
                .and_then(|file| file.set_modified(now))
        {
            tracing::warn!("can't touch thumbnail {}: {error}", path.display());
        }
        Ok(Some(data))
    }

    /// Deletes least recently viewed thumbnails until the cache is at 90 % of its cap.
    pub fn evict(&self) {
        let _evicting = self.inner.evicting.lock().unwrap_or_else(|p| p.into_inner());
        self.evict_to(|| (self.inner.max_bytes.load(Ordering::Relaxed) as f64 * EVICT_TO) as u64);
    }

    fn evict_in_background(&self) {
        if self.inner.eviction_queued.swap(true, Ordering::Relaxed) {
            return;
        }
        let cache = self.clone();
        let started = std::thread::Builder::new()
            .name("thumb eviction".into())
            .spawn(move || {
                cache.evict();
                // Only now: writes during the eviction don't start another walk of the whole cache.
                cache.inner.eviction_queued.store(false, Ordering::Relaxed);
                if cache.inner.bytes.load(Ordering::Relaxed) > cache.inner.max_bytes.load(Ordering::Relaxed) {
                    cache.evict_in_background();
                }
            });
        if let Err(error) = started {
            self.inner.eviction_queued.store(false, Ordering::Relaxed);
            tracing::error!("can't start the thumbnail eviction: {error}");
        }
    }

    /// Deletes all thumbnails; files being read right now stay (REQ-010-06).
    pub fn clear(&self) {
        let _evicting = self.inner.evicting.lock().unwrap_or_else(|p| p.into_inner());
        self.evict_to(|| 0);
    }

    fn evict_to(&self, target: impl Fn() -> u64) {
        self.wait_ready();
        let written_before = self.inner.written.load(Ordering::Relaxed);
        let mut files = list(&self.inner.dir, false);
        let mut bytes: u64 = files.iter().map(|file| file.size).sum();
        files.sort_by_key(|file| file.mtime);
        for file in files {
            if bytes <= target() {
                break;
            }
            match std::fs::remove_file(&file.path) {
                Ok(()) => bytes -= file.size,
                Err(error) if error.kind() == io::ErrorKind::NotFound => bytes -= file.size,
                // Being read right now (sharing violation on Windows); it goes next time.
                Err(_) => {}
            }
        }
        self.store_walked(bytes, written_before);
    }

    /// Sets the size from a folder walk that started when `written_before` bytes had been written, adding what was
    /// written since. Writes the walk saw anyway count twice until the next walk; too high is the safe side.
    fn store_walked(&self, walked: u64, written_before: u64) {
        let written_since = self.inner.written.load(Ordering::Relaxed) - written_before;
        self.inner.bytes.store(walked + written_since, Ordering::Relaxed);
    }

    fn measure(&self) {
        let written_before = self.inner.written.load(Ordering::Relaxed);
        let files = list(&self.inner.dir, true);
        self.store_walked(files.iter().map(|file| file.size).sum(), written_before);
        *self.inner.measured.lock().unwrap_or_else(|p| p.into_inner()) = true;
        self.inner.measured_changed.notify_all();
    }
}

/// Windows refuses to replace a file someone has open (a reader, or Defender scanning the file just written), so a
/// refused rename is retried for a moment (≈ 1.4 s; under load, Defender holds new files that long).
fn rename_replacing(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied && attempt < 10 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(25 * attempt));
            }
            result => return result,
        }
    }
}

/// `<path>.<process id>-<n>.tmp`: unique among concurrent writes, and tells this process's writes from leftovers.
fn temp_path(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let unique = format!("{}{}", own_temp_prefix(), NEXT.fetch_add(1, Ordering::Relaxed));
    let mut name = path.as_os_str().to_owned();
    name.push(format!("{unique}{TMP_SUFFIX}"));
    PathBuf::from(name)
}

fn own_temp_prefix() -> String {
    format!(".{}-", std::process::id())
}

/// All cached thumbnails. At startup, temp files of other processes can only be leftovers of a crash and are
/// removed; this process's own are writes in progress (REQ-005-13).
fn list(dir: &Path, remove_temp: bool) -> Vec<CachedFile> {
    let own_temp = own_temp_prefix();
    let mut files = Vec::new();
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else { continue };
            if file_type.is_dir() {
                folders.push(path);
            } else if file_type.is_file() {
                let name = path.as_os_str().to_string_lossy();
                if name.ends_with(TMP_SUFFIX) {
                    if remove_temp && !name.contains(&own_temp) {
                        let _ = std::fs::remove_file(&path);
                    }
                    continue;
                }
                // Deleted concurrently otherwise.
                if let Ok(meta) = entry.metadata()
                    && let Ok(mtime) = meta.modified()
                {
                    files.push(CachedFile {
                        path,
                        size: meta.len(),
                        mtime,
                    });
                }
            }
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> ThumbKey {
        ThumbKey {
            root_id: 1,
            rel_path: "trip/a.jpg".into(),
            size: 100,
            mtime: 1_700_000_000_000,
        }
    }

    fn seed(dir: &Path, name: &str, bytes: usize, mtime: SystemTime) {
        let path = dir.join(&name[0..2]).join(&name[2..4]).join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![0; bytes]).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = list(dir, false)
            .into_iter()
            .map(|file| file.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn year(year: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs((year - 1970) * 365 * 24 * 3600)
    }

    #[test]
    fn path_is_stable_and_fanned_out_by_hash_prefix() {
        let dir = Path::new("cache");
        let path = thumb_path(dir, &key(), 400);
        assert_eq!(path, thumb_path(dir, &key().clone(), 400));
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(path, dir.join(&name[0..2]).join(&name[2..4]).join(&name));
        assert!(name.ends_with(".webp") && name.len() == 40 + 5);
    }

    /// REQ-005-15: the same path as in existing caches (hashes computed with Node's `crypto` over `JSON.stringify`),
    /// also for paths with quotes, backslashes, control characters and non-ASCII text.
    #[test]
    fn req_005_15_matches_existing_cache_paths() {
        let dir = Path::new("cache");
        assert_eq!(
            thumb_path(dir, &key(), 400),
            dir.join("94/88/948889ff09a9ec246bfa4e934be3bdd5cd38c2be.webp")
        );
        let odd = ThumbKey {
            root_id: 12,
            rel_path: "Ürlaub/\"q\"\\b\u{1}\n\u{7f}😀.jpg".into(),
            size: 5,
            mtime: 1,
        };
        assert_eq!(
            thumb_path(dir, &odd, 800),
            dir.join("f4/51/f45119f0def369c4865e4776e2674a8b58484d78.webp")
        );
    }

    #[test]
    fn req_005_10_path_changes_with_every_part_of_the_key() {
        let dir = Path::new("cache");
        let variants = [
            thumb_path(dir, &key(), 400),
            thumb_path(dir, &key(), 800),
            thumb_path(dir, &ThumbKey { root_id: 2, ..key() }, 400),
            thumb_path(
                dir,
                &ThumbKey {
                    rel_path: "trip/b.jpg".into(),
                    ..key()
                },
                400,
            ),
            thumb_path(dir, &ThumbKey { size: 101, ..key() }, 400),
            thumb_path(
                dir,
                &ThumbKey {
                    mtime: key().mtime + 1,
                    ..key()
                },
                400,
            ),
        ];
        let unique: std::collections::HashSet<_> = variants.iter().collect();
        assert_eq!(unique.len(), variants.len());
    }

    #[test]
    fn req_005_13_measures_existing_thumbnails_and_drops_leftover_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path(), "aaaa.webp", 60, SystemTime::now());
        seed(dir.path(), "bbbb.webp.123.tmp", 10, SystemTime::now());

        let full = ThumbCache::open(dir.path().into(), 50);
        full.wait_ready();
        assert!(!full.has_room());
        assert_eq!(files(dir.path()), ["aaaa.webp"]);
        assert!(!dir.path().join("bb/bb/bbbb.webp.123.tmp").exists());

        let roomy = ThumbCache::open(dir.path().into(), 100);
        roomy.wait_ready();
        assert!(roomy.has_room());
    }

    /// REQ-005-13: the startup walk removes leftovers of a crash, not the temp file of a write in progress.
    #[test]
    fn req_005_13_keeps_temp_files_of_writes_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        let path = thumb_path(dir.path(), &key(), 400);
        let in_progress = temp_path(&path);
        std::fs::create_dir_all(in_progress.parent().unwrap()).unwrap();
        std::fs::write(&in_progress, b"webp").unwrap();

        let cache = ThumbCache::open(dir.path().into(), 100);
        cache.wait_ready();

        assert!(in_progress.exists());
    }

    /// REQ-005-13: a thumbnail held open for longer than a moment (Defender under load holds new files for seconds) is
    /// still replaced.
    #[cfg(windows)]
    #[test]
    fn req_005_13_replaces_a_thumbnail_held_open_for_seconds() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let cache = ThumbCache::open(dir.path().into(), 100);
        let path = cache.path(&key(), 400);
        cache.write(&path, b"webp").unwrap();
        let held = std::fs::File::options().read(true).share_mode(1).open(&path).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            drop(held);
        });

        cache.write(&path, b"newer").unwrap();

        release.join().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"newer");
        assert_eq!(files(dir.path()).len(), 1);
    }

    /// A thumbnail held open for a moment (as Defender does with new files) is still replaced.
    #[cfg(windows)]
    #[test]
    fn replaces_a_thumbnail_that_is_open_for_a_moment() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let cache = ThumbCache::open(dir.path().into(), 100);
        let path = cache.path(&key(), 400);
        cache.write(&path, b"webp").unwrap();
        let held = std::fs::File::options().read(true).share_mode(1).open(&path).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            drop(held);
        });

        cache.write(&path, b"newer").unwrap();

        release.join().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"newer");
    }

    #[test]
    fn req_005_13_writes_leave_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ThumbCache::open(dir.path().into(), 100);
        let path = cache.path(&key(), 400);
        cache.write(&path, b"webp").unwrap();
        cache.write(&path, b"newer").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"newer");
        assert_eq!(files(dir.path()).len(), 1);
    }

    /// REQ-005-11: a thumbnail rendered again counts once, and writes during the startup walk aren't lost.
    #[test]
    fn req_005_11_counts_replaced_and_concurrent_writes_once() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ThumbCache::open(dir.path().into(), 1000);
        let path = cache.path(&key(), 400);
        // A walk still running could find the first write and count it twice (on purpose, see `store_walked`).
        cache.wait_ready();
        cache.write(&path, &[0; 40]).unwrap();
        cache.write(&path, &[0; 50]).unwrap();
        cache.write(&path, &[0; 30]).unwrap();
        assert_eq!(cache.usage(), 30);

        let other = cache.path(&ThumbKey { size: 7, ..key() }, 400);
        let written_before = cache.inner.written.load(Ordering::Relaxed);
        cache.write(&other, &[0; 20]).unwrap();
        // As if a walk that started before the write had found only the first file.
        cache.store_walked(30, written_before);
        assert_eq!(cache.usage(), 50);
    }

    #[test]
    fn req_005_11_evicts_least_recently_viewed_thumbnails_once_over_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path(), "aaaa.webp", 40, year(2020));
        seed(dir.path(), "bbbb.webp", 40, year(2021));
        let cache = ThumbCache::open(dir.path().into(), 100);
        cache.wait_ready();

        let path = cache.path(&key(), 400);
        cache.write(&path, &[0; 40]).unwrap();
        cache.evict();

        // 120 bytes > 100 → down to ≤ 90: the oldest file goes.
        let mut expected = vec![
            path.file_name().unwrap().to_string_lossy().into_owned(),
            "bbbb.webp".into(),
        ];
        expected.sort();
        assert_eq!(files(dir.path()), expected);
        assert_eq!(cache.usage(), 80);
        assert!(cache.has_room());
    }

    /// REQ-005-11, REQ-010-05: a lowered cap evicts right away.
    #[test]
    fn evicts_down_to_a_lowered_cap() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path(), "aaaa.webp", 40, year(2020));
        seed(dir.path(), "bbbb.webp", 40, year(2021));
        let cache = ThumbCache::open(dir.path().into(), 100);
        cache.wait_ready();

        cache.set_max_bytes(50);

        // Evicts on its own thread.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while files(dir.path()).len() > 1 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(files(dir.path()), ["bbbb.webp"]);
        assert_eq!(cache.usage(), 40);
    }

    #[test]
    fn req_005_11_reading_counts_as_a_view_at_most_once_an_hour() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path(), "aaaa.webp", 40, year(2020));
        let recent = SystemTime::now() - Duration::from_secs(60);
        seed(dir.path(), "bbbb.webp", 40, recent);
        let cache = ThumbCache::open(dir.path().into(), 100);
        let mtime = |name: &str| {
            std::fs::metadata(dir.path().join(&name[0..2]).join(&name[2..4]).join(name))
                .unwrap()
                .modified()
                .unwrap()
        };

        assert_eq!(
            cache.read(&dir.path().join("aa/aa/aaaa.webp")).unwrap(),
            Some(vec![0; 40])
        );
        assert_eq!(
            cache.read(&dir.path().join("bb/bb/bbbb.webp")).unwrap(),
            Some(vec![0; 40])
        );
        assert_eq!(cache.read(&dir.path().join("cc/cc/cccc.webp")).unwrap(), None);

        assert!(mtime("aaaa.webp") > recent, "an old thumbnail is touched");
        assert_eq!(mtime("bbbb.webp"), recent, "a recently viewed one isn't");
    }

    #[test]
    fn req_010_06_clears_all_thumbnails() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path(), "aaaa.webp", 40, SystemTime::now());
        seed(dir.path(), "bbbb.webp", 40, SystemTime::now());
        let cache = ThumbCache::open(dir.path().into(), 100);
        assert_eq!(cache.usage(), 80);

        cache.clear();

        assert!(files(dir.path()).is_empty());
        assert_eq!(cache.usage(), 0);
    }
}
