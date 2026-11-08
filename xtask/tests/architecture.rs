//! `domain` and `meta` stay pure (docs/architecture.md): no IO, no UI, no database, so their rules are tested without
//! files, a GPU or an index.

use std::path::{Path, PathBuf};

const PURE: [&str; 2] = ["domain", "meta"];

/// Crates that would bring IO, a UI or the index into a pure crate.
const IMPURE_CRATES: [&str; 15] = [
    "app",
    "db",
    "media",
    "indexer",
    "eframe",
    "egui",
    "egui_extras",
    "wgpu",
    "rusqlite",
    "notify",
    "ureq",
    "image",
    "tract-onnx",
    "libloading",
    "rfd",
];

/// Paths that read or write the outside world.
const IO_PATHS: [&str; 6] = [
    "std::fs",
    "std::net",
    "std::process",
    "std::thread",
    "std::env",
    "std::io",
];

fn crate_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates").join(name)
}

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn pure_crates_depend_on_no_io_ui_or_index_crate() {
    for name in PURE {
        let manifest = std::fs::read_to_string(crate_dir(name).join("Cargo.toml")).unwrap();
        let dependencies = manifest
            .split("\n[")
            .filter(|section| section.starts_with("dependencies]") || section.starts_with("target."))
            .flat_map(|section| section.lines().skip(1));
        for line in dependencies {
            let dependency = line.split(['=', '.', ' ']).next().unwrap_or_default();
            assert!(
                !IMPURE_CRATES.contains(&dependency),
                "{name} depends on {dependency}; it has to stay pure"
            );
        }
    }
}

#[test]
fn pure_crates_do_no_io_outside_tests() {
    for name in PURE {
        let mut files = Vec::new();
        sources(&crate_dir(name).join("src"), &mut files);
        assert!(!files.is_empty());
        for file in files {
            let source = std::fs::read_to_string(&file).unwrap();
            let code = source.split("#[cfg(test)]").next().unwrap();
            for io in IO_PATHS {
                assert!(
                    !code.contains(io),
                    "{} uses {io}; {name} has to stay free of IO",
                    file.display()
                );
            }
        }
    }
}
