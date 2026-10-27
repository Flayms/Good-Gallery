# Progress

The current milestone is the first row that isn't `Done`. Statuses: `Not started` → `Planning` (plan drafted, waiting for approval) → `In progress` → `Done`. `Blocked` needs a decision by the user (say which in the log).

| Milestone | Title                                  | Status      | Plan                                    |
| --------- | -------------------------------------- | ----------- | --------------------------------------- |
| M0a       | Spikes: grid and look                  | Done        | [M00](rewrite/milestones/M00-spikes.md) |
| M0b       | Spikes: video                          | Done        | [M00](rewrite/milestones/M00-spikes.md) |
| M1        | Foundation                             | Done        | [M01](rewrite/milestones/M01-foundation.md) |
| M2        | Spec audit                             | Done        | [M02](rewrite/milestones/M02-spec-audit.md) |
| M3        | Database and libraries                 | Done        | [M03](rewrite/milestones/M03-database-libraries.md) |
| M4a       | Indexing: metadata                     | Done        | [M04](rewrite/milestones/M04-indexing.md) |
| M4b       | Indexing: scanning                     | Done        | [M04](rewrite/milestones/M04-indexing.md) |
| M4c       | Indexing: change detection, oracle     | Done        | [M04](rewrite/milestones/M04-indexing.md) |
| M5        | Thumbnails                             | Done        | [M05](rewrite/milestones/M05-thumbnails.md) |
| M6a       | Gallery: tiles                         | Done        | [M06](rewrite/milestones/M06-gallery.md) |
| M6b       | Gallery: views and toolbar             | Not started | [M06](rewrite/milestones/M06-gallery.md) |
| M7        | Search and sidebar                     | Not started | —                                       |
| M8        | Photo viewer and filmstrip             | Not started | —                                       |
| M9        | Video                                  | Not started | —                                       |
| M10       | Settings, design polish, glass         | Not started | —                                       |
| M11       | Packaging and release                  | Not started | —                                       |

## Log

Newest first. One entry per session: date, milestone, what was done, decisions (lasting ones also in `docs/architecture.md`), open issues, next step.

### 2026-10-27 · M6a · Gallery: tiles

- Plan written and self-approved (`docs/rewrite/milestones/M06-gallery.md`), split into M6a (tiles) and M6b (views, toolbar).
- Done, tests seen red then green: tasks 1–7. `domain::{layout, date_groups, zoom}`, `db::gallery` (layout + items with tag categories), `db::settings::{value, set_value}`, `app::textures` (spike streamer, LRU budget), `app::gallery` (loader thread, Masonry tiles, ThumbHash placeholders, fade-in, video badge, failed icon, Ctrl + wheel zoom stored as `gallery.columns`, scroll anchoring, live reload, skeleton and error states), `crates/app/tests/gallery.rs` + snapshots `gallery`, `gallery_loading`. Checked in the app: fixture library, on-demand rendering, and a copy of the real index with the Electron cache.
- Frame-time budget (REQ-006-12): `cargo xtask gallery-bench` builds a data folder with 100,000 items whose thumbnails are hard links into a cache copy and runs the app with `--bench-scroll` (vsync off, a warming pass, then 20 s at 1,440 pt/s, 12 columns). Result on mains power: p50 3.0 ms, p95 4.4 ms, p99 5.3 ms, max 11.9 ms. The first run showed textures at 324 MB, over the planned 256 MB budget: at 12 columns the visible tiles and the prefetch band alone take ≈ 330 MB, so the budget is 512 MB (as in M0); the second run peaked at 511 MB.
- Specs: 006 REQ-006-02, -07, -08, -09, -13 pass, -03, -04, -06, -10, -11 partial (view choice, slider, List view in M6b; tag suggestions M7), -12 measured; 016 -01, -04 … -06 partial (headers in M6b); 017 -01 partial; 005 -06, -14 tiles checked in the app (no automated tile test without the GPU streamer).
- Code review fixes: chunks forgotten with the remembered items (or loaded from a stale layout during a reload) were never asked for again; a reload laid out twice and snapped the view to the old position, so the anchor is now the media item (test: items added above stay out of the way); the gallery `AppState::new` builds now loads lazily, so the one `main` replaces never queries; a window resize lays out once the width settles (120 ms); loaded textures keep their LRU protection when a larger width is missing; missing thumbnails are looked for again after a reload; a usage string gap.
- Decisions in `docs/architecture.md` (gallery data and textures); pitfalls (egui Ctrl + wheel, per-frame layout clone, hard links).
- Open: (1) the Settings button sits in the top-right corner until the toolbar (M6b). (2) Thumbnail rendering still runs at normal priority; the bench ran with all thumbnails cached, so rendering while scrolling isn't measured.
- Next: M6b (toolbar with the View menu, List view, date group headers, tag indicators).

### 2026-10-27 · M5 · Thumbnails

- Plan written and self-approved (`docs/rewrite/milestones/M05-thumbnails.md`), one part.
- `domain::thumbs` (widths, `pick_thumb_width`, `ThumbKey`), `media::thumbnail` (decode with JPEG shrink-on-load and EXIF orientation, Lanczos3 resize into 800 × 2400, WebP q80, ThumbHash, embedded previews through exiftool), `media::ffmpeg` (video frames at 10 %, AVIF), `db::thumbs`, `indexer::thumb_cache` (the Electron version's layout, atomic writes, LRU eviction, view clock), `indexer::thumbnails::ThumbnailService` (priority queue, background batches, failed vs offline, full cache). The indexer kicks it after writes and scans; the app starts it with the data folder's `thumbs` and the cache limit.
- Spec 005 `Implemented`: all REQs pass except REQ-005-06 and -14, partial until the tiles exist (M6). REQ-010-05 / -06 partial (cache side). REQ-005-01 corrected (800 × 2400 box, no 3:1 crop).
- Decisions in `docs/architecture.md`: cache layout kept (Defender question: revisit in M6 with cold-scroll measurements), rendering stack, AVIF through ffmpeg, ICC profiles not applied (deviation).
- Oracle extended with thumbnails: 11 fixtures, same sizes, mean ≤ 2.72 levels. `cargo xtask thumbs` finds the real cache's thumbnails exactly as Node's hashing does (17,618 of 20,295).
- Found under full-suite load: replacing a thumbnail failed with "Access is denied" while the file was held open (Defender); `ThumbCache::write` now retries the rename (test held the file open: red, then green).
- Code review fixes: the cache's size count lost writes made during eviction or the startup walk and counted replaced files twice; every write over the cap started another eviction walk; a render thread waited for the cache measurement (with one thread, requests waited too); queued items of a library just found offline were still tried; a request arriving mid-render into a full cache got no files; large PNG / WebP hit the `image` crate's 512 MiB limit (now sharp's 268 Mpx); the variable `NOT IN` query evicted cached statements; the library status update is shared with the writer. Not tested: the decode limit (needs a > 512 MiB image) and the late-request window (needs a hook between two lines).
- Open: (1) thumbnails render at normal thread priority; M6 measures whether background rendering costs frame time. (2) ffmpeg must be bundled (M11); without it videos get no thumbnails (error toast). (3) Settings for cache size / usage / clearing come in M10.
- Next: M6 (Gallery): texture streaming reads through `ThumbCache::read` and requests missing thumbnails from the service; measure cold scrolling with the real cache.

### 2026-10-27 · M4c · Indexing: change detection, oracle

- Picked up an interrupted session: the watcher and schedule (committed with M4b) were half wired in. Now finished: the indexer starts a watcher after a completed scan and stops it when the library goes offline or is removed, and reports become changes or quick scans. The app runs the schedule (quick scans at startup and every interval, offline libraries every minute, full scans for outdated rows).
- Tests for REQ-003-05 (5,000-path limit when queueing), -06 (failed watch), -07, -10 (watched again after a reconnect). Spec 003 `Implemented`, 004 `Implemented` (REQ-004-03 partial: GPS zone deviation), REQ-001-05 and REQ-002-05 pass.
- Spec fix: REQ-003-06 no longer queues a scan when watching can't start (it would follow every completed scan); the schedule retries.
- `cargo xtask oracle`: fixture library (16 media items, 19 tags: offsets, sub-seconds, hierarchies, both face-region formats, orientations, both sidecar namings, GPS, Unicode and whitespace tags, a broken file, a video, housekeeping files), indexed by the reference (bundled with its vite, run with Node) and by this indexer, then diffed. 5 documented differences (face regions, GPS zone), 0 unexpected.
- Bug found while checking the app: exiftool outlived the app (two processes per pool slot). The app now puts itself into a kill-on-close job object (`media::helpers`, the first `unsafe`); the test saw the leak fail, then pass.
- Checked in the running app: a new data folder with the fixture library, where only the startup schedule indexes it (16 items, online); no exiftool left behind.
- Code review fixes: the scheduler's read-only connection opened before the index was created, so a fresh data folder couldn't start (checked in the app afterwards); a library removed while its scan completed could be watched again; watchers are no longer dropped under the state lock or started under the watcher lock; the job object allows breakaway for programs opened for the user; a REQ-003-07 test no longer sleeps; `--screenshot-after` rejects bad numbers.
- Not changed after review: watching starts after the first completed scan, so changes during a library's first scan in folders already walked wait for the next quick scan (as in the reference; REQ-003-07 is about watched libraries). `Scheduler::set_interval` has no caller yet because the setting has no UI; wire it in M10 (REQ-010-03).
- Open: (1) exiftool processes left over from this session's runs before the fix (PIDs started 08:35–09:47) couldn't be killed from the agent shell (blocked); end them in Task Manager. (2) REQ-002-11 still has no test. (3) Oracle needs Node, the reference's `node_modules` and ffmpeg; not in CI.
- Next: M5 (Thumbnails), starting with its plan and the open thumbnail cache layout decision.

### 2026-10-26 · M4b · Indexing: scanning

- `indexer::walk` + `retry`, `indexer::scan::scan_root` (full, quick and changes scans on worker threads, batched writes, removals, offline handling), `indexer::Indexer` (scan queue with merging, one scan thread, `catch_unwind` per scan and per file). The reference's scan tests are ported and pass; an end-to-end test indexes a PNG with its sidecar through the real exiftool and checks that no file changed.
- App: two connections (UI, indexer), indexer started with exiftool from `PATH` / `GOOD_GALLERY_EXIFTOOL` (an error toast if missing), library operations queue real scans, scan progress in the bottom-left corner, counts refreshed after the first written files, every 5 s, and at the end.
- M4a review fixes went into the M4a commit: exiftool-vendored's real arguments (`-fast`, `struct=1`, `keepUTCTime`, UTF-8 filter), face regions from the region structs (the reference misses them, REQ-004-09 wins), parallel sidecar reads, DST gap, JavaScript whitespace in tag names.
- check-specs: chained REQ prefixes in test names (`req_002_01_req_002_02_…`).
- Open: REQ-002-11 has no test (a folder that can't be listed is hard to make on Windows without admin rights); no startup scans yet (M4c).
- Next: M4c (watcher, changes scans, schedule, oracle).

### 2026-10-26 · M4a · Indexing: metadata

- M4 plan written (self-approved) and split into M4a (metadata), M4b (scanning), M4c (change detection, oracle).
- `domain`: media kinds, housekeeping folders, sidecar lookup, tag normalization, `MediaMetadata`. `media::exiftool`: process pool with exiftool-vendored's arguments. `media::metadata`: the reference's extraction with its test cases, dates with exiftool-vendored's zone inference (without geo-tz). `db::writer`: port of `IndexWriter`.
- Deviation recorded: no GPS-based time zones (`docs/architecture.md`).
- REQ-004-02, -04 … -09, -11 pass; REQ-004-03, -10, REQ-002-01, -02, -04, -14, REQ-011-10 partial until scanning exists; REQ-011-09 pass.
- Next: M4b (scanning).

### 2026-10-26 · M3 · Database and libraries

- `db`: the reference's migrations byte for byte, drizzle-compatible bookkeeping, pragmas, reader connections, library rows, settings storage. `reference.db` fixture made by the reference's own migrator proves compatibility both ways (REQ-001-08).
- `indexer::libraries`: add (absolute, normalized, reachable within 5 s, duplicate check, label), remove, rescan behind a `ScanRequests` trait (no-op until M4). `cargo xtask libraries --db <file> list|add|remove|rescan`.
- `app`: data folder `%APPDATA%good-gallery-rust` (`--data-dir`), Settings page with the *Libraries* section (add, browse, list, rescan, remove with confirmation, toasts), gallery empty states, lucide icons as SVG.
- A copy of the real index lists its two libraries (CLI) and the app opens it (20,383 media items).
- Spec 001 `Implemented` (REQ-001-05 partial until the scan queue exists); REQ-010-01 / -02 partial (storage), REQ-006-10 partial (two of four states).
- Decisions in `docs/architecture.md`: migrations and compatibility, data folder, icons, UI state and jobs. Pitfalls: parallel wgpu tests crash (serialized), copying from the reference is blocked.
- Next: M4 (Indexing, change detection, metadata).

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
