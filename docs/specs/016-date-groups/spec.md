# 016 · Date groups

**Status:** Implemented

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
| REQ-016-01 | `crates/domain/src/date_groups.rs` › req_016_01_groups_consecutive_items_by_local_day_and_counts_kinds, req_016_01_keeps_the_order_and_starts_a_group_when_the_day_changes, req_016_01_days_span_local_midnight_to_midnight_across_daylight_saving_changes; `crates/db/src/gallery.rs` › req_006_08_req_016_01_lists_every_match_in_date_order_with_aspects_and_groups; `crates/app/tests/gallery.rs` › req_016_02_req_016_03_groups_by_day_until_turned_off | pass |
| REQ-016-02 | `crates/app/src/format.rs` › req_016_02_names_the_day_with_its_weekday, req_016_02_counts_photos_and_videos_of_a_group; `crates/app/tests/gallery.rs` › req_016_02_req_016_03_groups_by_day_until_turned_off | pass (English until locale formats, REQ-013-13) |
| REQ-016-03 | `crates/domain/src/view.rs` › req_016_03_is_grouped_until_the_user_turns_it_off; `crates/app/tests/gallery.rs` › req_016_02_req_016_03_groups_by_day_until_turned_off | pass (the first visible item is kept by the same anchoring as REQ-006-07) |
| REQ-016-04 | `crates/domain/src/view.rs` › req_016_04_shows_groups_only_for_date_sorts_while_grouping_is_on; `crates/db/src/gallery.rs` › req_006_08_req_016_01_lists_every_match_in_date_order_with_aspects_and_groups; `crates/app/tests/search.rs` › req_007_12_sorts_from_the_view_menu | pass |
| REQ-016-05 | `crates/domain/src/layout.rs` › req_016_05_starts_the_columns_of_each_group_level_below_its_header, req_006_04_req_016_05_lays_out_the_grid_as_squares_each_group_starting_a_new_row, req_006_03_req_016_05_fills_justified_rows_per_group_and_keeps_the_last_row_unstretched, req_016_05_interleaves_group_headers_with_the_list_items, req_006_05_req_016_05_places_list_rows_and_group_header_rows | pass |
| REQ-016-06 | `crates/domain/src/layout.rs` › req_016_06_keeps_item_tops_in_order_in_every_view, req_016_06_finds_the_items_intersecting_a_range_including_tall_ones_that_start_above_it, req_016_06_finds_headers_and_the_first_item_from_a_position | partial: scrolling, zoom and resize as REQ-006-07 / -08; scrolling to the viewed item comes with the viewer (M8) |
