//! Depth-first listing of a library, one folder at a time (the reference's `walk`).

use crate::retry::{mtime_ms, with_retry};
use domain::media::{MediaKind, is_ignored_dir, is_sidecar, media_kind};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirListing {
    /// Relative to the library, `/`-separated; `""` for the library itself.
    pub rel_dir: String,
    pub abs_dir: PathBuf,
    /// Folder mtime, taken before listing it.
    pub mtime: i64,
    pub files: Vec<(String, MediaKind)>,
    /// Lower-cased sidecar file name → actual name.
    pub sidecars: HashMap<String, String>,
    /// Relative paths of the subfolders, housekeeping folders excluded.
    pub subdirs: Vec<String>,
}

#[derive(Debug)]
pub enum WalkItem {
    Listing(DirListing),
    /// A folder that couldn't be (fully) listed; it isn't yielded as a listing.
    Error(String, io::Error),
}

/// Called with a folder's mtime before listing it; returning folders skips the listing and continues with those
/// (e.g. the known subfolders of an unchanged folder).
pub type Skip<'a> = &'a dyn Fn(&str, i64) -> Option<Vec<String>>;
/// The subfolders of a listed folder to descend into.
pub type Descend<'a> = &'a dyn Fn(&DirListing) -> Vec<String>;

/// Yields one item per folder, each folder at most once. Skips symlinks, junctions and housekeeping folders.
pub struct Walk<'a> {
    root: PathBuf,
    pending: Vec<String>,
    visited: HashSet<String>,
    cancel: &'a AtomicBool,
    skip: Option<Skip<'a>>,
    descend: Option<Descend<'a>>,
}

impl<'a> Walk<'a> {
    /// Starts at `from` (the library itself: `[""]`).
    pub fn new(root: &Path, from: Vec<String>, cancel: &'a AtomicBool) -> Self {
        let mut pending = from;
        pending.reverse();
        Self {
            root: root.to_path_buf(),
            pending,
            visited: HashSet::new(),
            cancel,
            skip: None,
            descend: None,
        }
    }

    pub fn skip(mut self, skip: Skip<'a>) -> Self {
        self.skip = Some(skip);
        self
    }

    pub fn descend(mut self, descend: Descend<'a>) -> Self {
        self.descend = Some(descend);
        self
    }

    fn abs(&self, rel_dir: &str) -> PathBuf {
        if rel_dir.is_empty() {
            self.root.clone()
        } else {
            rel_dir.split('/').fold(self.root.clone(), |path, part| path.join(part))
        }
    }

    fn list(&mut self, rel_dir: String) -> Option<WalkItem> {
        let abs_dir = self.abs(&rel_dir);
        let mtime = match with_retry(self.cancel, || std::fs::metadata(&abs_dir)) {
            Ok(metadata) => mtime_ms(&metadata),
            Err(error) => return Some(WalkItem::Error(rel_dir, error)),
        };
        if let Some(next) = self.skip.and_then(|skip| skip(&rel_dir, mtime)) {
            self.pending.extend(next.into_iter().rev());
            return None;
        }
        let mut listing = DirListing {
            rel_dir,
            abs_dir,
            mtime,
            files: Vec::new(),
            sidecars: HashMap::new(),
            subdirs: Vec::new(),
        };
        let entries = match with_retry(self.cancel, || std::fs::read_dir(&listing.abs_dir)) {
            Ok(entries) => entries,
            Err(error) => return Some(WalkItem::Error(listing.rel_dir, error)),
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => return Some(WalkItem::Error(listing.rel_dir, error)),
            };
            // A file whose type can't be read makes the listing incomplete, so its folder counts as unlisted (its
            // files stay indexed) instead of the file counting as vanished.
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => return Some(WalkItem::Error(listing.rel_dir, error)),
            };
            let Some(name) = entry.file_name().to_str().map(String::from) else {
                tracing::warn!(
                    "skipped a file name that isn't valid Unicode in {}",
                    listing.abs_dir.display()
                );
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if !is_ignored_dir(&name) {
                    let rel = if listing.rel_dir.is_empty() {
                        name
                    } else {
                        format!("{}/{name}", listing.rel_dir)
                    };
                    listing.subdirs.push(rel);
                }
            } else if file_type.is_file() {
                if let Some(kind) = media_kind(&name) {
                    listing.files.push((name, kind));
                } else if is_sidecar(&name) {
                    listing.sidecars.insert(name.to_lowercase(), name);
                }
            }
        }
        let next = match self.descend {
            Some(descend) => descend(&listing),
            None => listing.subdirs.clone(),
        };
        self.pending.extend(next.into_iter().rev());
        Some(WalkItem::Listing(listing))
    }
}

impl Iterator for Walk<'_> {
    type Item = WalkItem;

    fn next(&mut self) -> Option<WalkItem> {
        while let Some(rel_dir) = self.pending.pop() {
            if self.cancel.load(Ordering::Relaxed) {
                return None;
            }
            if !self.visited.insert(rel_dir.clone()) {
                continue;
            }
            if let Some(item) = self.list(rel_dir) {
                return Some(item);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(root: &Path, rel: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "x").unwrap();
    }

    fn listings(walk: Walk) -> Vec<DirListing> {
        walk.map(|item| match item {
            WalkItem::Listing(listing) => listing,
            WalkItem::Error(dir, error) => panic!("{dir}: {error}"),
        })
        .collect()
    }

    #[test]
    fn req_002_02_walks_depth_first_and_skips_housekeeping() {
        let dir = tempfile::tempdir().unwrap();
        for rel in [
            "a.jpg",
            "b/c.MP4",
            "b/notes.txt",
            "b/._c.MP4",
            "b/c.MP4.xmp",
            "@eaDir/x.jpg",
            ".hidden/y.jpg",
        ] {
            touch(dir.path(), rel);
        }
        let cancel = AtomicBool::new(false);

        let found = listings(Walk::new(dir.path(), vec![String::new()], &cancel));

        assert_eq!(found.iter().map(|l| l.rel_dir.as_str()).collect::<Vec<_>>(), ["", "b"]);
        assert_eq!(found[0].files, [("a.jpg".to_string(), MediaKind::Image)]);
        assert_eq!(found[0].subdirs, ["b"]);
        assert_eq!(found[1].files, [("c.MP4".to_string(), MediaKind::Video)]);
        assert_eq!(found[1].sidecars["c.mp4.xmp"], "c.MP4.xmp");
    }

    #[test]
    fn skip_continues_with_the_given_folders_and_descend_limits_subfolders() {
        let dir = tempfile::tempdir().unwrap();
        for rel in ["a/x.jpg", "a/sub/y.jpg", "b/z.jpg"] {
            touch(dir.path(), rel);
        }
        let cancel = AtomicBool::new(false);
        let skip: Skip = &|rel, _| (rel.is_empty()).then(|| vec!["a".to_string()]);
        let descend: Descend = &|_| Vec::new();

        let found = listings(
            Walk::new(dir.path(), vec![String::new()], &cancel)
                .skip(skip)
                .descend(descend),
        );

        assert_eq!(found.iter().map(|l| l.rel_dir.as_str()).collect::<Vec<_>>(), ["a"]);
    }

    #[test]
    fn reports_folders_that_cant_be_listed() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);

        let items: Vec<_> = Walk::new(dir.path(), vec!["missing".into()], &cancel).collect();

        assert!(
            matches!(&items[..], [WalkItem::Error(dir, e)] if dir == "missing" && e.kind() == io::ErrorKind::NotFound)
        );
    }

    #[test]
    fn req_002_02_skips_symbolic_links() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "real/a.jpg");
        let link = dir.path().join("link");
        // Junctions need no privileges on Windows; symlinks may not be allowed.
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(dir.path().join("real"))
            .output()
            .is_ok_and(|out| out.status.success());
        if !made {
            eprintln!("skipped: couldn't create a junction");
            return;
        }
        let cancel = AtomicBool::new(false);

        let found = listings(Walk::new(dir.path(), vec![String::new()], &cancel));

        assert_eq!(found[0].subdirs, ["real"]);
    }
}
