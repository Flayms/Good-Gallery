# 016 · Date groups

**Status:** Approved

## Goal

In large libraries the user loses track of where they are in time while scrolling. Grouping the gallery by day, with the date and the number of media items above each group, shows when photos were taken. It also makes a day, like an event or a trip, easy to find. It is on by default because the gallery is sorted by date taken by default, and most libraries are browsed that way.

## Requirements

### REQ-016-01 · Group by day

- **Given** the gallery sorted by _Date taken_ or _Date modified_, with grouping on
- **When** it is shown
- **Then** consecutive media items whose sort date (REQ-007-12) falls on the same local calendar day form a group, and each group starts with a header
- **And** media items keep the order of the search. With _Date taken_, a media item without a date taken goes into the group of its date modified, as in sorting

### REQ-016-02 · Group header

- **Given** a group header
- **When** it is shown
- **Then** it shows the day with the weekday and the full date in the user's locale (e.g. "Monday, 26 October 2026"), and the number of media items in the group
- **And** the number is broken down by media kind: "42 photos", "1 video", or "42 photos · 3 videos". Numbers use the locale's grouping

### REQ-016-03 · Turn grouping on and off

- **Given** the gallery
- **When** the user switches _Group by day_ under _View_ in the toolbar
- **Then** the gallery shows or hides the groups and keeps the first visible media item in view
- **And** the choice is remembered across sessions. Grouping is on until the user turns it off

### REQ-016-04 · Sort orders without dates

- **Given** grouping is on
- **When** the gallery is sorted by _Name_, _Full path_ or _Size_
- **Then** the gallery shows no groups, and _Group by day_ is disabled with the hint "Only when sorted by date"
- **And** the groups come back when the user sorts by a date again

### REQ-016-05 · Views in groups

- **Given** grouping is on
- **When** the gallery is shown in any view (REQ-006-01)
- **Then** each group is laid out on its own under its header, following the view's rules (REQ-006-02 to REQ-006-05): Masonry columns and Grid rows start level below the header, and the last Justified row of a group isn't stretched
- **And** in the List view the headers are rows between the media items, below the column header

### REQ-016-06 · Large grouped galleries

- **Given** grouping is on and the search matches hundreds of thousands of media items in thousands of groups
- **When** the user scrolls, zooms, resizes the window or closes the viewer
- **Then** REQ-006-07 and REQ-006-08 still hold: the scrollbar covers the whole result right away, scrolling stays smooth, the first visible media item stays in view across zoom and resize, and the item shown in the viewer is scrolled into view

## Out of scope

- Grouping by month, year, folder or other fields. Other fields could be added later as other choices for the same setting.
- Sticky headers, collapsing groups, and selecting all media items of a group.
- Relative day names ("Today", "Yesterday").
- Groups in the viewer's filmstrip (015).

## Assumptions

- Days are local calendar days of the machine that runs the app, as with the date filter (REQ-007-08).
- The number of groups is small compared with the number of media items. Even one group per item still fits in memory.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-016-01 | — | no test |
| REQ-016-02 | — | no test |
| REQ-016-03 | — | no test |
| REQ-016-04 | — | no test |
| REQ-016-05 | — | no test |
| REQ-016-06 | — | no test |
