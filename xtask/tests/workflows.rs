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
        ("release-cache.yml", "save-if: ${{ github.ref == 'refs/heads/main' }}"),
        ("windows-packages.yml", "save-if: false"),
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
/// system and the architecture, with the changelog's notes. REQ-014-11: and the changes of unpublished versions.
#[test]
fn release_publishes_both_downloads_after_the_checks() {
    let release = workflow("release.yml");
    assert!(release.contains("tags: ['v[0-9]+.[0-9]+.[0-9]+']"));
    assert!(release.contains("uses: ./.github/workflows/ci.yml"));
    assert!(release.contains("needs: checks"));
    // The Windows job; the Linux archive's job runs `cargo xtask dist` too, without NSIS.
    let windows = workflow("windows-packages.yml");
    let commands = commands(&windows);
    let dist = commands.iter().position(|line| *line == "cargo xtask dist");
    // windows-2025 images don't come with NSIS, and the installer can't be built without it.
    let nsis = commands.iter().position(|line| line.starts_with("choco install nsis"));
    assert!(dist.is_some(), "release.yml doesn't run cargo xtask dist");
    assert!(
        nsis.is_some_and(|nsis| nsis < dist.unwrap()),
        "release.yml doesn't install NSIS before cargo xtask dist"
    );
    // REQ-014-11: the notes bring the changes of versions whose release run failed.
    let published = commands
        .iter()
        .position(|line| line.starts_with("published=$(gh release list"));
    let notes = commands
        .iter()
        .position(|line| line.starts_with("cargo xtask release-notes \"$TAG\" $published"));
    assert!(
        published.is_some_and(|published| notes.is_some_and(|notes| published < notes)),
        "release.yml doesn't write the notes with cargo xtask release-notes and the published releases"
    );
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

/// REQ-014-14: the builds don't wait for the checks or for each other, and nothing goes out until all of them passed.
#[test]
fn req_014_14_the_builds_run_side_by_side_and_publish_waits_for_them() {
    let release = workflow("release.yml");
    let job = |from: &str, to: &str| {
        let start = release.find(from).unwrap_or_else(|| panic!("no job {from}"));
        let end = release[start..].find(to).unwrap_or_else(|| panic!("no job {to}"));
        release[start..start + end].to_string()
    };
    assert!(
        !job("\n  linux:", "\n  web:").contains("needs:"),
        "Linux waits for another job"
    );
    assert!(
        !job("\n  windows:", "\n  publish:").contains("needs:"),
        "Windows waits for another job"
    );
    let publish = &release[release.find("\n  publish:").expect("no publish job")..];
    assert!(publish.contains("needs: [checks, linux, windows, web]"));
    assert!(publish.contains("gh release create"));
    assert!(job("\n  pages:", "\n  windows:").contains("needs: [web, publish]"));
}

/// REQ-014-14: `main` builds exactly what the release builds, into the cache the release's build jobs restore. A tag
/// can't restore what another tag saved, and the cache key holds the job's name unless a shared key replaces it.
#[test]
fn req_014_14_main_warms_the_cache_the_release_builds_restore() {
    const SHARED_KEY: &str = "shared-key: release";
    let windows = workflow("windows-packages.yml");
    assert!(
        windows.contains(SHARED_KEY),
        "the Windows build doesn't restore the cache"
    );
    let packages = workflow("linux-packages.yml");
    assert!(
        packages.contains(SHARED_KEY),
        "the Linux build doesn't restore the cache"
    );

    let warm = workflow("release-cache.yml");
    assert!(warm.contains(SHARED_KEY));
    assert!(warm.contains("save-if: ${{ github.ref == 'refs/heads/main' }}"));
    // Both systems the release builds on, and the image the Linux downloads need (REQ-018-12).
    assert!(packages.contains("runs-on: ubuntu-24.04"));
    assert!(warm.contains("os: [windows-latest, ubuntu-24.04]"));
    // When the dependencies change, and before GitHub evicts a cache unused for 7 days.
    for part in ["Cargo.lock", "rust-toolchain.toml", "schedule:", "workflow_dispatch:"] {
        assert!(warm.contains(part), "release-cache.yml lacks {part}");
    }
    // rust-cache hashes the CARGO variables into the key, so these have to match the release's.
    assert!(warm.contains("CARGO_TERM_COLOR: always"));
    // What `cargo xtask` compiles, then what `cargo xtask dist` builds.
    let commands = commands(&warm);
    assert!(commands.contains(&"cargo build -p xtask"), "{commands:?}");
    assert!(
        commands.contains(&"cargo build --release --locked -p app --features packaged"),
        "{commands:?}"
    );
    let dist = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/dist.rs")).unwrap();
    assert!(
        dist.contains(r#"["build", "--release", "--locked", "-p", "app", "--features", "packaged"]"#),
        "cargo xtask dist builds something else than release-cache.yml warms"
    );
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
    let linux = &release[release.find("  linux:").expect("no linux job")..release.find("  web:").unwrap()];
    assert!(linux.contains("uses: ./.github/workflows/linux-packages.yml"));
    assert!(commands(&workflow("linux-packages.yml")).contains(&"cargo xtask dist"));
    assert!(release.contains("needs: [checks, linux, windows, web]"));
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
        "needs: [web, publish]",
        "actions/download-artifact",
        "actions/upload-pages-artifact",
        "actions/deploy-pages",
    ] {
        assert!(pages.contains(step), "release.yml's pages job lacks {step}");
    }
}

/// REQ-018-12: every pull request touching the packages and every release builds them, installs each into a clean
/// system and only then attaches them.
#[test]
fn req_018_12_the_release_tests_and_attaches_the_packages() {
    let packages = workflow("linux-packages.yml");
    for part in [
        "workflow_call:",
        "pull_request:",
        "packaging/linux/**",
        "ubuntu:24.04",
        "ubuntu:26.04",
        "fedora:latest",
        "smoke-test.sh",
    ] {
        assert!(packages.contains(part), "linux-packages.yml lacks {part}");
    }
    let release = workflow("release.yml");
    let create = &release[release.find("gh release create").expect("no gh release create")..];
    for part in [
        "good-gallery-$VERSION-linux-x64.deb",
        "good-gallery-$VERSION-linux-x64.rpm",
    ] {
        assert!(create.contains(part), "the release doesn't attach {part}");
        let notes = workflow("windows-packages.yml");
        assert!(notes.contains(&format!("**{part}**")), "the notes don't list {part}");
    }
}

/// REQ-019-16: the job that builds the page hands it to Pages and attaches its zip to the release, so a failed Pages
/// deploy doesn't hold back the release.
#[test]
fn req_019_16_the_release_attaches_the_page_as_a_zip() {
    let release = workflow("release.yml");
    let web = &release[release.find("\n  web:").expect("release.yml has no web job")..];
    let web = &web[..web[1..].find("\n  pages:").expect("the pages job follows the web job")];
    for part in [
        "needs: checks",
        "cargo xtask web",
        "target/web",
        "target/dist/*-web.zip",
    ] {
        assert!(web.contains(part), "release.yml's web job lacks {part}");
    }
    let publish = &release[release.find("\n  publish:").expect("no publish job")..];
    assert!(
        publish.contains("needs: [checks, linux, windows, web]"),
        "the release doesn't wait for the zip"
    );
    let create = &publish[publish.find("gh release create").expect("no gh release create")..];
    assert!(
        create.contains("good-gallery-$VERSION-web.zip"),
        "the release doesn't attach the zip"
    );
    assert!(
        workflow("windows-packages.yml").contains("**good-gallery-$VERSION-web.zip**"),
        "the notes' download table doesn't list the zip"
    );
}

/// REQ-031-09: a release attaches the installer's signature, made after the checks, with the secret key.
#[test]
fn req_031_09_the_release_signs_the_installer() {
    let release = workflow("release.yml");
    let publish = &release[release.find("\n  publish:").expect("no publish job")..];
    let sign = publish
        .find("packaging/sign-update.sh")
        .expect("the release doesn't sign");
    let create = publish.find("gh release create").expect("no gh release create");
    assert!(sign < create, "the release is published before it is signed");
    assert!(publish.contains("UPDATE_SIGNING_KEY: ${{ secrets.UPDATE_SIGNING_KEY }}"));
    assert!(publish[create..].contains("good-gallery-$VERSION-windows-x64-setup.exe.sig"));
}

/// REQ-031-08: pull requests that touch the installer run its smoke test, which updates over a running app.
#[test]
fn req_031_08_the_installer_is_tested_before_a_release() {
    let packages = workflow("windows-packages.yml");
    for part in ["pull_request:", "packaging/windows/**", "smoke-test.ps1"] {
        assert!(packages.contains(part), "windows-packages.yml lacks {part}");
    }
}

/// REQ-014-16: the README screenshots are rendered on Windows after every successful release or by hand, never for pull
/// requests, and pushed to a branch of their own for a person to open a pull request from.
#[test]
fn req_014_16_renders_the_readme_screenshots_on_windows_and_offers_them() {
    let screenshots = workflow("screenshots.yml");
    for trigger in [
        "workflow_dispatch:",
        "workflow_run:",
        "workflows: [Release]",
        "types: [completed]",
    ] {
        assert!(screenshots.contains(trigger), "screenshots.yml lacks {trigger}");
    }
    // Not on a timer: a week without a release has nothing new to show. Not for pull requests: 15 minutes on Windows
    // would slow them down.
    for trigger in ["schedule:", "pull_request:", "push:"] {
        assert!(!screenshots.contains(trigger), "screenshots.yml runs on {trigger}");
    }
    assert!(
        screenshots
            .contains("if: github.event_name != 'workflow_run' || github.event.workflow_run.conclusion == 'success'"),
        "a failed release renders them"
    );
    assert!(
        screenshots.contains("ref: ${{ github.event.workflow_run.head_sha || github.sha }}"),
        "not the released commit"
    );
    assert!(
        workflow("release.yml").contains("name: Release"),
        "the release workflow is named otherwise"
    );
    assert!(
        screenshots.contains("runs-on: windows-latest"),
        "not rendered on Windows"
    );
    assert!(commands(&screenshots).contains(&"cargo xtask screenshots"));
    assert!(
        screenshots.contains("Set-DisplayResolution -Width 1920 -Height 1080"),
        "the runner's small screen shrinks the window"
    );
    assert!(screenshots.contains("contents: write"), "it can't push the branch");

    let push = &screenshots[screenshots.find("git push").expect("nothing is pushed")..];
    assert!(
        push.starts_with("git push --force origin screenshots-refresh"),
        "pushed elsewhere"
    );
    let step = &screenshots[..screenshots.find("git push").unwrap()];
    let step = &step[step.rfind("- name:").expect("the push is a named step")..];
    assert!(
        step.contains("git diff --quiet -- docs/screenshots"),
        "unchanged screenshots are pushed"
    );
    assert!(
        screenshots.contains("compare/main...screenshots-refresh"),
        "no link to a pull request"
    );
    // A pull request opened with the workflow's token wouldn't run CI, and this repository doesn't let workflows
    // open them.
    assert!(!screenshots.contains("gh pr create"), "a person opens the pull request");
    assert!(
        screenshots.contains("upload-artifact"),
        "the renders aren't attached to the run"
    );
}
