//! `cargo xtask libraries --db <file> list | add <path> | remove <id> | rescan <id>`: library operations without a
//! window. Scans are only printed until the indexer runs.

use anyhow::{Context, bail};
use indexer::libraries;
use indexer::{ScanKind, ScanRequests};
use std::path::PathBuf;

const USAGE: &str = "usage: cargo xtask libraries --db <file> list | add <path> | remove <id> | rescan <id>";

struct PrintScans;

impl ScanRequests for PrintScans {
    fn request_scan(&self, library: i64, kind: ScanKind) {
        println!("scan requested: library {library}, {kind:?}");
    }

    fn remove_library(&self, library: i64) {
        println!("scans of library {library} stopped");
    }
}

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let (db, rest) = match args {
        [flag, db, rest @ ..] if flag == "--db" => (PathBuf::from(db), rest),
        _ => bail!(USAGE),
    };
    let conn = db::open(&db).with_context(|| format!("can't open {}", db.display()))?;
    let id = |text: &String| text.parse::<i64>().context("the library id must be a number");
    match rest {
        [command] if command == "list" => {
            for library in libraries::list(&conn)? {
                let last = library.last_scan_at.map_or("never".into(), |ms| ms.to_string());
                println!(
                    "{}\t{}\t{}\t{}\tlast indexed: {last}",
                    library.id,
                    library.label,
                    library.status.as_str(),
                    library.path
                );
            }
        }
        [command, path] if command == "add" => {
            let library = libraries::add(&conn, path, &PrintScans)?;
            println!("Added {}, indexing… (id {})", library.label, library.id);
        }
        [command, library] if command == "remove" => libraries::remove(&conn, id(library)?, &PrintScans)?,
        [command, library] if command == "rescan" => {
            libraries::rescan(&conn, id(library)?, &PrintScans)?;
            println!("Rescan queued");
        }
        _ => bail!(USAGE),
    }
    Ok(())
}
