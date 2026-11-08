//! The LUTs the user imported (spec 043): kept byte for byte as `<id>.cube` in the data folder's `luts`, with
//! `<id>.name` for one without a title, listed by name; their tables are read once and shared with the preview and
//! saving.

use domain::lut::{Lut, lut_id};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// An imported LUT as the editor lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LutEntry {
    pub id: String,
    pub name: String,
}

/// The imported LUTs; shared between the UI and the threads that import and save.
#[derive(Debug, Default)]
pub struct Luts {
    /// Where they are kept; `None` keeps them in memory only (tests, the browser version).
    dir: Option<PathBuf>,
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    /// By name.
    entries: Vec<LutEntry>,
    tables: HashMap<String, Arc<Lut>>,
}

impl Luts {
    /// The LUTs kept in `dir`; files that can't be read are left out.
    pub fn open(dir: PathBuf) -> Self {
        let luts = Self {
            dir: Some(dir.clone()),
            inner: Mutex::default(),
        };
        for file in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = file.path();
            if path.extension().is_none_or(|extension| extension != "cube") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|stem| stem.to_str()).map(str::to_owned) else {
                continue;
            };
            let read = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| Lut::parse(&text).map_err(|e| e.to_string()));
            match read {
                Ok(lut) => {
                    let name = lut
                        .title
                        .clone()
                        .or_else(|| std::fs::read_to_string(dir.join(format!("{id}.name"))).ok())
                        .unwrap_or_else(|| id.clone());
                    luts.insert(id, name, lut);
                }
                Err(error) => tracing::warn!("can't read the LUT {}: {error}", path.display()),
            }
        }
        luts
    }

    fn insert(&self, id: String, name: String, lut: Lut) {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.entries.retain(|entry| entry.id != id);
        inner.entries.push(LutEntry { id: id.clone(), name });
        inner.entries.sort_by_key(|entry| entry.name.to_lowercase());
        inner.tables.insert(id, Arc::new(lut));
    }

    /// The imported LUTs, by name.
    pub fn entries(&self) -> Vec<LutEntry> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner()).entries.clone()
    }

    /// The table of LUT `id`, if this computer has it (REQ-043-03).
    pub fn table(&self, id: &str) -> Option<Arc<Lut>> {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .tables
            .get(id)
            .cloned()
    }

    /// Imports the `.cube` file at `path` (REQ-043-01), named by its title, else its file name. The same file
    /// imported again is the same LUT. Says why a file can't be used.
    pub fn import(&self, path: &Path) -> Result<LutEntry, String> {
        let file = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let bytes = std::fs::read(path).map_err(|error| format!("Couldn't read {file}: {error}"))?;
        let lut =
            Lut::parse(&String::from_utf8_lossy(&bytes)).map_err(|error| format!("{file} can't be used: {error}."))?;
        let id = lut_id(&bytes);
        let name = lut.title.clone().unwrap_or_else(|| {
            path.file_stem()
                .map_or_else(|| id.clone(), |stem| stem.to_string_lossy().into_owned())
        });
        if let Some(dir) = &self.dir {
            let kept = std::fs::create_dir_all(dir)
                .and_then(|()| std::fs::write(dir.join(format!("{id}.cube")), &bytes))
                .and_then(|()| match lut.title {
                    Some(_) => Ok(()),
                    None => std::fs::write(dir.join(format!("{id}.name")), &name),
                });
            kept.map_err(|error| format!("Couldn't keep {file}: {error}"))?;
        }
        self.insert(id.clone(), name.clone(), lut);
        Ok(LutEntry { id, name })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WARM: &str = "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";

    /// REQ-043-01: an imported LUT is kept in the data folder under its name, found again when the app starts, and
    /// the same file imported twice is one LUT; a file that isn't one says why.
    #[test]
    fn req_043_01_keeps_imported_luts() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Warm Evening.cube");
        std::fs::write(&file, WARM).unwrap();
        let store = dir.path().join("luts");
        let luts = Luts::open(store.clone());
        assert!(luts.entries().is_empty());

        let warm = luts.import(&file).unwrap();
        assert_eq!(warm.name, "Warm Evening");
        assert_eq!(luts.import(&file).unwrap(), warm, "the same file, the same LUT");
        assert_eq!(luts.entries().len(), 1);
        assert!(luts.table(&warm.id).is_some());

        let broken = dir.path().join("broken.cube");
        std::fs::write(&broken, "LUT_1D_SIZE 2\n0 0 0\n1 1 1\n").unwrap();
        let error = luts.import(&broken).unwrap_err();
        assert!(error.starts_with("broken.cube can't be used: it's a 1D LUT"), "{error}");

        let again = Luts::open(store);
        assert_eq!(
            again.entries(),
            std::slice::from_ref(&warm),
            "kept for the next start, with its name"
        );
        assert!(again.table(&warm.id).is_some());
    }
}
