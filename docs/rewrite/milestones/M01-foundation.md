# M01 · Foundation

**Status:** Done (approved 2026-10-26, self-approved: the user asked to work through the milestones without their input)
**Specs:** 012, 014 (REQ-014-01 only)

## Approach

**Workspace.** One Cargo workspace with `crates/{domain,db,media,indexer,app}` and `xtask/`, `resolver = "3"`, edition 2024. Shared versions and lints live in `[workspace.dependencies]` / `[workspace.lints]`; crates inherit them. `egui`, `eframe` and `egui_kittest` are pinned with `=0.37.0` (the spike's versions); `egui-wgpu` and `wgpu` come in through eframe at matching versions. `cargo xtask …` is an alias in `.cargo/config.toml`. The spikes stay outside the workspace (`exclude = ["spikes"]`).

**REQ reference convention (settles the open point of spec 012).** A test references a REQ either at the start of its name, `fn req_012_01_reports_unknown_reqs()`, or in a doc comment directly above it, `/// REQ-012-01, REQ-012-03: …`. `check-specs` reads `.rs` files below `crates/` and `xtask/`. Rust keeps unit tests in the file they test, so only the *test part* of a file counts: everything from the first `#[cfg(test)]` on, or the whole file below a `tests/` or `benches/` folder. Both spellings (`REQ-012-01` and `req_012_01`) count as a reference. Mentions of a REQ in production code (a doc comment explaining why some code exists) are therefore allowed and don't count as test coverage. As in the reference, the check's own tests build REQ IDs at runtime so they aren't counted as references.

**check-specs** is a port of `scripts/check-specs.ts`: the same parsing (headings `### REQ-…`, `### ~~REQ-…`, `**Status:**`, verification table rows), the same messages, the same exit code, `--summary`. Pure `check_specs(specs, tests) -> Report` plus `run(root, args, log) -> i32`, so both are unit-tested without a process. Regexes via the `regex` crate.

**App.** `cargo run -p app` opens an eframe window (wgpu) with the tokens from `docs/design.md` in a `theme` module (colors, type scale, radii, spacing, motion durations), Atkinson Hyperlegible Next (400 / 600 plus the `tnum` copy from the spike) bundled with `include_bytes!`, and the empty state "No libraries yet" with an *Add a library* button (it does nothing until M3). Native window decorations for now; the custom title bar from the spike comes with the toolbar (M6) and its polish in M10. The UI is built by a pure `fn ui(&mut egui::Ui, &mut AppState)`, so `egui_kittest` drives the same code as the window.

**Visual checks.** `cargo run -p app -- --screenshot <file.png>` renders a few frames, saves a screenshot of the window and exits (via `ctx.request_screenshot`; a failed save makes the process exit with an error). It works without anyone looking at the screen, and the next sessions use it for every UI change (written down in `docs/known-pitfalls.md`). Snapshot tests use `egui_kittest` with the `wgpu` and `snapshot` features; snapshots live in `crates/app/tests/snapshots/`, and `UPDATE_SNAPSHOTS=1 cargo test` rewrites them.

**CI.** `.github/workflows/ci.yml` on `windows-latest`: fmt, clippy, test, check-specs, with `Swatinem/rust-cache`. The repository has no remote yet (the user decided against pushing for now), so the workflow can't run in this milestone. The exit criterion "CI green" is replaced by running the same commands locally; REQ-014-01 stays `no test` until it has run on GitHub.

**Decisions** (recorded in `docs/architecture.md`):

- *Indexer as a thread, not a child process.* exiftool and ffmpeg already run as separate processes, and the in-process decoders (libwebp, `image`) are mature. A thread avoids an IPC layer and a second process to supervise. Each indexer job runs under `catch_unwind`, so a panic fails one job, not the app. Revisit if a C decoder crashes in practice: the indexer API is message-based (commands in, events out over channels), so it can move behind a pipe later without changing callers.
- *No async runtime.* Work is CPU- or disk-bound with few concurrent tasks: OS threads with `crossbeam-channel`, a small worker pool where needed. No tokio.
- *Errors:* `thiserror` enums in library crates (`domain`, `db`, `media`, `indexer`), `anyhow` in `app` and `xtask`.
- *Logging:* `tracing` in all crates; `app` installs `tracing-subscriber` with `RUST_LOG`-style filtering (default `info`) to stderr. A log file comes with packaging (M11).

## Reference

- `scripts/check-specs.ts` and `check-specs.test.ts`: ported line by line; test cases ported one to one.
- `.github/workflows/ci.yml`: jobs and triggers (push on all branches, pull requests, `workflow_call` for the release workflow), adapted to cargo.
- `src/renderer/src/components/gallery.tsx`: empty-state texts ("No libraries yet", "Add a library").
- `spikes/grid/src/look.rs` (this repo): font setup and tokens.

## Tasks

- [x] 1. Workspace skeleton: root `Cargo.toml`, five crates and `xtask`, `.cargo/config.toml` alias; `cargo build --workspace` passes.
- [x] 2. `xtask check-specs`: port parser and checks test-first (REQ-012-01 … REQ-012-06, REQ-012-08).
- [x] 3. `xtask check-specs`: `run` over a repository folder with the Rust test-region rule, exit status, `--summary` (REQ-012-07, REQ-012-08).
- [x] 4. Spec 012 assumptions updated to the Rust convention; spec status `Implemented`, Verification table filled; `cargo xtask check-specs` passes on this repo.
- [x] 5. `app`: theme module (tokens, fonts), window, empty state; `cargo run -p app` opens it.
- [x] 6. `egui_kittest` harness with one snapshot test of the empty state.
- [x] 7. `--screenshot` flag; documented in `docs/known-pitfalls.md`.
- [x] 8. CI workflow.
- [x] 9. Decisions in `docs/architecture.md`, progress log.

## Exit criteria

From `docs/rewrite/plan.md`: CI green; `cargo run -p app` opens a themed window; a snapshot test passes; decisions recorded.

Adjusted here: CI can't run without a remote, so "CI green" means the workflow's commands pass locally (`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask check-specs`).
