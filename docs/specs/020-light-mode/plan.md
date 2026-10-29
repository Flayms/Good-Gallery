# 020 · Light mode · Plan

## Approach

### Decisions to approve

The issue left these open; this plan proposes:

1. **Both, defaulting to the system.** An _Appearance_ setting with _System_ (default), _Light_ and _Dark_. _System_ is what most desktop apps do and needs no setup; the explicit choices cover users whose OS setting doesn't suit a photo app.
2. **The viewer stays dark.** Photos are judged against a dark surround (Lightroom, Photos and Windows' Photos app all keep their loupe dark), the viewer's ambient tint (REQ-013-11) is clamped to dark lightness, and REQ-013-14 darkens the window behind the viewer anyway. A light viewer would need its own tint rules for little gain.
3. **Same structure, other values.** Light reuses every surface, glass level and rule of `docs/design.md`; only color values change. No new surfaces.

### Tokens

`theme`'s color constants become fields of a `Palette` (`theme::DARK`, `theme::LIGHT`), named by role instead of by the dark value: `base` (Darkroom), `recess`, `raised`, `text` (Paper), `muted` (Mist), `star`, `alert`, `rim`, `hover`, `selected`, `field`, `ring`, `ring_outer`, `scrollbar_thumb`, `scrollbar_thumb_hover`, `glass_bar`, `glass_panel`. `SCRIM` stays one constant (the viewer is dark). Sizes, radii, spacing and durations stay constants.

Proposed light values, neutral grey without hue (contrast computed with the WCAG formula, glass over a black and a white backdrop):

| Role           | Dark (today)              | Light                      | Worst contrast of text / muted |
| -------------- | ------------------------- | -------------------------- | ------------------------------ |
| base           | `#1F1F1F`                 | `#F4F4F4`                  | 15.8 / 6.4                     |
| recess         | `#171717`                 | `#EAEAEA`                  | 14.5 / 5.8                     |
| raised         | `#2A2A2A`                 | `#FFFFFF`                  | 17.4 / 7.0                     |
| text           | `#F2F2F2`                 | `#1A1A1A`                  |                                |
| muted          | `#B4B4B4`                 | `#595959`                  |                                |
| star           | `#F5C451`                 | `#8F6300`                  | 4.4 on recess (icons need 3:1) |
| alert          | `#F2706A`                 | `#C42B1C`                  | 4.7 on recess                  |
| rim            | `rgb(255 255 255 / 0.10)` | `rgb(0 0 0 / 0.12)`        |                                |
| hover          | `rgb(255 255 255 / 0.06)` | `rgb(0 0 0 / 0.05)`        |                                |
| selected       | `rgb(255 255 255 / 0.12)` | `rgb(0 0 0 / 0.10)`        | 11.6 / 4.7                     |
| field          | `rgb(0 0 0 / 0.35)`       | `rgb(255 255 255 / 0.60)`  | 13.3 / 5.4 over black          |
| glass bar      | `rgb(28 28 28 / 0.72)`    | `rgb(248 248 248 / 0.72)`  | 8.3 over black                 |
| glass panel    | `rgb(36 36 36 / 0.86)`    | `rgb(255 255 255 / 0.86)`  | 12.6 / 5.1 over black          |
| focus ring     | Paper + `rgb(0 0 0 / 0.6)` outside | text + `rgb(255 255 255 / 0.8)` outside | ≥ 4.4 over any grey |
| scrollbar thumb | white 24 % / 40 %        | black 24 % / 40 %          |                                |

For light glass the worst case is a black backdrop (dark glass: white), so the contrast test measures both palettes over both.

### Choosing the palette

egui already tracks the system theme (eframe passes the OS and browser preference) and has a `ThemePreference` of `System`, `Light`, `Dark`. `theme::install` sets egui's visuals for both themes; the setting maps onto `ctx.options_mut(|o| o.theme_preference = …)`. `theme::colors(ctx) -> &'static Palette` returns `DARK` or `LIGHT` from `ctx.theme()`. It reads the context, not a global, so tests running in parallel with different appearances don't interfere. The viewer, filmstrip, video controls and info panel inside the viewer use `theme::DARK` directly (REQ-020-05).

The setting is stored as `appearance` (`"system"`, `"light"`, `"dark"`) with the other settings (`db::settings`, REQ-010-01); unknown values fall back to `"system"` (REQ-010-02).

### Window

- **Windows:** the app draws its caption buttons with theme colors, so they follow. The native frame's border follows `DWMWA_USE_IMMERSIVE_DARK_MODE`, set from the palette when the appearance changes.
- **Linux:** the app's own caption buttons follow. **macOS:** native caption, nothing to do.
- **Browser:** eframe's web backend reports `prefers-color-scheme`; the page's background color in `web/page.css` gets a `prefers-color-scheme: light` value so there is no dark flash before the canvas draws.

### Tests

- `theme`: the contrast test runs for both palettes, over black and white backdrops (REQ-020-04). A test that every light token is neutral (r = g = b) except star and alert (REQ-020-03).
- `crates/app/tests/settings.rs`: picking _Light_ switches `ctx.theme()` and stores the setting; a fresh index starts on _System_ (REQ-020-01).
- A test that sets egui's system theme to light and back with _System_ chosen (REQ-020-02).
- Snapshots of the gallery, the sidebar, a menu and the Settings page in light (REQ-020-03), and of the viewer opened in light, which matches the dark one (REQ-020-05). Linux and Windows snapshot files, as today.
- Reduced transparency in light (REQ-020-06) next to the existing glass test.

## Tasks

- [ ] `Palette` with `DARK` and `LIGHT`; replace the color constants at their ~170 call sites with `theme::colors(ctx)` (mechanical; the viewer keeps `DARK`) — REQ-020-03, REQ-020-05
- [ ] Contrast and neutrality tests for both palettes — REQ-020-04, REQ-020-03
- [ ] egui visuals for the light theme in `theme::install` (widgets, selection, text cursor, scrollbars) — REQ-020-03
- [ ] Glass: light fills and the light opaque fallback — REQ-020-03, REQ-020-06
- [ ] _Appearance_ setting on the Settings page, stored and validated — REQ-020-01
- [ ] Follow the system theme; dark when unknown — REQ-020-02
- [ ] Windows frame border, Linux caption buttons, `web/page.css` — REQ-020-02, REQ-020-03
- [ ] Light snapshots; README screenshot in light (`cargo xtask screenshots`)
- [ ] `docs/design.md`: light values next to the dark ones; `docs/architecture.md`: palette from the context
