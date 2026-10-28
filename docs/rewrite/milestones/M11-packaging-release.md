# M11 · Packaging and release

**Status:** Done (approved 2026-10-28, self-approved: the user asked to work through the milestones without their input)
**Specs:** 014 (all), 001 (REQ-001-09, new)

One part: about ten tasks, most of them tooling outside the app.

## Approach

**Release script (`cargo xtask release`).** A port of the reference's `scripts/release.ts` into `xtask/src/release.rs`, same arguments (`major` / `minor` / `patch` / `1.2.3`, `--dry-run`, `--yes`), same preconditions and messages, same git calls. The version lives in `[workspace.package] version` of the root `Cargo.toml` instead of `package.json`; the release commit also carries `Cargo.lock`, refreshed with `cargo update --workspace --offline`, so a release build with `--locked` doesn't find a stale lock file. git-cliff stays the changelog engine: `cliff.toml` is the reference's (same repository URL, `Flayms/Good-Gallery`, which is this repository now), and the script runs the `git-cliff` binary (`GIT_CLIFF` or `PATH`; CI installs 2.14.2, the version the reference's `node_modules` pins). The reference's `release.test.ts` is ported: the tests run against a throwaway repository with a bare `origin` and a tiny Cargo workspace. They skip without git-cliff, unless `GG_REQUIRE_HELPERS` or `CI` is set.

**Helpers.** `packaging/fetch-helpers.ps1` downloads the helper programs into `target/helpers/` (or a given folder), checks each download against a pinned SHA-256 and unpacks it into the layout the app looks for: `exiftool/exiftool.exe` + `exiftool_files/` (exiftool 13.59, the version the reference bundles), `ffmpeg/ffmpeg.exe` (ffmpeg-static's b6.1.1 build, the reference's), `mpv/libmpv-2.dll` (shinchiro's 20261021 build used since M0b). The script is PowerShell (not xtask) so xtask and the app keep no HTTP client (REQ-011-11's test checks `Cargo.lock`). CI runs it (cached) for the release build.

**Package (`cargo xtask dist`).** Builds `app` in release, stages `target/dist/good-gallery/` (`good-gallery.exe`, the helpers, `LICENSE`, `THIRD-PARTY-NOTICES.txt`), zips it into `good-gallery-<version>-portable.zip` (Windows' own `tar.exe`, which writes zip; Git Bash's GNU tar can't) and builds `good-gallery-<version>-setup.exe` with NSIS from `packaging/windows/installer.nsi` (`makensis` from `MAKENSIS`, `PATH` or `Program Files (x86)\NSIS`, which GitHub's Windows images have). The staging layout is a pure function with a test against the folders `media::helpers` looks in. Decided instead of `cargo-packager`: one NSIS script is ~100 lines, needs no extra tool install beyond NSIS, and keeps the helper folders exactly as the app expects them.

**Installer.** Per-user (no admin prompt), like the reference's `oneClick: false` + `allowToChangeInstallationDirectory`: directory page (default `%LOCALAPPDATA%\Programs\Good Gallery`, not the Electron version's `good-gallery` folder), install, finish page with _Run Good Gallery_. Start menu shortcut, an uninstaller registered under `HKCU\…\Uninstall\GoodGallery`. Uninstalling removes the program folder and the shortcut but never the data folder (index, settings, thumbnails). Silent mode (`/S /D=…`) works for the smoke test.

**App changes for a packaged build.**
- **Data folder:** the packaged app (feature `packaged`, set by `cargo xtask dist`; first planned as "release builds", changed after review so `cargo run --release` stays away from the real index) uses `%APPDATA%\good-gallery`, the Electron version's folder, so the installed app opens the user's existing index and thumbnail cache (REQ-001-08 makes that safe; new REQ-001-09). Other builds keep `good-gallery-rust`, so `cargo run` never touches the real index. `--data-dir` still overrides both.
- **No console window** in the packaged app (`windows_subsystem = "windows"`); a startup error shows a message box.
- **Log file:** `logs/app.log` in the data folder (the previous run's kept as `app.old.log`) besides stderr, as `docs/architecture.md` planned for M11. The app logs which helpers it found.
- **Icon:** the reference's `build/icon.svg`, rendered to `crates/app/assets/icon.ico` (16–256 px, PNG entries) and `icon.png`; the exe gets the icon and version info through `winresource` (build script, Windows only), the window gets it through `ViewportBuilder::with_icon` (eframe would show its own egui logo otherwise).

**Workflows.** `.github/workflows/release.yml` as the reference's: on a `vX.Y.Z` tag, run `ci.yml`, check the tag against the workspace version, fetch the helpers, `cargo xtask dist`, smoke-test the packaged app (silent install into a temp folder, run with `--screenshot` on an empty data folder, check the screenshot and the log's helper paths), write the notes with git-cliff plus the download table, `gh release create`. `ci.yml` installs git-cliff, so the release tests run on every push. A workflow test (`xtask/tests/workflows.rs`) checks that CI runs every check of REQ-014-01 and that the release workflow builds and attaches both downloads (REQ-014-08), so a later edit can't drop one silently.

**README screenshots.** `cargo xtask screenshots` ports the reference's `e2e/screenshots.ts` demo library: 56 generated landscapes in four trips (drawn with `image`: sky gradient, glow, sun, hill polylines, water), tagged through exiftool (keywords, hierarchies with People and Places, ratings, dates) and two videos from ffmpeg, indexed by this indexer into a fresh data folder on a `subst` drive (so no user path shows up), then the app is run with `--screenshot` per view (`gallery.view` set in the index between runs) and once with `--view` for the viewer with info panel and filmstrip. Output in `docs/screenshots/`, used by the README.

**Not done here (outward-facing, listed for the user):** pushing, tagging and publishing the first release; the branch protection rule on `main` (REQ-014-02, a repository setting); a run on a clean Windows machine (checked here by a silent install into a temp folder on this machine, run against a copy of the real index).

## Reference

- `scripts/release.ts`, `scripts/release.test.ts`: ported (arguments, preconditions, messages, git calls, tests).
- `cliff.toml`: copied unchanged.
- `.github/workflows/release.yml`: same steps; `pnpm dist` → `cargo xtask dist`, Playwright smoke test → silent install + `--screenshot`.
- `electron-builder.yml`: NSIS assisted installer, per-user, changeable folder, `<name>-<version>-setup.exe` and `-portable.zip`; extra resources → helper folders.
- `build/icon.svg`, `build/icon.png`: the app icon.
- `e2e/screenshots.ts`: demo library (palettes, trips, tags, dates, videos) and the screenshot list.
- `node_modules/exiftool-vendored.exe` (13.59), `ffmpeg-static` (b6.1.1): the helper versions.

## Tasks

- [x] 1. `xtask release` with `cliff.toml`, ported tests (REQ-014-03 … -07)
- [x] 2. Workflow test; `release.yml`, git-cliff in `ci.yml` (REQ-014-01, -08)
- [x] 3. App: data folder per build (REQ-001-09), log file, no console in release, startup error box, helper paths logged
- [x] 4. App icon (exe resource, window icon), version info
- [x] 5. `packaging/fetch-helpers.ps1` with pinned hashes
- [x] 6. `xtask dist`: staging layout (test against the helper lookup), portable zip, NSIS installer (REQ-014-08)
- [x] 7. Build the packages locally; silent install, run with a copy of the real index, uninstall keeps the data folder
- [x] 8. `xtask screenshots`; README rewritten with install, features and screenshots
- [ ] 9. Release dry run on this repository
- [x] 10. Docs: spec statuses, verification tables, architecture, pitfalls, progress

## Exit criteria

From `docs/rewrite/plan.md`: installs and runs on a clean Windows machine with the user's existing index; release dry run works.

Here: the installer installs silently and the installed app runs against a copy of the user's index with the bundled helpers; `cargo xtask release --dry-run` prints the next version and notes on this repository; all checks pass. The clean-machine run and the first published release are left to the user (open issues).
