//! The UI tests in one binary, instead of one per file that each linked the whole app (≈ 140 MB). `autotests` is off,
//! so a new file in `tests/` needs a `mod` line here; [`every_file_is_a_module`] checks that.

mod common;

mod appearance;
mod content;
mod context_menu;
mod edit;
mod faces;
mod fix_dates;
mod gallery;
mod indexing;
mod libraries;
mod loose;
mod map;
mod open_with;
mod rating;
mod rename;
mod search;
mod selection;
mod settings;
mod smart_albums;
mod snapshots;
mod tagging;
mod updates;
mod video;
mod viewer;
mod viewer_video;

#[test]
fn every_file_is_a_module() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let this = std::fs::read_to_string(dir.join("ui.rs")).unwrap();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if let Some(module) = name.strip_suffix(".rs").filter(|module| *module != "ui") {
            assert!(
                this.contains(&format!("\nmod {module};\n")),
                "tests/{name} isn't compiled: add `mod {module};` to tests/ui.rs"
            );
        }
    }
}
