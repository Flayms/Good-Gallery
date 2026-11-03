//! The face models on this computer (spec 032): files named by their SHA-256 in the data folder, each with a small note
//! on what it is. Downloading and adding end here, so everything else knows a model only by its hash.

use domain::face_models::{CATALOG, CatalogModel, DetectorFormat, InputStyle, ModelKind};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A model on this computer.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredModel {
    pub sha256: String,
    pub name: String,
    pub kind: ModelKind,
}

/// What became of a download or an added model.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum StoreError {
    #[error("cancelled")]
    Cancelled,
    #[error("the download differs from the published model")]
    WrongHash,
    #[error("only https links can be added")]
    NotHttps,
    #[error("{0}")]
    Failed(String),
}

/// Asks whether to go on, with the bytes so far and the total if known.
pub type Progress<'a> = &'a mut dyn FnMut(u64, Option<u64>) -> bool;

/// Models are large, but not this large; a link pointing elsewhere shouldn't fill the disk.
const MAX_BYTES: u64 = 4 << 30;

pub struct FaceStore {
    folder: PathBuf,
}

fn kind_to_note(kind: ModelKind) -> serde_json::Value {
    match kind {
        ModelKind::Detect(DetectorFormat::YuNet) => serde_json::json!({ "detect": "yunet" }),
        ModelKind::Detect(DetectorFormat::RetinaFace) => serde_json::json!({ "detect": "retinaface" }),
        ModelKind::Recognize { style, threshold } => serde_json::json!({
            "recognize": match style { InputStyle::ArcFace => "arcface", InputStyle::SFace => "sface" },
            "threshold": threshold,
        }),
    }
}

fn note_to_kind(note: &serde_json::Value) -> Option<ModelKind> {
    match note.get("detect").and_then(|value| value.as_str()) {
        Some("yunet") => return Some(ModelKind::Detect(DetectorFormat::YuNet)),
        Some("retinaface") => return Some(ModelKind::Detect(DetectorFormat::RetinaFace)),
        Some(_) => return None,
        None => {}
    }
    let style = match note.get("recognize")?.as_str()? {
        "arcface" => InputStyle::ArcFace,
        "sface" => InputStyle::SFace,
        _ => return None,
    };
    Some(ModelKind::Recognize {
        style,
        threshold: note.get("threshold")?.as_f64()? as f32,
    })
}

impl FaceStore {
    /// The store in `<data>/face-models`.
    pub fn in_data_folder(data: &Path) -> Self {
        Self {
            folder: data.join("face-models"),
        }
    }

    /// The file of a stored model.
    pub fn file(&self, sha256: &str) -> PathBuf {
        self.folder.join(format!("{sha256}.onnx"))
    }

    fn note(&self, sha256: &str) -> PathBuf {
        self.folder.join(format!("{sha256}.json"))
    }

    /// Every model here, by name.
    pub fn models(&self) -> Vec<StoredModel> {
        let mut models: Vec<StoredModel> = std::fs::read_dir(&self.folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension()? != "json" {
                    return None;
                }
                let sha256 = path.file_stem()?.to_str()?.to_owned();
                if !self.file(&sha256).exists() {
                    return None;
                }
                let note: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
                Some(StoredModel {
                    name: note.get("name")?.as_str()?.to_owned(),
                    kind: note_to_kind(&note)?,
                    sha256,
                })
            })
            .collect();
        models.sort_by(|a, b| a.name.cmp(&b.name));
        models
    }

    /// The model with this hash.
    pub fn model(&self, sha256: &str) -> Option<StoredModel> {
        self.models().into_iter().find(|model| model.sha256 == sha256)
    }

    /// Downloads a listed model: whatever the host sends, it counts only if its hash is the catalog's (REQ-032-02).
    pub fn download(&self, model: &CatalogModel, progress: Progress) -> Result<StoredModel, StoreError> {
        self.fetch(
            model.url,
            Some(model.sha256),
            model.name,
            model.kind,
            &|_| Ok(()),
            progress,
        )
    }

    /// Downloads an unlisted model from an `https` link, if `check` can run it (REQ-032-03).
    pub fn add_link(
        &self,
        url: &str,
        kind: ModelKind,
        check: &dyn Fn(&Path) -> Result<(), String>,
        progress: Progress,
    ) -> Result<StoredModel, StoreError> {
        if !url.to_ascii_lowercase().starts_with("https://") {
            return Err(StoreError::NotHttps);
        }
        let name = name_from_url(url);
        self.fetch(url, None, &name, kind, check, progress)
    }

    /// Copies an unlisted model from a file, if `check` can run it (REQ-032-03).
    pub fn add_file(
        &self,
        file: &Path,
        kind: ModelKind,
        check: &dyn Fn(&Path) -> Result<(), String>,
        progress: Progress,
    ) -> Result<StoredModel, StoreError> {
        let name = file
            .file_stem()
            .map_or_else(|| "Model".to_owned(), |stem| stem.to_string_lossy().into_owned());
        let source = std::fs::File::open(file).map_err(|error| StoreError::Failed(error.to_string()))?;
        let total = source.metadata().ok().map(|metadata| metadata.len());
        self.receive(source, total, None, &name, kind, check, progress)
    }

    fn fetch(
        &self,
        url: &str,
        sha256: Option<&str>,
        name: &str,
        kind: ModelKind,
        check: &dyn Fn(&Path) -> Result<(), String>,
        progress: Progress,
    ) -> Result<StoredModel, StoreError> {
        const CONNECT: Duration = Duration::from_secs(30);
        let response = ureq::get(url)
            .config()
            // A link that asked for https doesn't get redirected out of it.
            .https_only(url.starts_with("https://"))
            .timeout_connect(Some(CONNECT))
            .timeout_recv_response(Some(CONNECT))
            .build()
            .call()
            .map_err(|error| StoreError::Failed(format!("can't download: {error}")))?;
        let total = response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok());
        let reader = response.into_body().into_reader();
        self.receive(reader, total, sha256, name, kind, check, progress)
    }

    /// Writes beside the final name, and keeps the file only if everything holds.
    #[allow(clippy::too_many_arguments)]
    fn receive(
        &self,
        reader: impl Read,
        total: Option<u64>,
        sha256: Option<&str>,
        name: &str,
        kind: ModelKind,
        check: &dyn Fn(&Path) -> Result<(), String>,
        progress: Progress,
    ) -> Result<StoredModel, StoreError> {
        let failed = |error: std::io::Error| StoreError::Failed(error.to_string());
        std::fs::create_dir_all(&self.folder).map_err(failed)?;
        let temporary = tempfile::Builder::new()
            .prefix("incoming-")
            .suffix(".onnx")
            .tempfile_in(&self.folder)
            .map_err(failed)?;
        let hash = copy_hashing(reader, temporary.as_file(), total, progress)?;
        if sha256.is_some_and(|wanted| wanted != hash) {
            return Err(StoreError::WrongHash);
        }
        check(temporary.path()).map_err(StoreError::Failed)?;
        let note = {
            let mut note = kind_to_note(kind);
            note["name"] = name.into();
            note
        };
        temporary
            .persist(self.file(&hash))
            .map_err(|error| StoreError::Failed(error.to_string()))?;
        std::fs::write(self.note(&hash), note.to_string()).map_err(failed)?;
        Ok(StoredModel {
            sha256: hash,
            name: name.to_owned(),
            kind,
        })
    }

    /// Removes a model's file and note.
    pub fn remove(&self, sha256: &str) -> std::io::Result<()> {
        std::fs::remove_file(self.note(sha256))?;
        std::fs::remove_file(self.file(sha256))
    }
}

fn copy_hashing(
    mut reader: impl Read,
    mut file: &std::fs::File,
    total: Option<u64>,
    progress: Progress,
) -> Result<String, StoreError> {
    use std::io::Write;
    let failed = |error: std::io::Error| StoreError::Failed(error.to_string());
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    let mut copied = 0u64;
    loop {
        if !progress(copied, total) {
            return Err(StoreError::Cancelled);
        }
        let read = reader.read(&mut buffer).map_err(failed)?;
        if read == 0 {
            break;
        }
        copied += read as u64;
        if copied > MAX_BYTES {
            return Err(StoreError::Failed("the file is too large to be a model".into()));
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read]).map_err(failed)?;
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The file's name in a link, without its query.
fn name_from_url(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
    let name = last.strip_suffix(".onnx").unwrap_or(last);
    if name.is_empty() || name.contains(':') {
        "Model".to_owned()
    } else {
        name.to_owned()
    }
}

/// The catalog's entry for a stored model, for its size and license.
pub fn catalog_entry(sha256: &str) -> Option<&'static CatalogModel> {
    CATALOG.iter().find(|model| model.sha256 == sha256)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const KIND: ModelKind = ModelKind::Detect(DetectorFormat::YuNet);
    const BODY: &[u8] = b"pretend this is a model";
    const BODY_HASH: &str = "13c47d2ca9b3b9d6fdef86e6da1fa4e2c1a35c7a3e9c1d6a4a44a8c7d7a7cb2d";

    /// A one-shot local web server, sending `body`.
    fn serve(body: &'static [u8]) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 2048];
            let _ = std::io::Read::read(&mut stream, &mut request);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(body).unwrap();
        });
        format!("http://127.0.0.1:{port}/a%20model.onnx?download=1")
    }

    fn sha(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn leftovers(store: &FaceStore) -> usize {
        std::fs::read_dir(&store.folder).map_or(0, |entries| entries.count())
    }

    fn catalog_model(url: &'static str, sha256: &'static str) -> CatalogModel {
        CatalogModel {
            name: "Test",
            kind: KIND,
            url,
            sha256,
            bytes: 0,
            license: "MIT",
        }
    }

    #[test]
    fn req_032_02_keeps_a_download_only_if_its_hash_matches() {
        let dir = tempfile::tempdir().unwrap();
        let store = FaceStore::in_data_folder(dir.path());
        let good: &'static str = Box::leak(sha(BODY).into_boxed_str());
        let url: &'static str = Box::leak(serve(BODY).into_boxed_str());
        let mut seen = Vec::new();
        let model = store
            .download(&catalog_model(url, good), &mut |bytes, total| {
                seen.push((bytes, total));
                true
            })
            .unwrap();
        assert_eq!(model.sha256, good);
        assert_eq!(std::fs::read(store.file(good)).unwrap(), BODY);
        assert_eq!(store.models(), [model]);
        assert_eq!(seen.last(), Some(&(BODY.len() as u64, Some(BODY.len() as u64))));

        let dir = tempfile::tempdir().unwrap();
        let store = FaceStore::in_data_folder(dir.path());
        let url: &'static str = Box::leak(serve(BODY).into_boxed_str());
        let error = store.download(&catalog_model(url, BODY_HASH), &mut |_, _| true);
        assert_eq!(error, Err(StoreError::WrongHash));
        assert!(store.models().is_empty());
        assert_eq!(leftovers(&store), 0);
    }

    #[test]
    fn req_032_02_a_cancelled_or_failed_download_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = FaceStore::in_data_folder(dir.path());
        let url: &'static str = Box::leak(serve(BODY).into_boxed_str());
        let cancelled = store.download(&catalog_model(url, BODY_HASH), &mut |_, _| false);
        assert_eq!(cancelled, Err(StoreError::Cancelled));
        assert_eq!(leftovers(&store), 0);

        // Nobody listens there.
        let free = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let url: &'static str = Box::leak(format!("http://127.0.0.1:{free}/m.onnx").into_boxed_str());
        let failed = store.download(&catalog_model(url, BODY_HASH), &mut |_, _| true);
        assert!(matches!(failed, Err(StoreError::Failed(_))), "{failed:?}");
        assert_eq!(leftovers(&store), 0);
    }

    /// REQ-032-02: the catalog's real links, over real TLS, give the published files. `cargo test -- --ignored`.
    #[test]
    #[ignore = "downloads from the internet"]
    fn req_032_02_downloads_a_small_real_model_over_https() {
        let dir = tempfile::tempdir().unwrap();
        let store = FaceStore::in_data_folder(dir.path());
        for name in ["YuNet", "RetinaFace (MobileNetV2)"] {
            let model = CATALOG.iter().find(|model| model.name == name).unwrap();
            let stored = store.download(model, &mut |_, _| true).unwrap();
            assert_eq!(stored.sha256, model.sha256);
            assert_eq!(std::fs::metadata(store.file(model.sha256)).unwrap().len(), model.bytes);
        }
        assert_eq!(store.models().len(), 2);
    }

    #[test]
    fn req_032_03_adds_a_file_only_if_it_runs() {
        let dir = tempfile::tempdir().unwrap();
        let store = FaceStore::in_data_folder(dir.path());
        let source = dir.path().join("my model.onnx");
        std::fs::write(&source, BODY).unwrap();

        let refused = store.add_file(&source, KIND, &|_| Err("no".into()), &mut |_, _| true);
        assert_eq!(refused, Err(StoreError::Failed("no".into())));
        assert_eq!(leftovers(&store), 0);

        let kind = ModelKind::Recognize {
            style: InputStyle::SFace,
            threshold: 0.4,
        };
        let model = store.add_file(&source, kind, &|_| Ok(()), &mut |_, _| true).unwrap();
        assert_eq!(model.name, "my model");
        assert_eq!(store.model(&model.sha256), Some(model.clone()));
        assert!(source.exists());

        store.remove(&model.sha256).unwrap();
        assert!(store.models().is_empty());
    }

    #[test]
    fn req_032_03_links_must_be_https_and_name_the_model_by_their_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = FaceStore::in_data_folder(dir.path());
        let refused = store.add_link("http://example.com/a.onnx", KIND, &|_| Ok(()), &mut |_, _| true);
        assert_eq!(refused, Err(StoreError::NotHttps));
        assert_eq!(name_from_url("https://host/dir/my_model.onnx?x=1#y"), "my_model");
        assert_eq!(name_from_url("https://host/"), "host");
        assert_eq!(name_from_url("https://"), "Model");
    }
}
