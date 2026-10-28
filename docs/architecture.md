# Architecture

Target sketch from `docs/rewrite/plan.md`; this file becomes the source of truth as milestones settle it. Keep it describing the current state, not history.

```
crates/
  domain    pure types and logic: media, tags, search model, gallery layout, date groups, zoom math. No IO.
  db        rusqlite, migrations (SQL taken from the reference), queries incl. tag search
  media     thumbnail rendering, video frames (ffmpeg), exiftool process, thumbhash
  indexer   scan queue, walk, watcher, writer, reachability, rescan scheduling
  app       eframe binary: UI, texture streaming, viewer, settings
xtask/      check-specs, fixture library generator, release
```

## Decisions

Architectural decisions that outlive a milestone, newest last. One entry each: what was decided, why (with measurements where there are any), what was rejected, and the milestone plan with the details. Decisions that only matter within one milestone stay in that milestone's Approach section.

### 2026-10-26 · UI stack: eframe / egui 0.37 on wgpu 30 (M0a)

**Decided:** keep the planned stack, pinned to `eframe` / `egui` / `egui-wgpu` `=0.37.0`. The grid spike scrolls 100 000 (and 250 000) items at 12 columns with a p95 frame interval of 7.2 ms on an integrated AMD GPU (budget 16 ms; the Electron reference ≈ 110 ms). egui's own painter is enough; no custom tile pipeline is needed. Details: `docs/rewrite/milestones/M00-spikes.md`, *Results*.

### 2026-10-26 · Thumbnail textures (M0a)

**Decided:** thumbnails are our own `wgpu::Texture`s (`Rgba8Unorm`, CPU-built mip chain), decoded with libwebp and created and written on worker threads; the UI thread only registers them with the egui renderer, within a time budget per frame (3 ms). 4 decode workers at below-normal thread priority. LRU cache with a byte budget; textures in the prefetch band count as used. Requests are a priority list replaced every frame (visible, then 1.5 screens ahead, 0.5 behind), so stale work drops out.
**Why:** uploading on the UI thread cost 2.7 ms per texture; egui-managed textures have no mipmaps (aliasing at high zoom); more decode threads starved the UI thread and the iGPU; libwebp decodes 3× faster than the `image` crate.
**Rejected:** `ctx.load_texture` (p95 12.9 vs 7.2 ms, no mips), `image` crate WebP decoding.

### 2026-10-26 · Glass (M0a)

**Decided:** glass surfaces render the tiles below them into an offscreen target at half resolution, blur it with a dual-Kawase chain (4 passes) and composite saturate + tint + rounded corners in an `egui_wgpu` paint callback. Costs ≈ 0.4 ms per surface; matches Chromium's `backdrop-filter` within 1–2 color levels. Raised stays the fallback. Approved by the user from screenshots (2026-10-26).

### 2026-10-26 · Font and title bar (M0a)

**Decided:** Atkinson Hyperlegible Next as the variable TTF from google/fonts (OFL), weights via `FontTweak::coords`; a second copy with `tnum` baked in (`pyftfeatfreeze`) for numbers, since egui can't request OpenType features. Title bar: content runs into the caption, own caption buttons, and a window subclass answers `WM_NCHITTEST` with the native codes (`HTMAXBUTTON` etc.), so shadow, resize, Aero snap and snap layouts stay native (what Electron's `titleBarOverlay` does too).
**Rejected:** native caption (a second bar above the toolbar), undecorated window (no snap layouts, no top-edge resize).

### 2026-10-26 · Video playback: libmpv, software render API, display-synced (M0b)

**Decided:** video plays through libmpv (`libmpv-2.dll`, loaded at runtime with `libloading` through a small own FFI; `hwdec=auto-copy`). A render thread renders each frame with mpv's software render API (`rgb0`) at the size the video occupies on screen and writes it into a `wgpu::Texture` itself; the UI draws the texture as a quad, rotated by `video-params/rotate` (the software renderer ignores rotation). Timing is display-synced: `video-sync=display-resample` with the monitor's refresh rate, the UI wakes the render thread once per presented frame, which calls `report_swap` and skips frames mpv marks as repeats. The render thread never calls non-render libmpv functions; the UI hands it the video size. All four containers (MP4 H.264, MOV HEVC incl. 4K and rotation, MKV with chapters, WebM VP9 + Opus) play with 0–4 dropped frames per 600 and 1–40 off-cadence frames; 4K costs ≈ 2 cores and 5.7 ms render per frame. Ships as one 121 MB GPL DLL (hard-imports `vulkan-1.dll`); a trimmed build is an M11 task.
**Why:** mpv brings decoding, audio output, A/V sync, seeking and every container; the software render API works with wgpu as is. Display-synced timing removes the judder that audio-clock timing causes against our vsync (45–265 of 600 frames).
**Rejected:** `libmpv2` crate (only the OpenGL render API), audio-clock timing (judder), rendering at the video's own size (4K: ≈ 19 ms CPU per frame, judder), GL interop (not needed; would need a WGL context beside wgpu), `ffmpeg-next` + `cpal` and GStreamer (not needed). Details: `docs/rewrite/milestones/M00-spikes.md`, *Spike 3*.

### 2026-10-26 · Workspace and crate layout (M1)

**Decided:** one Cargo workspace (`resolver = "3"`, edition 2024) with `crates/{domain,db,media,indexer,app}` and `xtask/` as sketched above; shared versions and lints in `[workspace.dependencies]` / `[workspace.lints]`. `app` is a library (`app::ui`, `AppState`, `theme`) plus a thin `main.rs`, so `egui_kittest` tests drive the same UI code as the window. `cargo xtask <task>` is a Cargo alias. Spikes stay outside the workspace. Dependencies are optimized in dev builds (`[profile.dev.package."*"] opt-level = 3`): decoding and layout are too slow unoptimized to judge anything.

### 2026-10-26 · Indexer runs as a thread (M1)

**Decided:** the indexer runs on threads inside the app process, not as a child process. Its API is message-based (commands in, events out over `crossbeam-channel`), and every job runs under `catch_unwind`, so a panic fails that job instead of the app.
**Why:** exiftool and ffmpeg already run as separate processes; the in-process decoders (libwebp, the `image` crate) are mature. A child process would need an IPC layer, a second process to supervise and a second copy of the decoders in memory.
**Revisit if** a C decoder crashes in practice: because the API is message-based, it can move behind a pipe to `app --indexer` without changing callers.

### 2026-10-26 · No async runtime, errors, logging (M1)

**Decided:**
- *Concurrency:* OS threads and `crossbeam-channel`, small worker pools where needed. No tokio: the work is CPU- or disk-bound, with few concurrent tasks and no network protocol.
- *Errors:* `thiserror` enums in library crates (`domain`, `db`, `media`, `indexer`), `anyhow` in `app` and `xtask`.
- *Logging:* `tracing` everywhere; `app` installs `tracing-subscriber` with `RUST_LOG`-style filtering (default `info`) on stderr. A log file in the user data folder comes with packaging (M11).

### 2026-10-26 · REQ references in Rust tests (M1)

**Decided:** a test references a REQ at the start of its name (`fn req_012_01_reports_unknown_reqs()`, several chained: `fn req_002_01_req_002_02_…()`) or in a doc comment directly above it (`/// REQ-012-01: …`). `cargo xtask check-specs` reads `.rs` files below `crates/` and `xtask/` and counts only their test part: from the first `#[cfg(test)]` on, or the whole file below `tests/` or `benches/`. REQs mentioned in production code don't count as coverage. Spec: 012.

### 2026-10-26 · Visual checks (M1)

**Decided:** UI changes are checked two ways: `egui_kittest` snapshot tests (wgpu renderer, PNGs in `crates/app/tests/snapshots/`, `UPDATE_SNAPSHOTS=1` rewrites them) and `cargo run -p app -- --screenshot <file.png>`, which renders a few frames, saves the window and exits, so a session can look at the real app without anyone at the screen.

### 2026-10-26 · Index compatibility and migrations (M3)

**Decided:** `crates/db/migrations/` holds the reference's `drizzle/*.sql` and `meta/_journal.json` byte for byte (`.gitattributes`: `-text`). `db::open` migrates like drizzle's SQLite migrator: `__drizzle_migrations` created with drizzle's exact DDL text, newest `created_at` read, newer journal entries run chunk by chunk in one transaction, `(sha256 of the file, journal when)` inserted. Tests prove both directions against `crates/db/tests/fixtures/reference.db`, which the reference's own migrator creates (`make-reference-db.mjs`): the fixture opens here, and a database created here has the same schema and bookkeeping rows. A schema change therefore lands in the reference's migration folder format; new migrations must keep the journal order and `when` values increasing.
**Connections:** one write connection (pragmas as in the reference: WAL, `synchronous = NORMAL`, foreign keys, `busy_timeout = 5000`), read-only connections for readers.

### 2026-10-26 · Data folder during the rewrite (M3)

**Decided:** the app uses `%APPDATA%good-gallery-rust` (override `--data-dir`), not the Electron version's `%APPDATA%good-gallery`, so development runs never touch the user's real index. M11 switches to the shared folder (REQ-001-08 makes that safe).

### 2026-10-26 · Icons (M3)

**Decided:** lucide icons as SVG files, generated from the reference's `lucide-react` by `crates/app/assets/icons/make-icons.mjs` (white stroke 1.75), embedded with `include_bytes!` (`app::icons::Icon`) and rendered by `egui_extras`' SVG loader (resvg) at the size they are drawn, tinted per use.
**Rejected:** an icon font (lucide's font drops the stroke-width control), hand-drawn shapes (the spike's approach; doesn't scale to ~40 icons).

### 2026-10-26 · UI state and background jobs (M3)

**Decided:** `app::AppState` holds what the UI shows plus the write connection (until the indexer owns it in M4); slow work (reachability, folder dialog) runs on threads and sends an event back over a channel, applied at the start of the next frame (`AppState::poll`). Tests call `AppState::wait_for_jobs` before stepping the harness, so they never race the jobs. OS dialogs sit behind a trait (`FolderPicker`) so tests use a fake.

### 2026-10-26 · Metadata through exiftool, read like exiftool-vendored (M4a)

**Decided:** `media::exiftool` keeps a pool of `exiftool -stay_open True -@ -` processes (size = parallel file reads) and reads each file with the arguments exiftool-vendored v38 builds from its default options (`-charset filename=utf8 -json -fast`, its UTF-8 repair filter, `-api struct=1 -use MWG -api keepUTCTime`, numeric `-*Duration*# … -Orientation#`, `-all <file> -ignoreMinorErrors`), so both apps get the same tag values; values the filter repaired are unwrapped as exiftool-vendored does. Arguments go over stdin, one per line, no shell; paths are always absolute, so no file name is taken as an option. A read over 60 s replaces its process. Executable: `GOOD_GALLERY_EXIFTOOL`, `exiftoolexiftool.exe` next to the app, then `PATH` (bundled in M11; the reference bundles 13.59). Extraction (`media::metadata`) is a port of the reference's rules on the JSON. `METADATA_VERSION` stays the reference's value (1) as long as the rules are the same.
**Deviation:** exiftool-vendored infers a photo's zone from its GPS position (geo-tz, a large time-zone shape database) when nothing else names one. Not ported: such photos (GPS, no offset tags, no zoned capture date, no GPS timestamp) get the computer's local zone. Everything else of its zone inference is ported (offset tags incl. Nikon daylight saving, offsets of other capture dates, UTC for videos, offset to the GPS time). Revisit if the oracle shows it matters.
**Deviation (reference bug):** with `struct=1` exiftool returns face regions as structs (`RegionInfo.RegionList[]`, `RegionInfoMP.Regions[]`); the reference only reads the flattened `RegionName` / `RegionType` / `RegionPersonDisplayName`, which it never gets, so it misses MWG and Microsoft face regions. REQ-004-09 asks for them, so this port reads the structs too: files with such regions get `People|<name>` tags the reference didn't write.
**Tests that need exiftool** skip themselves when it isn't found, unless `GG_REQUIRE_HELPERS=1` (CI).

### 2026-10-26 · Pure media and tag rules in `domain` (M4a)

**Decided:** file kinds, housekeeping folders, sidecar lookup, tag normalization, `MediaMetadata` and `METADATA_VERSION` live in `domain`; `db` depends on `domain` for the writer, `media` for extraction. `db::writer::IndexWriter` owns the write connection (the indexer thread from M4b on).

### 2026-10-26 · Scans on threads (M4b)

**Decided:** `indexer::scan::scan_root` is the reference's algorithm with threads instead of promises: the calling thread walks the library and owns all index access (lookups, batched writes, removals at the end), `io_concurrency` scoped worker threads stat files and read metadata, connected by a bounded channel (backpressure) and a result channel. Cancelling (library removed) and "offline" are atomic flags. A panic while reading one file costs that file (`catch_unwind` per job); a panic anywhere else ends the scan as `Failed` (`catch_unwind` around the scan), and the next scan runs (REQ-002-13).
**Indexer:** `indexer::Indexer` holds the scan queue behind a mutex (commands are method calls from any thread, merged as the reference's `IndexerController` does) and one thread with the write connection that runs the scans; it reports `IndexerEvent`s over a channel and wakes the UI through a callback. It implements `ScanRequests`, so library operations queue real scans. The app keeps its own connection for library rows; SQLite's busy timeout serializes the two writers.

### 2026-10-27 · Change detection and schedule (M4c)

**Decided:** the indexer owns the watchers (`indexer::watcher::LibraryWatcher`, `notify` recursive, 2 s debounce, 10 s at most). A completed scan starts one for its library, an offline result or a removed library stops it. Reports become changes scans, or quick scans for unknown changes and more than 5,000 paths. When a watcher fails, it is dropped and a quick scan is queued. A watcher that can't start queues nothing; the next scheduled quick scan tries again (spec REQ-003-06 was corrected, since otherwise every completed scan would queue another). `indexer::schedule::Scheduler` is its own thread with a read connection: quick scans at startup and every quick check interval (full scans for libraries with rows behind `METADATA_VERSION`), offline libraries every minute. Queued scans merge per library, so there's never more than one reachability check per library in flight.

### 2026-10-27 · Helper processes end with the app (M4c)

**Decided:** at startup the app puts itself into a Windows job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (`media::helpers::end_helpers_with_app`). Every process it starts (exiftool's launcher and its child, later ffmpeg) joins the job and ends with the app, however it exits. Without the job, the app's exit (the indexer gets 2 s to stop) cut `Process::drop` short, and even a finished `taskkill /T /F` left the PAR-packed exiftool running. The job allows breakaway: programs opened for the user (Explorer, an editor, later milestones) must be started with `CREATE_BREAKAWAY_FROM_JOB`, or closing the app closes them too. This is the workspace's first `unsafe` (allowed in that one function).

### 2026-10-27 · Oracle (M4c)

**Decided:** `cargo xtask oracle` generates a fixture library (`xtask/src/oracle.rs`: images written with `image`, tags with exiftool through an argument file, a clip with ffmpeg) and indexes it twice. The reference runs its own `scanRoot` + `IndexWriter` + exiftool source (`xtask/oracle/reference-index.ts`, bundled with the reference's vite by `build-reference.mjs`, run with Node and `NODE_PATH` pointing to the reference's `node_modules`; ESM-only packages and their dependencies are bundled). This indexer runs `scan_root`. The tables are compared by path and tag name instead of ids, without scan times. Documented deviations are listed separately: face regions (the reference misses them) and the GPS time zone. A deviation that doesn't show up is reported but doesn't fail the run, since the GPS zone matches on a computer in Tokyo's zone. Result on 2026-10-27: 16 media items and 19 tags, 5 documented and 0 unexpected differences.

### 2026-10-27 · Thumbnail cache: the Electron version's layout (M5)

**Decided:** the cache stays one WebP file per thumbnail and width, `<data>/thumbs/<h0h1>/<h2h3>/<sha1>.webp` with `sha1(JSON.stringify([rootId, relPath, size, mtime, width]))` (`indexer::thumb_cache::thumb_path`; `serde_json` writes integers and strings like `JSON.stringify`, pinned by a test against Node). Writes are atomic (temp file + rename), `*.tmp` leftovers are removed at startup, eviction is LRU by file mtime down to 90 % of the limit, reading bumps the mtime at most once an hour. On a copy of the real index and cache, both apps find the same 17,618 of 20,295 400-px thumbnails (`cargo xtask thumbs`; the rest were evicted).
**Why:** the user's 66k cached thumbnails keep working (REQ-005-15) and both apps can share one cache. Defender's first-read scan (≈ 13 ms per file, see the M0 pitfall) only costs while scrolling fast through thumbnails not read for minutes.
**Rejected (for now):** a pack file (≈ 100× faster cold reads, but a new format, a migration of the existing cache and no sharing with the Electron version). **Revisit in M6** if cold scrolling at 12 columns misses its budget with the real cache.

### 2026-10-27 · Thumbnail rendering (M5)

**Decided:** `media::thumbnail::Renderer` follows the reference's `renderMedia`: one decode per file, upright by its EXIF orientation, fit into 800 × 2400 without enlarging (Lanczos3, `fast_image_resize`), each width from that, WebP quality 80 through libwebp (`webp` crate), ThumbHash at ≤ 100 px (`thumbhash`, its author's Rust port). JPEGs decode through `jpeg-decoder` with DCT scaling (shrink-on-load, as libvips does for sharp); CMYK JPEGs, PNG, GIF and WebP through `image`. Embedded previews (exiftool `PreviewImage`) replace large camera JPEGs when they are large enough and have the same shape. Video frames and **AVIF** come from ffmpeg (`media::ffmpeg`, found like exiftool: `GOOD_GALLERY_FFMPEG`, `ffmpeg\ffmpeg.exe` next to the app, `PATH`), since `image` decodes AVIF only through dav1d, a C library with a meson build. `cargo xtask oracle` compares both renderers' output within a pixel tolerance.
**Deviation:** sharp converts to sRGB through embedded ICC profiles; this port ignores ICC profiles, so wide-gamut photos may come out slightly less saturated. Revisit if real photos show it.
**Service:** `indexer::thumbnails::ThumbnailService`, the reference's `ThumbnailService` on threads: `io_concurrency` render threads, one priority queue (requests newest first above background jobs), background batches of 100 pending items whenever the queue is shorter than the number of threads, its own write connection. The indexer calls `kick` after every written batch and every scan (`IndexerDeps::on_written`); the app kicks once at start. Requests are answered through a callback (`ThumbnailReady`), which M6's texture loader uses. Each render runs under `catch_unwind`.

### 2026-10-27 · Gallery: layout up front, data and textures on demand (M6a)

**Decided:** `db::gallery::layout` returns every match's id and clamped aspect in sort order (and the day groups of a date sort); `domain::layout::tile_layout` positions all tiles at once (f64, a 500k-item gallery is taller than f32 places to the pixel), so the scroll length is exact and the visible range is a binary search. A loader thread with its own read connection runs the gallery's queries; tile data loads in chunks of 200 for the visible range ± 600 pt and is remembered up to 20,000 items. Only tiles near the viewport are drawn, each as a textured `RectShape` (cropped like `object-fit: cover`) over its ThumbHash placeholder, plus one `interact` for hover and the accessible name. Textures stream as decided in M0 (`app::textures`): 4 below-normal-priority workers read through `ThumbCache::read`, decode with libwebp, build mips and create the `wgpu::Texture`; missing files are requested from the `ThumbnailService`. Budget 512 MB: at 12 columns the visible tiles plus the prefetch band (1.5 screens ahead, 0.5 behind) take ≈ 330 MB. Ctrl + wheel zoom reads the raw wheel events (egui turns Ctrl + wheel into a zoom delta), and the zoom level is stored in the shared `settings` table as `gallery.columns`, which the Electron version ignores. Measured with `cargo xtask gallery-bench` (100,000 items, 12 columns, vsync off, AMD iGPU): p95 4.4 ms, max 11.9 ms.

### 2026-10-27 · Toolbar, view choices and the List view (M6b)

**Decided:** the toolbar (`app::toolbar`) is drawn over the top 48 px of the gallery, which scrolls underneath it; until the glass and the title bar (M10) it is Raised and the window keeps its native caption. The _View_ popup (`egui::Popup`) holds layout, zoom, _Group by day_ and tag indicators; sorting arrives with the search (M7). View choices live in the shared `settings` table under the Electron version's `localStorage` keys (`gallery.view`, `gallery.columns`, `gallery.groupByDay`, `gallery.tagIndicators`), parsed by `domain::view` as the reference parses them. The List view needs no per-row table: rows are 52 px and group header rows 40 px, so positions are arithmetic over the group starts (`domain::layout::ListRows`). Group headers and tiles are accessible nodes (heading, image or row with the file name, duration and tag indicators), which the UI tests query.

### 2026-10-27 · Search queries (M7a)

**Decided:** `domain::search::GallerySearch` is the search state (the reference's URL state with typed fields); `db::gallery::layout` builds its WHERE clause as SQL text with bound parameters, and the tag conditions are a port of the reference's `tag-search.ts`: subtrees by recursive CTE, the rarest tag driving up to 5,000 estimated matches, materialized id lists up to 100,000, primary key probes beyond, with the unary `+` plan pins kept exactly. Tag ids go into the SQL as numbers. `db::tags` holds suggestions, categories, popular tags, rating counts and library folders. Read connections use memory-mapped reads (512 MB) and a 64 MB page cache: searches walk a sort index and look up each row, which with the default cache is one read call per row (4–8× slower on 500k items). `cargo xtask oracle` compares 30 searches and the sidebar lists with the reference's router on the same index; `cargo xtask search-bench` measures REQ-007-14.
**Open:** broad searches (tens of thousands of matches) take 0.25–0.7 s on 500k items with dates unrelated to the insertion order; meeting 50 ms needs a covering index or a different storage order, a change of the shared schema (user decision).

### 2026-10-27 · Search bar, filters and sidebar (M7b)

**Decided:** the gallery owns the search (`Gallery::set_search` reloads from the top) and its loader thread also answers tag suggestions and the sidebar's lists, which reload with the gallery. The search bar (`app::search_bar`) is an egui `TextEdit` with chips and a suggestions `Area` in the foreground; it handles Enter and Escape in the frame the input gives up focus, and keeps the suggestions open while the pointer is over them. _Filters_ and _View_ are `egui::Popup`s under fixed ids (`toolbar::menu`): a button's automatic id changes with its look and with the widgets drawn before it, which closed the menus. The sidebar (`app::sidebar`) is a fixed 256 px left `Panel` on Recess with rows drawn by one helper; collapsed groups are stored as the reference's `sidebar.group.<id>` keys in `settings`.

### 2026-10-27 · Viewer core (M8a)

**Decided:** the viewer (`app::viewer`) is `AppState::viewer`, drawn in a foreground `Area` over the window while the gallery keeps drawing below; it takes its keys at the start of the frame, before anything else sees them. It reads the shown item from the gallery's tile data by position and follows its media id across reloads ("Media not found" when it is gone). Originals come from `app::originals`: two worker threads decode the wanted set (shown photo, then neighbours nearest first, REQ-009-13) with `media::original::load_original`, build the mips and create the `wgpu::Texture` themselves, like thumbnails; a request that leaves the set before it starts never runs, one that finishes after is dropped. Originals are shrunk only beyond the GPU's texture limit (8192 px). Zoom math, preloading order and when controls hide are pure functions in `domain::viewer` (ported from the reference's `zoom.ts` and `preload.ts`). Without a wgpu render state (tests) originals go through egui textures.

### 2026-10-27 · Viewer panels and filmstrip (M8b)

**Decided:** the viewer's info panel and the gallery's info sheet share `app::info` and load `db::media::detail` (fields, absolute path, tags with their category by top-level ancestor) on the gallery's loader thread, asked again after each reload. The viewer reports what it needs as `viewer::Action` (close, filter by tag, show in Explorer); Explorer goes through `AppState::file_manager` (`media::helpers::show_in_explorer`, started outside the app's job object) so tests record it. The filmstrip draws only the thumbnails in view (`domain::viewer::Strip`) and asks the gallery's texture streamer for them ahead of the tiles. The chrome's tint is the reference's OKLCH clamp (`domain::ambient`). Motion: `theme::motion` is off when Windows' animation effects are off (and in kittest, which zeroes `animation_time`); the tile ↔ viewer transition and other animations become instant then.

### 2026-10-27 · Video playback in the viewer (M9)

**Decided:** the viewer talks to a `video::Playback` (load, stop, pause, seek, volume, mute, state, frame); the app's is `video::MpvPlayer`, UI tests use `video::FakePlayback`, which records calls. `AppState::videos` (`video::Videos`) makes the player when the first video shows, or holds why it can't (libmpv not found); `libmpv-2.dll` is found like ffmpeg (`GOOD_GALLERY_MPV`, `mpv\libmpv-2.dll` next to the app, `PATH`). All of the app's video `unsafe` is in `video::mpv` (FFI, the render callback, the aligned frame buffer). `MpvPlayer` follows the M0b decision: a render thread renders `rgb0` at the size the video took on screen in the last frame (never larger than decoded) into a `wgpu::Texture` it creates, timed to the display; without a wgpu render state frames go through egui textures timed to mpv's clock. Frames belong to a load: the UI counts the `loadfile`s it sent, the event thread counts mpv's `START_FILE`s, and frames show only after the `FILE_LOADED` of the latest load, so a video stepped away from never flashes. The UI reads mpv properties at most every 100 ms. The viewer plays the shown video (stop on step, close and missing items), draws its frame rotated by `video-params/rotate` (preview until the first frame and during the tile transition) and its controls in a glass bar along the video's bottom (`video::controls`); ← / → seek and ↑ / ↓ change the volume while the last click was on the video or its controls (browser focus), else they step; a click on the video plays / pauses.
**Why:** the trait keeps libmpv, threads and timing out of the UI tests; the player costs nothing until a video is opened.
**Rejected:** egui's `Slider` for seek and volume (its rail doesn't show with our widget visuals; an own thin slider matches the design and gives press-to-seek), loading libmpv at startup.


### 2026-10-28 · Settings applied at once (M10a)

**Decided:** the Settings page stores through `db::settings::save` (validated; an invalid value is an error toast and nothing changes) and hands what changed to an `app::SettingsTargets` (the app's `RunningParts` in `main.rs`: `Scheduler::set_interval`, `Indexer::set_concurrency` + `ExifTool::set_max_processes` + `ThumbnailService::set_workers`, `ThumbCache::set_max_bytes`; UI tests record calls with a fake). `ThumbnailService` threads above the set number wait, more are started when it grows. The cache's usage is read without blocking every 10 s (every second for 10 s after a new limit); clearing runs on a job thread. Dates are formatted with ICU4X (`icu_datetime`, compiled data) in the locale `sys-locale` reports; the formatter is cached per thread (it isn't `Send`), and tests pin `en-GB` with `format::set_locale`. Relative paths from the index are checked by `domain::media::is_safe_rel_path` wherever a file is opened (`db::media::absolute_path`, `ThumbSource::file`). `xtask/tests/no_network.rs` keeps network crates and socket code out of the app.

### 2026-10-28 · Glass, title bar, focus and OS preferences (M10b)

**Decided:** `app::glass` is the M0 pass: each surface draws the frame's **backdrop** (tiles with their thumbnail texture views, placeholders and the page / sidebar as solids, recorded by the gallery and `app::ui` while drawing) into an offscreen chain at half resolution, blurs it with 4 dual-Kawase passes and composites saturation, the glass color (tinted by the item, REQ-013-11) and an opacity in a paint callback. Surfaces record nothing for each other ("no glass on glass"), except that the scrim behind the viewer also sees the toolbar. The viewer's chrome doesn't blur again: it composites from the scrim's blurred level 0 with the scrim's darkening first (`Source::Over`). Menus, combo lists, context menus, suggestions and dialogs paint their glass into shape slots kept before their content (`glass::behind_menu`, `behind_frame`), covering egui's opaque frame. Without the pass (no wgpu render state, as in kittest) or with Windows' transparency effects off, surfaces are opaque (Raised, Darkroom for the scrim); snapshots show that fallback, the pass is checked with screenshots of the running app. A paint callback ignores the painter's opacity, so surfaces pass it themselves.
**Title bar:** the M0 hybrid window subclass in `app::titlebar` (the module with the window `unsafe`): `WM_NCCALCSIZE` drops the caption, `WM_NCHITTEST` answers the top edge and the three caption buttons natively (snap layouts), button-up runs them. Unlike the spike, dragging isn't an `HTCAPTION` rect: empty caption areas (toolbar, Settings header, sidebar header) take the drag in egui and send `ViewportCommand::StartDrag` (a native caption drag, so Aero snap works), a double-click maximizes. 138 px stay free at the right of the caption.
**Focus and preferences:** one focus ring for the widget with keyboard focus (`theme::draw_focus_ring`, after everything; text inputs keep their border). `theme::OsPreferences` re-reads animation and transparency effects every 2 s in the app (not in tests, which keep kittest's zero animation time). Scrollbars are egui's floating bars styled as the design's pill, placed below the toolbar with `ScrollArea::scroll_bar_rect`.
**Rejected:** reading back egui's framebuffer for the blur (a callback runs inside egui's render pass), `HTCAPTION` drag rects (the window procedure can't know which parts of the toolbar are empty), Windows' `GetDateFormatEx` (no medium length).
