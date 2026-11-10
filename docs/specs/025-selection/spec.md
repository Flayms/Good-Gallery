# 025 · Selection

**Status:** Implemented

## Goal

Users who sort out a shoot want to act on many items at once instead of opening each one. A selection in the gallery holds them, in every view, up to all items of a search, and the first thing it does is rate them all (spec 024). Later actions (tags, People and Places, #32) use the same selection. A click still opens a photo as long as nothing is selected, so browsing stays one click.

## Requirements

### ~~REQ-025-01 · Select items~~

Deprecated (#191): replaced by REQ-025-10 to REQ-025-12. A click opens again, and a selection starts at a circle.

### REQ-025-02 · Select a range

- **Given** the gallery, and the item last selected or deselected (the anchor)
- **When** the user Shift-clicks another item
- **Then** all items from the anchor to that item, in gallery order, are added to the selection, and nothing opens
- **And** without an anchor only the clicked item is added, which also starts a selection

### ~~REQ-025-03 · Selection mode~~

Deprecated (#164). A selection changes what a click does again since #191, see REQ-025-12.

### REQ-025-04 · Select all, clear

- **Given** the gallery, with no text field focused
- **When** the user presses Ctrl+A, or clicks _Select all_ in the toolbar
- **Then** all items of the current search are selected, also those not loaded or scrolled into view yet and those indexed into the search while it stays selected, without slowing the gallery down (REQ-006-12)
- **And** Escape, _Clear_ in the toolbar, or a click on the gallery's background beside the items clears the selection

### REQ-025-05 · What the selection holds

- **Given** a selection
- **When** the search changes (filter, sort, library, folder), or selected items leave the index
- **Then** a changed search clears the selection, so no hidden item stays selected; items that left the index leave the selection
- **And** a rescan, or new items while indexing, keep the selection

### REQ-025-06 · Selection bar

- **Given** a selection
- **When** the gallery shows
- **Then** the toolbar shows how many items are selected (`3 selected`, the count grouped like other counts), _Rate_, _Tag_ and _Clear_ next to the search bar, which stays

### REQ-025-07 · Rate the selection

- **Given** a selection, in the desktop app
- **When** the user picks a rating or _Remove rating_ from _Rate_, or presses `1`–`5` in the gallery
- **Then** every selected item gets that rating (the keys don't toggle, as the items' ratings may differ), written as for one item (REQ-024-03), in the background, and the gallery, the sidebar and searches follow
- **And** more than 100 items are written only after the user confirms (`Rate 2,345 items with 4 stars? Their files are changed.`)
- **And** files that can't be written keep their rating and are counted in a toast that names the first one and why (REQ-024-04); the others are rated
- **And** the browser version doesn't show _Rate_ (REQ-024-05)

### ~~REQ-025-08 · Open with a double-click~~

Deprecated (#191): a click opens (REQ-025-10), and a double-click does nothing more.

### REQ-025-09 · Select a folder's items

- **Given** a library or folder in the sidebar
- **When** the user right-clicks it and picks _Select all in library_ or _Select all in folder_
- **Then** the gallery shows that library or folder, as a click on it does, and all its items are selected (REQ-025-04)

### REQ-025-10 · Open with a click

- **Given** the gallery in any view, with nothing selected
- **When** the user clicks an item, with or without Ctrl, or presses Enter on the focused item
- **Then** the viewer opens on it (REQ-009-01)

### REQ-025-11 · Start with a circle

- **Given** the gallery with nothing selected
- **When** the pointer is over an item
- **Then** a slightly transparent tick circle shows in its top-left corner (a row: on its small thumbnail), and clicking it selects the item without opening it
- **And** _Select_ in an item's context menu does the same, also where nothing can hover

### REQ-025-12 · Selection mode

- **Given** at least one selected item
- **When** the gallery shows
- **Then** every item shows a circle, empty or ticked, and a selected tile shrinks within its place, with the gallery showing around it (REQ-025-16; a row: the selected background)
- **And** a click anywhere on an item, or Space on the focused item, selects it or no longer does, and keeps the rest of the selection; Ctrl changes nothing; Enter on the focused item opens it
- **And** once nothing is selected any more a click opens again (REQ-025-10), and only the circle under the pointer shows

### REQ-025-13 · Darker under the pointer

- **Given** the gallery in a tile view
- **When** the pointer is over a tile
- **Then** the tile is slightly darker

### REQ-025-14 · Seeing a range before it is added

- **Given** a selection, and the pointer over an item
- **When** the user holds Shift
- **Then** the items a click would add (REQ-025-02) are slightly brighter, and the item under the pointer is too, instead of darker
- **And** letting go of Shift takes that away at once

### REQ-025-15 · The circle under the pointer while selecting

- **Given** at least one selected item
- **When** the pointer is over an item that isn't selected
- **Then** its circle shows the faint tick of REQ-025-11 instead of the empty circle, so the user sees a click selects it (#274)

### REQ-025-16 · Selected tiles shrink

- **Given** the gallery in a tile view
- **When** an item is selected
- **Then** its tile shrinks a little within its place, so the gallery shows around it, quickly animated; deselected, it grows back (#274)

### REQ-025-17 · Open while selecting

- **Given** at least one selected item, and the pointer over a tile or row
- **When** the user clicks the magnifier in the tile's top-right corner (a row: at its end)
- **Then** the viewer opens on it, and the selection stays as it was (#275)
- **And** without a selection no magnifier shows: a click opens (REQ-025-10)

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
| ~~REQ-025-01~~ | — | no test (deprecated) |
| REQ-025-02 | `crates/domain/src/selection.rs` › req_025_02_adds_the_range_from_the_anchor; `crates/app/tests/selection.rs` › req_025_02_shift_click_adds_a_range, req_025_02_shift_click_without_a_selection_selects_one | pass |
| ~~REQ-025-03~~ | — | no test (deprecated) |
| REQ-025-04 | `crates/domain/src/selection.rs` › req_025_04_selects_all_and_clears; `crates/app/tests/selection.rs` › req_025_04_req_025_05_selects_all_and_clears, req_025_04_select_all_button_and_a_click_on_the_background | pass |
| REQ-025-05 | `crates/domain/src/selection.rs` › req_025_05_forgets_items_that_left; `crates/app/tests/selection.rs` › req_025_04_req_025_05_selects_all_and_clears | pass |
| REQ-025-06 | `crates/app/tests/selection.rs` › req_025_06_req_025_12_circles_select_and_the_toolbar_counts | pass |
| REQ-025-07 | `crates/db/src/media.rs` › req_025_07_finds_the_files_of_the_selection; `crates/app/tests/selection.rs` › req_025_07_rates_the_selection, req_025_07_counts_the_files_it_cant_rate, req_025_07_asks_before_rating_many | pass |
| ~~REQ-025-08~~ | — | no test (deprecated) |
| REQ-025-09 | `crates/app/tests/selection.rs` › req_025_09_selects_a_library_or_folder_from_the_sidebar | pass |
| REQ-025-10 | `crates/app/tests/selection.rs` › req_025_10_a_click_opens, req_025_10_ctrl_does_nothing_special; `crates/app/tests/viewer.rs` › req_009_01_opens_the_focused_tile_with_enter | pass |
| REQ-025-11 | `crates/app/tests/selection.rs` › req_025_11_the_circle_shows_under_the_pointer (snapshot `selection_hover`), req_025_11_the_context_menu_selects | pass |
| REQ-025-12 | `crates/domain/src/selection.rs` › req_025_12_toggles_items, req_025_12_the_last_item_leaves_no_anchor; `crates/app/tests/selection.rs` › req_025_06_req_025_12_circles_select_and_the_toolbar_counts (snapshot `selection`), req_025_12_a_selection_makes_clicks_select, req_025_12_ctrl_does_nothing_special, req_025_12_enter_still_opens | pass |
| REQ-025-13 | `crates/app/tests/selection.rs` › req_025_11_the_circle_shows_under_the_pointer (snapshot `selection_hover`), req_025_13_req_025_14_shift_shows_what_a_click_would_add | pass |
| REQ-025-14 | `crates/domain/src/selection.rs` › req_025_14_says_what_a_shift_click_covers; `crates/app/tests/selection.rs` › req_025_13_req_025_14_shift_shows_what_a_click_would_add (snapshot `selection_preview`) | pass |
| REQ-025-15 | `crates/app/tests/selection.rs` › req_025_15_the_circle_under_the_pointer_shows_the_tick | pass |
| REQ-025-16 | `crates/app/tests/selection.rs` › req_025_16_a_selected_tile_shrinks (snapshot `selection`) | pass |
| REQ-025-17 | `crates/app/tests/selection.rs` › req_025_17_the_magnifier_opens_while_selecting | pass |
