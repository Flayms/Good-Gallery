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
        TESTS,
        DOCTESTS,
    ] {
        assert!(commands.contains(&check), "ci.yml doesn't run {check}");
    }
    assert!(!ci.contains("continue-on-error"), "a failing check must fail the run");
    // Every test runs, so the uploaded diffs show all failing snapshots.
    let nextest = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.config/nextest.toml"))
        .expect("can't read .config/nextest.toml");
    assert!(nextest.contains("[profile.ci]") && nextest.contains("fail-fast = false"));
}

/// The tests in CI: nextest runs the test binaries in parallel, and cargo test the doctests nextest skips.
const TESTS: &str = "cargo nextest run --workspace --profile ci";
const DOCTESTS: &str = "cargo test --workspace --doc";

/// REQ-014-01: only `main` saves the Rust cache. Pull requests restore it; their own caches would push it out.
#[test]
fn only_main_saves_the_rust_cache() {
    for (name, save_if) in [
        ("ci.yml", "save-if: ${{ github.ref == 'refs/heads/main' }}"),
        // A tag's cache can't be restored by any other ref.
        ("release.yml", "save-if: false"),
    ] {
        let workflow = workflow(name);
        let caches = workflow.matches("uses: Swatinem/rust-cache@v2").count();
        assert!(caches > 0, "{name} doesn't cache");
        assert_eq!(
            workflow.matches(save_if).count(),
            caches,
            "{name}: every rust-cache needs {save_if}"
        );
    }
}

/// REQ-014-01: a release commit pushed to `main` skips CI, since its tag's release run checks the same commit.
#[test]
fn ci_leaves_release_commits_to_the_release_run() {
    let ci = workflow("ci.yml");
    let skip = "if: github.ref != 'refs/heads/main' || !startsWith(github.event.head_commit.message, 'chore(release)')";
    for job in ["lint:", "test:", "test-linux:"] {
        let start = ci
            .find(&format!("\n  {job}"))
            .unwrap_or_else(|| panic!("ci.yml has no {job} job"));
        // The job's settings before its steps.
        let header = &ci[start..start + ci[start..].find("steps:").expect("a job without steps")];
        assert!(
            header.contains(skip),
            "ci.yml's {job} job also runs for release commits"
        );
    }
}

/// REQ-014-08, REQ-014-10: a version tag runs the checks, builds both downloads and publishes them, named after the
/// system and the architecture, with the changelog's notes.
#[test]
fn release_publishes_both_downloads_after_the_checks() {
    let release = workflow("release.yml");
    assert!(release.contains("tags: ['v[0-9]+.[0-9]+.[0-9]+']"));
    assert!(release.contains("uses: ./.github/workflows/ci.yml"));
    assert!(release.contains("needs: checks"));
    // The Windows job; the Linux archive's job runs `cargo xtask dist` too, without NSIS.
    let windows = &release[release.find("\n  release:").expect("no release job")..];
    let commands = commands(windows);
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
        "good-gallery-$VERSION-windows-x64-setup.exe",
        "good-gallery-$VERSION-windows-x64-portable.zip",
        "good-gallery-$VERSION-linux-x64.tar.gz",
        "--title \"Good Gallery $VERSION\"",
        "--notes-file release-notes.md",
    ] {
        assert!(publish.contains(part), "the release doesn't get {part}");
    }
    assert!(!release.contains("continue-on-error"));
}

/// REQ-018-01, REQ-018-09: CI checks Linux as well as Windows, and the release builds the Linux archive there.
#[test]
fn req_018_01_req_018_09_ci_and_the_release_cover_linux() {
    let ci = workflow("ci.yml");
    assert!(ci.contains("os: [windows-latest, ubuntu-latest]"), "lint runs on both");
    let tests = &ci[ci.find("  test-linux:").expect("no Linux test job")..];
    // The release the Linux snapshots were rendered on.
    assert!(tests.contains("container: ubuntu:26.04"));
    assert!(tests.contains("libimage-exiftool-perl ffmpeg libmpv2 mesa-vulkan-drivers"));
    assert!(commands(tests).contains(&TESTS) && commands(tests).contains(&DOCTESTS));
    assert!(tests.contains("GG_REQUIRE_HELPERS: '1'"));
    // The ruleset on main requires "Lint" and "Test" (REQ-014-02): jobs of those names report every platform's result.
    for (name, needs) in [("Lint", "needs: [lint]"), ("Test", "needs: [test, test-linux, web]")] {
        let job = &ci[ci
            .find(&format!("    name: {name}\n"))
            .unwrap_or_else(|| panic!("no job named {name}"))..];
        let job = &job[..job.find("\n\n").unwrap_or(job.len())];
        assert!(job.contains(needs) && job.contains("if: always() &&"), "{job}");
    }
    let release = workflow("release.yml");
    let linux = &release[release.find("  linux:").expect("no linux job")..release.find("  pages:").unwrap()];
    assert!(linux.contains("runs-on: ubuntu-latest") && linux.contains("needs: checks"));
    assert!(commands(linux).contains(&"cargo xtask dist"));
    assert!(release.contains("needs: [checks, linux]"));
}

/// REQ-019-14: CI builds the page and runs its browser tests; a release publishes it to GitHub Pages.
#[test]
fn req_019_14_ci_tests_the_page_and_the_release_publishes_it() {
    let ci = workflow("ci.yml");
    let commands = commands(&ci);
    for check in [
        "cargo clippy -p app -p meta --target wasm32-unknown-unknown -- -D warnings",
        "cargo xtask web",
        "cargo xtask web-test --no-build",
    ] {
        assert!(commands.contains(&check), "ci.yml doesn't run {check}");
    }
    assert!(
        ci.contains("needs: [test, test-linux, web]"),
        "the required Test check ignores the web job"
    );
    let release = workflow("release.yml");
    let pages = &release[release.find("\n  pages:").expect("release.yml has no pages job")..];
    for step in [
        "needs: checks",
        "cargo xtask web",
        "actions/upload-pages-artifact",
        "actions/deploy-pages",
    ] {
        assert!(pages.contains(step), "release.yml's pages job lacks {step}");
    }
}
