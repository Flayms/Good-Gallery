//! `cargo xtask thumbs --db <index> --cache <folder>`: how many of the index's rendered thumbnails this app finds in a
//! thumbnail cache, e.g. one written by an earlier version (REQ-005-15). Opens the index read-only, so give it a copy
//! of the real one anyway (the real data must never be opened in place).

use anyhow::{Context, bail};
use domain::thumbs::THUMB_WIDTHS;
use indexer::thumb_cache::thumb_path;
use std::path::PathBuf;

const USAGE: &str = "usage: cargo xtask thumbs --db <index copy> --cache <thumbnail folder>";

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let (db, cache) = match args {
        [db_flag, db, cache_flag, cache] if db_flag == "--db" && cache_flag == "--cache" => {
            (PathBuf::from(db), PathBuf::from(cache))
        }
        _ => bail!(USAGE),
    };
    let conn = db::open_reader(&db).with_context(|| format!("can't open {}", db.display()))?;
    let mut stmt = conn.prepare("SELECT id FROM media WHERE thumb_status = 'ready'")?;
    let ids: Vec<i64> = stmt.query_map([], |row| row.get(0))?.collect::<Result<_, _>>()?;
    let (mut found, mut missing) = ([0usize; THUMB_WIDTHS.len()], Vec::new());
    for id in &ids {
        let Some(source) = db::thumbs::thumb_source(&conn, *id)? else {
            continue;
        };
        for (i, width) in THUMB_WIDTHS.into_iter().enumerate() {
            if thumb_path(&cache, &source.key, width).is_file() {
                found[i] += 1;
            } else if missing.len() < 5 {
                missing.push(format!("{} ({width} px)", source.key.rel_path));
            }
        }
    }
    println!("{} media items with rendered thumbnails", ids.len());
    for (i, width) in THUMB_WIDTHS.into_iter().enumerate() {
        println!("{width} px: {} found in the cache", found[i]);
    }
    for example in missing {
        println!("missing, e.g. {example}");
    }
    Ok(())
}
