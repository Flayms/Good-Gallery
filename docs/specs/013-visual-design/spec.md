# 013 · Visual design

**Status:** Approved

## Goal

Good Gallery gets its own look instead of a UI toolkit's defaults. The photos are the only color, and controls float over them as glass. The toolbar fits small windows, the viewer floats over the blurred window and takes its color from the photo, and the app looks the same on Windows, macOS and Linux. Design decisions and tokens are recorded in [docs/design.md](../../design.md).

## Requirements

### REQ-013-01 · Same look on every platform

- **Given** the app on Windows, macOS or Linux
- **When** any screen is shown
- **Then** it uses the bundled typeface and the app's own colors and surfaces, independent of OS theme, accent color and installed fonts
- **And** only the native window controls differ between platforms

### REQ-013-02 · Content in the title bar

- **Given** the main window
- **When** it is shown
- **Then** the toolbar and sidebar header take the place of the title bar, and the native window controls (minimize, maximize, close) remain available
- **And** dragging an empty area of the toolbar or sidebar header moves the window, and double-clicking it maximizes or restores the window as the OS does
- **And** no control of the app lies under the window controls, at any window width

### REQ-013-03 · Controls float over the photos

- **Given** the gallery
- **When** the user scrolls
- **Then** media items pass underneath the toolbar and show through it, blurred
- **And** menus, popovers, the viewer's controls and its info panel show what is behind them blurred in the same way
- **And** tiles and list rows themselves are never blurred or translucent

### REQ-013-04 · Legible over any photo

- **Given** text or icons on any surface, including translucent ones and the tinted viewer chrome of REQ-013-11
- **When** the content behind is any photo, from pure white to pure black
- **Then** primary and secondary text reach a contrast ratio of at least 4.5:1

### REQ-013-05 · Visible focus over any photo

- **Given** keyboard focus on a control, a tile or a list row
- **When** it is shown over any background, from pure white to pure black
- **Then** the focus indicator reaches a contrast ratio of at least 3:1 against it

### REQ-013-06 · Reduced transparency

- **Given** the OS setting to reduce transparency
- **When** a translucent surface is shown
- **Then** it is opaque instead, with the same layout and colors otherwise

### REQ-013-07 · Reduced motion

- **Given** the OS setting to reduce motion
- **When** the viewer opens or closes, or a panel opens
- **Then** it appears or disappears without movement or scaling

### REQ-013-08 · Compact toolbar

- **Given** the gallery in a window of the minimum size (800 × 600)
- **When** the toolbar is shown
- **Then** every control is visible and usable without overlapping another or scrolling
- **And** the media kind, date and rating filters are grouped under _Filters_, which shows how many of them are active and offers _Clear filters_
- **And** gallery view, zoom, sort field and sort direction are grouped under _View_

### ~~REQ-013-09 · Immersive viewer~~

Deprecated 2026-10-25: a backdrop made from the item itself hides where the user came from. Replaced by REQ-013-14.

- **Given** the viewer
- **When** it shows a media item
- **Then** the area around the item is filled by a blurred, darkened version of the same item, and the gallery is not visible behind it
- **And** stepping to another item crossfades this backdrop

### REQ-013-10 · Viewer controls step aside

- **Given** the viewer
- **When** the pointer hasn't moved for 2.5 seconds
- **Then** the control bar and the previous / next buttons fade out, and they return on pointer movement or any key press
- **And** they stay visible while the pointer is over them, while the info panel is open, and while a video is paused

### REQ-013-11 · Photo-tinted viewer chrome

- **Given** a media item with a placeholder
- **When** the viewer shows it
- **Then** the viewer's control bar and info panel take on a dark tint of the item's colors, and the tint follows when stepping to another item
- **And** the tint never brings text below REQ-013-04

### REQ-013-12 · Tile grows into the viewer

- **Given** a tile in the Masonry, Justified or Grid view
- **When** the user opens it
- **Then** the item visibly grows from the tile's place into the viewer, and on close it returns to the tile's place in the gallery
- **And** when the tile isn't on screen (e.g. after stepping far away), the viewer fades instead

### REQ-013-13 · Readable dates

- **Given** a date shown in the info panel or the List view
- **When** it is formatted
- **Then** it uses the user's locale in medium length, with hours and minutes but without seconds (e.g. "2 Jul 2025, 07:12")

### REQ-013-14 · Viewer over the blurred window

- **Given** the viewer
- **When** it shows a media item
- **Then** the whole window behind it (gallery, toolbar and sidebar) stays visible around the item, blurred and darkened, so the user keeps a sense of where they were
- **And** the item and the viewer's controls stand out clearly against it
- **And** with reduced transparency (REQ-013-06) the area around the item is opaque instead

### REQ-013-15 · Scrollbars start below the toolbar

- **Given** a page that scrolls under the toolbar (the gallery in any view, the settings)
- **When** it is scrolled to the top
- **Then** its scrollbar, including the thumb, starts below the toolbar, so it never lies under the native window controls
- **And** the scrollbar looks the same on every platform (REQ-013-01)

## Out of scope

- A light theme or following the OS light / dark setting (the app stays dark).
- OS-specific materials such as Windows Mica / Acrylic or macOS vibrancy.
- Grouping the gallery by date and a timeline scrubber (separate spec).
- Face crops as avatars for _People_.
- Building and testing the app on macOS and Linux. This spec only avoids anything that would look different there.
- Platform-specific wording such as _Show in Explorer_ vs _Show in Finder_ (REQ-009-10).

## Assumptions

- The OS settings for reduced transparency and reduced motion are Windows' _Transparency effects_ and _Animation effects_ (Settings › Accessibility › Visual effects); the app follows them while it runs.
- Glass over the scrolling gallery keeps scrolling smooth on a typical laptop GPU: M0 measured ≈ 0.4 ms per glass surface on an integrated GPU.
- The placeholder of a media item represents its colors well enough to tint the viewer.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-013-01 | — | no test |
| REQ-013-02 | — | no test |
| REQ-013-03 | — | no test |
| REQ-013-04 | — | no test |
| REQ-013-05 | — | no test |
| REQ-013-06 | — | no test |
| REQ-013-07 | — | no test |
| REQ-013-08 | — | no test |
| ~~REQ-013-09~~ | — | no test (deprecated) |
| REQ-013-10 | — | no test |
| REQ-013-11 | — | no test |
| REQ-013-12 | — | no test |
| REQ-013-13 | — | no test |
| REQ-013-14 | — | no test |
| REQ-013-15 | — | no test |
