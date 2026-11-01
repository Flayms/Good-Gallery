# 025 · Selection

**Status:** Implemented

## Goal

Users who sort out a shoot want to act on many items at once instead of opening each one. A selection in the gallery holds them, in every view, up to all items of a search, and the first thing it does is rate them all (spec 024). Later actions (tags, People and Places, #32) use the same selection.

## Requirements

### REQ-025-01 · Select items

- **Given** the gallery in any view
- **When** the user Ctrl-clicks an item, or presses Space on the focused item
- **Then** the item is selected, or no longer selected if it was
- **And** a selected item shows a ring and a check mark (a row in the List view: the selected background and a check mark)

### REQ-025-02 · Select a range

- **Given** the gallery, and the item last clicked or Ctrl-clicked (the anchor)
- **When** the user Shift-clicks another item
- **Then** all items from the anchor to that item, in gallery order, are added to the selection
- **And** without an anchor only the clicked item is added

### REQ-025-03 · Selection mode

- **Given** a selection of at least one item
- **When** the user clicks an item without a modifier
- **Then** the item is selected or deselected instead of opening in the viewer
- **And** without a selection a click opens the viewer as before (REQ-009-01)

### REQ-025-04 · Select all, clear

- **Given** the gallery, with no text field focused
- **When** the user presses Ctrl+A
- **Then** all items of the current search are selected, also those not loaded or scrolled into view yet and those indexed into the search while it stays selected, without slowing the gallery down (REQ-006-12)
- **And** Escape, or _Clear_ in the toolbar, clears the selection

### REQ-025-05 · What the selection holds

- **Given** a selection
- **When** the search changes (filter, sort, library, folder), or selected items leave the index
- **Then** a changed search clears the selection, so no hidden item stays selected; items that left the index leave the selection
- **And** a rescan, or new items while indexing, keep the selection

### REQ-025-06 · Selection bar

- **Given** a selection
- **When** the gallery shows
- **Then** the toolbar shows how many items are selected (`3 selected`, the count grouped like other counts), _Rate_ and _Clear_ in place of the search bar

### REQ-025-07 · Rate the selection

- **Given** a selection, in the desktop app
- **When** the user picks a rating or _Remove rating_ from _Rate_, or presses `1`–`5` in the gallery
- **Then** every selected item gets that rating (the keys don't toggle, as the items' ratings may differ), written as for one item (REQ-024-03), in the background, and the gallery, the sidebar and searches follow
- **And** more than 100 items are written only after the user confirms (`Rate 2,345 items with 4 stars? Their files are changed.`)
- **And** files that can't be written keep their rating and are counted in a toast that names the first one and why (REQ-024-04); the others are rated
- **And** the browser version doesn't show _Rate_ (REQ-024-05)

## Out of scope

- Moving, copying, deleting or exporting a selection.
- Tags, People and Places for a selection (later in #32).
- Shift + arrow keys and rubber-band (drag) selection.
- Keeping the selection across a changed search.

## Assumptions

- A selection of all items of a search is kept as "all except these", so selecting 500,000 items costs nothing; it is turned into ids only when an action runs, from the layout that already holds them.
- Rating many items runs one short exiftool per file; a thousand photos take a minute or two, which the toast says while it runs.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-025-01 | `crates/domain/src/selection.rs` › req_025_01_req_025_03_toggles_items; `crates/app/tests/selection.rs` › req_025_01_req_025_06_ctrl_click_selects; snapshot `selection` | pass |
| REQ-025-02 | `crates/domain/src/selection.rs` › req_025_02_adds_the_range_from_the_anchor; `crates/app/tests/selection.rs` › req_025_02_shift_click_adds_a_range | pass |
| REQ-025-03 | `crates/domain/src/selection.rs` › req_025_01_req_025_03_toggles_items; `crates/app/tests/selection.rs` › req_025_03_a_click_selects_in_selection_mode | pass |
| REQ-025-04 | `crates/domain/src/selection.rs` › req_025_04_selects_all_and_clears; `crates/app/tests/selection.rs` › req_025_04_req_025_05_selects_all_and_clears | pass |
| REQ-025-05 | `crates/domain/src/selection.rs` › req_025_05_forgets_items_that_left; `crates/app/tests/selection.rs` › req_025_04_req_025_05_selects_all_and_clears | pass |
| REQ-025-06 | `crates/app/tests/selection.rs` › req_025_01_req_025_06_ctrl_click_selects | pass |
| REQ-025-07 | `crates/db/src/media.rs` › req_025_07_finds_the_files_of_the_selection; `crates/app/tests/selection.rs` › req_025_07_rates_the_selection, req_025_07_counts_the_files_it_cant_rate, req_025_07_asks_before_rating_many | pass |
