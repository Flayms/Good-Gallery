# Design

How Good Gallery looks, and why. Read this before any UI work. Values live in `crates/app/src/theme.rs` (tokens) and this guide explains them. If the two disagree, fix one of them in the same change.

Spec: [013 · Visual design](specs/013-visual-design/spec.md).

## Subject

A personal archive of photos and videos, often on a NAS, browsed by one person who knows their library. The job is **finding and looking**, not editing. The interface is a frame around the user's photos and should never compete with them.

## Principles

1. **Photos are the only color.** Chrome is neutral grey with no hue, like the neutral backgrounds of photo-editing tools, so it doesn't shift how colors in a photo are perceived. Hue appears only where it carries meaning (rating stars, errors) or comes from the photo itself.
2. **Glass only over real content.** Translucent surfaces exist where something worth seeing is behind them: the gallery under the toolbar, the photo under the viewer controls, the window under a menu. No decorative gradients or blobs behind glass.
3. **One loud idea.** The viewer floats over the blurred, darkened window, so the user still senses where they were, and the photo tints the chrome around it. Everything else stays quiet so this can be noticed.
4. **The same on every platform.** Windows, macOS and Linux look identical: bundled font, own colors, no OS materials (Mica, vibrancy). Only the native window controls differ.
5. **Dense, but legible.** Libraries are large and the user scans a lot of names and numbers. Prefer compact layouts and tabular figures, but never trade away contrast or hit size.

## Surfaces

| Surface         | Role (`theme`)  | Value                                                     | Used for                                                     |
| --------------- | --------------- | --------------------------------------------------------- | ------------------------------------------------------------ |
| Darkroom        | `base`          | `#1F1F1F`                                                 | Gallery, settings page                                       |
| Recess          | `recess`        | `#171717`                                                 | Sidebar (nothing is behind it, so it is solid)               |
| Glass bar       | `glass_bar`     | `rgb(28 28 28 / 0.72)`, blur 24px, saturate 160 %, Rim    | Toolbar, viewer control bar, viewer arrows, filmstrip        |
| Glass panel     | `glass_panel`   | `rgb(36 36 36 / 0.86)`, blur 32px, saturate 140 %, Rim    | Menus, popovers, select lists, command list, dialogs, sheets, info panel |
| Field           | `field`         | `rgb(0 0 0 / 0.35)`                                       | Inputs and the search bar on glass                           |
| Scrim           | `SCRIM`         | `rgb(0 0 0 / 0.5)`, blur 28px, saturate 120 %             | The whole window behind the viewer                           |
| Raised          | `raised`        | `#2A2A2A`                                                 | Opaque fallback for both glass levels (Darkroom for the Scrim) |

Glass is drawn by the app, not the OS: the tiles under a glass surface are rendered into an offscreen target at half resolution, blurred with a dual-Kawase chain and composited with saturation, tint and rounded corners in a wgpu paint callback (≈ 0.4 ms per surface; see `docs/architecture.md`). What lies under glass is the frame's backdrop: the tiles the gallery drew and the solid surfaces (page, sidebar), without text, which the blur hides anyway. The viewer's chrome reuses the scrim's blur. `app::glass` holds the pass; menus and dialogs get it through `glass::panel` / `glass::behind_menu`.

Rules:

- **Fallback:** when Windows' _Transparency effects_ are off, or the GPU can't run the blur pass, glass becomes Raised with the same Rim.
- **Never on tiles.** No blur on tiles, list rows or anything repeated per media item. Badges on tiles are solid (`rgb(0 0 0 / 0.6)`).
- **No glass on glass.** A glass surface blurs the gallery below it, not other glass. Overlays (menus, popovers) sit on their own layer above the toolbar and blur the gallery directly.
- **Few layers.** At most the toolbar plus one open overlay blur at the same time while the gallery scrolls.
- **Text on Glass bar** is Paper only. Secondary text and placeholders sit on a Field.
- **Title bar:** the toolbar and the sidebar header are the caption; the window answers hit tests with the native codes (caption, minimize, maximize, close), so snap layouts, shadows and resizing stay native. Space for the window controls is reserved on the right.

## Color

| Name     | Value                    | Token                              | Role                                                   |
| -------- | ------------------------ | ---------------------------------- | ------------------------------------------------------ |
| Darkroom | `#1F1F1F`                | `--background`                     | Base                                                   |
| Recess   | `#171717`                | `--sidebar`                        | Sidebar                                                |
| Paper    | `#F2F2F2`                | `--foreground`, `--primary`        | Text, icons, primary buttons (Paper fill, Darkroom text) |
| Mist     | `#B4B4B4`                | `--muted-foreground`               | Secondary text, counts                                 |
| Rim      | `rgb(255 255 255 / 0.10)` | `--border`                        | Hairlines, glass edges                                 |
| Star     | `#F5C451`                | `--star`                           | Rating stars, nothing else                             |
| Alert    | `#F2706A`                | `--destructive`                    | Errors, offline libraries                              |
| Ambient  | per media item           | `--ambient`                        | Viewer backdrop tint (see below)                       |

- **No brand accent.** Selection, active and hover states use lightness: hover `rgb(255 255 255 / 0.06)`, selected `rgb(255 255 255 / 0.12)` with Paper text and weight 600.
- **Ambient** is the media item's average color, taken from its placeholder. It is clamped to OKLCH lightness ≤ 0.28 and chroma ≤ 0.06 and mixed 30 % into both glass levels, so text contrast holds for any photo.
- **Focus** is a 2px Paper ring with a 2px `rgb(0 0 0 / 0.6)` ring outside it, so it shows over white and over black. The app draws it for every focusable widget (one helper in `theme`); text inputs highlight their border instead.
- **Contrast floor:** Paper and Mist reach 4.5:1 on every surface, glass measured over a white backdrop (the worst case). A test enforces this from the tokens in `theme`.

### Light appearance

The _Appearance_ setting (spec 020) is _System_ (the OS or browser decides, dark if it says nothing), _Light_ or _Dark_. Light keeps every surface, glass level and rule above; only values change, still neutral grey without hue. The viewer stays dark in both: photos are judged against a dark surround.

| Role (`theme`)  | Light                      | Dark (above)              |
| --------------- | -------------------------- | ------------------------- |
| `base`          | `#F4F4F4`                  | `#1F1F1F` Darkroom        |
| `recess`        | `#EAEAEA`                  | `#171717`                 |
| `raised`        | `#FFFFFF`                  | `#2A2A2A`                 |
| `placeholder`   | `#E0E0E0`                  | `#2A2A2A`                 |
| `text`          | `#1A1A1A`                  | `#F2F2F2` Paper           |
| `muted`         | `#595959`                  | `#B4B4B4` Mist            |
| `star`          | `#8F6300`                  | `#F5C451`                 |
| `alert`         | `#C42B1C`                  | `#F2706A`                 |
| `rim`           | `rgb(0 0 0 / 0.12)`        | `rgb(255 255 255 / 0.10)` |
| `hover`         | `rgb(0 0 0 / 0.05)`        | `rgb(255 255 255 / 0.06)` |
| `selected`      | `rgb(0 0 0 / 0.10)`        | `rgb(255 255 255 / 0.12)` |
| `field`         | `rgb(255 255 255 / 0.60)`  | `rgb(0 0 0 / 0.35)`       |
| `glass_bar`     | `rgb(248 248 248 / 0.72)`  | `rgb(28 28 28 / 0.72)`    |
| `glass_panel`   | `rgb(255 255 255 / 0.86)`  | `rgb(36 36 36 / 0.86)`    |
| focus ring      | `text` + white 80 % outside | Paper + black 60 % outside |

- **Contrast floor** as in dark; light glass is measured over a black backdrop, its worst case.
- **Placeholders** are a step darker than the page in light (a step lighter in dark): white tiles would vanish.

## Type

**Atkinson Hyperlegible Next** (variable TTF from google/fonts, OFL), bundled in the binary. Weights are set through the variable axis; a second copy has `tnum` baked in, because egui can't request OpenType features (`theme::numbers()`).

Why: the app is full of file names and numbers like `IMG_0011` or `DSC_1107`, and the font's letterforms keep `I l 1` and `O 0` distinct. It was designed for legibility at small sizes, which also helps on glass over busy photos. It has tabular figures for counts, durations and dates. It is uncommon in app UIs, so the app doesn't read as a template, while staying quiet.

| Size | Weight | Role                                                         |
| ---- | ------ | ------------------------------------------------------------ |
| 12   | 400    | Counts, badges, durations, captions                          |
| 14   | 400    | Default UI text: sidebar, menus, toolbar, list rows          |
| 14   | 600    | Active item, labels of groups                                |
| 16   | 600    | Viewer file name, dialog titles                              |
| 18   | 600    | Section titles in settings                                   |
| 24   | 600    | Page titles (settings)                                       |
| 36   | 600    | Empty-state headline (first run)                             |

- The scale follows the classic typographic scale (12, 14, 16, 18, 24, 36). Don't add sizes in between.
- Line height 1.4 for UI, 1.55 for descriptions. Descriptions are at most 70 characters wide.
- Tabular figures (`theme::numbers()`) wherever numbers line up or change in place (counts, list columns, durations, progress).
- Sentence case everywhere. No all-caps labels and no letter-spacing on labels.

## Shape and space

| Element                      | Radius     |
| ---------------------------- | ---------- |
| Tiles                        | 4px, 2px from 8 columns on |
| Controls (buttons, inputs, menu items) | 6px |
| Glass panels, dialogs        | 12px       |
| Viewer control bar, chips    | full (pill) |

- Spacing uses a 4px grid. The gallery gap is 8px, 4px from 8 columns on.
- Photos aren't cards: no shadow, no border on tiles.
- Hit targets are at least 28px high (toolbar and sidebar rows: 32px).

## Icons

lucide, 16px, stroke 1.75 (in lucide's 24-unit grid, so it scales with the icon). An icon earns its place when it helps scanning or replaces a word. Don't repeat the same icon on every row of a list (e.g. a tag icon on every tag).

## Motion

| Token (`theme`)   | Value                              | Use                                     |
| ----------------- | ---------------------------------- | --------------------------------------- |
| ease out          | `cubic-bezier(0.2, 0.8, 0.2, 1)`   | Things appearing                        |
| `DURATION_FAST`   | 120ms                              | Menus, popovers, hover                  |
| `DURATION_BASE`   | 200ms                              | Thumbnail fade-in, panels               |
| `DURATION_HERO`   | 320ms                              | Tile ↔ viewer                           |

- **One signature moment:** opening a media item, the tile grows into the viewer and shrinks back into place on close.
- Other motion only answers an action (a menu opening, a panel sliding in). Nothing moves on its own except progress indicators.
- With reduced motion (Windows' _Animation effects_ off), the tile ↔ viewer transition is instant and panels don't zoom or slide. Fades of at most `DURATION_FAST` stay.

## Window

- Content runs into the title bar: the toolbar and sidebar header are the window's drag area, and empty toolbar space drags the window.
- The native window controls stay (right on Windows and Linux, left on macOS). Their space is reserved, so nothing in the app overlaps them. This is the only platform difference.
- Scrollbars are the app's own: a thin pill (white at 24 %, 40 % on hover) on a transparent track. Pages that scroll under the toolbar start the track below it, so the thumb never hides under the window controls.

## Writing

- Use the [glossary](glossary.md) terms. In the UI: _Library_, _Photos_ / _Videos_, _Viewer_, _Tags_, _People_, _Places_.
- Buttons say what happens (_Add library_), and the confirmation uses the same verb (_Library added_).
- Errors say what happened and what to do next, without apologizing: "Can't reach \\\\nas\\photos. Showing the last indexed state."
- Empty states invite the next step: "Add a library to start" with the button right there.
- Dates use the locale's medium format without seconds ("2 Jul 2025, 07:12"). Sizes and counts use the locale's grouping.

## Decisions

| Date       | Decision                                                  | Why                                                                                     |
| ---------- | --------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| 2026-10-25 | Glass, but only over real content                         | Glass needs something behind it. In a gallery that's the photos; gradients would be decoration. |
| 2026-10-25 | No OS materials (Mica, vibrancy), native window controls kept | macOS and Linux are planned and should look the same. Replacing window controls would feel foreign on each OS. |
| 2026-10-25 | Achromatic chrome, no brand accent                        | Neutral chrome doesn't bias how photo colors are perceived. The viewer's Ambient is the color moment. |
| 2026-10-25 | Atkinson Hyperlegible Next instead of Geist               | Distinct `I l 1` / `O 0` for file names, legible at small sizes on glass, tabular figures, bundled for all platforms. |
| 2026-10-25 | Viewer over the blurred window, not over a blurred copy of the photo | Users keep a sense of where they came from in the gallery (REQ-013-14 replaces REQ-013-09). |
| 2026-10-25 | Dark only                                                 | A light theme is out of scope for now. Tokens stay semantic so it can be added later.    |
| 2026-10-25 | Glass over the scrolling gallery                         | Measured on 3 000 photos: no frame-time cost compared with an opaque toolbar (see `known-pitfalls.md`). |
| 2026-10-25 | Glass bar at 0.72 and panel at 0.86 opacity               | Over a white photo: Paper on the bar 6.0:1, Mist on a Field on the bar 5.4:1 (on the bare bar only 3.3:1, hence the Field rule), Paper / Mist on the panel 8.9:1 / 4.8:1. |
| 2026-10-26 | Same look, rebuilt in egui                                | Tokens live in `crates/app/src/theme.rs`; glass is an own wgpu blur pass, approved from screenshots. Mechanics changed, values didn't. |
| 2026-10-28 | Glass blurs the recorded tiles, not the rendered frame     | A paint callback can't read egui's target mid-pass; tiles and solids are cheap to redraw at half resolution, and text vanishes under a 24–32 px blur anyway. |
| 2026-10-28 | Dates in the user's locale through ICU4X                   | Medium dates differ per locale ("2 Jul 2025", "Jul 2, 2025", "02.07.2025"); Windows has no medium length. Counts keep English grouping for now. |
