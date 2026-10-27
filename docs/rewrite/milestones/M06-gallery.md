# M06 · Gallery

**Status:** Done (approved 2026-10-27, self-approved: the user asked to work through the milestones without their input; M6a and M6b done 2026-10-27)
**Specs:** 006, 016, 017 (and REQ-005-02, -06, -11 reading side, -14 tiles)

Too large for one part (≈ 1 200 lines of reference UI code, texture streaming, a frame-time benchmark). Split:

- **M6a · Tile gallery:** layout math, gallery data, tiles with textures streamed from the thumbnail cache, zoom, scroll anchoring, live updates, states, the frame-time and memory budgets.
- **M6b · Views and toolbar:** a toolbar with the _View_ menu (view, zoom slider and buttons, group by day, tag indicators), the List view, date group headers, tag indicators, persisted choices.

## Approach

### M6a · Tile gallery

**Layout (`domain::layout`).** A port of `gallery-layout.ts`, `justified.ts`, `masonry.ts` and `aspect.ts` with their tests: `tile_layout` (Masonry: shortest column; Grid: squares; Justified: rows at ¾ of a Masonry column), groups with headers, `items_in_view`, `headers_in_view`, `first_item_from`, `group_of_item`, list positions. Struct of arrays (`Vec<f32>` top / left / width / height), binary search for the visible range. Aspects are clamped to 0.3–3 (`tile_aspect`).

**Date groups (`domain::date_groups`).** `date_groups` as the reference's shared function: local calendar days via `chrono::Local`, worked out again only when a date leaves the current day.

**Zoom (`domain::zoom`).** Columns 2–12 (default 5) and `accumulate_wheel` (100 per step, a change of direction starts over), with the reference's tests.

**Gallery queries (`db::gallery`).** `layout(conn, search)` returns ids, aspects (as `f32`, no integer packing needed) and, for date sorts, the day groups; `items(conn, ids)` returns tile data with the tag categories of each item (`categories_of`, the reference's recursive query, needed by 017 in M6b). `GallerySearch` holds only the sort (date taken, newest first) until M7 adds filters and sorts.

**Gallery state (`app::gallery`).** The layout query runs on a job thread (a few hundred ms for 500k rows) and replaces the current one when it arrives; tile data loads in chunks of 200 for the visible range, also on a job thread, from a read-only connection, and is remembered (up to 20 000 items) as in `use-gallery-media`. Positions without data show a gray placeholder (REQ-006-08).

**Tile view.** `egui::ScrollArea::show_viewport` with the layout's total height, so the scrollbar covers the whole result; only tiles intersecting the viewport ± 600 px are drawn, positioned from the layout (no widgets per tile beyond one `interact` for hover and the accessible name). Each tile: rounded rectangle with the ThumbHash placeholder (decoded to a tiny egui texture, cached per item), the thumbnail texture fading in over 150 ms, the "no image" icon for failed items (REQ-005-06, -14), a play badge with the duration for videos (REQ-006-09).

**Texture streaming (`app::textures`).** The spike's streamer: worker threads (4, below-normal priority) read through `ThumbCache::read` (REQ-005-11's view clock), decode with libwebp (`webp` crate, safe API), build the mip chain and create the `wgpu::Texture` themselves; the UI thread only registers finished textures within 3 ms per frame. Requests are a priority list replaced every frame: visible tiles, then 1.5 screens ahead, then 0.5 behind; stale work drops out. Width per tile from `pick_thumb_width` (REQ-005-02); a larger width replaces a smaller texture. A missing file is requested from the `ThumbnailService` unless the item is `error` (REQ-005-07) and loaded again when it answers. LRU with a byte budget (512 MB, as in M0: at 12 columns the visible tiles and the prefetch band alone take ≈ 330 MB) where requested textures count as used (REQ-006-13). Without a wgpu render state (tests on other backends) tiles keep their placeholders.

**Zoom and anchoring.** Ctrl + wheel over the gallery accumulates steps (REQ-006-06, slider and buttons in M6b); columns are stored in the `settings` table under `gallery.columns` (the Electron version ignores keys it doesn't know). Before a relayout (zoom, width, new data) the first visible item is remembered and scrolled back to the top afterwards (REQ-006-07).

**Live updates and states.** The gallery reloads its layout on the same schedule as the counts (REQ-006-11: after the first written files, every 5 s, at the end). Loading shows skeleton tiles, a failed query "Couldn't load media" with the error (REQ-006-10).

**Budgets (REQ-006-12, -13).** `cargo xtask gallery-bench` builds a data folder with 100 000 media items in a synthetic library whose thumbnails are hard links to a real cache copy (no 3 GB copy), and runs the app with `--bench-scroll <seconds>`: vsync off, 12 columns, continuous scrolling, frame intervals recorded after a warm-up pass (so Defender's first-read cost is out, as in M0); it prints p50 / p95 / max and the texture bytes. The budget: p95 < 16 ms.

### M6b · Views and toolbar

**Toolbar (`app::toolbar`).** A 48 px bar over the top of the gallery, Raised until the glass arrives (M10), with the tiles scrolling underneath (content starts 48 + 12 px down). For now it holds the _View_ button and Settings (the sidebar's place, M7); search and filters come in M7, the title-bar integration in M10 (the window keeps its native caption until then). The _View_ button shows the current view's icon, "View" and a chevron, and opens a popup (`egui::Popup`) with the reference's sections: _Layout_ (four toggle buttons named Masonry, Justified, Grid, List), _Zoom_ (zoom out, a slider from 12 to 2 columns, zoom in, "N columns" or "Not in the List view", disabled in the List view), _Group by day_ (a switch, disabled with "Only when sorted by date" for other sorts, which M7 adds), _Tag indicators_ (a switch, and Rating / Places / People / Tags toggles disabled while it is off). _Sort by_ belongs to the search (spec 007) and comes in M7.

**Choices.** Stored in the `settings` table under the reference's `localStorage` keys: `gallery.view` (`"masonry"` …), `gallery.groupByDay` (`"off"` turns it off), `gallery.tagIndicators` (`{"shown": …, "picked": […]}`), parsed as the reference does (`domain::view`).

**List view.** Rows of 52 px (thumbnail 40 px, name with a play icon and duration for videos, folder, date taken or the date modified dimmed with a tooltip, size, dimensions, rating stars) under a 36 px column header that stays at the top, group header rows of 40 px between them. Row positions are arithmetic (`i × 52 + headers before × (40 − 52)`), so no per-row table is needed; the visible range is a binary search. The same data loading and texture streaming as the tiles (40 px thumbnails pick the 400 px width).

**Date groups.** Headers of 40 px with the day ("Monday, 26 October 2026") and the counts ("42 photos · 3 videos"), bottom-aligned, in every view; the layout already places groups. Grouping follows `groups_shown` (on, and a date sort).

**Tag indicators.** `domain::view::indicators_of` (port with tests); a solid badge (`rgb(0 0 0 / 0.6)`) in the tile's bottom-left corner with Star-colored stars and white pin / person / tag icons, clipped so it never covers the video badge; each indicator has a tooltip, and the tile's accessible name lists them (REQ-017-02).

**Spec fix.** REQ-006-02 says "an 8 px gap"; the reference and `docs/design.md` use 4 px from 8 columns on (with 2 px tile corners). The spec is corrected.

## Reference

- `src/renderer/src/lib/{gallery-layout,justified,masonry}.ts` (+ tests), `src/shared/aspect.ts`, `src/shared/date-groups.ts` (+ test in `lib/date-groups.test.ts`), `hooks/use-ctrl-wheel-zoom.ts` (+ test), `hooks/use-columns.ts`, `hooks/use-gallery-media.ts`, `components/tile-gallery.tsx`, `components/media-tile.tsx`.
- `src/main/trpc/routers/media.ts` (`layout`, `byIds`, sort order with id tiebreak), `src/main/db/tag-categories.ts` (`categoriesOf`).
- `spikes/grid/src/stream.rs` (this repo): texture streaming, mip chain, LRU.

## Tasks

### M6a

- [x] 1. `domain::layout`: aspect clamp, Masonry, Grid, Justified, groups, view queries (REQ-006-02, -03, -04, REQ-016-05)
- [x] 2. `domain::date_groups` (REQ-016-01) and `domain::zoom` (REQ-006-06)
- [x] 3. `db::gallery`: layout query with groups, items with tag categories (REQ-006-08, REQ-016-01)
- [x] 4. `app::textures`: streaming, mips, LRU budget, thumbnail requests (REQ-006-13, REQ-005-02, -07)
- [x] 5. `app::gallery`: data loading, tile view, placeholders, badges, failed icon (REQ-006-08, -09, REQ-005-06, -14)
- [x] 6. Zoom with Ctrl + wheel, persisted; scroll anchoring (REQ-006-06, -07)
- [x] 7. Live updates, loading and error states (REQ-006-10, -11)
- [x] 8. `cargo xtask gallery-bench` and `--bench-scroll`; measure (REQ-006-12, -13)
- [x] 9. M6a docs: spec tables, architecture, pitfalls, progress; review; commit

### M6b

- [x] 10. Toolbar with the _View_ menu; persisted view choice (REQ-006-01, -06 slider and buttons)
- [x] 11. List view (REQ-006-05)
- [x] 12. Date group headers and _Group by day_ (REQ-016-02 … -06)
- [x] 13. Tag indicators (REQ-017-01 … -03)
- [x] 14. M6b docs; review; commit

## Exit criteria

From `docs/rewrite/plan.md`: frame-time budget met at 12 columns over 100k items; REQs of 006, 016, 017 pass.
