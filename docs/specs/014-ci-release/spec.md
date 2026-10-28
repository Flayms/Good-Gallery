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
- **Then** it runs the checks of REQ-014-01, builds the Windows installer (`setup.exe`) and the portable zip, and publishes a GitHub release named after the version, whose notes are the version's changelog section plus download instructions and which has both attached

### REQ-014-09 · Installer and portable app

- **Given** the installer or the portable zip of a release
- **When** a user installs it (for their own user, without admin rights, into a folder they can change) or extracts the zip
- **Then** the app runs without anything else installed: exiftool, ffmpeg and the video player come with it
- **And** the installer adds a Start menu shortcut and an entry under Windows' installed apps
- **And** uninstalling removes the program, its shortcut and its entry, but keeps the data folder (index, settings, thumbnails)

## Out of scope

- Code signing (Windows SmartScreen warns about unsigned downloads). Free option for later: SignPath Foundation.
- Keeping the portable version's data next to the executable: it uses the same user data folder as an installed app.
- Auto-updates in the app.
- Builds for macOS or Linux.
- Pre-releases and release branches.

## Assumptions

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org) (`feat: …`, `fix(scope): …`, `feat!: …` / `BREAKING CHANGE:` for breaking changes).
- Merge protection is a GitHub ruleset on `main` that requires the CI checks `Lint` and `Test` (jobs in `ci.yml` that report the result of every platform's lint and test jobs); it lives in the repository settings, not in the repository. Repository admins bypass it, since `cargo xtask release` pushes the release commit straight to `main`.
- git-cliff (2.14.2 in CI) is installed where the release script runs, and NSIS where the installer is built (the release workflow installs it; GitHub's windows-2025 images don't have it).

## Verification

| REQ        | Test                                                                                       | Result  |
| ---------- | ------------------------------------------------------------------------------------------ | ------- |
| REQ-014-01 | `xtask/tests/workflows.rs` › ci_runs_every_check_on_main_and_pull_requests, ci_leaves_release_commits_to_the_release_run (the workflow itself runs on GitHub) | pass |
| REQ-014-02 | — (repository ruleset "main: CI must pass": requires Lint and Test from GitHub Actions; admins bypass it so the release script can push) | no test |
| REQ-014-03 | `xtask/src/release.rs` › req_014_03_derives_the_next_version_from_the_commits | pass |
| REQ-014-04 | `xtask/src/release.rs` › req_014_04_parses_overrides, req_014_04_overrides_the_bump_or_the_version | pass |
| REQ-014-05 | `xtask/src/release.rs` › req_014_05_lists_failed_preconditions, req_014_05_stops_when_a_precondition_fails, req_014_05_undoes_a_release_whose_push_fails | pass |
| REQ-014-06 | `xtask/src/release.rs` › req_014_06_writes_the_changelog | pass |
| REQ-014-07 | `xtask/src/release.rs` › req_014_07_sets_the_workspace_version, req_014_07_commits_tags_and_pushes_the_release; `cargo xtask release --dry-run` on this repository | pass |
| REQ-014-08 | `xtask/tests/workflows.rs` › release_publishes_both_downloads_after_the_checks; `xtask/src/dist.rs` › stages_the_helpers_where_the_app_finds_them, names_the_downloads_after_the_version; `cargo xtask dist` built both locally | partial (publishing runs with the first release) |
| REQ-014-09 | `xtask/src/dist.rs` › stages_the_helpers_where_the_app_finds_them; `packaging/smoke-test.ps1` (release workflow; run locally: silent install, app run, uninstall keeps the data folder) | pass |
