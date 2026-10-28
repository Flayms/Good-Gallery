# 014 · CI and release · Plan

Records the approach that was taken. Decisions that outlive this spec are in `docs/architecture.md`.

## Approach

- **CI** (`.github/workflows/ci.yml`, Windows): fmt, clippy, check-specs and all tests on pushes to `main` and on pull requests (not on pushes to other branches, which would check a pull request's commits twice), and as `workflow_call` for the release. It installs git-cliff 2.14.2 and fetches the bundled exiftool and ffmpeg (cached), so their tests run with `GG_REQUIRE_HELPERS=1`. libmpv isn't fetched in CI (whether the runners have a Vulkan loader is unknown).
- **Release script** (`cargo xtask release`, `xtask/src/release.rs`): `major` / `minor` / `patch` / `1.2.3`, `--dry-run`, `--yes`. git-cliff (`GIT_CLIFF` or `PATH`) with `cliff.toml` computes the version and writes `CHANGELOG.md`; the version lives in `[workspace.package]`, and the release commit carries `Cargo.lock` (refreshed with `cargo update --workspace --offline`). A failed push is undone locally (tag deleted, reset to the start). Tests run against a throwaway repository with a bare `origin` and a tiny Cargo workspace; they skip without git-cliff unless `GG_REQUIRE_HELPERS` or `CI` is set.
- **Helpers** (`packaging/fetch-helpers.ps1`): downloads exiftool 13.59, ffmpeg-static b6.1.1 and libmpv 20261021, checks their SHA-256 and unpacks them into `target/helpers/` in the layout the app looks for (`media::helpers::{EXIFTOOL,FFMPEG,MPV}_BUNDLED`).
- **Package** (`cargo xtask dist`, REQ-014-09): builds `app` in release (`--locked --features packaged`), stages `target/dist/good-gallery/` (exe, helpers, `LICENSE.txt`, `THIRD-PARTY-NOTICES.txt`), zips it with Windows' own `tar.exe` and builds the installer from `packaging/windows/installer.nsi` (NSIS, MUI2): per-user, default folder `%LOCALAPPDATA%\Programs\Good Gallery`, Start menu shortcut, uninstall entry `HKCU\…\Uninstall\GoodGallery`. A picked folder that already holds other files gets `Good Gallery` appended, so the uninstaller only deletes its own folders; the data folder is never touched.
- **Release workflow** (`.github/workflows/release.yml`, REQ-014-08): on a `vX.Y.Z` tag, run `ci.yml`, check the tag against the workspace version, fetch the helpers, `cargo xtask dist`, smoke-test (`packaging/smoke-test.ps1`: silent install, run with `--screenshot`, check the log's helper paths, uninstall), write git-cliff's notes and publish both downloads. `xtask/tests/workflows.rs` keeps the workflows' checks and downloads from being dropped.

## Tasks

- [x] CI workflow (REQ-014-01)
- [x] `cargo xtask release` with `cliff.toml` (REQ-014-03 … -07)
- [x] Workflow tests; release workflow (REQ-014-01, -08)
- [x] Helper downloads with pinned hashes
- [x] `cargo xtask dist`: staging, portable zip, NSIS installer (REQ-014-08, -09)
- [ ] Publish the first release (the release workflow has never run on GitHub; REQ-014-08 partial)
- [ ] Branch protection on `main` requiring CI (REQ-014-02, a repository setting)
