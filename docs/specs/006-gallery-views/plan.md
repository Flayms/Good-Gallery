# 006 · Gallery views · Plan

Records the approach that was taken. Decisions that outlive this spec are in `docs/architecture.md`.

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
- [ ] Measure scrolling while thumbnails render in the background (rendering runs at normal thread priority; the bench ran with every thumbnail cached)
- [ ] Measure the glass pass's per-frame cost at 12 columns with `gallery-bench` (it rebuilds tile bind groups every frame)
