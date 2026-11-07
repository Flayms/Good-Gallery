//! Renaming files on disk (spec 035): each file with its sidecars and the backup of its unedited original, in the
//! order `domain::rename::steps` gives, and a journal that outlives a crash and gives _Undo_ (REQ-035-06 … -09).

use domain::edit::backup_rel_path;
use domain::rename::{Step, steps};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

/// The renames in one library, old and new paths relative to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub root_id: i64,
    pub library: PathBuf,
    pub renames: Vec<(String, String)>,
}

/// Where a renamed file ended up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowResult {
    pub old: String,
    pub new: String,
    /// The new path, or the old one when it couldn't be renamed (or, rarely, its temporary name).
    pub at: String,
    pub error: Option<String>,
}

fn on_disk(library: &Path, rel_path: &str) -> PathBuf {
    library.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR))
}

/// The sidecars `IMG_1.jpg.xmp` and `IMG_1.xmp` of a file, in either case of the extension, as `(from, to)` pairs
/// for a rename, and whether the second is shared with another file of that name (then it is copied, not moved).
pub(crate) fn sidecars(library: &Path, from: &str, to: &str) -> Vec<(String, String, bool)> {
    let stem = |rel: &str| -> String {
        match rel.rfind('.') {
            Some(dot) if dot > rel.rfind('/').map_or(0, |slash| slash + 1) => rel[..dot].to_string(),
            _ => rel.to_string(),
        }
    };
    let mut found = Vec::new();
    for ext in [".xmp", ".XMP"] {
        found.push((format!("{from}{ext}"), format!("{to}{ext}"), false));
        found.push((format!("{}{ext}", stem(from)), format!("{}{ext}", stem(to)), true));
    }
    found.retain(|(sidecar, _, _)| on_disk(library, sidecar).is_file());
    let shared = |stem_sidecar: &str| {
        let path = on_disk(library, stem_sidecar);
        let (Some(folder), Some(own_stem)) = (path.parent(), Path::new(stem_sidecar).file_stem()) else {
            return false;
        };
        let own = on_disk(library, from);
        std::fs::read_dir(folder).into_iter().flatten().flatten().any(|entry| {
            let other = entry.path();
            other != own
                && other.file_stem() == Some(own_stem)
                && !other.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("xmp"))
        })
    };
    found
        .into_iter()
        .map(|(sidecar, target, by_stem)| {
            let copy = by_stem && shared(&sidecar);
            (sidecar, target, copy)
        })
        .collect()
}

/// Renames one file with its sidecars and its backup (REQ-035-06, REQ-035-07). Never replaces a file: a name that is
/// taken fails.
pub fn move_file(library: &Path, from: &str, to: &str) -> io::Result<()> {
    // Found before the file moves: a shared sidecar is found by the other files next to it.
    let sidecars = sidecars(library, from, to);
    rename_new(&on_disk(library, from), &on_disk(library, to))?;
    for (sidecar, target, copy) in sidecars {
        let (sidecar, target) = (on_disk(library, &sidecar), on_disk(library, &target));
        let moved = if copy {
            if target.exists() {
                Err(io::ErrorKind::AlreadyExists.into())
            } else {
                std::fs::copy(&sidecar, &target).map(drop)
            }
        } else {
            rename_new(&sidecar, &target)
        };
        if let Err(error) = moved {
            tracing::warn!("can't rename the sidecar {}: {error}", sidecar.display());
        }
    }
    let backup = on_disk(library, &backup_rel_path(from));
    if backup.is_file() {
        let target = on_disk(library, &backup_rel_path(to));
        let moved = target
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| rename_new(&backup, &target));
        if let Err(error) = moved {
            tracing::warn!("can't rename the backup {}: {error}", backup.display());
        }
    }
    Ok(())
}

/// `std::fs::rename` that fails rather than replace a file (REQ-024-06). The same name in another case is the file
/// itself on most file systems, which `domain::rename::steps` never asks for.
fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(to).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} exists", to.display()),
        ));
    }
    std::fs::rename(from, to)
}

/// Runs the renames of `batch`, skipping steps in `done` and recording each finished one with `record`. A rename that
/// fails takes its file back to its old name if it can.
pub fn execute(batch: &Batch, done: &HashSet<usize>, mut record: impl FnMut(usize)) -> Vec<RowResult> {
    let library = &batch.library;
    let mut results: Vec<RowResult> = batch
        .renames
        .iter()
        .map(|(old, new)| RowResult {
            old: old.clone(),
            new: new.clone(),
            at: old.clone(),
            error: None,
        })
        .collect();
    let all: Vec<Step> = steps(&batch.renames);
    for (index, step) in all.iter().enumerate() {
        let result = &mut results[step.row];
        if result.error.is_some() {
            continue;
        }
        // Done before an interruption: recorded, or the file is where the step takes it.
        let landed = !on_disk(library, &step.from).exists() && on_disk(library, &step.to).exists();
        if done.contains(&index) || landed {
            result.at = step.to.clone();
            continue;
        }
        match move_file(library, &step.from, &step.to) {
            Ok(()) => {
                result.at = step.to.clone();
                record(index);
            }
            Err(error) => result.error = Some(error.to_string()),
        }
    }
    // A file stopped at its temporary name goes back to its old one, if that is still free.
    for result in &mut results {
        if result.error.is_some() && result.at != result.old && move_file(library, &result.at, &result.old).is_ok() {
            result.at = result.old.clone();
        }
    }
    results
}

/// What a journal says (REQ-035-08, REQ-035-09).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JournalState {
    pub batches: Vec<Batch>,
    /// The index has the new names.
    pub indexed: bool,
    /// Finished steps, as `(batch, step)`.
    pub done: HashSet<(usize, usize)>,
    pub finished: bool,
}

/// The file recording a rename while it runs, kept until the next one: a line per event, so a crash loses at most
/// the last.
pub struct Journal {
    file: std::fs::File,
}

impl Journal {
    /// Starts a journal at `path` for `batches`, replacing the last one.
    pub fn start(path: &Path, batches: &[Batch]) -> io::Result<Self> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let batches: Vec<Value> = batches
            .iter()
            .map(|batch| {
                json!({
                    "root_id": batch.root_id,
                    "library": batch.library.to_string_lossy(),
                    "renames": batch.renames,
                })
            })
            .collect();
        let mut journal = Self {
            file: std::fs::File::create(path)?,
        };
        journal.line(&json!({ "batches": batches }), true)?;
        Ok(journal)
    }

    fn line(&mut self, value: &Value, sync: bool) -> io::Result<()> {
        writeln!(self.file, "{value}")?;
        if sync {
            self.file.sync_data()?;
        }
        Ok(())
    }

    /// The index has the new names.
    pub fn indexed(&mut self) -> io::Result<()> {
        self.line(&json!({ "indexed": true }), true)
    }

    pub fn done(&mut self, batch: usize, step: usize) -> io::Result<()> {
        self.line(&json!({ "done": [batch, step] }), false)
    }

    pub fn finish(&mut self) -> io::Result<()> {
        self.line(&json!({ "finished": true }), true)
    }

    /// Continues the journal at `path`, to finish an interrupted rename.
    pub fn reopen(path: &Path) -> io::Result<Self> {
        let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
        // A line cut off by a crash ends here, so the next one starts on its own line.
        file.write_all(b"\n")?;
        Ok(Self { file })
    }

    /// What the journal at `path` says; `None` without one. Lines that can't be read (a crash's last) are skipped.
    pub fn read(path: &Path) -> io::Result<Option<JournalState>> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut state = JournalState::default();
        let mut started = false;
        for line in text.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()) {
            if let Some(batches) = line.get("batches").and_then(Value::as_array) {
                started = true;
                state.batches = batches.iter().filter_map(read_batch).collect();
            } else if line.get("indexed").is_some() {
                state.indexed = true;
            } else if line.get("finished").is_some() {
                state.finished = true;
            } else if let Some([batch, step]) = line.get("done").and_then(Value::as_array).map(Vec::as_slice)
                && let (Some(batch), Some(step)) = (batch.as_u64(), step.as_u64())
            {
                state.done.insert((batch as usize, step as usize));
            }
        }
        Ok(started.then_some(state))
    }
}

fn read_batch(value: &Value) -> Option<Batch> {
    let renames = value
        .get("renames")?
        .as_array()?
        .iter()
        .filter_map(|pair| Some((pair.get(0)?.as_str()?.to_string(), pair.get(1)?.as_str()?.to_string())))
        .collect();
    Some(Batch {
        root_id: value.get("root_id")?.as_i64()?,
        library: PathBuf::from(value.get("library")?.as_str()?),
        renames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Folder(tempfile::TempDir);

    impl Folder {
        fn new(files: &[&str]) -> Self {
            let dir = tempfile::tempdir().unwrap();
            for file in files {
                let path = on_disk(dir.path(), file);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, file.as_bytes()).unwrap();
            }
            Self(dir)
        }

        fn root(&self) -> &Path {
            self.0.path()
        }

        /// Which of `rel` exists, and what it holds (the name it was created with).
        fn holds(&self, rel: &str) -> Option<String> {
            std::fs::read_to_string(on_disk(self.root(), rel)).ok()
        }

        fn batch(&self, renames: &[(&str, &str)]) -> Batch {
            Batch {
                root_id: 1,
                library: self.root().to_path_buf(),
                renames: renames.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            }
        }
    }

    #[test]
    fn req_035_06_req_035_07_moves_sidecars_and_the_backup_along() {
        let folder = Folder::new(&[
            "d/IMG_1.jpg",
            "d/IMG_1.jpg.xmp",
            "d/IMG_2.jpg",
            "d/IMG_2.xmp",
            "d/IMG_3.jpg",
            "d/IMG_3.mov",
            "d/IMG_3.xmp",
            ".good-gallery/originals/d/IMG_1.jpg",
        ]);
        move_file(folder.root(), "d/IMG_1.jpg", "d/a.jpg").unwrap();
        assert_eq!(folder.holds("d/a.jpg").as_deref(), Some("d/IMG_1.jpg"));
        assert_eq!(folder.holds("d/a.jpg.xmp").as_deref(), Some("d/IMG_1.jpg.xmp"));
        assert_eq!(
            folder.holds(".good-gallery/originals/d/a.jpg").as_deref(),
            Some(".good-gallery/originals/d/IMG_1.jpg")
        );
        assert_eq!(folder.holds("d/IMG_1.jpg"), None);
        move_file(folder.root(), "d/IMG_2.jpg", "d/b.jpg").unwrap();
        assert_eq!(folder.holds("d/b.xmp").as_deref(), Some("d/IMG_2.xmp"));
        // IMG_3.xmp belongs to the video too: the photo gets a copy.
        move_file(folder.root(), "d/IMG_3.jpg", "d/c.jpg").unwrap();
        assert_eq!(folder.holds("d/c.xmp").as_deref(), Some("d/IMG_3.xmp"));
        assert_eq!(folder.holds("d/IMG_3.xmp").as_deref(), Some("d/IMG_3.xmp"));
        // A name that is taken isn't replaced.
        assert!(move_file(folder.root(), "d/b.jpg", "d/c.jpg").is_err());
        assert_eq!(folder.holds("d/c.jpg").as_deref(), Some("d/IMG_3.jpg"));
    }

    #[test]
    fn req_035_06_swaps_and_changes_case_and_a_failure_costs_only_its_file() {
        let folder = Folder::new(&["a.jpg", "b.jpg", "x.jpg", "y.jpg"]);
        let batch = folder.batch(&[
            ("a.jpg", "b.jpg"),
            ("b.jpg", "a.jpg"),
            ("x.jpg", "X.jpg"),
            ("gone.jpg", "z.jpg"),
        ]);
        let mut recorded = Vec::new();
        let results = execute(&batch, &HashSet::new(), |step| recorded.push(step));
        assert_eq!(folder.holds("b.jpg").as_deref(), Some("a.jpg"));
        assert_eq!(folder.holds("a.jpg").as_deref(), Some("b.jpg"));
        let names: Vec<String> = std::fs::read_dir(folder.root())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.contains(&"X.jpg".to_string()) && !names.contains(&"x.jpg".to_string()),
            "{names:?}"
        );
        assert!(names.iter().all(|name| !name.ends_with(".gg-rename")), "{names:?}");
        assert_eq!(results[3].at, "gone.jpg");
        assert!(results[3].error.is_some());
        assert!(results[..3].iter().all(|r| r.error.is_none() && r.at == r.new));
        assert_eq!(recorded.len(), 6, "every finished step is recorded");

        // Steps done before an interruption aren't done again.
        let folder = Folder::new(&["p.jpg", "q.jpg"]);
        let batch = folder.batch(&[("p.jpg", "r.jpg"), ("q.jpg", "s.jpg")]);
        std::fs::rename(on_disk(folder.root(), "p.jpg"), on_disk(folder.root(), "r.jpg")).unwrap();
        let results = execute(&batch, &HashSet::from([0]), |_| {});
        assert!(results.iter().all(|r| r.error.is_none()), "{results:?}");
        assert_eq!(folder.holds("s.jpg").as_deref(), Some("q.jpg"));
    }

    #[test]
    fn req_035_08_req_035_09_the_journal_records_what_happened() {
        let folder = Folder::new(&[]);
        let path = folder.root().join("rename-journal.json");
        assert_eq!(Journal::read(&path).unwrap(), None);
        let batches = vec![folder.batch(&[("a.jpg", "b.jpg")])];
        let mut journal = Journal::start(&path, &batches).unwrap();
        let state = Journal::read(&path).unwrap().unwrap();
        assert_eq!(
            (state.batches.clone(), state.indexed, state.finished),
            (batches.clone(), false, false)
        );
        journal.indexed().unwrap();
        journal.done(0, 0).unwrap();
        drop(journal);
        // Continued after a restart.
        let mut journal = Journal::reopen(&path).unwrap();
        journal.finish().unwrap();
        let state = Journal::read(&path).unwrap().unwrap();
        assert!(state.indexed && state.finished);
        assert_eq!(state.done, HashSet::from([(0, 0)]));
        // A cut-off last line is ignored.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{\"do")
            .unwrap();
        assert!(Journal::read(&path).unwrap().unwrap().finished);
        // A new rename replaces it.
        Journal::start(&path, &batches).unwrap();
        assert!(!Journal::read(&path).unwrap().unwrap().indexed);
    }
}
