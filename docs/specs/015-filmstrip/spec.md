# 015 · Filmstrip

**Status:** Approved (ported from the reference, not yet implemented in Rust)

## Goal

In the viewer, the user sees a single row of thumbnails of the whole gallery below the photo, with the shown item highlighted, so they can jump to any item with one click instead of stepping one by one with ← / →. It is optional, so the photo keeps the whole window for users who don't want it.

## Requirements

### REQ-015-01 · Show and hide the filmstrip

- **Given** the viewer
- **When** the user clicks _Filmstrip_ in the control bar or presses `f`
- **Then** a single row of thumbnails of all gallery items appears below the photo or video, in the gallery's order with its current filters and sort order. Doing it again hides the row
- **And** the photo or video fits into the space above the filmstrip and is never covered by it

### REQ-015-02 · Remember the choice

- **Given** the user has shown or hidden the filmstrip
- **When** they close the viewer and open it again, also after restarting the app
- **Then** the filmstrip is shown or hidden as they left it. It is hidden until the user shows it the first time

### REQ-015-03 · Highlight the shown item

- **Given** the filmstrip
- **When** the viewer shows an item, by opening it, stepping with ← / →, or clicking a thumbnail
- **Then** that item's thumbnail is highlighted and marked as the current one for assistive technology
- **And** the filmstrip scrolls so the highlighted thumbnail is centered, as far as the ends of the gallery allow
- **And** an item that isn't part of the gallery (e.g. opened from a link) highlights nothing

### REQ-015-04 · Jump to an item

- **Given** the filmstrip
- **When** the user clicks a thumbnail, or focuses it and presses Enter
- **Then** the viewer shows that item, the same way as stepping to it (REQ-009-02): no history entry, and the photos around it start preloading (REQ-009-13)

### REQ-015-05 · Browse the filmstrip

- **Given** the filmstrip
- **When** the user turns the mouse wheel over it
- **Then** it scrolls sideways without changing the shown item
- **And** thumbnails load as they come into view, and the filmstrip stays smooth for galleries of 500 000 items

### REQ-015-06 · Steps aside with the controls

- **Given** the filmstrip
- **When** the viewer's controls step aside (REQ-013-10)
- **Then** the filmstrip fades out with them and returns with them, and stays while the pointer is over it
- **And** the photo or video doesn't move or change size when it fades

### REQ-015-07 · Videos stand out

- **Given** the filmstrip
- **When** it shows a video's thumbnail
- **Then** a play symbol over the thumbnail marks it as a video; photos have none

## Out of scope

- More than one row, a resizable filmstrip, or a setting for the thumbnail size.
- Selecting several items, reordering, or dragging items out of the filmstrip.
- A filmstrip in the gallery outside the viewer.
- Video durations and ratings on the filmstrip's thumbnails.

## Assumptions

- The filmstrip uses the same thumbnails as the gallery's tiles, so most are already in the thumbnail cache or in memory.
- Videos show their poster-frame thumbnail like in the gallery.

## Verification

| REQ        | Test                                                                                                                                 | Result                                                                                         |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------- |
| REQ-015-01 | — | no test |
| REQ-015-02 | — | no test |
| REQ-015-03 | — | no test |
| REQ-015-04 | — | no test |
| REQ-015-05 | — | no test |
| REQ-015-06 | — | no test |
| REQ-015-07 | — | no test |
