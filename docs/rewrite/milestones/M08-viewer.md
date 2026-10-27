# M08 · Photo viewer and filmstrip

**Status:** Approved (2026-10-27, self-approved: the user asked to work through the milestones without their input; M8a done 2026-10-27)
**Specs:** 009 (without REQ-009-06 video playback, M9), 015, REQ-013-10 … 14, REQ-016-06 (scroll to the viewed item)

Too large for one part. Split:

- **M8a · Viewer core:** open, step, close, original quality, zoom and pan, click to zoom, preloading, instant preview, failures, missing items, controls that step aside, the scrim.
- **M8b · Viewer panels:** info panel and info sheet, filter by tag, Show in Explorer, context menus, filmstrip, photo-tinted chrome, tile ↔ viewer transition.

## Approach

### M8a · Viewer core

**Pure logic (`domain::viewer`).** The reference's `lib/zoom.ts` with its tests: `ZoomView { scale, x, y }` (offset of the content's center from the viewport's center, scale 1 = fitted), `clamp_view`, `zoom_to` (keeps the point under the pointer in place), `fit_size` (never enlarged), `toggle_scale` (fitted ↔ actual pixels, at least 2×, at most 8×). From `lib/preload.ts`: `neighbours(count, index)` (two places each way, nearest first). From `media-viewer.tsx`: `controls_hidden(idle, hovering, info_open, video_paused)`.

**Originals (`media::original`).** `load_original(file, ffmpeg)` decodes the whole file upright: JPEG through `jpeg-decoder` at full size (EXIF orientation), other formats through the `image` crate, AVIF through ffmpeg (full-size PNG frame). Larger than the GPU's texture limit (8192 px for wgpu's default limits) it is shrunk to fit with Lanczos3; the original stays the source (REQ-009-04), not the thumbnail cache.

**Original textures (`app::originals`).** Two worker threads load the wanted set: the shown photo first, then its neighbours nearest first (REQ-009-13). `set(wanted)` replaces the set; queued requests outside it never start, finished textures outside it are freed, and a decode that finishes after its item left the set is dropped. Like the thumbnail streamer, a worker builds the mip chain and creates the `wgpu::Texture` itself, so a 24 MP photo never stalls a frame; without a wgpu render state (tests) the decoded image goes through `egui::Context::load_texture`. A failed load is remembered with its message (REQ-009-15) until the item is shown again.

**Viewer (`app::viewer`).** `AppState::viewer: Option<Viewer>` holds the shown item (gallery position and media id), the zoom view, the idle timer and a pending backdrop click. It draws in a foreground `Area` over the whole window: a dark scrim (blur follows with the glass in M10), the media fitted into the window minus room for the controls (64 px left and right, 48 px top, 80 px bottom), _Previous_ / _Next_ round buttons at the sides and a control bar at the bottom (file name, date, rating, _Close_). The gallery keeps drawing below; the foreground layer takes the pointer, and the viewer consumes its keys before the gallery sees them.

- **Open** (REQ-009-01): a click on (or Enter on a focused) tile or list row asks the gallery to open that position; `AppState` creates the viewer. While it is open the gallery scrolls the shown item into view when it isn't (REQ-016-06), so closing lands on it.
- **Step** (REQ-009-02): ← / → and the buttons move one gallery position; no _Previous_ on the first, no _Next_ on the last. Item data loads with the gallery's chunks. Each step resets the zoom.
- **Close** (REQ-009-03): Escape, _Close_, the mouse's back button, Alt + ←, or a click on the scrim. A scrim click closes only after 250 ms without a second click: a double-click there zooms instead. Clicks on the photo, the end of a pan and double-clicks never close.
- **Zoom and pan** (REQ-009-05, -16): wheel around the pointer (`exp(-delta · 0.002)`, a wheel line counts 100 points), `+` / `=` / `-` around the center by 1.25×, `0` fits, a click on the photo toggles fitted ↔ actual pixels around the pointer (the second click of a double-click doesn't switch back), a double-click on the scrim does the same, dragging pans a zoomed photo (egui's own drag threshold separates clicks from drags). Cursor: zoom-in over a fitted photo, grab / grabbing over a zoomed one, default around it.
- **Instant preview** (REQ-009-14): the item's thumbnail texture, if the gallery's streamer has one, else the largest thumbnail is requested ahead of the tiles; the ThumbHash placeholder only without a thumbnail. Drawn at the place and size the original takes; the original fades in over it (`DURATION_BASE`), or shows at once when it was already loaded when the item came up.
- **Failures** (REQ-009-15): "Can't load the original. Showing a preview." in a pill over the bottom of the photo; the preview stays.
- **Missing items** (REQ-009-12): after a reload the viewer finds its media id in the new result; if it is gone it shows "Media not found" (stepping still works from its old position).
- **Controls step aside** (REQ-013-10): after 2.5 s without pointer movement or key presses the control bar and the buttons fade out (`DURATION_BASE`) and the pointer hides; they stay while the pointer is over them (info panel and paused video join in M8b / M9).
- **Videos** show their preview until M9 plays them; they don't zoom.
- **Scrim** (REQ-013-14, partial): `rgb(0 0 0 / 0.8)` until the blur of M10, which brings it to the design's 0.5.

Differences from the reference: no `byId` query, the viewer reads the item from the gallery's loaded tile data (it has every field the viewer shows in M8a); originals are capped at the GPU's texture size (a browser tiles larger images itself).

### M8b · Viewer panels

**Info panel and sheet** (REQ-009-08, -11): a `media_detail(id)` query (all REQ-009-08 fields plus tags with their category, the library's label and path), a 320 px panel on the viewer's right (the media area shrinks so it never covers the photo) toggled with `i` / _Info_; the same content in a sheet beside the gallery from the context menu's _Information_. Dates as REQ-013-13, the path selectable. **Filter by tag** (REQ-009-09): a click adds the tag as included (keeping the other filters) and closes the viewer.

**Show in Explorer** (REQ-009-10): `explorer.exe /select,"<path>"` started detached from the app's job object (`CREATE_BREAKAWAY_FROM_JOB`, as exiftool's job object kills children on exit); a missing file shows an error toast. **Context menus** (REQ-009-11) on tiles, rows and the viewer's media: _Show in Explorer_, _Information_.

**Filmstrip** (spec 015): a strip of square thumbnails along the bottom, toggled with `f` / the button and stored as the reference's `viewer.filmstrip` setting; the shown item highlighted and kept in view; a click jumps; wheel and drag browse; it hides with the controls; videos marked. The media area fits above it.

**Photo-tinted chrome** (REQ-013-11): the control bar, buttons and info panel take a dark tint from the item's ThumbHash average color (the reference's `ambientGlassStyle`), clamped so text keeps 4.5:1 (REQ-013-04). **Tile ↔ viewer** (REQ-013-12): the photo grows from its tile's rect into the viewer (`DURATION_HERO`) and shrinks back on close; without the tile on screen it fades; with Windows' animations off it is instant.

## Results (M8a)

- 12 kittest tests drive the viewer on a library of real PNG files (`crates/app/tests/viewer.rs`), snapshot `viewer`.
- Running app on the user's library (`--view <position> --screenshot …`): 12 MP phone JPEGs open at full resolution, fitted, the window behind darkened (sidebar background 31 → 5); controls step aside after 2.5 s.
- Differences found while testing: kittest's default step of 0.25 s puts two clicks two frames apart outside egui's double-click window, so the viewer tests run at 60 fps; fades are timed by egui's frame time, not the wall clock, so tests and screenshots see them finish.

## Reference

- `src/renderer/src/lib/zoom.ts`, `zoom.test.ts`: `clampView`, `zoomTo`, `fitSize`, MIN/MAX scale; tests ported.
- `src/renderer/src/lib/preload.ts`, `preload.test.ts`: `neighbours` (radius 2, nearest first), the preloader's replace-the-set rule, `pickPreview` (sharpest loaded thumbnail).
- `src/renderer/src/components/zoomable-image.tsx`: wheel sensitivity 0.002, key step 1.25, drag threshold 4 px, click delay 250 ms, toggle to `max(2, natural / fitted)`, click-on-image vs. backdrop by position, instant original without fade, failure message.
- `src/renderer/src/components/media-viewer.tsx`: idle 2.5 s, `controlsHidden`, keys (Escape, ←/→, `i`, `f`, ignored with Ctrl/Alt/Meta), padding of the media area (px-16, pt-12, 80 px bottom), control bar content.

## Tasks

### M8a

- [x] 1. `domain::viewer`: zoom math, `toggle_scale`, `neighbours`, `controls_hidden` with the reference's tests (REQ-009-05, REQ-009-13, REQ-009-16, REQ-013-10)
- [x] 2. `media::original::load_original`: full-size upright decode, AVIF through ffmpeg, cap at the texture limit (REQ-009-04)
- [x] 3. `app::originals`: worker threads, replace-the-set, cancellation, failures, GPU and egui backends (REQ-009-13, REQ-009-15)
- [x] 4. Gallery: open from tile / row click and Enter, item lookup by position and id, thumbnail request for the viewer, scroll the shown item into view (REQ-009-01, REQ-016-06)
- [x] 5. Viewer: overlay, fitted original with preview and fade, stepping, closing, controls and their auto-hide (REQ-009-01 … -03, -12, -14, -15, REQ-013-10)
- [x] 6. Viewer: wheel, keys, click and double-click zoom, drag pan, cursors (REQ-009-05, REQ-009-16)
- [x] 7. UI tests (kittest) and a `viewer` snapshot; check in the running app with a real library
- [x] 8. Docs: spec statuses and Verification, architecture, pitfalls, progress

### M8b

- [ ] 9. `db::media::detail` query and the info panel / sheet (REQ-009-08, REQ-013-13)
- [ ] 10. Filter by tag from the panel (REQ-009-09)
- [ ] 11. Show in Explorer and context menus (REQ-009-10, REQ-009-11)
- [ ] 12. Filmstrip (REQ-015-01 … -07)
- [ ] 13. Photo-tinted chrome (REQ-013-11) and tile ↔ viewer transition (REQ-013-12)
- [ ] 14. UI tests, snapshots, running-app check, docs

## Exit criteria

From `docs/rewrite/plan.md`: the REQs of 009 (without video) and 015 pass in egui_kittest tests. Added here: stepping to a preloaded photo shows its original in the first frame; a 24 MP JPEG never stalls a frame (decoded and uploaded off the UI thread).
