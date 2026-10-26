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

### Open: thumbnail cache layout vs Defender (decide in M5)

Defender scans every first read of a file (≈ 13 ms per thumbnail, verdicts expire within minutes); a pack file reads 300 thumbnails in 29 ms instead of ≈ 3.9 s. Keeping the reference's per-file layout keeps the cache compatible but leaves cold scrolling at 12 columns mostly placeholders.
