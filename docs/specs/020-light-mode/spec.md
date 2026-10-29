# 020 · Light mode

**Status:** Draft

## Goal

Some users work in bright rooms or simply prefer dark text on a light background. They get a light appearance that keeps the app's character: neutral chrome that never competes with the photos, glass over real content, and the same legibility as the dark appearance. By default the app follows the operating system, so it matches the rest of the desktop without being configured.

## Requirements

### REQ-020-01 · Choose the appearance

- **Given** the Settings page
- **When** the user picks _System_, _Light_ or _Dark_ under _Appearance_
- **Then** the app is drawn in that appearance from the next frame on, without a restart
- **And** a fresh installation uses _System_, and the choice is kept across restarts

### REQ-020-02 · Follow the system

- **Given** the appearance _System_
- **When** the app starts, or the operating system (in the browser version: the browser) switches between light and dark while it runs
- **Then** the app uses the system's appearance and follows each switch
- **And** where the system doesn't report one, the app is dark

### REQ-020-03 · Light surfaces

- **Given** the light appearance
- **When** the gallery, sidebar, toolbar, menus, popovers, dialogs, toasts, the info sheet or the Settings page are drawn
- **Then** they use the light tokens: neutral greys without hue, dark text and icons, and the same glass, shapes, spacing and type as the dark appearance
- **And** hue still appears only where it carries meaning (rating stars, errors, offline libraries)

### REQ-020-04 · Same legibility

- **Given** the light appearance
- **When** text, icons or the focus indicator are shown on any surface, glass over any photo from pure white to pure black included
- **Then** primary and secondary text reach a contrast ratio of at least 4.5:1, and the focus indicator at least 3:1 (as REQ-013-04 and REQ-013-05 do for dark)

### REQ-020-05 · The viewer stays dark

- **Given** the light appearance
- **When** the viewer is open
- **Then** the viewer, its controls, info panel and filmstrip look as in the dark appearance, and the window behind it is blurred and darkened as in REQ-013-14

### REQ-020-06 · Reduced transparency

- **Given** the light appearance and the OS setting to reduce transparency
- **When** a translucent surface is shown
- **Then** it is the opaque light surface instead, with the same layout and colors otherwise (as REQ-013-06)

## Out of scope

- Other themes, accent colors or a high-contrast mode.
- A light viewer.
- Switching the appearance by time of day, other than by following the system.
- A light app icon or installer.

## Assumptions

- eframe reports the system's light or dark preference on Windows, macOS and in browsers (`prefers-color-scheme`). On Linux it depends on the desktop (the XDG settings portal); where nothing is reported, REQ-020-02 falls back to dark.
- Photos are judged against a dark surround in the viewer, which is why it stays dark; the gallery is for finding, where a light surround is acceptable.

## Verification

| REQ        | Test | Result  |
| ---------- | ---- | ------- |
| REQ-020-01 | —    | no test |
| REQ-020-02 | —    | no test |
| REQ-020-03 | —    | no test |
| REQ-020-04 | —    | no test |
| REQ-020-05 | —    | no test |
| REQ-020-06 | —    | no test |
