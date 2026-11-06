//! Measures the faces and content passes (spec 037) over a folder of photos: one photo at a time against the
//! threads the app uses, with the app's own services, and whether both give the same results (REQ-037-01, REQ-037-02).
//!
//! - `analysis-bench faces <yunet.onnx> <photo folder> [threads]`: the faces pass with YuNet.
//! - `analysis-bench content <data folder> <model id> <photo folder> [threads]`: the content pass with a model already
//!   in `<data folder>/content-models` (a copy of the app's data folder).
//!
//! Each runs one photo at a time, then `threads` at once (by default as many as the app takes: half the cores).
//!
//! The photos are the folder's JPEG, PNG and WebP files, not its subfolders. Each run starts from a scratch index.

use anyhow::{Context, Result, bail};
use db::libraries::LibraryStatus;
use db::writer::{IndexWriter, IndexedFile};
use domain::face_models::{DetectorFormat, ModelKind};
use domain::media::{MediaKind, MediaMetadata};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

/// A scratch index of the photos in `folder`, at `path`.
fn index(path: &Path, folder: &Path) -> Result<usize> {
    let conn = db::open(path)?;
    let root = db::libraries::insert(&conn, &folder.to_string_lossy(), "Photos", LibraryStatus::Online)?.id;
    let mut files = Vec::new();
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let lower = name.to_ascii_lowercase();
        if !entry.file_type()?.is_file()
            || ![".jpg", ".jpeg", ".png", ".webp"]
                .iter()
                .any(|ext| lower.ends_with(ext))
        {
            continue;
        }
        let metadata = entry.metadata()?;
        files.push(IndexedFile {
            rel_path: name.clone(),
            file_name: name,
            kind: MediaKind::Image,
            size: metadata.len() as i64,
            mtime: 1_000,
            sidecar_mtime: None,
            metadata: MediaMetadata {
                taken_at: Some(files.len() as i64),
                ..Default::default()
            },
            read_error: None,
        });
    }
    if files.is_empty() {
        bail!("no photos in {}", folder.display());
    }
    let count = files.len();
    IndexWriter::new(conn).write(root, &files)?;
    Ok(count)
}

fn report(label: &str, photos: usize, seconds: f64) {
    println!(
        "{label}: {photos} photos in {seconds:.1} s, {:.2} photos/s",
        photos as f64 / seconds
    );
}

fn faces(model: &Path, folder: &Path, threads: usize) -> Result<()> {
    let mut results = Vec::new();
    for at_once in [1, threads] {
        let data = tempfile::tempdir()?;
        let store = media::face_store::FaceStore::in_data_folder(data.path());
        let kind = ModelKind::Detect(DetectorFormat::YuNet);
        let added = store
            .add_file(model, &|_| Ok(kind), &mut |_, _| true)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let path = data.path().join("gallery.db");
        let photos = index(&path, folder)?;
        let conn = db::open(&path)?;
        db::settings::set_value(&conn, db::faces::DETECTOR_KEY, &serde_json::json!(added.sha256))?;
        let start = Instant::now();
        let service = indexer::faces::FaceService::start(indexer::faces::FaceDeps {
            conn: db::open(&path)?,
            work: Box::new(indexer::faces::RunModels::new(data.path(), None).with_threads(at_once)),
            is_reachable: Arc::new(|_| true),
            on_change: Box::new(|| {}),
        });
        service.wait_idle();
        let seconds = start.elapsed().as_secs_f64();
        if let Some(problem) = service.status().problem {
            bail!("{problem}");
        }
        report(&format!("faces, {at_once} at once"), photos, seconds);
        let found: Vec<Vec<[f32; 4]>> = (1..=photos as i64)
            .map(|media| {
                db::faces::unknown_faces_on(&conn, media, &added.sha256, "")
                    .map(|faces| faces.into_iter().map(|face| face.rect).collect())
            })
            .collect::<Result<_, _>>()?;
        results.push(found);
    }
    println!("same faces: {}", results[0] == results[1]);
    Ok(())
}

fn content(data_folder: &Path, model: &str, folder: &Path, threads: usize) -> Result<()> {
    let mut results = Vec::new();
    for at_once in [1, threads] {
        let scratch = tempfile::tempdir()?;
        let path = scratch.path().join("gallery.db");
        let photos = index(&path, folder)?;
        let conn = db::open(&path)?;
        db::settings::set_value(&conn, db::content::MODEL_KEY, &serde_json::json!(model))?;
        let thumbs = indexer::thumb_cache::ThumbCache::open(scratch.path().join("thumbs"), 1 << 30);
        let work =
            indexer::content::RunContent::new(data_folder, thumbs, None, Arc::new(|_| true)).with_threads(at_once);
        let start = Instant::now();
        let service = indexer::content::ContentService::start(indexer::content::ContentDeps {
            conn: db::open(&path)?,
            work: Box::new(work),
            on_change: Box::new(|| {}),
        });
        service.wait_idle();
        let seconds = start.elapsed().as_secs_f64();
        if let Some(problem) = service.status().problem {
            bail!("{problem}");
        }
        report(&format!("content, {at_once} at once"), photos, seconds);
        let mut vectors = Vec::new();
        db::content::each_vector(&conn, model, |media, vector| vectors.push((media, vector.to_vec())))?;
        vectors.sort();
        results.push(vectors);
    }
    println!("same vectors: {}", results[0] == results[1]);
    Ok(())
}

pub fn run(args: &[String]) -> Result<()> {
    let arg = |n: usize| {
        args.get(n)
            .context("missing argument, see the top of analysis_bench.rs")
    };
    let threads = |n: usize| -> Result<usize> {
        match args.get(n) {
            Some(threads) => threads.parse().context("threads must be a number"),
            None => Ok(indexer::analysis::threads()),
        }
    };
    match args.first().map(String::as_str) {
        Some("faces") => faces(Path::new(arg(1)?), Path::new(arg(2)?), threads(3)?),
        Some("content") => content(Path::new(arg(1)?), arg(2)?, Path::new(arg(3)?), threads(4)?),
        _ => bail!(
            "usage: analysis-bench faces <yunet.onnx> <photo folder> | content <data folder> <model id> <photo folder>"
        ),
    }
}
