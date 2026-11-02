# 013 · Visual design · Plan

Records the approach that was taken. Tokens and surfaces are in `docs/design.md`; lasting decisions are under _Decisions_ at the end.

## Approach

- **Theme** (`app::theme`): the tokens of `docs/design.md` (colors, type scale, radii, spacing, motion durations), Atkinson Hyperlegible Next as a variable TTF (weights through `FontTweak::coords`) plus a copy with `tnum` baked in for numbers, own colors whatever the OS theme (REQ-013-01). Contrast tests run over the tokens, including tinted glass over 59 photo backdrops (REQ-013-04) and the focus ring over all backdrops (REQ-013-05).
- **Glass** (`app::glass`, REQ-013-03): each surface draws the frame's backdrop (tiles with their thumbnail textures, placeholders, page and sidebar as solids, recorded while drawing) into an offscreen chain at half resolution, blurs it with 4 dual-Kawase passes and composites saturation, the glass color and an opacity in an `egui_wgpu` paint callback (≈ 0.4 ms per surface). Surfaces record nothing for each other ("no glass on glass"), except that the viewer's scrim also sees the toolbar. Menus, combo lists, context menus, suggestions and dialogs paint their glass into shape slots kept before their content. The viewer's chrome reuses the scrim's blur (a shared blur that includes the photo while it grows from its tile), tinted by the item (REQ-013-11, `domain::ambient`: OKLCH clamp of the ThumbHash average color).
- **Fallbacks** (REQ-013-06, -07): without a wgpu render state (kittest) or with Windows' _Transparency effects_ off, surfaces are opaque (Raised; Darkroom for the scrim). Without _Animation effects_, motion is instant. `theme::OsPreferences` re-reads both every 2 s while frames are drawn.
- **Title bar** (`app::titlebar`, REQ-013-02, -08): a window subclass drops the caption in `WM_NCCALCSIZE` and answers the top edge and the three caption buttons natively in `WM_NCHITTEST` (snap layouts); the app draws the buttons. Empty caption areas (toolbar, Settings header, sidebar header) take drags in egui and send `ViewportCommand::StartDrag`, so Aero snap works; a double-click maximizes. 138 px stay free at the right where a bar reaches the window's edge. Minimum window 800 × 600, tested.
- **Focus** (REQ-013-05): one ring for the widget with keyboard focus (`theme::draw_focus_ring`, after everything); text inputs keep their border.
- **Viewer chrome** (REQ-013-10, -12, -14): controls and pointer step aside after 2.5 s idle (not while hovered, the info panel is open or a video is paused); the photo grows from its tile and shrinks back on close (fades without a tile on screen).
- **Dates** (REQ-013-13): ICU4X (`icu_datetime`, compiled data) in the locale `sys-locale` reports, medium date with hours and minutes; the formatter is cached per thread; tests pin `en-GB`.
- **Scrollbars** (REQ-013-15): egui's floating bars styled as the design's pill, placed below the toolbar with `ScrollArea::scroll_bar_rect`.

## Known limits

- The viewer's chrome blurs the scrim, not the photo under it (only visible when a zoomed photo reaches under the bar).
- The date pickers' popups stay Raised.
- Counts use English digit grouping.

## Tasks

- [x] Theme module, bundled font, own colors (REQ-013-01)
- [x] Glass and focus tokens, contrast tests, focus ring (REQ-013-04, -05)
- [x] OS preferences followed while running (REQ-013-06, -07)
- [x] Locale dates (REQ-013-13)
- [x] Glass pass, fallback, surfaces, scrim (REQ-013-03, -06, -14)
- [x] Title bar, caption buttons, drag areas, minimum size (REQ-013-02, -08)
- [x] Scrollbars (REQ-013-15)
- [x] Tinted chrome, tile ↔ viewer transition, controls stepping aside (REQ-013-10 … -12)
- [x] `gallery-bench --viewer`, `--no-glass` and `--glass-matrix` to measure the glass pass (#14)
- [x] Run `gallery-bench --thumbs <cache copy> --glass-matrix` on the integrated laptop GPU and record the cost here (#14)

## Decisions

### 2026-10-26 · Glass

**Decided:** glass surfaces render what lies below them into an offscreen target at half resolution, blur it with a dual-Kawase chain (4 passes) and composite saturate + tint + rounded corners in an `egui_wgpu` paint callback. ≈ 0.4 ms per surface. Raised is the fallback. Details in spec 013's plan.

### 2026-10-26 · Font and title bar

**Decided:** Atkinson Hyperlegible Next as the variable TTF from google/fonts (OFL), weights via `FontTweak::coords`; a second copy with `tnum` baked in (`pyftfeatfreeze`) for numbers, since egui can't request OpenType features. Title bar: content runs into the caption, own caption buttons, and a window subclass answers `WM_NCHITTEST` with the native codes (`HTMAXBUTTON` etc.), so shadow, resize, Aero snap and snap layouts stay native.
**Rejected:** native caption (a second bar above the toolbar), undecorated window (no snap layouts, no top-edge resize).

### 2026-10-26 · Icons

**Decided:** lucide icons (ISC, `crates/app/assets/icons/LICENSE.lucide.txt`) as SVG files with `stroke="#fff"` and `stroke-width="1.75"`, embedded with `include_bytes!` (`app::icons::Icon`) and rendered by `egui_extras`' SVG loader (resvg) at the size they are drawn, tinted per use. A new icon is a line in `icons!`; `cargo xtask icons` writes its SVG from a pinned lucide-static release.
**Rejected:** an icon font (lucide's font drops the stroke-width control), hand-drawn shapes (doesn't scale to ~40 icons).

### 2026-10-28 · Glass, title bar, focus and OS preferences

**Decided:** `app::glass` draws each surface's **backdrop** (tiles with their thumbnail texture views, placeholders and the page / sidebar as solids, recorded while drawing) into an offscreen chain at half resolution and blurs and composites it in a paint callback. Surfaces record nothing for each other, except that the viewer's scrim also sees the toolbar; the viewer records the scrim's darkening and the photo (its texture view, as far as it is drawn) over them, which menus over the viewer blur. Its chrome composites from the scrim's blur; only while the photo grows from its tile, the one time it lies under the chrome, it composites from one shared blur of the window, the darkening and the photo (one extra pass during that animation). Without the pass (no wgpu render state, as in kittest) or with Windows' transparency effects off, surfaces are opaque; snapshots show that fallback, the pass is checked with screenshots of the running app. A paint callback ignores the painter's opacity, so surfaces pass it themselves.
**Title bar:** `app::titlebar` (the module with the window `unsafe`): `WM_NCCALCSIZE` drops the caption, `WM_NCHITTEST` answers the top edge and the three caption buttons natively. Empty caption areas take the drag in egui and send `ViewportCommand::StartDrag` (a native caption drag, so Aero snap works).
**Rejected:** reading back egui's framebuffer for the blur (a callback runs inside egui's render pass), `HTCAPTION` drag rects (the window procedure can't know which parts of the toolbar are empty), Windows' `GetDateFormatEx` (no medium length).

### 2026-11-02 · Glass pass cost

**Measured** (#14): `cargo xtask gallery-bench --thumbs <3,000 thumbnails of a real cache> --glass-matrix`, 100,000 items, 12 columns, vsync off, AMD Radeon iGPU (Vulkan), mains power, two runs of 20 s per combination. Frame interval in ms:

| | p50 | p95 | p99 |
| --- | --- | --- | --- |
| gallery, glass | 7.6 / 7.2 | 9.8 / 11.6 | 11.5 / 19.2 |
| gallery, no glass | 4.6 / 5.8 | 6.9 / 8.1 | 8.5 / 9.6 |
| viewer open, glass | 9.2 / 9.6 | 13.6 / 12.5 | 16.1 / 14.9 |
| viewer open, no glass | 5.9 / 5.0 | 8.8 / 7.1 | 12.4 / 8.5 |

The pass costs ≈ 2–4 ms per frame at p50, ≈ 5 ms at p95 with the viewer's scrim open. The first run's gallery-with-glass pass stalled once for 117 s (max, left out); every other max was below 235 ms.
**Decided:** keep rebuilding the per-tile bind groups and buffers every frame and keep slot textures until exit. With the viewer open, p95 stays below the 16 ms budget (REQ-006-12), the threshold #14 set for reusing them.
**Noticed:** frame times are higher than spec 006's measurement of 2026-10-27 (gallery with glass p95 4.4 ms), and thumbnail textures peaked at 866–895 MB of the 512 MB budget. Eviction keeps textures used in this or the last frame (REQ-006-13), so the peak depends on the window size and on the widths of the linked thumbnails; not investigated here.
