//! The GitHub workflows run what spec 014 promises, so an edit can't drop a check or a download unnoticed.

use std::path::Path;

fn workflow(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.github/workflows")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("can't read {}: {e}", path.display()))
}

/// The commands of `run:` steps, one per line, without the `- run:` prefix.
fn commands(workflow: &str) -> Vec<&str> {
    workflow
        .lines()
        .map(str::trim)
        .map(|line| line.trim_start_matches("- ").trim_start_matches("run:").trim())
        .collect()
}

/// REQ-014-01: every push and pull request checks formatting, clippy, the specs and all tests.
#[test]
fn ci_runs_every_check_on_pushes_and_pull_requests() {
    let ci = workflow("ci.yml");
    for trigger in ["push:", "branches: ['**']", "pull_request:", "workflow_call:"] {
        assert!(ci.contains(trigger), "ci.yml lacks {trigger}");
    }
    let commands = commands(&ci);
    for check in [
        "cargo fmt --all --check",
        "cargo clippy --workspace --all-targets -- -D warnings",
        "cargo xtask check-specs",
        "cargo test --workspace --no-fail-fast",
    ] {
        assert!(commands.contains(&check), "ci.yml doesn't run {check}");
    }
    assert!(!ci.contains("continue-on-error"), "a failing check must fail the run");
}

/// REQ-014-08: a version tag runs the checks, builds both downloads and publishes them with the changelog's notes.
#[test]
fn release_publishes_both_downloads_after_the_checks() {
    let release = workflow("release.yml");
    assert!(release.contains("tags: ['v[0-9]+.[0-9]+.[0-9]+']"));
    assert!(release.contains("uses: ./.github/workflows/ci.yml"));
    assert!(release.contains("needs: checks"));
    let commands = commands(&release);
    assert!(commands.contains(&"cargo xtask dist"));
    assert!(commands.iter().any(|line| line.starts_with("git-cliff --current")));
    let publish = &release[release.find("gh release create").expect("no gh release create")..];
    for part in [
        "good-gallery-$VERSION-setup.exe",
        "good-gallery-$VERSION-portable.zip",
        "--title \"Good Gallery $VERSION\"",
        "--notes-file release-notes.md",
    ] {
        assert!(publish.contains(part), "the release doesn't get {part}");
    }
    assert!(!release.contains("continue-on-error"));
}
