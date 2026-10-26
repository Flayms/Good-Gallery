# Progress

The current milestone is the first row that isn't `Done`. Statuses: `Not started` → `Planning` (plan drafted, waiting for approval) → `In progress` → `Done`. `Blocked` needs a decision by the user (say which in the log).

| Milestone | Title                                  | Status      | Plan                                    |
| --------- | -------------------------------------- | ----------- | --------------------------------------- |
| M0a       | Spikes: grid and look                  | Done        | [M00](rewrite/milestones/M00-spikes.md) |
| M0b       | Spikes: video                          | Done        | [M00](rewrite/milestones/M00-spikes.md) |
| M1        | Foundation                             | Done        | [M01](rewrite/milestones/M01-foundation.md) |
| M2        | Spec audit                             | Done        | [M02](rewrite/milestones/M02-spec-audit.md) |
| M3        | Database and libraries                 | Not started | —                                       |
| M4        | Indexing, change detection, metadata   | Not started | —                                       |
| M5        | Thumbnails                             | Not started | —                                       |
| M6        | Gallery                                | Not started | —                                       |
| M7        | Search and sidebar                     | Not started | —                                       |
| M8        | Photo viewer and filmstrip             | Not started | —                                       |
| M9        | Video                                  | Not started | —                                       |
| M10       | Settings, design polish, glass         | Not started | —                                       |
| M11       | Packaging and release                  | Not started | —                                       |

## Log

Newest first. One entry per session: date, milestone, what was done, decisions (lasting ones also in `docs/architecture.md`), open issues, next step.

### 2026-10-26 · M2 · Spec audit

- Specs audited (plan: `docs/rewrite/milestones/M02-spec-audit.md`, self-approved). 011 rewritten: REQ-011-01…07 deprecated (no web page, IPC, protocols), new REQ-011-08 (files only from the index), -09 (helpers can't be steered by file names), -10 (hostile metadata), -11 (no network).
- New REQs: REQ-001-08 (existing index stays compatible both ways), REQ-005-15 (existing thumbnail cache is used), REQ-006-12 (frame-time budget), REQ-006-13 (thumbnail memory budget). Reworded: REQ-002-13 (indexer failure instead of process crash), REQ-007-13 (search kept while viewing, no URL), REQ-009-02 / -03 / -12 (mouse back button, no viewer URL), REQ-014-01 / -07 (Rust checks, `Cargo.toml`), REQ-015-04.
- `docs/design.md` in egui terms (tokens in `theme`, own glass pass, Windows settings for the fallbacks); glossary (*Search*, *Indexer*).
- Doc-only milestone: no code review run (nothing to review beyond wording); `cargo xtask check-specs` clean.
- Next: M3 (Database and libraries).

### 2026-10-26 · M1 · Foundation

- The user asked to work through all milestones without their input and to commit after each one (no remote, no PRs). Milestone plans are self-approved from here on; the commit is the review point.
- Cargo workspace with `crates/{domain,db,media,indexer,app}` and `xtask`; `cargo xtask check-specs` ported from the reference with its tests (spec 012 `Implemented`, 8/8 pass). REQ convention settled: `fn req_012_01_…` or `/// REQ-012-01: …`, only the test part of a file counts.
- `cargo run -p app` opens a themed window (tokens and font in `app::theme`, first-run empty state); `egui_kittest` snapshot test `first_run`; `--screenshot <file.png>` for visual checks.
- CI workflow written (`.github/workflows/ci.yml`, Windows, fmt / clippy / test / check-specs) but not run: the repo has no remote. REQ-014-01 stays `no test`.
- Decisions in `docs/architecture.md`: indexer as a thread with message API, no async runtime, thiserror / anyhow, tracing, REQ references, visual checks.
- Open: (1) CI has never run; push to GitHub to check it (snapshot tests on WARP may need a threshold); (2) custom title bar from the spike not ported yet (comes with the toolbar in M6, polish in M10).
- Next: M2 (Spec audit).

### 2026-10-26 · M0b · Spikes: video

- Fixtures generated with ffmpeg 7.1 into `%TEMP%\gg-spike\video` (`spikes/video/fixtures.sh`): MP4 H.264, MOV HEVC (1080p, 4K, rotated 90°), MKV H.264 + Opus with chapters, WebM VP9 + Opus, plus 1 s GOP variants; 30 s with timecode, frame number and a flash + beep every second.
- **Video: Go with libmpv.** `spikes/video`: own FFI over `libmpv-2.dll` (shinchiro build 20261021, loaded at runtime), software render API on a render thread at display size, written into a `wgpu::Texture`, display-synced timing. All fixtures play; 0–4 mpv drops per 600 frames, 1–40 off-cadence frames, exact seeks on target; 1080p ≈ 1 core, 4K ≈ 2 cores (2.9 / 5.7 ms render per frame). Mains-power rerun after the first matrix was skewed by the battery.
- Decisions in `docs/architecture.md` (video playback); pitfalls added (libmpv render-thread rules, `rgb0` alpha, rotation, judder with audio-clock timing, `vulkan-1.dll` import, power source, Python).
- Open: (1) manual check by the user: controls by mouse and audio by ear (`cd spikes/video && cargo run --release -- --sync display`, fixtures in the combo box); (2) real phone videos not tried, none provided; (3) the UI misses a vblank in ≈ 12 % of frames while 4K plays on the iGPU, zero-copy (GL interop) is the lever if M9 needs it; (4) `libmpv-2.dll` is 121 MB and hard-imports `vulkan-1.dll`, a trimmed build in M11; (5) still open from M0a: snap layouts check, thumbnail cache layout (M5), fast flings (M6).
- Next: M1 (Foundation). M0 is done: three go results.

### 2026-10-26 · M0a · Spikes: grid and look

- M0 plan written and approved, split into M0a (grid + look, `spikes/grid`) and M0b (video). Thumbnail cache copied to `%TEMP%\gg-spike\thumbs` (66 096 files, 3.7 GB); the original wasn't opened.
- **Grid: Go.** p95 frame interval 7.2 ms at 12 columns over 100k items (7.3 ms at 250k), vsync off, integrated AMD GPU. Keys: worker-side texture creation, 4 low-priority decode threads, libwebp, own mipmapped textures, LRU that keeps the prefetch band.
- **Look: Go, approved by the user from screenshots.** Glass (dual Kawase in a paint callback) matches Chromium within 1–2 color levels at ≈ 0.4 ms per surface; Atkinson Hyperlegible Next with a baked `tnum` copy; title bar variant 3 (own buttons, native hit-test codes) works.
- Decisions in `docs/architecture.md`; pitfalls added (Defender first-read cost, vsync and p95, egui 0.37 API notes, …).
- Open: (1) manual check that snap layouts show when hovering the maximize button (`cargo run --release` in `spikes/grid`); (2) thumbnail cache layout vs Defender, to decide in M5; (3) fast flings at 12 columns outrun decoding, handle in M6.
- Next: M0b (video spike), `docs/rewrite/milestones/M00-spikes.md` tasks 10–13.

### 2026-10-26 · Setup

- Repo created next to the reference (`../good-gallery`). AGENTS.md, master plan, progress log, architecture stub, known pitfalls, glossary and design copies, all 17 specs ported (requirements unchanged, Verification tables reset, statuses `Approved`), `.claude/settings.json` (reference readable, not editable).
- Open: MSVC C++ build tools aren't installed yet, so nothing links (see `docs/known-pitfalls.md`). The user installs them before M0.
- Next: M0, write `docs/rewrite/milestones/M00-spikes.md`.
