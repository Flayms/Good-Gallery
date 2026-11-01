//! Worktrees can share one CARGO_TARGET_DIR without building each other's code (#107): every workspace crate reads the
//! worktree's path that `.cargo/config.toml` sets, so cargo rebuilds it when another worktree built there last.

use std::path::{Path, PathBuf};

/// The crate roots of the workspace members: `crates/*` and `xtask`.
fn crate_roots() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut packages: Vec<PathBuf> = std::fs::read_dir(root.join("crates"))
        .expect("crates/")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.join("Cargo.toml").is_file())
        .collect();
    packages.push(root.join("xtask"));
    packages
        .iter()
        .flat_map(|package| ["src/lib.rs", "src/main.rs"].map(|root| package.join(root)))
        .filter(|root| root.is_file())
        .collect()
}

#[test]
fn every_workspace_crate_is_rebuilt_for_another_worktree() {
    let config = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.cargo/config.toml")).unwrap();
    assert!(
        config.contains(r#"GOOD_GALLERY_WORKTREE = { value = "", relative = true, force = true }"#),
        ".cargo/config.toml doesn't set the worktree's path"
    );
    let roots = crate_roots();
    assert!(roots.len() >= 7, "found only {roots:?}");
    for root in roots {
        let source = std::fs::read_to_string(&root).unwrap();
        // A binary beside a library rebuilds with it.
        if root.ends_with("main.rs") && root.with_file_name("lib.rs").is_file() {
            continue;
        }
        assert!(
            source.contains(r#"option_env!("GOOD_GALLERY_WORKTREE")"#),
            "{} doesn't read GOOD_GALLERY_WORKTREE, so another worktree's build of it counts as fresh",
            root.display()
        );
    }
}
