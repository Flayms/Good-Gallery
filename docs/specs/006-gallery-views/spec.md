# 006 · Gallery views

**Status:** Implemented

## Goal

The user scrolls through hundreds of thousands of media items as smoothly as through a few hundred, in the layout that suits the content: photos at their natural aspect ratio (Masonry, Justified), a uniform overview (Grid) or file details (List). Tile size is adjustable, and the gallery reflects indexing progress live.

## Requirements

### REQ-006-01 · Choose a gallery view

- **Given** the gallery
- **When** the user picks Masonry, Justified, Grid or List under _View_ in the toolbar (013)
- **Then** the gallery shows the same media items in the same order in that view
- **And** the choice is remembered across sessions (default: Masonry), and clicking the active view keeps it

### REQ-006-02 · Masonry view

- **Given** the Masonry view
- **When** it is shown
- **Then** tiles are placed in columns of equal width, each tile into the currently shortest column, with an 8 px gap (4 px from 8 columns on)
- **And** each tile keeps its media item's aspect ratio, limited to between 3:1 landscape and 1:3 portrait (cropped beyond that), and is square if the dimensions are unknown

### REQ-006-03 · Justified view

- **Given** the Justified view
- **When** it is shown
- **Then** tiles are placed in rows that fill the full width exactly, all tiles of a row equally tall and at their aspect ratio (limited as in REQ-006-02)
- **And** rows are about as tall as 3/4 of a Masonry column at the same zoom level, the last row isn't stretched, and an item too wide for the target height gets a row of its own

### REQ-006-04 · Grid view

- **Given** the Grid view
- **When** it is shown
- **Then** tiles are equal squares in columns, with the thumbnail cropped to fill them

### REQ-006-05 · List view

- **Given** the List view
- **When** it is shown
- **Then** each media item is a row with a small thumbnail, file name (videos with a play icon and duration), folder, date, size, dimensions and rating stars, under a header that stays visible while scrolling
- **And** the date is the date taken. Without one, the date modified is shown dimmed and its tooltip says so

### REQ-006-06 · Zoom

- **Given** the Masonry, Justified or Grid view
- **When** the user moves the zoom slider or clicks _Zoom in_ / _Zoom out_ under _View_, or turns the mouse wheel while holding Ctrl over the gallery
- **Then** the tiles grow or shrink in steps between 12 and 2 columns (default 5), one step per wheel notch, with small touchpad movements adding up to steps and a change of direction starting over
- **And** the zoom level is remembered across sessions, and the zoom controls are disabled in the List view

### REQ-006-07 · Keep the scroll position

- **Given** the gallery scrolled to some item
- **When** the zoom level or the window size changes
- **Then** the item that was first visible stays in view

### REQ-006-08 · Large galleries

- **Given** a search matching hundreds of thousands of media items
- **When** the gallery shows it
- **Then** the scrollbar covers the whole result right away, so the user can jump anywhere, and scrolling stays smooth
- **And** item details load only for the visible range. Until then, positions show a gray placeholder

### REQ-006-09 · Video tiles

- **Given** a video tile
- **When** it is shown
- **Then** it shows a play badge with the duration (`m:ss`, or `h:mm:ss` from one hour on)

### REQ-006-10 · Empty, loading and error states

- **Given** the gallery
- **When** there are no libraries yet / nothing matches the search / the media items are still loading / loading failed
- **Then** it shows "No libraries yet" with an _Add a library_ link to Settings / "No media found" with a hint that new files appear while indexing / skeleton tiles or rows matching the view / "Couldn't load media" with the error message

### REQ-006-11 · Live updates while indexing

- **Given** a running scan
- **When** it writes media items to the index
- **Then** the gallery, sidebar counts and tag suggestions refresh right after the first written files, then at most every 5 seconds, and once more when the scan ends

### REQ-006-12 · Frame-time budget

- **Given** a gallery of 100,000 media items at 12 columns, with their thumbnails in the thumbnail cache
- **When** the user scrolls continuously
- **Then** the 95th percentile of frame times stays below 16 ms on an integrated laptop GPU (measured without vsync)

### REQ-006-13 · Thumbnail memory budget

- **Given** the user scrolls through a large gallery
- **When** thumbnails are loaded for display
- **Then** the memory they take on the GPU stays within a fixed budget, dropping the least recently shown first
- **And** thumbnails of visible tiles and of tiles about to scroll into view are kept

### REQ-006-14 · Autoscroll with the middle mouse button

- **Given** the gallery in any view
- **When** the user presses the middle mouse button over it and moves the pointer up or down
- **Then** a marker shows where the button was pressed, and the gallery scrolls in the pointer's direction, not at all within 12 points of the marker and beyond that with a speed that grows faster than the distance, as in Firefox (`(d / 12)^1.5 − 1` points every 20 ms at distance `d`, so slow near the marker and fast far from it)
- **And** it stops at the start and the end of the gallery, without scrolling past them
- **And** releasing the button stops it if the pointer had left those 12 points; otherwise autoscroll stays on until the next click of any button, a key press or a turn of the mouse wheel or touchpad, and that click does nothing else (it doesn't open a media item or a context menu)

### REQ-006-15 · Frame-time budget while thumbnails render

- **Given** a gallery of 100,000 media items at 12 columns whose thumbnails aren't in the thumbnail cache yet
- **When** the user scrolls continuously while they render
- **Then** the 95th percentile of frame times stays within REQ-006-12's budget (below 16 ms on an integrated laptop GPU, measured without vsync)
- **And** rendering thumbnails runs at a lower priority than the UI

### REQ-006-16 · Years beside the scrollbar

- **Given** a gallery sorted by a date that doesn't fit the window
- **When** the pointer is near the scrollbar, or the scrollbar is being dragged
- **Then** the years of the result show beside the scrollbar, each at the height the thumb has when the gallery is scrolled to that year's first item, so a year holding more items takes more of the bar
- **And** a year too close to the one above it isn't shown, so labels never overlap
- **And** clicking a year scrolls to its first item
- **And** with the pointer elsewhere (unless the gallery scrolls, REQ-006-18), under a menu or the viewer, during autoscroll, or for another sort, no years show

### REQ-006-17 · Month while dragging

- **Given** a gallery sorted by a date
- **When** the user drags the scrollbar
- **Then** the month and year at the top of the gallery show beside the thumb ("June 2023"), and go away on release

### REQ-006-18 · Years while scrolling

- **Given** a gallery sorted by a date that doesn't fit the window
- **When** the user scrolls it with the wheel or the touchpad, wherever the pointer is
- **Then** the years show beside the scrollbar as in REQ-006-16, and fade out about a second after the scrolling stops
- **And** a new result, or a jump to a year, doesn't show them by itself

### REQ-006-19 · Month at the pointer

- **Given** a gallery sorted by a date, with the years shown because the pointer is near the scrollbar (REQ-006-16)
- **When** the pointer rests at a height of the bar, without dragging
- **Then** the month and year the gallery would show at its top with the thumb at that height show beside the bar at the pointer ("June 2023"), above any year they would touch
- **And** they follow the pointer, and go away with the years

### REQ-006-20 · Jump to a month

- **Given** the month and year showing at the pointer beside the scrollbar (REQ-006-19)
- **When** the user clicks them
- **Then** the gallery scrolls to the first item of that month, as clicking a year does to the year's first item (REQ-006-16)

### REQ-006-21 · Drag beside the bar

- **Given** the years shown beside the scrollbar (REQ-006-16)
- **When** the user presses beside the bar, where the years are, and drags
- **Then** the gallery scrolls as when dragging the scrollbar: the thumb's middle follows the pointer and the month shows beside it (REQ-006-17)
- **And** the gallery isn't dragged like content and no photo opens; a click on a year or the month without dragging still jumps (REQ-006-16, REQ-006-20)

### REQ-006-22 · Years cover the tiles

- **Given** the years shown beside the scrollbar over the edge of the gallery
- **When** the pointer is on them
- **Then** the tile or row below shows no hover and no selection circle (#276)

## Out of scope

- Selecting multiple media items, and drag and drop to other apps.
- Grouping by folder (section headers). Grouping by day is in 016.
- Custom column sets or sorting by clicking list headers (sorting is in 007).
- A setting for the gap size or tile corner radius.
- Autoscroll sideways, and a setting for its speed.

## Assumptions

- Width and height in the index are the displayed dimensions (REQ-004-02), so layouts can be computed without loading any image.
- A search result fits in memory as a list of ids and aspect ratios (about 500k entries).

## Verification

| REQ        | Test                                                                                                                                                         | Result                                                                    |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------- |
| REQ-006-01 | `crates/domain/src/view.rs` › req_006_01_remembers_the_view_default_masonry; `crates/app/tests/gallery.rs` › req_006_01_picks_the_view_in_the_view_menu_and_remembers_it (snapshot `view_menu`), req_006_01_the_view_menu_holds_still | pass |
| REQ-006-02 | `crates/domain/src/layout.rs` › req_006_02_clamps_aspects_and_treats_unknown_sizes_as_squares, req_006_02_splits_columns_and_scales_tiles, req_006_02_places_each_masonry_tile_into_the_shortest_column; `crates/app/tests/gallery.rs` › req_006_08_req_006_09_shows_a_tile_per_media_item_and_video_durations | pass |
| REQ-006-03 | `crates/domain/src/layout.rs` › req_006_03_fills_rows_to_the_container_width_and_keeps_the_last_row_at_the_target_height, req_006_03_gives_an_over_wide_panorama_a_row_of_its_own, req_006_03_ends_full_rows_flush, req_006_03_req_016_05_fills_justified_rows_per_group_and_keeps_the_last_row_unstretched; `crates/app/tests/gallery.rs` › req_006_01_picks_the_view_in_the_view_menu_and_remembers_it | pass |
| REQ-006-04 | `crates/domain/src/layout.rs` › req_006_04_req_016_05_lays_out_the_grid_as_squares_each_group_starting_a_new_row | pass |
| REQ-006-05 | `crates/domain/src/layout.rs` › req_006_05_req_016_05_places_list_rows_and_group_header_rows; `crates/app/src/gallery/list.rs` › req_006_05_splits_the_width_into_columns; `crates/app/tests/gallery.rs` › req_006_05_shows_a_row_per_media_item_in_the_list_view (snapshot `gallery_list`) | pass |
| REQ-006-06 | `crates/domain/src/zoom.rs` › req_006_06_turns_a_wheel_notch_into_one_step, req_006_06_collects_small_touchpad_deltas_until_they_add_up_to_a_step, req_006_06_splits_large_deltas_and_drops_the_remainder_when_the_direction_changes, req_006_06_zooms_between_2_and_12_columns_default_5; `crates/app/tests/gallery.rs` › req_006_06_ctrl_wheel_zooms_in_steps_and_remembers_the_level, req_006_06_zooms_with_the_buttons_and_not_in_the_list_view | pass |
| REQ-006-07 | `crates/app/tests/gallery.rs` › req_006_07_keeps_the_first_visible_item_in_view_when_zooming, req_006_07_req_006_11_keeps_the_first_visible_item_in_view_when_items_are_added_above | pass (a resize changes the same layout key, once the width settles) |
| REQ-006-08 | `crates/domain/src/layout.rs` › req_006_08_finds_no_items_before_the_width_is_known; `crates/db/src/gallery.rs` › req_006_08_req_016_01_lists_every_match_in_date_order_with_aspects_and_groups; `crates/app/tests/gallery.rs` › req_006_08_req_006_09_shows_a_tile_per_media_item_and_video_durations | pass |
| REQ-006-09 | `crates/app/src/format.rs` › req_006_09_formats_durations_as_minutes_or_hours; `crates/app/tests/gallery.rs` › req_006_08_req_006_09_shows_a_tile_per_media_item_and_video_durations (snapshot `gallery`) | pass |
| REQ-006-10 | `crates/app/tests/snapshots.rs` › req_006_10_first_run_shows_the_no_libraries_empty_state, req_006_10_shows_no_media_found_while_nothing_is_indexed; `crates/app/tests/gallery.rs` › req_006_10_shows_skeleton_tiles_while_loading_and_the_error_when_loading_fails | pass (skeleton rows of the List view checked in the app) |
| REQ-006-11 | `crates/indexer/src/scan.rs` › writes_read_files_while_the_scan_still_runs; `crates/app/tests/indexing.rs` › refreshes_after_the_first_written_files_and_when_the_scan_ends; `crates/app/tests/gallery.rs` › req_006_11_shows_media_items_written_while_indexing | pass (the sidebar reloads with the gallery; suggestions are asked for per input) |
| REQ-006-12 | `cargo xtask gallery-bench` (100,000 items, 12 columns, vsync off, AMD iGPU, mains power, 2026-10-27) | no test (measured: p95 4.4 ms, p99 5.3 ms, max 11.9 ms) |
| REQ-006-13 | `crates/app/src/textures.rs` › req_006_13_frees_least_recently_shown_textures_down_to_90_percent_of_the_budget, req_006_13_keeps_textures_shown_or_requested_in_this_or_the_last_frame; `cargo xtask gallery-bench` (at most 511 MB of a 512 MB budget) | pass |
| REQ-006-14 | `crates/domain/src/autoscroll.rs` › req_006_14_scrolls_toward_the_pointer_faster_the_farther_it_is, req_006_14_releasing_after_moving_stops_it, req_006_14_a_click_keeps_it_on_until_the_next_click; `crates/app/tests/gallery.rs` › req_006_14_scrolls_while_the_middle_button_is_held_and_after_a_middle_click, req_006_14_stops_at_the_end_without_scrolling_past_it, req_006_14_the_wheel_or_a_key_stops_it | pass |
| REQ-006-15 | `crates/indexer/src/thumbnails.rs` › req_006_15_renders_below_the_ui_threads_priority; `cargo xtask gallery-bench --originals` (Linux VM with lavapipe: cold p95 527 → 307 ms with the lowered priority, warm 223 ms) | partial (the 16 ms budget on an integrated laptop GPU not measured yet) |
| REQ-006-16 | `crates/domain/src/scrollbar.rs` › req_006_16_marks_where_each_year_starts_in_either_order, req_006_16_places_a_year_where_the_thumb_is_when_scrolled_there, req_006_16_a_long_gallery_has_the_shortest_thumb, req_006_16_drops_years_too_close_together; `crates/app/tests/gallery.rs` › req_006_16_shows_years_near_the_scrollbar_and_jumps_to_one (snapshot `gallery_years`), req_006_16_leaves_out_years_too_close_together, req_006_16_shows_no_years_for_other_sorts, req_006_16_shows_no_years_without_a_scrollbar | pass |
| REQ-006-17 | `crates/app/src/format.rs` › req_006_17_names_the_month_and_year; `crates/app/tests/gallery.rs` › req_006_17_shows_the_month_while_dragging_the_scrollbar (snapshot `gallery_years_dragging`) | pass |
| REQ-006-18 | `crates/app/tests/gallery.rs` › req_006_18_scrolling_shows_the_years_until_it_stops, req_006_18_a_jump_shows_no_years | pass |
| REQ-006-19 | `crates/domain/src/scrollbar.rs` › req_006_19_the_offset_at_a_height_undoes_the_thumbs_center; `crates/app/tests/gallery.rs` › req_006_19_shows_the_month_at_the_pointer (snapshot `gallery_years_hover`) | pass |
| REQ-006-20 | `crates/app/tests/gallery.rs` › req_006_20_clicking_the_month_jumps_to_its_first_item | pass |
| REQ-006-21 | `crates/app/tests/gallery.rs` › req_006_21_dragging_beside_the_bar_scrolls_like_the_bar | pass |
| REQ-006-22 | `crates/app/tests/gallery.rs` › req_006_22_a_year_hides_the_tile_below_from_the_pointer | pass |
