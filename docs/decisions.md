# Decisions

Project-wide decisions that belong to no single spec, oldest first. Decisions about one feature are in its spec's `plan.md`, under _Decisions_; the current architecture is in `docs/architecture.md`. One entry each: what was decided, why (with measurements where there are any), what was rejected.

### 2026-10-26 · UI stack: eframe / egui 0.37 on wgpu

**Decided:** `eframe` / `egui` / `egui-wgpu` pinned to `=0.37.0`. A prototype scrolled 100,000 (and 250,000) items at 12 columns with a p95 frame interval of 7.2 ms on an integrated AMD GPU (budget 16 ms). egui's own painter is enough; no custom tile pipeline is needed.

### 2026-10-26 · Workspace and crate layout

**Decided:** one Cargo workspace (`resolver = "3"`, edition 2024) with the crates above; shared versions and lints in `[workspace.dependencies]` / `[workspace.lints]`. `app` is a library (`app::ui`, `AppState`, `theme`) plus a thin `main.rs`, so `egui_kittest` tests drive the same UI code as the window. `cargo xtask <task>` is a Cargo alias. Dependencies are optimized in dev builds (`[profile.dev.package."*"] opt-level = 3`): decoding and layout are too slow unoptimized to judge anything.

### 2026-10-26 · No async runtime, errors, logging

**Decided:**
- *Concurrency:* OS threads and `crossbeam-channel`, small worker pools where needed. No tokio: the work is CPU- or disk-bound, with few concurrent tasks and no network protocol.
- *Errors:* `thiserror` enums in library crates (`domain`, `db`, `media`, `indexer`), `anyhow` in `app` and `xtask`.
- *Logging:* `tracing` everywhere; `app` installs `tracing-subscriber` with `RUST_LOG`-style filtering (default `info`) on stderr and `logs\app.log` in the data folder (the previous run's kept as `app.old.log`).

### 2026-10-26 · Visual checks

**Decided:** UI changes are checked two ways: `egui_kittest` snapshot tests (wgpu renderer, PNGs in `crates/app/tests/snapshots/`, `UPDATE_SNAPSHOTS=1` rewrites them) and `cargo run -p app -- --screenshot <file.png>`, which renders a few frames, saves the window and exits, so a session can look at the real app without anyone at the screen. `--view`, `--info`, `--scroll` and `--screenshot-after` set up the shot.

### 2026-10-26 · UI state and background jobs

**Decided:** `app::AppState` holds what the UI shows; slow work (reachability, folder dialog, Explorer, cache clearing) runs on threads and sends an event back over a channel, applied at the start of the next frame (`AppState::poll`). Tests call `AppState::wait_for_jobs` before stepping the harness, so they never race the jobs. OS dialogs sit behind a trait (`FolderPicker`) so tests use a fake.

### 2026-10-26 · Pure media and tag rules in `domain`

**Decided:** file kinds, housekeeping folders, sidecar lookup, tag normalization, `MediaMetadata` and `METADATA_VERSION` live in `domain`; `db` depends on `domain` for the writer, `media` for extraction. `db::writer::IndexWriter` owns the write connection (on the indexer thread).

### 2026-10-27 · Helper processes end with the app

**Decided:** at startup the app puts itself into a Windows job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (`media::helpers::end_helpers_with_app`). Every process it starts (exiftool's launcher and its child, ffmpeg) joins the job and ends with the app, however it exits. Without it, exiftool's PAR-packed launcher outlived the app. The job allows breakaway: programs opened for the user (Explorer) are started with `CREATE_BREAKAWAY_FROM_JOB`, or closing the app would close them too.

### 2026-11-01 · Worktrees sharing a target folder

**Decided:** `.cargo/config.toml` sets `GOOD_GALLERY_WORKTREE` to the worktree's path (`[env]`, `relative`, `force`), and every workspace crate root reads it with `option_env!`. Cargo tracks the value, so a crate built by another worktree into the same `CARGO_TARGET_DIR` is rebuilt instead of counting as fresh (cargo names workspace crates' artifacts by their path relative to the workspace). Dependencies stay shared. `xtask/tests/worktrees.rs` checks that every crate root reads it.
**Rejected:** a target folder per worktree (every worktree builds all dependencies again, gigabytes each), an agent hook that cleans the workspace crates when the worktree changes (it sees only the session's folder, not a `cd` in the command, and misses builds outside the agent).

### 2026-11-08 · README screenshots rendered after each release

**Decided:** the README shows the Windows app, but it is mostly developed on Linux, so its screenshots fell behind (#264). `.github/workflows/screenshots.yml` runs `cargo xtask screenshots` on Windows after every successful release (`workflow_run` of _Release_), and when started by hand. It pushes changed screenshots to the `screenshots-refresh` branch and links a pull request from that branch in the run's summary, for a person to open (REQ-014-16).
**Rejected:** opening the pull request from the workflow. A pull request opened with `GITHUB_TOKEN` starts no workflows, so the required `Lint` and `Test` checks would never run, and the repository doesn't let workflows open pull requests. A token of its own would be one more secret to keep. Also rejected: taking the screenshots on Linux, since they would show Linux's window controls and paths. Pushing to `main` was rejected too, as the ruleset requires CI. A weekly schedule was rejected as well: most weeks it would spend 15 minutes on Windows to find nothing new. A job inside `release.yml` was rejected because a failed screenshot would turn a published release red.
