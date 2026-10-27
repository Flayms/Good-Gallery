//! `cargo xtask gallery-bench --thumbs <folder> [--items 100000] [--out <data folder>] [--seconds 20]`: the
//! frame-time budget of REQ-006-12. Builds a data folder whose index holds `items` media items of a library that
//! doesn't exist (so no scan touches them), with every thumbnail in the cache: hard links to the WebP files found
//! below `--thumbs` (e.g. a copy of a real cache), reused round-robin. Then runs the app with `--bench-scroll` at 12
//! columns.

use anyhow::{Context as _, bail};
use db::libraries::{self, LibraryStatus};
use domain::thumbs::ThumbKey;
use indexer::thumb_cache::thumb_path;
use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str =
    "usage: cargo xtask gallery-bench --thumbs <folder> [--items 100000] [--out <data folder>] [--seconds 20]";
/// A drive letter no test machine has: the library stays offline, so scans never remove its media items.
const LIBRARY: &str = "B:\\gallery-bench";
/// Photo-like shapes, height / width.
const SHAPES: [(i64, i64); 5] = [(400, 300), (300, 400), (400, 400), (600, 400), (400, 600)];

fn webp_files(dir: &Path) -> Vec<PathBuf> {
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
            } else if path.extension().is_some_and(|ext| ext == "webp") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Writes the index and links the thumbnails; returns the number of media items.
fn build(out: &Path, thumbs: &Path, items: usize) -> anyhow::Result<()> {
    let sources = webp_files(thumbs);
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

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let (mut thumbs, mut items, mut out, mut seconds) = (None, 100_000usize, None, 20.0f32);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--thumbs" => thumbs = Some(PathBuf::from(args.next().context(USAGE)?)),
            "--items" => items = args.next().context(USAGE)?.parse().context(USAGE)?,
            "--out" => out = Some(PathBuf::from(args.next().context(USAGE)?)),
            "--seconds" => seconds = args.next().context(USAGE)?.parse().context(USAGE)?,
            other => bail!("unknown argument {other} ({USAGE})"),
        }
    }
    let thumbs = thumbs.context(USAGE)?;
    let temp = tempfile::tempdir_in(thumbs.parent().unwrap_or(&thumbs))?;
    let out = out.unwrap_or_else(|| temp.path().to_path_buf());
    if out.join("gallery.db").exists() {
        println!("reusing {}", out.display());
    } else {
        build(&out, &thumbs, items)?;
    }
    let status = Command::new(env!("CARGO"))
        .args(["run", "--release", "-q", "-p", "app", "--", "--data-dir"])
        .arg(&out)
        .args(["--bench-scroll", &seconds.to_string()])
        .status()?;
    if !status.success() {
        bail!("the app failed");
    }
    Ok(())
}
