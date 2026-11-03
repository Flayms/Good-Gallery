# Architecture

How Good Gallery is built today. It describes the current state, not its history: why something was built a certain way is under _Decisions_ in the `plan.md` of its spec, and project-wide decisions are in `docs/decisions.md`. Terms are defined in `docs/glossary.md`, the visual design in `docs/design.md`.

## Overview

Good Gallery is one Rust binary (eframe / egui on wgpu) plus three helper programs it starts as separate processes: exiftool (metadata, writing ratings and tags), ffmpeg (video frames, AVIF) and libmpv (video playback, loaded as a library). Windows bundles the helpers; Linux uses the distribution's. The same UI also builds for `wasm32` and runs as a static page in the browser, without helpers (spec 019).

```
            ┌────────────────────────── app (eframe, UI thread) ──────────────────────────┐
            │ AppState · gallery · sidebar · viewer · editor · settings · glass · theme  │
            └──────┬─────────────────┬──────────────────┬──────────────────┬─────────────┘
       commands /  │ events   reads  │          requests│ textures         │ frames
                   ▼                 ▼                  ▼                  ▼
   indexer (threads): scans,    db (SQLite WAL):   textures / originals   video (libmpv:
   watcher, schedule,           index, settings    (decode workers)       render + event
   thumbnail service                 ▲                  ▲                 threads)
        │      │                     │                  │
        │      └── writes ───────────┘          thumbnail cache (WebP files)
        ▼
   media: exiftool pool, ffmpeg, thumbnail rendering, editing, writing ratings and tags
```

## Crates

```
crates/
  domain    pure types and logic: media, tags, search model, gallery layout, date groups, zoom and viewer math,
            edit recipes, selection. No IO.
  meta      metadata rules (exiftool-named tags → MediaMetadata) and pure-Rust EXIF / XMP / IPTC / container readers.
            No IO.
  db        rusqlite: migrations, connections, index writer, gallery and tag queries, settings
  media     exiftool process pool, metadata extraction, thumbnail rendering, originals, ffmpeg, editing, subject
            detection, face models and their store, writing ratings, tags and face regions, helper lookup (desktop only)
  indexer   scan queue, walk, scans, watcher, schedule, reachability, thumbnail cache and service, face search,
            library operations
  app       eframe binary: UI, texture streaming, glass, viewer, video, editor, settings; `app::web` for the browser
xtask/      check-specs, libraries CLI, benchmarks, screenshots, dist, release, web, web-test
web/        the browser page: index.html (content security policy), browser check, Trunk config; Playwright tests
packaging/  helper downloads, NSIS installer, smoke test (windows/); .desktop file, install script (linux/)
```

Dependencies point one way:

```
domain ◄── meta ◄── media ◄──┐
   ▲                          ├── indexer ◄── app
   └────── db ◄───────────────┘
```

- `domain` and `meta` do no IO, so layout, search, tag and metadata rules are tested without files, a database or a GPU. New pure logic goes there.
- `db` knows SQL and the schema, nothing about files. `media` knows files and helper processes, nothing about the index.
- `indexer` joins them: it walks folders, reads files through `media` and writes the index through `db`.
- `app` is a library (`app::ui`, `AppState`) with a thin `main.rs` / `desktop.rs`, so the UI tests drive the same code as the window.
- `media` is desktop only; in the browser `meta`'s readers and `app::web` take its place.

## Threads and data flow

There is no async runtime: work runs on OS threads that talk over channels (`crossbeam-channel`) and wake the UI with `request_repaint`.

| Thread | Owns | Talks to the UI by |
| --- | --- | --- |
| UI (eframe) | `AppState`, a read connection for library rows | — |
| Indexer | the write connection (`IndexWriter`), the scan queue, the watchers | `IndexerEvent`s, applied in `AppState::poll` |
| Scan workers (`io_concurrency`) | stat and metadata reads of one scan | results to the indexer thread |
| Scheduler | a read connection | queues scans on the indexer |
| Thumbnail service (`io_concurrency`) | rendering, its own write connection | a callback per finished thumbnail |
| Faces (1) | its own write connection, the loaded face models | a status the settings read, a repaint |
| Gallery loader | a read connection | layout, tile data, suggestions, sidebar lists |
| Texture workers (4) | thumbnail decode, mips, `wgpu::Texture` creation | finished textures, registered by the UI within 3 ms per frame |
| Originals workers (2) | full-size decodes for the viewer | finished textures |
| mpv render / event | libmpv's render context and events | a published frame per load |
| Background jobs | reachability, folder dialog, cache clearing | an event applied in `AppState::poll` |
| Writer (1) | every write into a library file: ratings, tags, face regions, saving and reverting edits, one after another in the order made (REQ-024-07) | an event applied in `AppState::poll` |

- The UI never waits for IO. Each frame it applies what arrived (`AppState::poll`), draws, and sends requests; requests are priority lists replaced every frame, so work that scrolled out of view drops out.
- Every job runs under `catch_unwind`: a panic costs that file or that job, not the app.
- Library files are never renamed over: `media::replace` writes a new version beside the file, moves the original aside, renames the new one into the free name and reads it back, putting the original back if the file system lost it (REQ-024-06, #140).
- Tests call `AppState::wait_for_jobs` before stepping the harness, so they never race the threads. OS dialogs and the video player sit behind traits (`FolderPicker`, `video::Playback`) with fakes in tests.

## Storage

- **Index:** SQLite in WAL mode in the data folder. One write connection (the indexer's), read-only connections for readers. Migrations are drizzle-compatible SQL files in `crates/db/migrations/`, so indexes made by earlier versions keep opening (REQ-001-08); a schema change is a new migration, never an edited one.
- **Settings:** the `settings` table of the index, validated by `db::settings`.
- **Faces:** `detected_faces`, `face_embeddings`, `face_rejections` and `faces_scanned` in the index. Names aren't there: a detected face overlapping a named region in `media_faces` is that person, worked out when read. Models are files named by their SHA-256 in `<data>/face-models/`, with a note on what each is.
- **Thumbnail cache:** one WebP file per thumbnail and width under `<data>/thumbs/`, named by a hash of the file's identity, LRU by file mtime within a size limit.
- **Data folder:** `%APPDATA%\good-gallery` (Windows) or `$XDG_DATA_HOME/good-gallery` (Linux) for the packaged app; development builds use `good-gallery-rust`. Logs are in `logs/` there.
- **User files:** the app writes into them only when the user asks: ratings, tags and confirmed face regions (through exiftool, into the file or its XMP sidecar), and saved edits (with the recipe in our `gg:` XMP namespace; replaced originals are kept in `<library>/.good-gallery/originals/`). Indexing then reads them back with its normal rules.
- **Browser:** the index is an in-memory SQLite database with a snapshot in the Origin Private File System; thumbnails are OPFS files with the desktop cache's names.

## Rendering

- egui draws everything with its own painter on wgpu. Thumbnails, originals and video frames are our own `wgpu::Texture`s (with mips), created off the UI thread and registered with egui's renderer.
- The gallery lays out all tiles up front (`domain::layout`), so the scroll length is exact; only tiles near the viewport are drawn and only their data and textures are loaded. A byte budget (LRU) bounds GPU memory.
- **Glass** (`app::glass`) blurs what lies under the toolbar, menus and the viewer's chrome in a paint callback: a recorded backdrop rendered at half resolution, a dual-Kawase blur, then tint and rounded corners. Without a wgpu render state (kittest) or with reduced transparency (the setting, or the OS), surfaces are opaque.
- **Photo editing** previews the recipe with `edit.wgsl` in a paint callback over the original's texture; saving runs the same formulas on the CPU at full resolution (`media::edit`).
- **Theme:** `theme` holds a dark and a light palette named by role; `theme::begin_frame` picks the current one each frame.

## Platforms

Platform code sits behind `cfg(windows)`, `cfg(target_os = "linux")` and `cfg(target_arch = "wasm32")`; `app::WEB` hides desktop-only actions in shared code.

| | Windows | Linux | Browser |
| --- | --- | --- | --- |
| Window | own caption buttons; `WM_NCHITTEST` answers natively (snap layouts) | undecorated; GNOME-style buttons and resize handles drawn by `app::titlebar` | a canvas in the page |
| Helpers | bundled; ended with the app by a job object | from the distribution; `PR_SET_PDEATHSIG` | none: `meta`'s readers and the browser's own image and video decoding |
| Threads | as above | as above | one wasm instance on the main thread; slow work is the browser's, our own is sliced (`app::web::Slice`) |
| Package | NSIS installer, portable zip | `.tar.gz` with an install script | static files on GitHub Pages |

## Conventions

- **Errors:** `thiserror` enums in library crates, `anyhow` in `app` and `xtask`. **Logging:** `tracing`, to stderr and `logs/app.log`.
- **Network:** the desktop app connects only to download a face model the user asked for (spec 032). The browser version sends nothing (REQ-019-12); `xtask/tests/no_network.rs` keeps it that way.
- **Specs and tests:** every test that covers a REQ names it (`fn req_007_01_…` or a doc comment); `cargo xtask check-specs` checks it.
- **Visual checks:** `egui_kittest` snapshots (`UPDATE_SNAPSHOTS=1`), and `cargo run -p app -- --screenshot <file.png>` for the real window with glass.
- **Pinned UI crates:** egui, eframe, egui_kittest and wgpu are pinned to exact versions.
