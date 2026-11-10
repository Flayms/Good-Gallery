//! The crates' layering (docs/architecture.md): `domain` and `meta` stay pure (no IO, no UI, no database, so their rules
//! are tested without files, a GPU or an index), and workspace crates depend on each other only as the diagram shows.

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

/// Every dependency between workspace crates, as `from -> to` with its platform when it has one. A new edge is an
/// architecture change: add it here and to the diagram in docs/architecture.md.
const EDGES: [&str; 16] = [
    "app -> db",
    "app -> domain",
    "app -> indexer",
    "app -> media [desktop]",
    "app -> meta [web]",
    "db -> domain",
    "indexer -> db",
    "indexer -> domain",
    "indexer -> media [desktop]",
    "media -> domain",
    "media -> meta",
    "meta -> domain",
    "xtask -> db",
    "xtask -> domain",
    "xtask -> indexer",
    "xtask -> media",
];

#[test]
fn workspace_crates_depend_on_each_other_only_as_documented() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = std::process::Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps", "--offline"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages = metadata["packages"].as_array().unwrap();
    let members: Vec<&str> = packages
        .iter()
        .map(|package| package["name"].as_str().unwrap())
        .collect();
    let mut edges: Vec<String> = packages
        .iter()
        .flat_map(|package| {
            let from = package["name"].as_str().unwrap();
            package["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .map(move |dependency| (from, dependency))
        })
        .filter(|(_, dependency)| members.contains(&dependency["name"].as_str().unwrap()))
        .map(|(from, dependency)| {
            let to = dependency["name"].as_str().unwrap();
            match dependency["target"].as_str() {
                None => format!("{from} -> {to}"),
                Some(r#"cfg(not(target_arch = "wasm32"))"#) => format!("{from} -> {to} [desktop]"),
                Some(r#"cfg(target_arch = "wasm32")"#) => format!("{from} -> {to} [web]"),
                Some(target) => format!("{from} -> {to} [{target}]"),
            }
        })
        .collect();
    edges.sort();
    edges.dedup();
    let mut documented = EDGES.map(String::from).to_vec();
    documented.sort();
    assert_eq!(
        edges, documented,
        "the dependencies between workspace crates changed; update EDGES and the diagram in docs/architecture.md"
    );
}
