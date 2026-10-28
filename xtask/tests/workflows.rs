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

/// REQ-014-01: pushes to `main` and pull requests check formatting, clippy, the specs and all tests.
#[test]
fn ci_runs_every_check_on_main_and_pull_requests() {
    let ci = workflow("ci.yml");
    for trigger in ["push:", "branches: [main]", "pull_request:", "workflow_call:"] {
        assert!(ci.contains(trigger), "ci.yml lacks {trigger}");
    }
    // A push to a pull request's branch would otherwise run the checks twice, once per trigger.
    assert!(
        !ci.contains("branches: ['**']"),
        "ci.yml runs on pushes to every branch"
    );
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
    let dist = commands.iter().position(|line| *line == "cargo xtask dist");
    // windows-2025 images don't come with NSIS, and the installer can't be built without it.
    let nsis = commands.iter().position(|line| line.starts_with("choco install nsis"));
    assert!(dist.is_some(), "release.yml doesn't run cargo xtask dist");
    assert!(
        nsis.is_some_and(|nsis| nsis < dist.unwrap()),
        "release.yml doesn't install NSIS before cargo xtask dist"
    );
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
