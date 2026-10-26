# M00 · Spikes

**Status:** Done (2026-10-26)
**Specs:** none (spikes are throwaway; results feed `docs/architecture.md` and M2)

## Approach

Three standalone crates in `spikes/`, each with its own `Cargo.toml`, lockfile and `target/` (not part of any workspace, since none exists before M1). No specs, no TDD, but every spike ends with measured numbers and a go / no-go, recorded under **Results** below and summarized in `docs/architecture.md`.

The milestone is split into two sessions:

- **M0a: grid + look.** The look spike needs a scrolling gallery behind its glass anyway, and glass cost has to be measured against the grid baseline. So both live in one crate, `spikes/grid`, with the look parts behind toggles.
- **M0b: video.** Independent, with its own native dependency (libmpv), in `spikes/video`.

### Pinned versions (checked on crates.io, 2026-10-26)

`eframe` / `egui` / `egui-wgpu` **0.37.0** (wgpu 30.0, skrifa + harfrust text stack, `VariationCoords` for variable fonts). libmpv from the mpv-dev build 20261021 (own FFI, see Spike 3). Spikes pin exact versions (`=0.37.0`) so M1 can take them over unchanged if they hold up.

### Test machine

Windows 11, AMD Radeon integrated graphics, 1920 px wide display at 60 Hz. All numbers are from this machine. They are a floor: a discrete GPU will only do better.

### Measuring frame time

At 60 Hz with vsync every frame interval is ≈ 16.7 ms, so "p95 < 16 ms" can't be read off vsynced intervals. Each bench run therefore records:

1. **Frame interval with vsync off** (`PresentMode::AutoNoVsync`): the go criterion, p95 < 16 ms.
2. **CPU time of `update`** (layout, visible range, texture bookkeeping, painting), to see where time goes.
3. **Dropped frames with vsync on**: intervals > 20 ms per 1 000 frames, which is what the user feels.
4. **Placeholder rate**: share of visible tiles still showing a placeholder per frame, i.e. whether streaming keeps up.
5. Process RSS and texture bytes at the end of the run.

Scroll is scripted (`--bench`): 1 440 px/s (the speed the reference was measured at, see its `known-pitfalls.md`: p95 ≈ 110 ms at 12 columns) and a 6 000 px/s fling, 20 s each, at 5 and 12 columns, justified and masonry, window 1920 × 1080. Results go to `spikes/grid/results/*.json` (git-ignored) plus a table here.

### Spike 1 · Grid

- **Data:** the user's real thumbnail cache (66 096 WebP files, 3.7 GB) is copied with `robocopy` to `%TEMP%\gg-spike\thumbs`. The original is never opened. Items are the copied files, repeated to 100 000 (and once to 250 000 to check that layout cost scales). The aspect ratio comes from the WebP header (VP8 / VP8L / VP8X), read once at startup on a thread pool, with startup time measured.
- **Layout:** quick port of `justified.ts` and the masonry part of `gallery-layout.ts` (throwaway, the real port with tests is M6). Visible range by binary search over `top`, as in `itemsInView`.
- **Painting:** egui `Painter` with one textured, rounded rect per visible tile. If that misses the budget, a second variant draws the tiles with a custom instanced wgpu pipeline through an `egui_wgpu` paint callback, and the plan records the gap between the two.
- **Texture streaming:** decode on a worker pool (`image` crate WebP vs `webp` / libwebp, decode time per thumbnail measured for both), upload to the GPU, LRU cache with a byte budget (default 512 MB). Requests are prioritized: visible first, then a prefetch band in the scroll direction; requests that scrolled out of reach are dropped before they're decoded. Thumbnail width per tile follows `pickThumbWidth` (400 or 800). Grey placeholder while loading (thumbhash comes with M5).
- **Upload path:** `ctx.load_texture` (egui-managed, uploaded in the frame's texture delta) vs creating `wgpu::Texture`s directly and registering them with `register_native_texture`. Large uploads in one frame are a likely hitch source, so uploads per frame are capped and the cap is measured.

### Spike 2 · Look (on top of the grid)

- **Glass bar:** a toolbar over the scrolling gallery: tiles under the toolbar are rendered into a small offscreen texture by the instanced pipeline (the spike owns the `wgpu::Texture`s, so it can draw them itself), blurred with a dual-Kawase downsample / upsample chain to approximate `blur(24px) saturate(1.6)`, composited in a paint callback with the Glass bar tint `rgb(28 28 28 / 0.72)` and the Rim hairline. A Glass panel (`blur(32px)`, 0.86) as a floating popover over the gallery. Measured: frame time with glass vs Raised fallback.
- **Text:** Atkinson Hyperlegible Next, variable TTF from `google/fonts` (`ofl/atkinsonhyperlegiblenext`, OFL; the reference's fontsource package ships only WOFF2). Weights 400 / 600 through `VariationCoords`, sizes 12 / 14 / 16. Tabular figures: try to request `tnum` through egui's shaping; if egui can't, bake `tnum` into a copy of the font (`pyftfeatfreeze`, fonttools) and use that as the numbers face. Check hinting options against the reference's Chromium rendering.
- **Title bar**, three variants:
  1. Native title bar, dark (DWM immersive dark mode, caption color Darkroom).
  2. No decorations, own drawn controls, `StartDrag` on empty toolbar space.
  3. Content in the title bar with the **native** caption buttons kept (subclass the window: `WM_NCCALCSIZE` to drop the caption, `WM_NCHITTEST` for drag area and buttons, so Windows 11 snap layouts still show on maximize hover).
  `docs/design.md` asks for variant 3; the spike says whether it is feasible with winit 0.30 and what it costs.
- **Screenshots:** in-app screenshots via `ViewportCommand::Screenshot` saved to PNG; the title bar variants need an OS screenshot (PowerShell `CopyFromScreen`). Side by side with `../good-gallery/docs/screenshots/` (justified, grid, viewer) for the user to approve.

### Spike 3 · Video (M0b)

- **Fixtures**, generated with the installed ffmpeg 7.1 into `%TEMP%\gg-spike\video`: 30 s, 1080p, with a tone and burned-in timecode (for checking A/V sync and seek accuracy): MP4 (H.264 + AAC), MOV (HEVC + AAC, plus a 4K variant and one with rotation metadata like a phone portrait video), MKV (H.264 + Opus, with chapters), WebM (VP9 + Opus). If the user has real phone videos, those are tried too (read-only, copied first).
- **Candidate 1, libmpv** (mpv-dev build for Windows x86_64 downloaded into `spikes/video/vendor/`, git-ignored; planned with `libmpv2` 6.0.0, replaced by an own FFI because that crate has no SW render API, see *Results*). Render with the SW render API into a buffer uploaded to a wgpu texture each frame (works with the wgpu backend as is). Measure CPU load and dropped frames at 1080p and 4K, with hardware decoding (`hwdec=auto-copy`). If SW rendering is too slow at 4K, try the OpenGL render API via a shared texture (WGL_NV_DX_interop) and note the complexity. Audio, seeking, volume and pause come from mpv.
- **Candidate 2, `ffmpeg-next` + `cpal`** only if libmpv fails: own decoding, audio output and A/V sync. Much more code to own, so only as a fallback.
- **GStreamer** is not tried unless both fail (large runtime to ship).
- **Licensing and shipping:** GPL-3 matches this repo's license. Record which DLLs ship next to the executable and their size.

### Out of scope

Real crates, tests, CI and anything in `crates/` (M1). The spikes don't read the user's database; aspect ratios come from the thumbnails.

## Reference

- `src/renderer/src/lib/gallery-layout.ts`, `justified.ts`, `masonry.ts`: layout math to port for the spike (row height ratio 0.75, justified rows, shortest-column masonry, `itemsInView` with `maxHeight` look-back).
- `src/shared/media-urls.ts`: thumbnail widths 400 / 800, `pickThumbWidth`.
- `src/indexer/thumb-cache.ts`: cache layout `<dir>/ab/cd/<sha1>.webp`, key `JSON.stringify([rootId, relPath, size, mtime, width])`. Not needed for the spike (it enumerates files), noted for M5.
- `src/renderer/src/styles.css`, `docs/design.md`: tokens (Darkroom `#1F1F1F`, Glass bar `rgb(28 28 28 / 0.72)` blur 24 px saturate 1.6, Glass panel `rgb(36 36 36 / 0.86)` blur 32 px saturate 1.4, Rim), type scale, gaps (8 px, 4 px from 8 columns), tile radius (4 px, 2 px from 8 columns).
- `docs/known-pitfalls.md` of the reference: 12-column scroll at 1 440 px/s drops frames (p95 ≈ 110 ms over 3 000 photos), glass added no measurable cost there.
- `docs/screenshots/*.png`: comparison targets for the look spike.
- `e2e/screenshots.ts`: how the reference generates its fixture videos with ffmpeg.

## Tasks

### M0a · Grid and look

- [x] 1. Copy the thumbnail cache to `%TEMP%\gg-spike\thumbs`; scaffold `spikes/grid` (eframe 0.37 on wgpu, pinned); item list from WebP headers, repeated to 100k; measure startup.
- [x] 2. Justified and masonry layout, visible range, zoom by column count (Ctrl + wheel), scrollbar.
- [x] 3. Texture streaming: worker pool, `image` vs libwebp decode, priority + cancellation, LRU byte budget, upload cap per frame, both upload paths.
- [x] 4. `--bench` mode with scripted scroll and the five measurements; run the matrix (5 / 12 columns × justified / masonry × 1 440 / 6 000 px/s, 100k and 250k items). Instanced-pipeline variant if egui painting misses the budget.
- [x] 5. Glass bar and Glass panel (offscreen tiles + dual Kawase), bench with glass vs Raised.
- [x] 6. Font: Atkinson Hyperlegible Next at 12 / 14 / 16, weights 400 / 600, tabular figures (or a baked `tnum` font), hinting comparison.
- [x] 7. Title bar variants 1–3.
- [x] 8. Screenshots next to the reference ones; user approves glass and text (or picks the Raised fallback).
- [x] 9. Results and go / no-go for grid and look recorded here and in `docs/architecture.md`, pitfalls added, `docs/progress.md` updated.

### M0b · Video

- [x] 10. Generate the fixture videos.
- [x] 11. `spikes/video` with libmpv: SW render into a wgpu texture, play / pause / seek / volume, all four containers, rotation.
- [x] 12. Measure CPU and dropped frames at 1080p and 4K; try GL interop if SW is too slow; fallback candidate only if libmpv fails.
- [x] 13. Results, go / no-go, shipping notes (DLLs, sizes, license) recorded here and in `docs/architecture.md`; `docs/progress.md` updated.

## Exit criteria

From `docs/rewrite/plan.md`:

- Three go / no-go results.
- Go: grid p95 < 16 ms at 12 columns (frame interval with vsync off, see *Measuring frame time*); video plays all four containers; glass and text are approved by the user from screenshots.
- A no-go on video or look stops the rewrite until the user decides (fallbacks: Raised surfaces instead of glass; Tauri with the existing UI).

Added here:

- Grid also reports dropped frames with vsync on and the placeholder rate at 6 000 px/s; no hard limit, but the numbers inform M6.
- The title bar result says which of the three variants M1 uses.

## Results

### Spike 1 · Grid: **Go**

Measured 2026-10-26 on the test machine, window maximized (1920 × 1009 client), 100 000 items, 4 decode threads, native upload, libwebp, files warmed first (see *Thumbnail IO* below). Raw JSON in `spikes/grid/results/` (git-ignored); rerun with `spikes/grid/matrix.sh`.

| Run | Frame interval p50 / p95 / p99 (ms) | UI CPU p95 (ms) | Placeholders (mean) |
| --- | --- | --- | --- |
| justified, 5 cols, 1 440 px/s | 4.3 / 6.0 / 8.4 | 0.2 | 0 % |
| justified, 5 cols, 6 000 px/s | 4.7 / 7.0 / 10.1 | 0.3 | 0 % |
| **justified, 12 cols, 1 440 px/s** | **5.2 / 7.2 / 10.9** | 0.4 | 0 % |
| justified, 12 cols, 6 000 px/s | 4.3 / 6.6 / 10.1 | 0.4 | 97 % |
| masonry, 5 cols, 1 440 / 6 000 px/s | 4.3 / 5.9 / 7.7 · 4.4 / 6.2 / 8.9 | 0.2 | 0 % |
| masonry, 12 cols, 1 440 / 6 000 px/s | 5.0 / 6.9 / 8.6 · 3.9 / 6.4 / 9.2 | 0.3 | 0 % · 94 % |
| justified, 12 cols, **250 000 items** | 5.1 / 7.3 / 9.8 | 0.4 | 0 % |
| justified, 12 cols, vsync on, 1 440 / 6 000 px/s | 16.7 / 18.7 · 16.7 / 28.9; > 20 ms: 15 / 263 per 1 000 | 0.5 · 0.8 | 0 % · 96 % |
| justified, 12 cols, egui upload path | 7.1 / 12.9 / 18.4 | 3.5 | 0 % |
| justified, 12 cols, `image` crate decoder | 5.0 / 10.5 / 15.9 | 0.3 | 94 % |
| justified, 12 cols, cold files | 4.2 / 6.7 / 8.9 | 0.3 | 81 % |

- **p95 at 12 columns: 7.2 ms** (budget 16 ms; the reference: ≈ 110 ms). Layout of 100 000 items takes 2 ms, 250 000 about 5 ms, so recomputing it on zoom or resize is fine.
- **What made the difference** (first attempt: p95 67 ms):
  1. Uploading on the UI thread cost ≈ 2.7 ms per texture. Workers now create the `wgpu::Texture` and write it themselves (`Queue` is `Send + Sync`); the UI thread only registers it with egui under a 3 ms budget per frame (≈ 0.03 ms each).
  2. 14 decode threads starved the UI thread and the integrated GPU (p95 67 ms); 4 threads at below-normal priority give the best frame times with no placeholders at 1 440 px/s.
  3. The LRU must count prefetched textures as used, or it evicts what it just loaded and loops (2 000 decodes in 6 s on a still screen).
- **libwebp vs `image`:** 5.4 vs 16 ms per thumbnail (mostly 800 px wide, 474 kpx). libwebp scaled decoding saves no decode time (VP8 decodes at full size), only memory; the real app reads the 400 px file instead.
- **egui-managed textures** have no mipmaps (`mip_level_count: 1` in egui-wgpu), so 800 px thumbnails at 150 px tiles alias, and uploads run on the UI thread. Own textures with a CPU mip chain are both faster and better looking.
- **6 000 px/s at 12 columns:** decoding can't keep up (≈ 650 tiles/s needed). With vsync on this run also misses vblanks (26 %), probably from texture writes of tiles that are already gone. M6: skip decoding while the scroll velocity is above a threshold and show placeholders (thumbhash), then fill in when it slows down.
- **Instanced-pipeline variant:** not needed; egui's painter is far under budget.

#### Thumbnail IO: Windows Defender

Every **first** read of a thumbnail file costs ≈ 13 ms (single thread; ≈ 21 ms p50 under parallel load) because Defender scans it on open; a re-read costs 0.16 ms. The verdict cache expires within minutes or by count: files read 35 minutes earlier were cold again. 300 thumbnails packed into one 19 MB file read in **29 ms** instead of ≈ 3 900 ms. The reference reads the same per-file cache and pays the same cost.

Cold runs keep frame times (p95 6.7 ms) but show 81 % placeholders at 12 columns. **Decision needed in M5:** keep the reference's one-file-per-thumbnail layout (compatible, slow cold reads) or move to a pack file (fast, needs a migration and breaks sharing the cache with the reference). Rejected for now: asking users to add a Defender exclusion.

### Spike 2 · Look: **Go** (glass and text approved by the user from screenshots, 2026-10-26)

- **Glass** (dual Kawase over an offscreen render of the tiles under the surface, composited with saturate, tint and rounded corners in an `egui_wgpu` paint callback): compared with Chromium's `backdrop-filter` on the same frame (headless Edge, `results/css-ref.html`), mean color within 1–2 levels; the toolbar is slightly blurrier at 4 passes (stddev 9.6 vs 11.9), 3 passes is slightly sharper (13.7). Default: 4 passes for both surfaces; fine-tune the Kawase offset in M10.
- **Glass cost:** p95 at 12 columns 7.6 ms Raised → 8.0 ms with the glass bar → 8.9 ms with the glass popover too (≈ 0.4 ms per surface). At 5 columns 6.6 → 7.1 → 7.4 ms.
- **Font:** the variable TTF from google/fonts loads in egui 0.37; weights through `FontTweak::coords` (the same file registered at `wght` 400 and 600). egui has no API for OpenType features, so tabular figures come from a copy with `tnum` baked in (`pyftfeatfreeze -f tnum`; all digits 632 units wide afterwards). Rendering at 12 / 14 / 16 px is on par with Chromium's; hinting on / off / default differ only by single pixels, so the default stays.
- No bundled font has U+2605 (★); rating stars are drawn as shapes (the reference uses an icon).
- **Title bar:** variant 3 (`hybrid`) works with winit 0.30 through a window subclass: `WM_NCHITTEST` reports `HTMINBUTTON` / `HTMAXBUTTON` / `HTCLOSE` on the drawn buttons, `HTCAPTION` on empty toolbar space and `HTTOP` on the top edge (checked with `titlebar-check.ps1`), and keeps the native shadow, resize and snap. Variant 1 adds a second 32 px bar above the toolbar; variant 2 reports `HTCLIENT` everywhere (no snap layouts, no top-edge resize). **M1 uses variant 3.** Snap layouts on hover still need a manual check with a real mouse.
- Screenshots for approval (local only, they show the user's photos): `spikes/grid/results/compare-window.png` (whole window next to the reference), `compare-text.png` (sidebar at 2×), `compare-glass.png` (glass vs Chromium), `compare-titlebar.png`, `compare-hinting.png`.

### Spike 3 · Video: **Go** (libmpv)

Measured 2026-10-26 on the test machine (on mains power, see *Power* below), window maximized, 20 s of playback per run after the first frames, then 8 exact seeks while paused. Raw JSON in `spikes/video/results/` (git-ignored); rerun with `spikes/video/fixtures.sh` and `spikes/video/matrix.sh`.

**Setup that won:** libmpv (`vo=libmpv`, `hwdec=auto-copy`) with the software render API on a render thread. The thread renders `rgb0` at the size the video occupies on screen and writes it into a `wgpu::Texture` itself (`Queue` is `Send + Sync`). Timing is **display-synced**: `video-sync=display-resample`, `override-display-fps`, the UI wakes the render thread once per presented frame, which calls `mpv_render_context_report_swap` and renders only when mpv's next-frame info isn't a repeat.

| Fixture (display-synced) | Decoder | CPU (1 core = 100 %) | Render p50 (ms) | mpv drops / 600 | Judder / 600 | A/V sync at end | Seek restart median / max (ms) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| MP4 H.264 + AAC, 1080p | d3d11va-copy | 91 % | 2.9 | 4 | 40 | +14 ms | 224 / 309 |
| MP4 H.264, 1080p, 1 s GOP | d3d11va-copy | 97 % | 2.5 | 3 | 26 | +3 ms | 74 / 236 |
| MOV HEVC + AAC, 1080p | d3d11va-copy | 103 % | 2.4 | 0 | 1 | +0.5 ms | |
| MOV HEVC, 1080p, rotated 90° | d3d11va-copy | 97 % | 2.5 | 0 | 3 | −1 ms | |
| MKV H.264 + Opus, chapters | d3d11va-copy | 107 % | 2.6 | 0 | 7 | +0.1 ms | |
| WebM VP9 + Opus, 1080p | d3d11va-copy | 117 % | 3.2 | 0 | 8 | −14 ms | 156 / 473 |
| MP4 H.264, 2160p | d3d11va-copy | 190 % | 5.2 | 0 | 26 | +9 ms | |
| MOV HEVC, 2160p | d3d11va-copy | 215 % | 5.7 | 1 | 19 | +7 ms | 576 / 789 |
| MOV HEVC, 2160p, 1 s GOP | d3d11va-copy | 196 % | 5.4 | 2 | 15 | +15 ms | 208 / 518 |
| *audio-synced* H.264 1080p (2 runs) / HEVC 2160p | d3d11va-copy | 97–124 % / 210 % | (blocks) | 0 | **45–265** / 7 | 0 | |
| *native size* H.264 1080p / HEVC 2160p | d3d11va-copy | 68 % / 341 % | 2.0 / **9.5 + 9.2** | 7 / 2 | 30 / **177** | | |
| *software decoding* H.264 1080p / HEVC 2160p / VP9 1080p | no | 141 % / 254 % / 136 % | 3.1 / 4.8 / 3.2 | 0 / 3 / 0 | 18 / 13 / 7 | | |

*Judder* = video frames whose on-screen interval was outside 25–42 ms (each 30 fps frame should stay for exactly 2 vblanks at 60 Hz). *Render* is mpv's software render (scaling + YUV → RGB); forcing alpha and `write_texture` add ≈ 0.7 ms each at display size. CPU is the whole process; the machine has 16 logical cores, so 100 % ≈ 6 % of the machine.

- **All four containers play** (plus rotation and 4K): MKV reports its 3 chapters, Opus and AAC go out through WASAPI, every exact seek lands on its target (152 seeks, largest error 2e-15 s). Play / pause, seek (keyframe seeks while dragging, exact on release), volume and arrow keys work through mpv properties and commands.
- **Display-synced timing is required.** With mpv's default timing (frames timed to the audio clock, `mpv_render_context_render` blocking until the frame's display time) the frame lands at a random phase against our vsync, and 45–265 of 600 frames show for 1 or 3 vblanks instead of 2, varying run to run. Display-synced: 1–40.
- **Render at display size, not at the video's size.** 4K at native size costs 9.5 ms render + 5 ms alpha + 4 ms upload per frame and judders (177 / 600); scaled to the window it costs 5.7 ms on the render thread.
- **4K on the integrated GPU:** hardware decoding doesn't save CPU over software decoding at 4K (copy-back + scaling ≈ 2 cores either way), and the UI misses a vblank in ≈ 11–13 % of frames while 4K plays (idle window: 2–4 %, 1080p: 1–10 %). Acceptable for a gallery's viewer; a zero-copy path is the lever if M9 needs more.
- **GL interop (WGL_NV_DX_interop) not tried:** the software path meets the bar at 4K, and interop would need a WGL context next to wgpu's D3D12 / Vulkan device plus a shared texture. Noted as the option for zero-copy. Candidate 2 (`ffmpeg-next` + `cpal`) and GStreamer weren't needed.
- **Rotation:** the software renderer ignores the display matrix (`video-params/rotate` = 90, but frames come out landscape and `dwidth` / `dheight` stay unrotated). The UI rotates the quad's UVs instead (free on the GPU); the result matches ffmpeg's autorotate.
- **Seek latency** depends on the GOP: exact seeks decode from the previous keyframe. x264's default GOP (250 frames, 8 s) is the worst case (≈ 0.2–0.3 s at 1080p, ≈ 0.6–0.8 s at 4K); a 1 s GOP like phone videos 74 ms median at 1080p.
- **Not done:** real phone videos (none provided); audio was checked through mpv's state (WASAPI output open, A/V sync), not by ear.

#### Binding

`libmpv2` 6.0.0 only wraps the OpenGL render API, so the spike uses its own FFI (`spikes/video/src/mpv.rs`, ≈ 300 lines) and loads `libmpv-2.dll` at runtime with `libloading`. Runtime loading also means a missing or broken DLL disables video instead of preventing startup.

#### Shipping and license

- One DLL: `libmpv-2.dll` from shinchiro/mpv-winbuild-cmake release 20261021 (`mpv-dev-x86_64-20261021-git-e471496c56.7z`, baseline x86-64; a `-v3` AVX2 build exists). **121 MB** uncompressed (47 MB gzipped, 31.6 MB in the 7z). FFmpeg and every other library is linked statically.
- Imports only system DLLs, but **`vulkan-1.dll` is a hard import**: on a machine without the Vulkan loader (old GPU drivers, some VMs) the DLL doesn't load, so loading at runtime and failing soft matters.
- The build is GPL (mpv + FFmpeg with GPL parts), compatible with this repo's GPL-3; a release must point to the exact sources (M11). A trimmed custom build (only the decoders and demuxers we need, no Vulkan / shaderc / libplacebo) would cut the size a lot; also M11.

#### Power

The first matrix ran on battery (9 %, `Win32_Battery.BatteryStatus` = 1): an idle, continuously repainting window measured 30 ms per frame in one run and 16.7 ms in the next. All numbers above are from a rerun on mains power.
