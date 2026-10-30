# Architecture

Good Gallery is one Rust binary (eframe / egui on wgpu) plus three helper programs: exiftool (metadata), ffmpeg (video frames, AVIF) and libmpv (video playback). Windows bundles them; Linux uses the system's. The same UI also runs in the browser as a static page without helpers (spec 019). Keep this file describing the current state; how a feature was built is in its spec's `plan.md`.

```
crates/
  domain    pure types and logic: media, tags, search model, gallery layout, date groups, zoom and viewer math. No IO.
  db        rusqlite: migrations, connections, index writer, gallery and tag queries, settings
  meta      metadata rules (exiftool-named tags → MediaMetadata) and pure-Rust EXIF / XMP / IPTC / container readers. No IO.
  media     exiftool process pool, metadata extraction, thumbnail rendering, originals, ffmpeg, helper lookup (desktop)
  indexer   scan queue, walk, scans, watcher, schedule, reachability, thumbnail cache and service, library operations
  app       eframe binary: UI, texture streaming, glass, viewer, video, settings; `app::web` for the browser
xtask/      check-specs, libraries CLI, benchmarks, screenshots, dist, release, web, web-test
web/        the browser page: index.html (content security policy), browser check, Trunk config; Playwright tests
packaging/  helper downloads, NSIS installer, smoke test (windows/); .desktop file, install script (linux/)
```

## Decisions

Architectural decisions, newest last. One entry each: what was decided, why (with measurements where there are any), what was rejected.

### 2026-10-26 · UI stack: eframe / egui 0.37 on wgpu

**Decided:** `eframe` / `egui` / `egui-wgpu` pinned to `=0.37.0`. A prototype scrolled 100,000 (and 250,000) items at 12 columns with a p95 frame interval of 7.2 ms on an integrated AMD GPU (budget 16 ms). egui's own painter is enough; no custom tile pipeline is needed.

### 2026-10-26 · Thumbnail textures

**Decided:** thumbnails are our own `wgpu::Texture`s (`Rgba8Unorm`, CPU-built mip chain), decoded with libwebp and created and written on worker threads; the UI thread only registers them with the egui renderer, within 3 ms per frame. 4 decode workers at below-normal thread priority. LRU cache with a byte budget (512 MB); textures in the prefetch band count as used. Requests are a priority list replaced every frame (visible, then 1.5 screens ahead, 0.5 behind), so stale work drops out.
**Why:** uploading on the UI thread cost 2.7 ms per texture; egui-managed textures have no mipmaps (aliasing at high zoom); more decode threads starved the UI thread and the iGPU; libwebp decodes 3× faster than the `image` crate.
**Rejected:** `ctx.load_texture` (p95 12.9 vs 7.2 ms, no mips), `image` crate WebP decoding.

### 2026-10-26 · Glass

**Decided:** glass surfaces render what lies below them into an offscreen target at half resolution, blur it with a dual-Kawase chain (4 passes) and composite saturate + tint + rounded corners in an `egui_wgpu` paint callback. ≈ 0.4 ms per surface. Raised is the fallback. Details in spec 013's plan.

### 2026-10-26 · Font and title bar

**Decided:** Atkinson Hyperlegible Next as the variable TTF from google/fonts (OFL), weights via `FontTweak::coords`; a second copy with `tnum` baked in (`pyftfeatfreeze`) for numbers, since egui can't request OpenType features. Title bar: content runs into the caption, own caption buttons, and a window subclass answers `WM_NCHITTEST` with the native codes (`HTMAXBUTTON` etc.), so shadow, resize, Aero snap and snap layouts stay native.
**Rejected:** native caption (a second bar above the toolbar), undecorated window (no snap layouts, no top-edge resize).

### 2026-10-26 · Video playback: libmpv, software render API, display-synced

**Decided:** video plays through libmpv (`libmpv-2.dll`, loaded at runtime with `libloading` through a small own FFI; `hwdec=auto-copy`). A render thread renders each frame with mpv's software render API (`rgb0`) at the size the video occupies on screen and writes it into a `wgpu::Texture`; the UI draws the texture as a quad, rotated by `video-params/rotate` (the software renderer ignores rotation). Timing is display-synced: `video-sync=display-resample` with the monitor's refresh rate; the UI wakes the render thread once per presented frame, which calls `report_swap` and skips frames mpv marks as repeats. The render thread never calls non-render libmpv functions. MP4 H.264, MOV HEVC (incl. 4K and rotation), MKV and WebM VP9 + Opus play with 0–4 dropped frames per 600; 4K costs ≈ 2 cores and 5.7 ms render per frame. Ships as one 121 MB GPL DLL that hard-imports `vulkan-1.dll`.
**Why:** mpv brings decoding, audio output, A/V sync, seeking and every container; the software render API works with wgpu as is. Display-synced timing removes the judder that audio-clock timing causes against our vsync (45–265 of 600 frames).
**Rejected:** `libmpv2` crate (only the OpenGL render API), audio-clock timing (judder), rendering at the video's own size (4K: ≈ 19 ms CPU per frame), GL interop (would need a WGL context beside wgpu), `ffmpeg-next` + `cpal`, GStreamer.

### 2026-10-26 · Workspace and crate layout

**Decided:** one Cargo workspace (`resolver = "3"`, edition 2024) with the crates above; shared versions and lints in `[workspace.dependencies]` / `[workspace.lints]`. `app` is a library (`app::ui`, `AppState`, `theme`) plus a thin `main.rs`, so `egui_kittest` tests drive the same UI code as the window. `cargo xtask <task>` is a Cargo alias. Dependencies are optimized in dev builds (`[profile.dev.package."*"] opt-level = 3`): decoding and layout are too slow unoptimized to judge anything.

### 2026-10-26 · Indexer runs as a thread

**Decided:** the indexer runs on threads inside the app process, not as a child process. Its API is message-based (commands in, events out over `crossbeam-channel`), and every job runs under `catch_unwind`, so a panic fails that job instead of the app.
**Why:** exiftool and ffmpeg already run as separate processes; the in-process decoders (libwebp, the `image` crate) are mature. A child process would need an IPC layer, a second process to supervise and a second copy of the decoders in memory.
**Revisit if** a C decoder crashes in practice: because the API is message-based, it can move behind a pipe to `app --indexer` without changing callers.

### 2026-10-26 · No async runtime, errors, logging

**Decided:**
- *Concurrency:* OS threads and `crossbeam-channel`, small worker pools where needed. No tokio: the work is CPU- or disk-bound, with few concurrent tasks and no network protocol.
- *Errors:* `thiserror` enums in library crates (`domain`, `db`, `media`, `indexer`), `anyhow` in `app` and `xtask`.
- *Logging:* `tracing` everywhere; `app` installs `tracing-subscriber` with `RUST_LOG`-style filtering (default `info`) on stderr and `logs\app.log` in the data folder (the previous run's kept as `app.old.log`).

### 2026-10-26 · REQ references in Rust tests

**Decided:** a test references a REQ at the start of its name (`fn req_012_01_reports_unknown_reqs()`, several chained: `fn req_002_01_req_002_02_…()`) or in a doc comment directly above it (`/// REQ-012-01: …`). `cargo xtask check-specs` reads `.rs` files below `crates/` and `xtask/` and counts only their test part: from the first `#[cfg(test)]` on, or the whole file below `tests/` or `benches/`. REQs mentioned in production code don't count as coverage. Spec: 012.

### 2026-10-26 · Visual checks

**Decided:** UI changes are checked two ways: `egui_kittest` snapshot tests (wgpu renderer, PNGs in `crates/app/tests/snapshots/`, `UPDATE_SNAPSHOTS=1` rewrites them) and `cargo run -p app -- --screenshot <file.png>`, which renders a few frames, saves the window and exits, so a session can look at the real app without anyone at the screen. `--view`, `--info`, `--scroll` and `--screenshot-after` set up the shot.

### 2026-10-26 · Index compatibility and migrations

**Decided:** existing indexes must keep opening without re-indexing (REQ-001-08). `crates/db/migrations/` holds the SQL files and `meta/_journal.json` byte for byte (`.gitattributes`: `-text`), and `db::open` migrates with drizzle-orm's SQLite bookkeeping, which existing indexes carry: `__drizzle_migrations` created with drizzle's exact DDL text, newest `created_at` read, newer journal entries run chunk by chunk in one transaction, `(sha256 of the file, journal when)` inserted. Tests open `crates/db/tests/fixtures/legacy-index.db`, an index from an earlier version, and check that a new index gets the same schema and bookkeeping. A schema change is a new migration file with a journal entry whose `when` is larger than the last.
**Connections:** one write connection (WAL, `synchronous = NORMAL`, foreign keys, `busy_timeout = 5000`), read-only connections for readers (memory-mapped reads of 512 MB and a 64 MB page cache, see spec 007's plan).

### 2026-10-26 · Icons

**Decided:** lucide icons (ISC, `crates/app/assets/icons/LICENSE.lucide.txt`) as SVG files with `stroke="#fff"` and `stroke-width="1.75"`, embedded with `include_bytes!` (`app::icons::Icon`) and rendered by `egui_extras`' SVG loader (resvg) at the size they are drawn, tinted per use. A new icon is the lucide SVG with those two attributes set.
**Rejected:** an icon font (lucide's font drops the stroke-width control), hand-drawn shapes (doesn't scale to ~40 icons).

### 2026-10-26 · UI state and background jobs

**Decided:** `app::AppState` holds what the UI shows; slow work (reachability, folder dialog, Explorer, cache clearing) runs on threads and sends an event back over a channel, applied at the start of the next frame (`AppState::poll`). Tests call `AppState::wait_for_jobs` before stepping the harness, so they never race the jobs. OS dialogs sit behind a trait (`FolderPicker`) so tests use a fake.

### 2026-10-26 · Metadata through exiftool

**Decided:** `media::exiftool` keeps a pool of `exiftool -stay_open True -@ -` processes (size = parallel file reads) and reads each file with the argument set of exiftool-vendored v38's defaults (`-charset filename=utf8 -json -fast`, its UTF-8 repair filter, `-api struct=1 -use MWG -api keepUTCTime`, numeric durations, GPS and orientation, `-all <file> -ignoreMinorErrors`), which existing indexes were read with, so their rows stay valid. Arguments go over stdin, one per line, no shell; paths are always absolute. A read over 60 s replaces its process. `METADATA_VERSION` is 1; bump it when the extraction rules change.
**Deviation:** no time zone from the GPS position (it would need a time-zone shape database); such photos get the computer's local zone. See spec 004's plan.
**Since 2026-10-29** the rules that turn exiftool's tags into `MediaMetadata` live in the crate `meta`, beside the browser's readers (spec 019); `media::metadata` re-exports them.
**Tests that need exiftool** (or ffmpeg, libmpv, git-cliff) skip themselves when it isn't found, unless `GG_REQUIRE_HELPERS=1` (CI).

### 2026-10-26 · Pure media and tag rules in `domain`

**Decided:** file kinds, housekeeping folders, sidecar lookup, tag normalization, `MediaMetadata` and `METADATA_VERSION` live in `domain`; `db` depends on `domain` for the writer, `media` for extraction. `db::writer::IndexWriter` owns the write connection (on the indexer thread).

### 2026-10-26 · Scans on threads

**Decided:** `indexer::scan::scan_root` walks on the calling thread, which owns all index access (lookups, batched writes, removals at the end); `io_concurrency` scoped worker threads stat files and read metadata, connected by a bounded channel (backpressure) and a result channel. Cancelling (library removed) and "offline" are atomic flags. A panic while reading one file costs that file; a panic anywhere else ends the scan as `Failed`, and the next scan runs (REQ-002-13).
**Indexer:** `indexer::Indexer` holds the scan queue behind a mutex (commands are method calls from any thread, merged per library) and one thread with the write connection that runs the scans; it reports `IndexerEvent`s over a channel and wakes the UI through a callback. It implements `ScanRequests`, so library operations queue real scans. The app keeps its own connection for library rows; SQLite's busy timeout serializes the writers.

### 2026-10-27 · Change detection and schedule

**Decided:** the indexer owns the watchers (`notify` recursive, 2 s debounce, 10 s at most). A completed scan starts one for its library, an offline result or a removed library stops it. `indexer::schedule::Scheduler` is its own thread with a read connection: quick scans at startup and every quick check interval, offline libraries every minute. Details in spec 003's plan.

### 2026-10-27 · Helper processes end with the app

**Decided:** at startup the app puts itself into a Windows job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (`media::helpers::end_helpers_with_app`). Every process it starts (exiftool's launcher and its child, ffmpeg) joins the job and ends with the app, however it exits. Without it, exiftool's PAR-packed launcher outlived the app. The job allows breakaway: programs opened for the user (Explorer) are started with `CREATE_BREAKAWAY_FROM_JOB`, or closing the app would close them too.

### 2026-10-27 · Thumbnail cache layout

**Decided:** one WebP file per thumbnail and width, `<data>/thumbs/<h0h1>/<h2h3>/<sha1>.webp` with `sha1` of `[rootId, relPath, size, mtime, width]` serialized as `JSON.stringify` does (`serde_json` matches it for integers and strings; a test pins hashes computed with Node). This is the layout existing caches use, so they keep working (REQ-005-15). Writes are atomic (temp file + rename), `*.tmp` leftovers are removed at startup, eviction is LRU by file mtime down to 90 % of the limit, reading bumps the mtime at most once an hour.
**Rejected (for now):** a pack file. Cold reads would be ≈ 100× faster under Defender's first-read scan (≈ 13 ms per file), but it needs a new format and a migration of existing caches, and the cost only shows while scrolling fast through thumbnails not read for minutes.

### 2026-10-27 · Thumbnail rendering

**Decided:** `media::thumbnail::Renderer`: one decode per file, upright by its EXIF orientation, fit into 800 × 2400 without enlarging (Lanczos3, `fast_image_resize`), each width from that, WebP quality 80 through libwebp, ThumbHash at ≤ 100 px. JPEGs decode through `jpeg-decoder` with DCT scaling (shrink-on-load); other formats through `image`. Embedded previews (exiftool `PreviewImage`) replace large camera JPEGs when they are large enough and have the same shape. Video frames and **AVIF** come from ffmpeg (`media::ffmpeg`), since `image` decodes AVIF only through dav1d, a C library with a meson build.
**Deviation:** ICC profiles are ignored, so wide-gamut photos may come out slightly less saturated.
**Service:** `indexer::thumbnails::ThumbnailService`: `io_concurrency` render threads, one priority queue (requests newest first above background jobs), background batches of 100 pending items, its own write connection. Requests are answered through a callback, which the texture streamer uses. Each render runs under `catch_unwind`.

### 2026-10-27 · Gallery: layout up front, data and textures on demand

**Decided:** `db::gallery::layout` returns every match's id and clamped aspect in sort order (and the day groups of a date sort); `domain::layout::tile_layout` positions all tiles at once (f64), so the scroll length is exact and the visible range is a binary search. A loader thread with its own read connection runs the gallery's queries (and the suggestions and sidebar lists); tile data loads in chunks of 200 for the visible range ± 600 pt. Only tiles near the viewport are drawn. Measured with `cargo xtask gallery-bench` (100,000 items, 12 columns, vsync off, AMD iGPU): p95 4.4 ms, max 11.9 ms. View choices are stored in the `settings` table (`gallery.view`, `gallery.columns`, `gallery.groupByDay`, `gallery.tagIndicators`); the List view's row positions are arithmetic over the group starts.

### 2026-10-27 · Search queries

**Decided:** `domain::search::GallerySearch` is the search state; `db::gallery::layout` builds its WHERE clause as SQL text with bound parameters. Tag conditions pick a query shape per search (rarest tag drives up to 5,000 estimated matches, materialized id lists up to 100,000, primary key probes beyond), with unary `+` operators pinning the plans. `cargo xtask search-bench` measures REQ-007-14.
**Open:** broad searches (tens of thousands of matches) take 0.25–0.7 s on 500k items; meeting 50 ms needs a covering index or rows stored in sort order (a schema change). See spec 007's plan.

### 2026-10-27 · Search bar, filters and sidebar

**Decided:** the gallery owns the search (`Gallery::set_search` reloads from the top). _Filters_ and _View_ are `egui::Popup`s under fixed ids: a button's automatic id changes with its look and with the widgets drawn before it, which closed the menus. The sidebar is a fixed 256 px left panel; collapsed groups are stored as `sidebar.group.<id>` in `settings`. The groups' order and which of them show (`domain::sidebar::SidebarLayout`) are stored once for the app as `sidebar.groups`, a list of `{group, shown}`; reading it repairs what an older or newer version left behind instead of rejecting it.

### 2026-10-27 · Viewer

**Decided:** the viewer (`app::viewer`) is `AppState::viewer`, drawn in an area over the window while the gallery keeps drawing below; it takes its keys at the start of the frame. Originals come from `app::originals`: two worker threads decode the wanted set (shown item, then neighbours nearest first) and create mipped `wgpu::Texture`s themselves; a request that leaves the set before it starts never runs. Zoom math, preloading order and control timing are pure functions in `domain::viewer`. The info panel and the gallery's info sheet share `app::info`; the viewer reports what it needs as `viewer::Action` (close, filter by tag, show in Explorer). `theme::motion` is off when Windows' animation effects are off.

### 2026-10-27 · Video playback in the viewer

**Decided:** the viewer talks to a `video::Playback`; the app's is `video::MpvPlayer`, UI tests use `video::FakePlayback`. `AppState::videos` makes the player when the first video shows, or holds why it can't (libmpv not found). All of the app's video `unsafe` is in `video::mpv`. Frames belong to a load: the UI counts the `loadfile`s it sent, the event thread counts mpv's `START_FILE`s, and frames show only after the `FILE_LOADED` of the latest load, so a video stepped away from never flashes. The UI reads mpv properties at most every 100 ms.
**Rejected:** egui's `Slider` for seek and volume (its rail doesn't show with our widget visuals), loading libmpv at startup.

### 2026-10-28 · Settings applied at once

**Decided:** the Settings page stores through `db::settings::save` (validated; an invalid value is an error toast and nothing changes) and hands what changed to an `app::SettingsTargets` (the app's running parts in `main.rs`; UI tests record calls with a fake). Dates are formatted with ICU4X (`icu_datetime`, compiled data) in the locale `sys-locale` reports; tests pin `en-GB`. Relative paths from the index are checked by `domain::media::is_safe_rel_path` wherever a file is opened. `xtask/tests/no_network.rs` keeps network crates and socket code out of the app.

### 2026-10-28 · Glass, title bar, focus and OS preferences

**Decided:** `app::glass` draws each surface's **backdrop** (tiles with their thumbnail texture views, placeholders and the page / sidebar as solids, recorded while drawing) into an offscreen chain at half resolution and blurs and composites it in a paint callback. Surfaces record nothing for each other, except that the viewer's scrim also sees the toolbar; the viewer's chrome composites from the scrim's blur. Without the pass (no wgpu render state, as in kittest) or with Windows' transparency effects off, surfaces are opaque; snapshots show that fallback, the pass is checked with screenshots of the running app. A paint callback ignores the painter's opacity, so surfaces pass it themselves.
**Title bar:** `app::titlebar` (the module with the window `unsafe`): `WM_NCCALCSIZE` drops the caption, `WM_NCHITTEST` answers the top edge and the three caption buttons natively. Empty caption areas take the drag in egui and send `ViewportCommand::StartDrag` (a native caption drag, so Aero snap works).
**Rejected:** reading back egui's framebuffer for the blur (a callback runs inside egui's render pass), `HTCAPTION` drag rects (the window procedure can't know which parts of the toolbar are empty), Windows' `GetDateFormatEx` (no medium length).

### 2026-10-28 · Packaging and releases

**Decided:**
- *Data folder:* the packaged app (feature `packaged`, set by `cargo xtask dist`) uses `%APPDATA%\good-gallery`, the folder every earlier version used; every other build uses `%APPDATA%\good-gallery-rust`, so `cargo run` (also `--release`) never touches the real index (`app::startup::default_data_dir`).
- *The packaged app* has no console (`windows_subsystem = "windows"`); a startup failure shows a message box and panics are logged. The app logs which helpers it found.
- *Icon:* `crates/app/assets/app-icon.svg`, rendered into `app-icon.ico` (16–256 px PNG entries; exe icon and version info through `winresource` in `crates/app/build.rs`, needs `rc.exe` from the Windows SDK) and `app-icon.png` (window icon). Re-render both with any SVG renderer when the SVG changes.
- *Helpers:* `packaging/fetch-helpers.ps1` downloads pinned versions (exiftool 13.59, ffmpeg-static b6.1.1, libmpv 20261021) and checks their SHA-256. The folder layout is one set of constants (`media::helpers::{EXIFTOOL,FFMPEG,MPV}_BUNDLED`) used by the app's lookup and by `cargo xtask dist`. PowerShell, not xtask, so no HTTP client enters the workspace (REQ-011-11).
- *Package and releases:* `cargo xtask dist` (NSIS installer, portable zip), `cargo xtask release` (git-cliff with `cliff.toml`, version in `[workspace.package]`), `.github/workflows/release.yml`. Details in spec 014's plan.
- *README screenshots:* `cargo xtask screenshots` generates a demo library (landscapes drawn with `image`, tags through exiftool, two ffmpeg videos) on a `subst` drive, indexes it and runs the app with `--screenshot` per view.
**Rejected:** `cargo-packager` (another tool for one NSIS script, and its resource layout would differ from the folders the app looks in), WiX / MSI (per-machine by habit), a separate portable data folder (out of scope in spec 014), an HTTP client in xtask for the helpers.

### 2026-10-28 · Linux

**Decided:** Linux is a supported platform (spec 018), built and tested in CI beside Windows. Platform code stays behind `cfg(windows)` / `cfg(target_os = "linux")` (not `unix`: macOS will need its own answers); POSIX path rules (`indexer::libraries`, `db::libraries::find_by_path`) branch on `cfg!(windows)`.
- *Data folder:* `$XDG_DATA_HOME/good-gallery` (else `~/.local/share`), `good-gallery-rust` for development builds (`app::startup::data_home`).
- *Helpers* come from the distribution: exiftool and ffmpeg on `PATH`, libmpv by its soname `libmpv.so.2` through the dynamic loader. Every helper starts through `media::helpers::spawn_helper`: on Linux from one long-lived thread with `PR_SET_PDEATHSIG = SIGKILL`, the counterpart of the Windows job object.
- *Title bar:* the window is undecorated; `app::titlebar` draws GNOME-style round caption buttons (`Area`s on the `Foreground` order, so the content below gets no presses) and 4 px resize handles (12 px corners, on top of the `Middle` order: above the viewer, below menus) that send `ViewportCommand::BeginResize`, which winit hands to the compositor on X11 and Wayland.
- *Desktop services* run processes, not a D-Bus crate: _Show in folder_ is `org.freedesktop.FileManager1.ShowItems` through `gdbus` / `dbus-send`, else `xdg-open`; reduced motion reads `gsettings` / `kreadconfig` on a background thread every 2 s. Decode threads get nice 10.
- *Package:* `cargo xtask dist` writes a `.tar.gz` (app, `.desktop` file, icon, `install.sh` into `~/.local`), built on `ubuntu-latest` for an older glibc. Snapshot tests keep a Linux set (`*_linux.png`, lavapipe renders like WARP otherwise), checked in CI in an `ubuntu:26.04` container.
- *Video:* mpv gets `video-rotate=no` and the UI reads `video-dec-params/rotate`, since mpv 0.41's software renderer aborts on rotated videos.
**Rejected:** winit's client-side decorations (a bar above the toolbar, so the search isn't in the title bar), zbus (its async runtime trips REQ-011-11's guard), bundling the helpers on Linux (distributions ship them; an AppImage can bundle them later).

### 2026-10-29 · Browser version

**Decided:** the app also builds for `wasm32-unknown-unknown` and runs as a static page on GitHub Pages (spec 019; details in its plan).
- *One wasm instance on the main thread.* No wasm threads (nightly and COOP / COEP headers, which Pages can't send). Slow work is the browser's (decoding, encoding, file reads); our own is sliced by `app::web::Slice`, which yields after 8 ms.
- *Seams:* desktop-only code sits behind `cfg(not(target_arch = "wasm32"))` in `app`, `indexer` and the crate graph (`media` is desktop only); `app::web` implements the same seams the desktop fills (`ScanRequests`, `IndexerEvent`s, `ThumbnailReady`s, `video::Playback`). `app::WEB` hides desktop-only actions. The desktop entry point is `crates/app/src/desktop.rs`.
- *Index:* `db` on `sqlite-wasm-rs`'s in-memory file system, with a snapshot in the Origin Private File System, saved after changes and when the page is hidden. *Thumbnails:* OPFS files under `thumbs/<library>/`, the desktop cache's names, size limit and LRU.
- *Metadata without exiftool:* `meta`'s readers produce exiftool-shaped tags, including `-use MWG`'s keyword reconciliation; a test compares them with exiftool on the same files.
- *Privacy:* a content security policy without inline scripts (`cargo xtask web` moves Trunk's start script into a file); `no_network.rs` checks the `web-sys` features and the page's scripts.
- *Tests:* Playwright (`web/tests`, `cargo xtask web-test`) in Chromium with SwiftShader, clicking by position in a fixed viewport and comparing screenshots; npm only there.
**Rejected:** an index worker with SQLite on OPFS (`sahpool`), a separate web UI, sql.js / IndexedDB, `image` + libwebp in wasm, a DOM `<video>` over the canvas, Chromium only.

### 2026-10-30 · Appearance: light, dark or the system's

**Decided:** `theme` holds two `Palette`s (`DARK`, `LIGHT`) named by role (`base`, `recess`, `raised`, `text`, `muted`, …). egui gets a style for each theme in `theme::install`; the _Appearance_ setting maps onto egui's `ThemePreference` (System follows eframe's report of the OS or browser preference, dark where there is none), and egui's own `ViewportCommand::SetTheme` sets the native window frame. Each frame, `theme::begin_frame` sets a per-thread current palette from `ctx.theme()`; call sites read `theme::text()` and so on. The viewer draws inside `theme::with_palette(&DARK, …)` and its area gets egui's dark style; menus apply the current palette's style themselves (egui builds popups from the global style). Details in spec 020's plan.
**Rejected:** passing the context to every color lookup (the same result, threaded through ~170 call sites), a global (tests draw in parallel on their own threads, with different appearances).
