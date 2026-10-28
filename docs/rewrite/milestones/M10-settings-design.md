# M10 · Settings, design polish, glass

**Status:** Done (approved 2026-10-27, self-approved: the user asked to work through the milestones without their input)
**Specs:** 010 (all), 011 (REQ-011-08, -11), 013 (REQ-013-01 … -08, -13 … -15)

Split in two parts, both done in this session:

- **M10a · Settings and rules:** the Settings page's indexing and thumbnail cache sections, the rules that are left of 011, contrast and focus from the tokens, reduced motion followed while the app runs, dates in the user's locale.
- **M10b · Glass and title bar:** the blur pass from M0 in the app (toolbar, menus, viewer chrome, the scrim over the window), reduced transparency, content in the title bar with native hit tests, the minimum window size, the app's own scrollbars below the toolbar.

## Approach

### M10a

**Settings page.** Below _Libraries_ come _Indexing_ (_Check for missed changes_: a combo box with the reference's presets plus a stored non-preset value; _Parallel file reads_: a 1–16 slider that only saves on release) and _Thumbnail cache_ (_Maximum size_: presets plus a stored value; _Used_: the cache's size, refreshed every 10 s, "…" until it is measured; _Clear_ asks first, then clears on a job thread and confirms with a toast). Rows are the reference's `SettingRow`: label, description (Mist, ≤ 70 characters wide) and the control on the right, in a bordered group. Labels and descriptions are the reference's.

**Applying settings.** `AppState::update_settings(patch)` validates and stores through `db::settings::save` (an invalid value is an error toast and nothing changes, REQ-010-02), then hands the new values to the running parts through a `SettingsTargets` trait object (scheduler interval, indexer + exiftool pool + thumbnail threads, cache limit with eviction on a job thread), so UI tests record the calls with a fake. New: `ThumbnailService::set_workers` (threads above the limit wait; more are started when it grows), `Scheduler::set_interval` gets its caller. The cache usage comes from `ThumbCache::usage` (already counted), read through the same trait.

**Files only from the index (REQ-011-08).** `domain::media::is_safe_rel_path` refuses absolute paths, drive or UNC prefixes, `..`, `.` and empty components and backslashes inside components; `db::media::absolute_path` and `ThumbSource::file` return `None` for such paths, so the viewer treats the item as missing, Explorer isn't opened and the thumbnail fails.

**No network (REQ-011-11).** An `xtask` test reads `Cargo.lock` and fails on known network crates (HTTP clients, async network runtimes, TLS) and scans the workspace's sources for `std::net` / socket use. The app has no network code; the test keeps it that way.

**Contrast and focus (REQ-013-04, -05).** `theme` gets the glass tokens (`GLASS_BAR`, `GLASS_PANEL`, `SCRIM`, `RING`, `RING_OUTER`, scrollbar thumb) and a tiny `color` helper (composite, WCAG contrast); the reference's `design-tokens.test.ts` is ported over the Rust tokens, including the tinted glass over 59 backdrops. Focus: `theme::focus_ring(painter, rect, radius)` draws a 2 px Paper ring with a 2 px `rgb(0 0 0 / 0.6)` ring outside; the app draws it for the focused widget each frame (egui's own focus stroke is turned off), and tiles and rows draw theirs over the thumbnail.

**Reduced motion (REQ-013-07).** Already instant when Windows' animation effects are off at start; now the app re-reads the OS setting every 2 s (and transparency, M10b) and updates the style, so it follows the setting while it runs. Panels (info panel, filmstrip) don't slide; they fade within `DURATION_FAST`.

**Readable dates (REQ-013-13).** `format::medium_date_time` formats with ICU4X (`icu_datetime`, compiled data) in the user's locale (`sys-locale`), medium date with hours and minutes. Tests set the locale (`format::set_locale`), so `en-GB` gives "2 Jul 2025, 07:12". Counts keep their English grouping (not part of a REQ).

### M10b

**Glass (`app::glass`).** The M0 spike's pass moves into the app: tiles under a glass surface are drawn into an offscreen target at half resolution, blurred with 4 dual-Kawase passes and composited with saturation, tint, rounded corners and an opacity in an `egui_wgpu` paint callback. What lies under glass is the **backdrop** of the frame: the gallery records each drawn tile (rect, clip, its wgpu texture view or its placeholder color, UV, fade) and the solid surfaces (Darkroom page, Recess sidebar); it is stored in the egui context for the frame. Surfaces: the toolbar and Settings header (Glass bar), menus, popovers, context menus, suggestions, dialogs and the date pickers (Glass panel), the viewer's control bar, arrows, filmstrip, video controls and info panel (bar / panel, tinted by the item) over a backdrop of the shown photo, and the **scrim** over the whole window behind the viewer (the backdrop blurred, darkened by 0.5, faded with the transition). A surface is painted into a shape slot reserved before its content, so text stays on top; menus use an own frame helper (`glass::panel`) that paints glass and Rim behind the content.

**Fallback (REQ-013-06).** Without a wgpu render state (kittest), with Windows' _Transparency effects_ off (`HKCU\…\Themes\Personalize\EnableTransparency`, re-read every 2 s) or when the GPU can't create the pipelines, glass is Raised with the same Rim, and the scrim is opaque Darkroom around the item. Snapshots use the fallback; the real pass is checked in the running app with screenshots.

**Title bar (REQ-013-02, -08).** The M0 hybrid variant: the native frame stays (shadow, resize, snap), a window subclass removes the caption in `WM_NCCALCSIZE`, answers the top resize edge and the caption buttons (`HTMINBUTTON`, `HTMAXBUTTON`, `HTCLOSE`, so Windows 11 shows snap layouts) in `WM_NCHITTEST` and runs them on button-up. The app draws the caption buttons (46 px each, red close hover) on top of everything, and reserves their width on the right of the toolbar and the Settings header. Dragging: empty space of the toolbar, the Settings header and the sidebar header take the drag in egui (`ViewportCommand::StartDrag`, which Windows treats as a caption drag, so Aero snap works) and a double-click maximizes or restores. Decision differs from the spike's `HTCAPTION` rect: egui knows which parts are empty, the window procedure doesn't. All Win32 code (and its `unsafe`) lives in `app::titlebar`; other platforms keep the native caption. The minimum window size stays 800 × 600, and a UI test at that size checks that toolbar controls don't overlap each other or the caption buttons.

**Scrollbars (REQ-013-15).** egui's floating scroll bars styled as the design's thin pill (white 24 %, 40 % hovered or dragged, no track); pages under the toolbar (gallery, Settings) start the bar below it, using a scroll area inset (`ScrollArea::content_margin` / a bar rect offset) so the thumb never reaches under the caption buttons.

**Side-by-side screenshots (exit).** App screenshots of the gallery with glass toolbar and open View menu, the viewer, and the Settings page, compared with the reference's `docs/screenshots`. The user approves them later; the comparison is noted in the log.

## Reference

- `components/general-settings.tsx`: presets (`0, 15, 30, 60, 180, 360, 1440` minutes; `1, 2, 5, 10, 20, 50, 100` GB), labels ("Only at startup", "Every 15 minutes", "Every hour", "Every 3 hours", "Daily"), `withCurrent`, descriptions, slider commit on release, 10 s usage refresh, "…" / "unknown", confirm + "Thumbnail cache cleared".
- `components/library-settings.tsx`: already ported in M3.
- `styles.css`: tokens (`--glass-bar`, `--glass-panel`, `--field`, `--scrim`, `--ring`, `--ring-outer`, scrollbar thumb 0.24 / 0.4), glass utilities (blur and saturate per level, Raised fallback), reduced motion.
- `design-tokens.test.ts`: contrast checks (solid surfaces, glass over white and black, tinted glass over 59 backdrops, focus ring over all backdrops); `lib/color.ts` (composite, WCAG contrast).
- `components/app-header.tsx`, `routes/settings.tsx`: header height 48, page content padded below it, scrollbar inset.
- `main/window.ts` (title bar overlay): what Electron's `titleBarOverlay` does is what the M0 hybrid variant does natively.
- `spikes/grid/src/{glass.rs, glass_*.wgsl, titlebar.rs}` (M0a): the blur pass and the window subclass.

## Tasks

### M10a

- [x] 1. `ThumbnailService::set_workers` (REQ-010-04)
- [x] 2. `AppState::update_settings` with `SettingsTargets`, error toasts, cache usage and clear jobs (REQ-010-01 … -06)
- [x] 3. Settings page: Indexing and Thumbnail cache sections, confirm dialog, UI tests (REQ-010-01 … -06)
- [x] 4. Safe relative paths in `domain` / `db`, callers treat refused paths as missing (REQ-011-08)
- [x] 5. No-network test in `xtask` (REQ-011-11)
- [x] 6. Glass and focus tokens, contrast tests from the tokens (REQ-013-04, -05), focus ring helper on widgets, tiles and rows
- [x] 7. OS preferences re-read while running (REQ-013-07)
- [x] 8. Locale dates with ICU4X (REQ-013-13)

### M10b

- [x] 9. `app::glass`: shaders, resources, backdrop recording, paint callback with opacity, fallback (REQ-013-03, -06)
- [x] 10. Glass on the toolbar, menus, popovers, dialogs; Settings header (REQ-013-03)
- [x] 11. Viewer: blurred scrim over the window, glass chrome tinted (REQ-013-14, REQ-013-11)
- [x] 12. Title bar: window subclass, caption buttons, drag areas, reserved space, minimum size test (REQ-013-02, -08)
- [x] 13. Scrollbars styled and starting below the toolbar (REQ-013-15)
- [x] 14. Same look everywhere: bundled font and own colors only, test (REQ-013-01)
- [x] 15. Snapshots, running-app screenshots compared with the reference, docs (specs, architecture, design, pitfalls, progress)

## Notes from implementing

- Both parts landed in one working tree (the glass, title bar and settings code touch the same files), so they are recorded and committed together.
- Differences from the plan: the viewer chrome's backdrop is the scrim's blur, not the shown photo (the chrome sits beside the photo; reusing the blur costs nothing); the search suggestions' and dialogs' glass is drawn behind egui's own frames; the Settings page's _Back to gallery_ and title moved into its glass header; a `--scroll <points>` option was added for screenshots of the glass toolbar over tiles.
- Side-by-side: app screenshots (gallery scrolled under the glass toolbar, viewer over the blurred window with glass chrome and filmstrip, Settings) compared with the reference's `docs/screenshots/{masonry,viewer}.png`: same layout, tokens and type; the toolbar reads a little darker over dark rows, as the reference does. The user's approval is still open.

## Exit criteria

From `docs/rewrite/plan.md`: user approved side-by-side screenshots with the reference; REQs pass.
Here: the screenshots are taken and compared in this session; the user's approval is recorded as open in the log.
