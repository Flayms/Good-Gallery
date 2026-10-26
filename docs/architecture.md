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

**Decided:** a test references a REQ at the start of its name (`fn req_012_01_reports_unknown_reqs()`) or in a doc comment directly above it (`/// REQ-012-01: …`). `cargo xtask check-specs` reads `.rs` files below `crates/` and `xtask/` and counts only their test part: from the first `#[cfg(test)]` on, or the whole file below `tests/` or `benches/`. REQs mentioned in production code don't count as coverage. Spec: 012.

### 2026-10-26 · Visual checks (M1)

**Decided:** UI changes are checked two ways: `egui_kittest` snapshot tests (wgpu renderer, PNGs in `crates/app/tests/snapshots/`, `UPDATE_SNAPSHOTS=1` rewrites them) and `cargo run -p app -- --screenshot <file.png>`, which renders a few frames, saves the window and exits, so a session can look at the real app without anyone at the screen.

### Open: thumbnail cache layout vs Defender (decide in M5)

Defender scans every first read of a file (≈ 13 ms per thumbnail, verdicts expire within minutes); a pack file reads 300 thumbnails in 29 ms instead of ≈ 3.9 s. Keeping the reference's per-file layout keeps the cache compatible but leaves cold scrolling at 12 columns mostly placeholders.
