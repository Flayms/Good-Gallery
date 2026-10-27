# 006 · Gallery views

**Status:** Approved

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
- **Then** tiles are placed in columns of equal width, each tile into the currently shortest column, with an 8 px gap
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

## Out of scope

- Selecting multiple media items, and drag and drop to other apps.
- Grouping by folder (section headers). Grouping by day is in 016.
- Custom column sets or sorting by clicking list headers (sorting is in 007).
- A setting for the gap size or tile corner radius.

## Assumptions

- Width and height in the index are the displayed dimensions (REQ-004-02), so layouts can be computed without loading any image.
- A search result fits in memory as a list of ids and aspect ratios (about 500k entries).

## Verification

| REQ        | Test                                                                                                                                                         | Result                                                                    |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------- |
| REQ-006-01 | — | no test |
| REQ-006-02 | `crates/domain/src/layout.rs` › req_006_02_clamps_aspects_and_treats_unknown_sizes_as_squares, req_006_02_splits_columns_and_scales_tiles, req_006_02_places_each_masonry_tile_into_the_shortest_column; `crates/app/tests/gallery.rs` › req_006_08_req_006_09_shows_a_tile_per_media_item_and_video_durations | pass |
| REQ-006-03 | `crates/domain/src/layout.rs` › req_006_03_fills_rows_to_the_container_width_and_keeps_the_last_row_at_the_target_height, req_006_03_gives_an_over_wide_panorama_a_row_of_its_own, req_006_03_ends_full_rows_flush, req_006_03_req_016_05_fills_justified_rows_per_group_and_keeps_the_last_row_unstretched | partial: layout; choosing the view comes with the toolbar (M6b) |
| REQ-006-04 | `crates/domain/src/layout.rs` › req_006_04_req_016_05_lays_out_the_grid_as_squares_each_group_starting_a_new_row | partial: layout; choosing the view comes with the toolbar (M6b) |
| REQ-006-05 | — | no test |
| REQ-006-06 | `crates/domain/src/zoom.rs` › req_006_06_turns_a_wheel_notch_into_one_step, req_006_06_collects_small_touchpad_deltas_until_they_add_up_to_a_step, req_006_06_splits_large_deltas_and_drops_the_remainder_when_the_direction_changes, req_006_06_zooms_between_2_and_12_columns_default_5; `crates/app/tests/gallery.rs` › req_006_06_ctrl_wheel_zooms_in_steps_and_remembers_the_level | partial: Ctrl + wheel; slider, buttons and the List view come with the toolbar (M6b) |
| REQ-006-07 | `crates/app/tests/gallery.rs` › req_006_07_keeps_the_first_visible_item_in_view_when_zooming, req_006_07_req_006_11_keeps_the_first_visible_item_in_view_when_items_are_added_above | pass (a resize changes the same layout key, once the width settles) |
| REQ-006-08 | `crates/domain/src/layout.rs` › req_006_08_finds_no_items_before_the_width_is_known; `crates/db/src/gallery.rs` › req_006_08_req_016_01_lists_every_match_in_date_order_with_aspects_and_groups; `crates/app/tests/gallery.rs` › req_006_08_req_006_09_shows_a_tile_per_media_item_and_video_durations | pass |
| REQ-006-09 | `crates/app/src/format.rs` › req_006_09_formats_durations_as_minutes_or_hours; `crates/app/tests/gallery.rs` › req_006_08_req_006_09_shows_a_tile_per_media_item_and_video_durations (snapshot `gallery`) | pass |
| REQ-006-10 | `crates/app/tests/snapshots.rs` › req_006_10_first_run_shows_the_no_libraries_empty_state, req_006_10_shows_no_media_found_while_nothing_is_indexed; `crates/app/tests/gallery.rs` › req_006_10_shows_skeleton_tiles_while_loading_and_the_error_when_loading_fails | partial: skeleton rows of the List view come with it (M6b) |
| REQ-006-11 | `crates/indexer/src/scan.rs` › writes_read_files_while_the_scan_still_runs; `crates/app/tests/indexing.rs` › refreshes_after_the_first_written_files_and_when_the_scan_ends; `crates/app/tests/gallery.rs` › req_006_11_shows_media_items_written_while_indexing | partial: library list, counts and gallery; tag suggestions follow in M7 |
| REQ-006-12 | `cargo xtask gallery-bench` (100,000 items, 12 columns, vsync off, AMD iGPU, mains power, 2026-10-27) | no test (measured: p95 4.4 ms, p99 5.3 ms, max 11.9 ms) |
| REQ-006-13 | `crates/app/src/textures.rs` › req_006_13_frees_least_recently_shown_textures_down_to_90_percent_of_the_budget, req_006_13_keeps_textures_shown_or_requested_in_this_or_the_last_frame; `cargo xtask gallery-bench` (at most 511 MB of a 512 MB budget) | pass |
