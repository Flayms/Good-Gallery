# Rewrite plan

**Status:** Approved (2026-10-26)

## Goal

Rewrite Good Gallery from Electron + React to Rust (eframe / egui on wgpu) with the same behavior, defined by the specs. Expected gains: smaller install and memory footprint, smooth scrolling at high zoom (the reference drops frames at 12 columns, see its `docs/known-pitfalls.md`), one language, and a better base for the roadmap (GPU photo editing, face detection via ONNX).

## Rules

- **Specs are the contract.** The reference specs are ported to `docs/specs/` with their requirements unchanged and Verification tables reset. M2 deprecates the Electron-specific REQs and adds the ones the rewrite needs. A milestone is done when its REQs pass here.
- **Compatible data.** The SQLite schema (reference: `drizzle/*.sql`) and the thumbnail cache layout (reference: `src/indexer/thumb-cache.ts`) stay compatible, so:
  - the user's existing index and cache keep working after the switch,
  - the reference indexer can serve as a **test oracle**: index the same fixture library with both and diff the databases (from M4 on).
  Deviations need a decision in `docs/architecture.md` and a migration.
- **Feature freeze in the reference.** Fixes made there during the rewrite become spec updates here.
- **Budgets are tests.** Performance targets are checked by benchmarks or measured tests, not by impression.

## Target architecture

Initial sketch. M0 and M1 refine it; `docs/architecture.md` holds the current state and its decisions.

```
crates/
  domain    pure types and logic: media, tags (normalization, hierarchy, categories), search model,
            gallery layout (masonry, justified, grid), date groups, zoom math. No IO.
  db        rusqlite, migrations (SQL taken from the reference), queries incl. tag search
  media     thumbnail rendering (image, fast_image_resize, libwebp), video frames (ffmpeg),
            exiftool process (-stay_open), thumbhash
  indexer   scan queue, walk, watcher (notify), writer, reachability, rescan scheduling
  app       eframe binary: UI, texture streaming, viewer, settings
xtask/      check-specs, fixture library generator, release
```

- **Processes:** the UI owns read connections; the indexer owns the single write connection (WAL), as in the reference. Whether the indexer runs as a thread or as a child process of the same binary (crash isolation from C decoders) is decided in M1.
- **Helpers:** exiftool and ffmpeg ship as bundled binaries next to the executable.
- **UI:** immediate mode, app state as plain Rust structs, background work over channels. Tile views position tiles themselves from `domain` layout results; textures stream from worker threads with an LRU cache and a GPU memory budget.
- **Tests:** unit tests per crate, `egui_kittest` for UI behavior and snapshots, oracle tests against the reference for indexing and search, `criterion` benchmarks for budgets.

## Milestones

Status lives in `docs/progress.md`. Each milestone gets its own plan in `docs/rewrite/milestones/` before implementation.

### M0 · Spikes

Throwaway crates in `spikes/`, each with measurements and a go/no-go, recorded in the M0 plan and summarized in `docs/architecture.md`.

1. **Grid:** justified grid over ≥ 100k thumbnails (copy of the user's real thumbnail cache, or generated images), texture streaming, fast scrolling. Measure p95 frame time at 5 and 12 columns.
2. **Video:** play MP4 (H.264), MOV (HEVC), MKV and WebM inside an eframe window with seeking and audio. Candidates: libmpv render API (needs OpenGL interop or the glow backend), GStreamer, `ffmpeg-next`. Note licensing (GPL-3 is fine) and how binaries are shipped.
3. **Look:** blurred glass panel over a scrolling gallery (custom wgpu pass), Atkinson Hyperlegible Next at 12 / 14 / 16 px with tabular figures, custom title bar vs native window controls on Windows 11. Compare with screenshots from `../good-gallery/docs/screenshots/`.

**Reference:** `src/renderer/src/lib/gallery-layout.ts`, `justified.ts`, `masonry.ts`, `src/renderer/src/styles.css`, `docs/design.md`, `src/indexer/thumb-cache.ts`.
**Exit:** three go/no-go results. Go: grid p95 < 16 ms at 12 columns; video plays all four containers; glass and text are approved by the user from screenshots. A no-go on video or look stops the rewrite until the user decides (fallbacks: Raised surfaces instead of glass; Tauri with the existing UI).

### M1 · Foundation

- Cargo workspace with the crates above (empty but compiling), `xtask`.
- `cargo xtask check-specs`: port of `scripts/check-specs.ts` with its tests, adapted to Rust test naming (settle the REQ reference convention here).
- CI on GitHub Actions (Windows): fmt, clippy, test, check-specs.
- `app`: eframe window with design tokens as a theme module, bundled font, empty states.
- `egui_kittest` harness with one snapshot test, and a way to screenshot the running app for visual checks (document it in `docs/known-pitfalls.md` or a `run` skill).
- Decisions in `docs/architecture.md`: crate layout, indexer thread vs child process, error handling (`thiserror` / `anyhow`), logging (`tracing`), async or not.

**Reference:** `scripts/check-specs.ts` (+ test), `.github/workflows/ci.yml`, `docs/architecture.md`.
**Exit:** CI green; `cargo run -p app` opens a themed window; a snapshot test passes; decisions recorded.

### M2 · Spec audit

- Specs, `docs/design.md` and the glossary were already copied during setup (2026-10-26) with reset Verification tables. This milestone only does the parts that need judgment.
- Per spec: deprecate Electron-specific REQs (most of 011, the CSS mechanics in 013, IPC and protocol REQs) with a reason, rewrite technology-bound wording, add REQs the rewrite needs (indexer isolation, texture memory budget, frame-time budget, data compatibility with the reference).
- Adapt `docs/design.md` to egui terms (tokens, surfaces, fallbacks) without changing the look.
- Update `docs/rewrite/plan.md` if the audit changes milestone scope.

**Exit:** user approved the ported specs; `cargo xtask check-specs` runs clean (warnings for `no test` are expected).

### M3 · Database and libraries

- `db` crate: open or create the database, apply the reference migrations (same SQL, same order, compatible migration bookkeeping so a database created by the reference opens here and vice versa), WAL, read and write connections.
- Library management (add, remove, rescan request) behind a headless API and a small CLI for testing.

**Specs:** 001. **Reference:** `drizzle/`, `src/main/db/*`, `src/main/trpc/routers/libraries.ts`, `src/main/settings.ts`.
**Exit:** a copy of the user's real `gallery.db` opens and lists its libraries; 001 REQs pass.

### M4 · Indexing, change detection, metadata

- Walk, housekeeping folders, scan queue, quick / full / changes scans, watcher, scheduled checks, offline libraries, retries.
- exiftool process with batching, sidecars, ratings, tag hierarchy, face regions, categories, `METADATA_VERSION`.
- **Oracle harness** (`xtask`): generate a fixture library, index it with the reference and with this indexer, diff the databases (ignoring ids and timestamps).

**Specs:** 002, 003, 004. **Reference:** `src/indexer/*` (except thumbnails), `src/main/indexer*.ts`, `rescan.ts`, `reachability.ts`, `src/shared/tags.ts`, `metadata.ts`, `e2e/smoke.spec.ts` (fixture generation).
**Exit:** oracle diff is empty on the fixture library; 002–004 REQs pass.

### M5 · Thumbnails

- Rendering at the reference widths, EXIF orientation (port the pinned orientation test), embedded JPEG previews, ffmpeg video frames, WebP output, thumbhash placeholders, cache layout and size limits, worker pool.

**Specs:** 005. **Reference:** `src/indexer/thumbnail*.ts`, `thumb-cache.ts`, `src/shared/media-urls.ts` (`pickThumbWidth`).
**Exit:** reads the reference cache; images rendered from the same input match within a pixel tolerance; 005 REQs pass.

*M4–M5 (backend) and M6–M8 (UI on fake or copied data) may run in parallel worktrees once M3 is done.*

### M6 · Gallery

- Port the layout code (masonry, justified, grid) with its tests, list view, zoom (Ctrl + wheel), date groups with headers, tag indicators, scroll anchoring, texture streaming from the cache.

**Specs:** 006, 016, 017. **Reference:** `src/renderer/src/lib/{gallery-layout,justified,masonry,date-groups,tag-indicators}.ts`, `components/{tile-gallery,media-list,media-tile,tag-indicators,date-group-header}.tsx`, `hooks/use-*`.
**Exit:** frame-time budget (from M0) met at 12 columns over 100k items; REQs pass.

### M7 · Search and sidebar

- Tag search with suggestions, file name, date range, rating, kind and folder filters, sorting; sidebar with libraries, folder tree, People, Places, Ratings, Tags.

**Specs:** 007, 008. **Reference:** `src/main/db/tag-search.ts` (keep the unary `+` plan pins), `tag-categories.ts`, `src/renderer/src/lib/{search,folders,tag-tree}.ts`, `components/{tag-search,filters-menu,app-sidebar,folder-tree,tag-tree,sidebar-*}.tsx`.
**Exit:** result sets match the reference for a list of queries on the fixture library; REQs pass.

### M8 · Photo viewer and filmstrip

- Viewer over the gallery, stepping, instant preview, preloading ahead, original quality, zoom and pan, click to zoom, info panel, filter by tag, Show in Explorer, context menu, missing items, filmstrip, tile ↔ viewer transition.

**Specs:** 009 (without video), 015. **Reference:** `components/{media-viewer,zoomable-image,media-info,media-context-menu,filmstrip}.tsx`, `lib/{zoom,preload,filmstrip,view-transition,ambient}.ts`.
**Exit:** REQs pass in `egui_kittest`. Probably split into two parts.

### M9 · Video

- Playback with the backend chosen in M0: play, pause, seek, volume, keyboard control, controls that hide while playing.

**Specs:** REQ-009-06 and related. **Exit:** fixture videos in all four containers play; REQs pass.

### M10 · Settings, design polish, glass

- Settings page (indexing, thumbnail cache), glass surfaces with fallback for reduced transparency, ambient tint, focus rings, reduced motion, contrast test from tokens.

**Specs:** 010, 011 (what is left), 013. **Reference:** `components/{general-settings,library-settings}.tsx`, `styles.css`, `design-tokens.test.ts`, `lib/{ambient,color}.ts`.
**Exit:** user approved side-by-side screenshots with the reference; REQs pass.

### M11 · Packaging and release

- Windows installer (NSIS via `cargo-packager` or similar) and portable zip, bundled exiftool and ffmpeg (and the video backend), release script with git-cliff and conventional commits, GitHub release workflow, README with screenshots.

**Specs:** 014. **Reference:** `scripts/release.ts` (+ test), `.github/workflows/release.yml`, `electron-builder.yml`, `cliff.toml`.
**Exit:** installs and runs on a clean Windows machine with the user's existing index; release dry run works.

## After the rewrite

Roadmap items from `../good-gallery/docs/roadmap.md` (auto updates, manual tagging, cropping, faces, photo editing, macOS and Linux) move into `docs/roadmap.md` here. The reference repo is archived.
