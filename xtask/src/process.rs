//! Helpers shared by the tasks that drive other programs or index a library.

use anyhow::{Context as _, bail};
use db::libraries::{self, LibraryStatus};
use db::writer::IndexWriter;
use indexer::ScanScope;
use indexer::scan::{ScanDeps, ScanOptions, ScanOutcome, scan_root};
use media::exiftool::ExifTool;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

/// Runs `command` to its end; an error with its output when it fails.
pub fn run_command(command: &mut Command) -> anyhow::Result<()> {
    let output = command.output().with_context(|| format!("can't run {command:?}"))?;
    if !output.status.success() {
        bail!(
            "{command:?} failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Indexes `library` into `index` with the app's indexer, as a library labelled `label`.
pub fn index_library(exiftool: PathBuf, library: &Path, index: &Path, label: &str) -> anyhow::Result<()> {
    let conn = db::open(index)?;
    let path = library.to_str().context("library path isn't UTF-8")?;
    let id = libraries::insert(&conn, path, label, LibraryStatus::Unknown)?.id;
    let mut writer = IndexWriter::new(conn);
    let metadata = ExifTool::new(exiftool, 2);
    let cancel = AtomicBool::new(false);
    let mut on_progress = |_| {};
    let outcome = scan_root(
        ScanDeps {
            writer: &mut writer,
            metadata: &metadata,
            is_reachable: &|_: &Path| true,
        },
        id,
        &ScanScope::Full,
        ScanOptions::new(2, &cancel, &mut on_progress),
    )?;
    if outcome != ScanOutcome::Completed {
        bail!("the scan ended {outcome:?}");
    }
    Ok(())
}
