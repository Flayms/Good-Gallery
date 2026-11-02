# 006 · Gallery views · Plan

Records the approach that was taken. Lasting decisions are under _Decisions_ at the end.

## Approach

- **Layout** (`domain::layout`): `tile_layout` positions every tile at once (Masonry: shortest column; Grid: squares; Justified: rows at ¾ of a Masonry column), with group headers. Positions are `f64` (a 500k-item gallery is taller than `f32` places to the pixel), so the scroll length is exact and the visible range is a binary search. Aspects are clamped to 0.3–3. Gap 8 px, 4 px from 8 columns on.
- **Zoom** (`domain::zoom`): 2–12 columns (default 5); Ctrl + wheel accumulates 100 per step, a change of direction starts over. egui turns Ctrl + wheel into a zoom delta, so the raw wheel events are read.
- **Data** (`db::gallery`): `layout(search)` returns every match's id and clamped aspect in sort order, plus the day groups of a date sort; `items(ids)` returns tile data with the tag categories of each item. A loader thread with its own read connection runs the queries; tile data loads in chunks of 200 for the visible range ± 600 pt and is remembered up to 20,000 items. Positions without data show a gray placeholder.
- **Tiles** (`app::gallery::tiles`): only tiles near the viewport are drawn, each a textured `RectShape` (cropped like `object-fit: cover`) over its ThumbHash placeholder, fading in over 150 ms, with a play badge and duration for videos and the "no image" icon for failed items; one `interact` per tile for hover, clicks and the accessible name.
- **Textures** (`app::textures`): 4 below-normal-priority workers read through `ThumbCache::read`, decode with libwebp, build the mip chain and create the `wgpu::Texture`; the UI thread registers finished ones within 3 ms per frame. Requests are a priority list replaced every frame (visible, 1.5 screens ahead, 0.5 behind). LRU with a 512 MB budget where requested textures count as used (REQ-006-13: at 12 columns the visible tiles and the prefetch band take ≈ 330 MB). Missing thumbnails are requested from the `ThumbnailService`.
- **Anchoring** (REQ-006-07): the first visible media item is the anchor; after a relayout (zoom, width, new data) it is scrolled back to the top. A window resize lays out once the width has settled (120 ms).
- **Toolbar and _View_ menu** (`app::toolbar`): a 48 px bar over the top of the gallery, which scrolls underneath. The _View_ popup holds layout, zoom (slider and buttons, disabled in the List view), sort, _Group by day_ and tag indicators. Popups use fixed ids: a button's automatic id changes with its look, which closed the menus.
- **List view** (`app::gallery::list`): 52 px rows (thumbnail, name with play icon and duration, folder, date taken or the dimmed date modified, size, dimensions, rating) under a 36 px column header, 40 px group header rows. Positions are arithmetic over the group starts (`domain::layout::ListRows`), no per-row table.
- **Choices** are stored in the `settings` table under `gallery.view`, `gallery.columns`, `gallery.groupByDay`, `gallery.tagIndicators` (`domain::view`).
- **Live updates and states** (REQ-006-10, -11): the layout reloads after the first written files, every 5 s during a scan and at its end; skeleton tiles while loading, "Couldn't load media" on errors, empty states for no libraries and no media.
- **Budgets** (REQ-006-12, -13): `cargo xtask gallery-bench` builds a data folder with 100,000 items whose thumbnails are hard links into a cache copy and runs the app with `--bench-scroll` (vsync off, a warming pass, then 20 s at 1,440 pt/s, 12 columns). Result (AMD iGPU, mains power): p50 3.0 ms, p95 4.4 ms, max 11.9 ms; textures peaked at 511 MB.
- **Scrolling while thumbnails render** (REQ-006-15): `gallery-bench --originals <photos>` links 100,000 originals into a real library with nothing cached and runs `--bench-scroll --bench-cold` (no warming pass), then prints how many thumbnails rendered. The render threads run at below-normal priority, like the texture workers. Measured on the Linux dev VM (2 cores, lavapipe: the UI renders on the CPU, so absolute times are far above the budget and only the comparison counts), 15 s each, two runs: warm cache p95 223 ms; cold at normal priority p95 527 / 535 ms (108 / 118 thumbnails rendered); cold at lowered priority p95 307 / 331 ms (25 / 27 rendered). Lowering the priority takes most of rendering's cost off the frame time; where the CPU is saturated (software rendering) thumbnails then appear more slowly. Not yet measured on an integrated laptop GPU against the 16 ms budget.

## Spec changes

- **REQ-006-02:** the gap is 8 px, 4 px from 8 columns on (as `docs/design.md` says).

## Tasks

- [x] Layout: aspect clamp, Masonry, Grid, Justified, groups, view queries (REQ-006-02 … -04)
- [x] Gallery queries and loader thread (REQ-006-08)
- [x] Texture streaming with mips and the LRU budget (REQ-006-13)
- [x] Tiles: placeholders, fade, badges, failed icon (REQ-006-08, -09)
- [x] Ctrl + wheel zoom, persisted; scroll anchoring (REQ-006-06, -07)
- [x] Live updates, loading and error states (REQ-006-10, -11)
- [x] `cargo xtask gallery-bench` and `--bench-scroll` (REQ-006-12)
- [x] Toolbar with the _View_ menu, persisted choices (REQ-006-01, -06)
- [x] List view (REQ-006-05)
- [x] `gallery-bench --originals`: measure scrolling while thumbnails render; render threads at below-normal priority (REQ-006-15)
- [ ] Measure REQ-006-15 on an integrated laptop GPU
- [x] Measure the glass pass's per-frame cost at 12 columns with `gallery-bench` (it rebuilds tile bind groups every frame): within budget, recorded in spec 013's plan (#14)

## Decisions

### 2026-10-26 · Thumbnail textures

**Decided:** thumbnails are our own `wgpu::Texture`s (`Rgba8Unorm`, CPU-built mip chain), decoded with libwebp and created and written on worker threads; the UI thread only registers them with the egui renderer, within 3 ms per frame. 4 decode workers at below-normal thread priority. LRU cache with a byte budget (512 MB); textures in the prefetch band count as used. Requests are a priority list replaced every frame (visible, then 1.5 screens ahead, 0.5 behind), so stale work drops out.
**Why:** uploading on the UI thread cost 2.7 ms per texture; egui-managed textures have no mipmaps (aliasing at high zoom); more decode threads starved the UI thread and the iGPU; libwebp decodes 3× faster than the `image` crate.
**Rejected:** `ctx.load_texture` (p95 12.9 vs 7.2 ms, no mips), `image` crate WebP decoding.

### 2026-10-27 · Gallery: layout up front, data and textures on demand

**Decided:** `db::gallery::layout` returns every match's id and clamped aspect in sort order (and the day groups of a date sort); `domain::layout::tile_layout` positions all tiles at once (f64), so the scroll length is exact and the visible range is a binary search. A loader thread with its own read connection runs the gallery's queries (and the suggestions and sidebar lists); tile data loads in chunks of 200 for the visible range ± 600 pt. Only tiles near the viewport are drawn. Measured with `cargo xtask gallery-bench` (100,000 items, 12 columns, vsync off, AMD iGPU): p95 4.4 ms, max 11.9 ms. View choices are stored in the `settings` table (`gallery.view`, `gallery.columns`, `gallery.groupByDay`, `gallery.tagIndicators`); the List view's row positions are arithmetic over the group starts.
