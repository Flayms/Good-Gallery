//! Source files stay small enough to read whole (#332). A file over `LIMIT` lines fails, unless it is listed in
//! `LARGER` with its ceiling. Split a listed file rather than raising its ceiling; lower the ceiling when it shrinks, and
//! drop the entry when it fits under the limit.

use std::path::{Path, PathBuf};

/// Lines a source file may have. `tests.rs` files and integration tests (`tests/`) aren't counted.
const LIMIT: usize = 800;

/// Files over the limit when the check came, with their size then as their ceiling.
const LARGER: &[(&str, usize)] = &[
    ("crates/app/src/desktop.rs", 829),
    ("crates/app/src/edit/gpu.rs", 966),
    ("crates/app/src/edit/panel.rs", 1019),
    ("crates/app/src/glass.rs", 1009),
    ("crates/app/src/settings_page.rs", 1227),
    ("crates/app/src/sidebar.rs", 953),
    ("crates/app/src/state.rs", 1336),
    ("crates/app/src/theme.rs", 880),
    ("crates/app/src/titlebar.rs", 810),
    ("crates/app/src/viewer/mod.rs", 1152),
    ("crates/db/src/faces.rs", 1281),
    ("crates/db/src/gallery.rs", 1089),
    ("crates/db/src/writer.rs", 982),
    ("crates/domain/src/face_models.rs", 893),
    ("crates/indexer/src/faces.rs", 1054),
    ("crates/indexer/src/indexer.rs", 1025),
    ("crates/indexer/src/scan.rs", 1485),
    ("crates/indexer/src/thumbnails.rs", 842),
    ("crates/media/src/edit.rs", 1902),
    ("crates/meta/src/rules.rs", 1035),
    ("xtask/src/release.rs", 866),
];

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") && !path.ends_with("tests.rs") {
            found.push(path);
        }
    }
}

#[test]
fn source_files_stay_under_the_line_limit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root.join("crates")).unwrap() {
        let src = entry.unwrap().path().join("src");
        if src.is_dir() {
            sources(&src, &mut files);
        }
    }
    sources(&root.join("xtask/src"), &mut files);
    assert!(files.len() > 100, "found only {} files", files.len());

    let mut problems = Vec::new();
    for file in &files {
        let name = file.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let lines = std::fs::read_to_string(file).unwrap().lines().count();
        let ceiling = LARGER
            .iter()
            .find(|(listed, _)| *listed == name)
            .map(|(_, ceiling)| *ceiling);
        match ceiling {
            None if lines > LIMIT => problems.push(format!("{name} has {lines} lines, over {LIMIT}: split it")),
            Some(ceiling) if lines > ceiling => problems.push(format!(
                "{name} grew to {lines} lines, over its ceiling of {ceiling}: split it"
            )),
            Some(_) if lines <= LIMIT => problems.push(format!(
                "{name} has {lines} lines, under {LIMIT}: remove it from LARGER"
            )),
            Some(ceiling) if lines < ceiling => problems.push(format!(
                "{name} shrank to {lines} lines: lower its ceiling in LARGER from {ceiling}"
            )),
            _ => {}
        }
    }
    for (listed, _) in LARGER {
        if !files.iter().any(|file| file.ends_with(listed)) {
            problems.push(format!("{listed} is in LARGER but doesn't exist: remove it"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
