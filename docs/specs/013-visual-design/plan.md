# 013 · Visual design · Plan

Records the approach that was taken. Tokens and surfaces are in `docs/design.md`; decisions that outlive this spec are in `docs/architecture.md`.

## Approach

- **Theme** (`app::theme`): the tokens of `docs/design.md` (colors, type scale, radii, spacing, motion durations), Atkinson Hyperlegible Next as a variable TTF (weights through `FontTweak::coords`) plus a copy with `tnum` baked in for numbers, own colors whatever the OS theme (REQ-013-01). Contrast tests run over the tokens, including tinted glass over 59 photo backdrops (REQ-013-04) and the focus ring over all backdrops (REQ-013-05).
- **Glass** (`app::glass`, REQ-013-03): each surface draws the frame's backdrop (tiles with their thumbnail textures, placeholders, page and sidebar as solids, recorded while drawing) into an offscreen chain at half resolution, blurs it with 4 dual-Kawase passes and composites saturation, the glass color and an opacity in an `egui_wgpu` paint callback (≈ 0.4 ms per surface). Surfaces record nothing for each other ("no glass on glass"), except that the viewer's scrim also sees the toolbar. Menus, combo lists, context menus, suggestions and dialogs paint their glass into shape slots kept before their content. The viewer's chrome reuses the scrim's blur, tinted by the item (REQ-013-11, `domain::ambient`: OKLCH clamp of the ThumbHash average color).
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
