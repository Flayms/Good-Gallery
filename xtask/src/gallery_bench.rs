//! `cargo xtask gallery-bench (--thumbs | --originals) <folder> [--items 100000] [--out <data folder>] [--seconds 20]`:
//! the frame-time budget of REQ-006-12. Then runs the app with `--bench-scroll` at 12 columns.
//!
//! - `--thumbs`: builds a data folder whose index holds `items` media items of a library that doesn't exist (so no
//!   scan touches them), with every thumbnail in the cache: hard links to the WebP files found below the folder (e.g. a
//!   copy of a real cache), reused round-robin.
//! - `--originals`: no thumbnail is cached, so they render while the gallery scrolls (REQ-006-15). The library is a
//!   folder of `items` hard links to the JPEG files found below the folder (e.g. a copy of real photos), reused
//!   round-robin and indexed as a scan would, so the app's scans find nothing changed. Every run starts with an empty
//!   cache, and the app skips the warming pass.

use anyhow::{Context as _, bail};
use db::libraries::{self, LibraryStatus};
use domain::thumbs::ThumbKey;
use indexer::thumb_cache::thumb_path;
use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "usage: cargo xtask gallery-bench (--thumbs | --originals) <folder> [--items 100000] [--out <data folder>] [--seconds 20]";
/// A drive letter no test machine has: the library stays offline, so scans never remove its media items.
const LIBRARY: &str = "B:\\gallery-bench";
/// Photo-like shapes, height / width.
const SHAPES: [(i64, i64); 5] = [(400, 300), (300, 400), (400, 400), (600, 400), (400, 600)];

/// The files below `dir` whose extension is one of `extensions` (lower case), sorted.
fn files_with(dir: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| extensions.contains(&ext.to_ascii_lowercase().as_str()))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Writes the index and links the thumbnails; returns the number of media items.
fn build(out: &Path, thumbs: &Path, items: usize) -> anyhow::Result<()> {
    let sources = files_with(thumbs, &["webp"]);
    if sources.is_empty() {
        bail!("no .webp files below {}", thumbs.display());
    }
    std::fs::create_dir_all(out)?;
    let conn = db::open(&out.join("gallery.db"))?;
    let root = libraries::insert(&conn, LIBRARY, "Bench", LibraryStatus::Offline)?.id;
    db::settings::set_value(&conn, "gallery.columns", &serde_json::Value::from(12))?;
    let cache = out.join("thumbs");
    let tx = conn.unchecked_transaction()?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime, width, height, taken_at, thumb_status)
             VALUES (?1, ?2, ?3, 'image', ?4, ?5, ?6, ?7, ?8, 'ready')",
        )?;
        for i in 0..items {
            let (name, rel_path) = (format!("{i:06}.jpg"), format!("bench/{i:06}.jpg"));
            let (size, mtime) = (1000 + i as i64, 1_700_000_000_000 + i as i64);
            let (width, height) = SHAPES[i % SHAPES.len()];
            // One photo a minute, newest last, so the gallery shows them in index order.
            let taken_at = 1_700_000_000_000 - i as i64 * 60_000;
            insert.execute((root, &rel_path, &name, size, mtime, width, height, taken_at))?;
            let key = ThumbKey {
                root_id: root,
                rel_path,
                size,
                mtime,
            };
            let target = thumb_path(&cache, &key, 400);
            std::fs::create_dir_all(target.parent().context("thumbnail without folder")?)?;
            let source = &sources[i % sources.len()];
            std::fs::hard_link(source, &target)
                .with_context(|| format!("can't link {} (same drive as the data folder?)", source.display()))?;
        }
    }
    tx.commit()?;
    println!("{items} media items, thumbnails linked from {} files", sources.len());
    Ok(())
}

/// Links `items` originals from `originals` into a library in `out` and indexes them with their thumbnails pending.
fn build_cold(out: &Path, originals: &Path, items: usize) -> anyhow::Result<()> {
    let sources = files_with(originals, &["jpg", "jpeg"]);
    if sources.is_empty() {
        bail!("no .jpg files below {}", originals.display());
    }
    let library = out.join("library");
    let folder = library.join("bench");
    std::fs::create_dir_all(&folder)?;
    let conn = db::open(&out.join("gallery.db"))?;
    let path = library.to_str().context("data folder path isn't UTF-8")?;
    let root = libraries::insert(&conn, path, "Bench", LibraryStatus::Online)?.id;
    db::settings::set_value(&conn, "gallery.columns", &serde_json::Value::from(12))?;
    let tx = conn.unchecked_transaction()?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime, width, height, taken_at, thumb_status)
             VALUES (?1, ?2, ?3, 'image', ?4, ?5, ?6, ?7, ?8, 'pending')",
        )?;
        for i in 0..items {
            let name = format!("{i:06}.jpg");
            let target = folder.join(&name);
            let source = &sources[i % sources.len()];
            std::fs::hard_link(source, &target)
                .with_context(|| format!("can't link {} (same drive as the data folder?)", source.display()))?;
            let metadata = std::fs::metadata(&target)?;
            let (width, height) = SHAPES[i % SHAPES.len()];
            let taken_at = 1_700_000_000_000 - i as i64 * 60_000;
            insert.execute((
                root,
                format!("bench/{name}"),
                &name,
                metadata.len() as i64,
                indexer::retry::mtime_ms(&metadata),
                width,
                height,
                taken_at,
            ))?;
        }
    }
    tx.commit()?;
    println!("{items} media items, linked from {} originals", sources.len());
    Ok(())
}

/// Empties the thumbnail cache of a reused cold data folder and marks every thumbnail pending again.
fn reset_cold(out: &Path) -> anyhow::Result<()> {
    let cache = out.join("thumbs");
    if cache.exists() {
        std::fs::remove_dir_all(&cache)?;
    }
    let conn = db::open(&out.join("gallery.db"))?;
    conn.execute("UPDATE media SET thumbhash = NULL, thumb_status = 'pending'", [])?;
    Ok(())
}

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let (mut thumbs, mut originals, mut items, mut out, mut seconds) = (None, None, 100_000usize, None, 20.0f32);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--thumbs" => thumbs = Some(PathBuf::from(args.next().context(USAGE)?)),
            "--originals" => originals = Some(PathBuf::from(args.next().context(USAGE)?)),
            "--items" => items = args.next().context(USAGE)?.parse().context(USAGE)?,
            "--out" => out = Some(PathBuf::from(args.next().context(USAGE)?)),
            "--seconds" => seconds = args.next().context(USAGE)?.parse().context(USAGE)?,
            other => bail!("unknown argument {other} ({USAGE})"),
        }
    }
    let (source, cold) = match (thumbs, originals) {
        (Some(thumbs), None) => (thumbs, false),
        (None, Some(originals)) => (originals, true),
        _ => bail!(USAGE),
    };
    let temp = tempfile::tempdir_in(source.parent().unwrap_or(&source))?;
    let out = out.unwrap_or_else(|| temp.path().to_path_buf());
    if out.join("gallery.db").exists() {
        println!("reusing {}", out.display());
        if cold {
            reset_cold(&out)?;
        }
    } else if cold {
        build_cold(&out, &source, items)?;
    } else {
        build(&out, &source, items)?;
    }
    let mut app = Command::new(env!("CARGO"));
    app.args(["run", "--release", "-q", "-p", "app", "--", "--data-dir"])
        .arg(&out)
        .args(["--bench-scroll", &seconds.to_string()]);
    if cold {
        app.arg("--bench-cold");
    }
    let status = app.status()?;
    if !status.success() {
        bail!("the app failed");
    }
    if cold {
        let conn = db::open(&out.join("gallery.db"))?;
        let count = |status: &str| -> anyhow::Result<i64> {
            Ok(
                conn.query_row("SELECT count(*) FROM media WHERE thumb_status = ?1", [status], |row| {
                    row.get(0)
                })?,
            )
        };
        println!(
            "thumbnails rendered during the run: {} ({} still pending)",
            count("ready")?,
            count("pending")?
        );
    }
    Ok(())
}
