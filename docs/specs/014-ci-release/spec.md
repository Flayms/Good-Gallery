# 014 · CI and releases

**Status:** Implemented

## Goal

Every commit is checked automatically, so a broken change never lands on `main` unnoticed and a pull request can only be merged when its checks pass. Cutting a release is one command: the version follows from the conventional commits since the last release, the changelog writes itself, and the GitHub release comes with readable notes and the Windows installer to download.

## Requirements

### REQ-014-01 · Checks on `main` and pull requests

- **Given** a commit pushed to `main`, or a pull request
- **When** CI runs
- **Then** it checks the formatting, runs the linter (clippy), the spec check and all tests including the UI tests, and the run fails if any of them fails
- **And** a release commit (`chore(release): vX.Y.Z`) pushed to `main` isn't checked twice: its version tag's release run (REQ-014-08) checks it

### REQ-014-02 · Checks gate merges

- **Given** a pull request into `main`
- **When** its CI run hasn't passed
- **Then** it can't be merged

### REQ-014-03 · Version from the commits

- **Given** commits since the last release (a `vX.Y.Z` tag)
- **When** the release script runs
- **Then** the next version bumps the major version for a breaking change (the minor version before 1.0.0), the minor version for a `feat` and the patch version otherwise. Without any release yet, the version is 0.1.0

### REQ-014-04 · Overriding the version

- **Given** the release script is run with `major`, `minor`, `patch` or an explicit version like `1.2.3`
- **When** it computes the next version
- **Then** it uses that bump or version instead of the one derived from the commits. Unknown arguments, or a bump together with a version, are rejected

### REQ-014-05 · Preconditions

- **Given** the checked-out branch isn't `main`, the working tree has uncommitted changes, `main` is behind `origin/main`, there are no commits since the last release, or the tag of the next version already exists
- **When** the release script runs
- **Then** it stops with a message saying what to fix, and changes nothing

### REQ-014-06 · Changelog

- **Given** a release
- **When** the release script writes `CHANGELOG.md`
- **Then** it has a section per version with its date, breaking changes first, then the commits grouped by type (features, bug fixes, performance, refactoring, documentation, reverts, maintenance), each linking its commit, and a link comparing it with the previous version. Release commits and commits that don't follow the conventional format aren't listed

### REQ-014-07 · Commit, tag and push

- **Given** the preconditions hold
- **When** the release script runs
- **Then** it prints the next version and its notes, asks for confirmation, then sets the version in `Cargo.toml`, commits it with `CHANGELOG.md` as `chore(release): vX.Y.Z`, tags the commit `vX.Y.Z` and pushes both. With `--dry-run` it stops after printing, with `--yes` it doesn't ask

### REQ-014-08 · Published release

- **Given** a pushed `vX.Y.Z` tag
- **When** the release workflow runs
- **Then** it runs the checks of REQ-014-01, builds the Windows installer (`setup.exe`) and the portable zip, and publishes a GitHub release named after the version, whose notes are the version's changelog section (with the unpublished versions of REQ-014-11) plus download instructions and which has both attached

### REQ-014-09 · Installer and portable app

- **Given** the installer or the portable zip of a release
- **When** a user installs it (for their own user, without admin rights, into a folder they can change) or extracts the zip
- **Then** the app runs without anything else installed: exiftool, ffmpeg and the video player come with it
- **And** the installer adds an entry under Windows' installed apps, and the shortcuts and _Open with_ entries chosen on its options page (REQ-014-15)
- **And** uninstalling removes the program, its shortcuts and its entries, but keeps the data folder (index, settings, thumbnails)

### REQ-014-10 · Download names

- **Given** a release's downloads
- **When** a user looks at their file names
- **Then** each name says the app, the version, the operating system and the processor architecture it is for, and for Windows whether it is the installer or the portable zip: `good-gallery-<version>-windows-x64-setup.exe`, `good-gallery-<version>-windows-x64-portable.zip`, `good-gallery-<version>-linux-x64.tar.gz`, `good-gallery-<version>-linux-x64.deb`, `good-gallery-<version>-linux-x64.rpm`. The browser version's zip says `web` instead of a system: `good-gallery-<version>-web.zip` (REQ-019-16)

### REQ-014-11 · Notes of unpublished versions

- **Given** versions tagged since the last published GitHub release whose release run failed, so they never got a release
- **When** the release workflow publishes the next version
- **Then** its notes list the changes of those versions too, in one section per type, say which versions they come from, and the comparison link starts at the last published release
- **And** `CHANGELOG.md` keeps one section per tagged version

### REQ-014-12 · Helpers in development builds

- **Given** a checkout where `packaging/fetch-helpers.ps1` has put the helpers into `target/helpers`
- **When** a developer runs the app or the tests from it without pointing `GOOD_GALLERY_EXIFTOOL`, `GOOD_GALLERY_FFMPEG` or `GOOD_GALLERY_MPV` at them
- **Then** the development build finds them there, so videos play and the video tests run instead of skipping
- **And** the packaged app keeps looking only next to itself (and on `PATH`)

### REQ-014-13 · Rebuilding the downloads

- **Given** downloads from an earlier `cargo xtask dist` that another process still holds open (an antivirus scan, an Explorer window)
- **When** `cargo xtask dist` runs again
- **Then** it writes the new downloads under a temporary name and replaces the old ones as soon as they are free
- **And** if they stay locked, it keeps the new file and says which file is locked and that `--out` writes elsewhere

### REQ-014-14 · A release doesn't wait for itself

- **Given** a pushed `vX.Y.Z` tag
- **When** the release workflow runs
- **Then** the checks, the Windows build and the Linux build run at the same time, and the release is published only after all of them passed
- **And** the browser version goes live only after the release is published
- **Given** dependencies unchanged since `main` last built the downloads
- **When** the release workflow builds them
- **Then** both builds reuse those compiled dependencies, and the release is published within 15 minutes
- **Given** no such cache exists
- **When** the release workflow runs
- **Then** the release still succeeds, only slower

### REQ-014-15 · Installer options

- **Given** the Windows installer
- **When** a user installs the app
- **Then** a page after the folder offers: _Start menu shortcut_ (on), _Desktop shortcut_ (off) and _Show under "Open with"_ for photos and videos (on, REQ-027-05), each with a line saying what it does
- **And** installing again or updating, also silently from the app (REQ-031-08), starts from the choices made last time, including a later change in the settings (REQ-027-06): an update brings back nothing the user turned off, and a choice turned off now is removed

### REQ-014-16 · README screenshots from Windows

- **Given** the README's screenshots, which show the Windows app
- **When** a week has passed, or someone starts the _Screenshots_ workflow by hand
- **Then** GitHub renders them on Windows with `cargo xtask screenshots`, in the dark appearance and the app's full window size, with no hover from wherever the mouse pointer lies
- **And** if any changed, it pushes them to the `screenshots-refresh` branch, replacing what was there, and its summary links to a pull request from that branch, which a person opens so CI checks it; if none changed, it says so and pushes nothing
- **Given** a pull request that changes the workflow or `xtask/src/screenshots.rs`
- **When** CI runs
- **Then** the workflow renders the screenshots and attaches them to the run, and pushes nothing

## Out of scope

- Code signing (Windows SmartScreen warns about unsigned downloads). Free option for later: SignPath Foundation.
- Keeping the portable version's data next to the executable: it uses the same user data folder as an installed app.
- Auto-updates in the app: spec 031.
- Builds for macOS or Linux.
- Pre-releases and release branches.

## Assumptions

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org) (`feat: …`, `fix(scope): …`, `feat!: …` / `BREAKING CHANGE:` for breaking changes).
- Merge protection is a GitHub ruleset on `main` that requires the CI checks `Lint` and `Test` (jobs in `ci.yml` that report the result of every platform's lint and test jobs); it lives in the repository settings, not in the repository. Repository admins bypass it, since `cargo xtask release` pushes the release commit straight to `main`.
- git-cliff (2.14.2 in CI) is installed where the release script runs, and NSIS where the installer is built (the release workflow installs it; GitHub's windows-2025 images don't have it).

## Verification

| REQ        | Test                                                                                       | Result  |
| ---------- | ------------------------------------------------------------------------------------------ | ------- |
| REQ-014-01 | `xtask/tests/workflows.rs` › ci_runs_every_check_on_main_and_pull_requests, ci_leaves_release_commits_to_the_release_run, only_main_saves_the_rust_cache (the workflow itself runs on GitHub) | pass |
| REQ-014-02 | `xtask/tests/workflows.rs` › req_018_01_req_018_09_ci_and_the_release_cover_linux (the `Lint` and `Test` checks exist and sum up every platform); the ruleset itself is the repository setting "main: CI must pass" (requires Lint and Test from GitHub Actions; admins bypass it so the release script can push) | partial |
| REQ-014-03 | `xtask/src/release.rs` › req_014_03_derives_the_next_version_from_the_commits | pass |
| REQ-014-04 | `xtask/src/release.rs` › req_014_04_parses_overrides, req_014_04_overrides_the_bump_or_the_version | pass |
| REQ-014-05 | `xtask/src/release.rs` › req_014_05_lists_failed_preconditions, req_014_05_stops_when_a_precondition_fails, req_014_05_undoes_a_release_whose_push_fails | pass |
| REQ-014-06 | `xtask/src/release.rs` › req_014_06_writes_the_changelog | pass |
| REQ-014-07 | `xtask/src/release.rs` › req_014_07_sets_the_workspace_version, req_014_07_commits_tags_and_pushes_the_release; `cargo xtask release --dry-run` on this repository | pass |
| REQ-014-08 | `xtask/tests/workflows.rs` › release_publishes_both_downloads_after_the_checks; `xtask/src/dist.rs` › stages_the_helpers_where_the_app_finds_them, req_014_10_names_say_the_system_and_the_architecture; `cargo xtask dist` built both locally | partial (publishing runs with the first release) |
| REQ-014-09 | `xtask/src/dist.rs` › stages_the_helpers_where_the_app_finds_them; `packaging/smoke-test.ps1` (release workflow; run locally: silent install, app run, uninstall keeps the data folder) | pass |
| REQ-014-10 | `xtask/src/dist.rs` › req_014_10_names_say_the_system_and_the_architecture; `xtask/tests/workflows.rs` › release_publishes_both_downloads_after_the_checks | pass |
| REQ-014-11 | `xtask/src/release.rs` › req_014_11_finds_the_versions_that_werent_published, req_014_11_release_notes_bring_the_unpublished_changes; `xtask/tests/workflows.rs` › release_publishes_both_downloads_after_the_checks; `cargo xtask release-notes v0.2.1 v0.1.2` on this repository lists 0.2.0's changes | pass |
| REQ-014-12 | `crates/media/src/helpers.rs` › req_014_12_development_builds_find_the_fetched_helpers | pass |
| REQ-014-13 | `xtask/src/dist.rs` › req_014_13_replaces_a_locked_download_once_it_is_free, req_014_13_names_a_download_that_stays_locked | pass |
| REQ-014-15 | `xtask/tests/packaging.rs` › req_014_15_the_installer_offers_and_remembers_its_options; `packaging/smoke-test.ps1` (Windows packages workflow: the defaults of a silent install, an update keeping changed choices) | pass |
| REQ-014-14 | `xtask/tests/workflows.rs` › req_014_14_the_builds_run_side_by_side_and_publish_waits_for_them, req_014_14_main_warms_the_cache_the_release_builds_restore; the 15 minutes are measured by the next release | partial |
| REQ-014-16 | `xtask/tests/workflows.rs` › req_014_16_renders_the_readme_screenshots_on_windows_and_offers_them; `crates/app/src/desktop.rs` › req_014_16_screenshots_ignore_the_pointer | partial (the dark appearance and the window size only by looking at a run's screenshots) |
