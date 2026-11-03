# 014 · CI and release · Plan

Records the approach that was taken. Lasting decisions are under _Decisions_ at the end.

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

## Decisions

### 2026-10-28 · Packaging and releases

**Decided:**
- *Data folder:* the packaged app (feature `packaged`, set by `cargo xtask dist`) uses `%APPDATA%\good-gallery`, the folder every earlier version used; every other build uses `%APPDATA%\good-gallery-rust`, so `cargo run` (also `--release`) never touches the real index (`app::startup::default_data_dir`).
- *The packaged app* has no console (`windows_subsystem = "windows"`); a startup failure shows a message box and panics are logged. The app logs which helpers it found.
- *Icon:* `crates/app/assets/app-icon.svg`, rendered into `app-icon.ico` (16–256 px PNG entries; exe icon and version info through `winresource` in `crates/app/build.rs`, needs `rc.exe` from the Windows SDK) and `app-icon.png` (window icon). `cargo xtask icons` renders both again when the SVG changes.
- *Helpers:* `packaging/fetch-helpers.ps1` downloads pinned versions (exiftool 13.59, ffmpeg-static b6.1.1, libmpv 20261021) and checks their SHA-256. The folder layout is one set of constants (`media::helpers::{EXIFTOOL,FFMPEG,MPV}_BUNDLED`) used by the app's lookup and by `cargo xtask dist`. PowerShell, not xtask, so no HTTP client enters the workspace (REQ-011-11).
- *Package and releases:* `cargo xtask dist` (NSIS installer, portable zip), `cargo xtask release` (git-cliff with `cliff.toml`, version in `[workspace.package]`), `.github/workflows/release.yml`. Details in spec 014's plan.
- *README screenshots:* `cargo xtask screenshots` generates a demo library (landscapes drawn with `image`, tags through exiftool, two ffmpeg videos) on a `subst` drive, indexes it and runs the app with `--screenshot` per view.
**Rejected:** `cargo-packager` (another tool for one NSIS script, and its resource layout would differ from the folders the app looks in), WiX / MSI (per-machine by habit), a separate portable data folder (out of scope in spec 014), an HTTP client in xtask for the helpers.

### 2026-10-31 · Faster CI

**Decided:** CI runs the tests with cargo-nextest (`.config/nextest.toml`, profile `ci`): the test binaries run in parallel instead of one after another, and every test in its own process, so the `GPU` lock of the UI tests (one wgpu harness per process at a time) still holds. A test that hangs fails after 5 minutes. nextest skips doctests, so `cargo test --workspace --doc` runs them. Only `main` saves the Rust cache (pull requests restore it; a tag's cache can't be restored by any other ref). `cargo test --workspace` stays the local command.
**Rejected:** skipping wasm-opt in CI's web build (the browser tests would check a different build than the one published), a different linker on the Windows runners (CI would link differently from the release), a Dev Drive there (another third-party action for a gain not measured yet), skipping CI for documentation changes (the spec check reads `docs/specs`).

### 2026-11-03 · Faster releases (#157)

**Decided:** v0.4.0 took 43 minutes: the Windows build (20 min) waited for the Linux build (15 min), and both started without a cache, because rust-cache's key holds the job's name and no job of those names ever saved one. So the checks and both builds now run side by side, and a `publish` job without a toolchain attaches their files once all passed; the Windows job writes the notes, since it has compiled xtask anyway. `release-cache.yml` builds what a release builds on `main`, when the dependencies change and weekly (GitHub evicts a cache unused for 7 days), under a `shared-key` the release's build jobs restore. It builds with `cargo build`, not `cargo xtask dist`, so it needs neither NSIS nor nfpm; a workflow test compares the command with `dist.rs`. The page goes live only after `publish`, so never for a version whose release failed.
**Rejected:** skipping the checks on a release commit (REQ-014-01 relies on them, and side by side they cost nothing), weakening the release profile (it has nothing to remove, and it would change what ships), moving `dist` out of `xtask` so packaging compiles less (the cache covers it), rust-cache's `lookup-only` to skip a build whose cache exists (its second instance in one job complicates saving, and the build only recompiles the workspace crates).
