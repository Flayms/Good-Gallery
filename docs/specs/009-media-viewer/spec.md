# 009 · Media viewer

**Status:** Implemented

## Goal

The user looks at one media item at full quality without losing their place in the gallery: steps through the current result with the keyboard, zooms into photos, plays videos, checks a file's details and tags, and jumps to the file in Explorer.

## Requirements

### REQ-009-01 · Open an item

- **Given** the gallery in any view
- **When** the user clicks (or presses Enter on) a tile or list row
- **Then** the viewer opens over the whole window, showing the item and its file name
- **And** the gallery behind it keeps its scroll position and scrolls the viewed item into view, so closing the viewer lands on it

### REQ-009-02 · Step through the gallery

- **Given** the viewer
- **When** the user presses ← / → or clicks _Previous_ / _Next_
- **Then** the previous or next item of the gallery is shown, following its current filters and sort order. There is no _Previous_ on the first item and no _Next_ on the last
- **And** the _Previous_ / _Next_ buttons stay in place while they are pressed
- **And** while video controls have focus, ← / → seek the video instead
- **And** the mouse's back button (or Alt + ←) closes the viewer instead of stepping back through the items seen

### REQ-009-03 · Close the viewer

- **Given** the viewer
- **When** the user presses Escape, clicks _Close_, presses the mouse's back button or Alt + ←, or clicks the dark area around the photo or video
- **Then** the viewer closes and the gallery is shown with the same search
- **And** clicks on the media itself, the release of a pan, and double-clicks don't close it

### REQ-009-04 · Original quality

- **Given** an image or video in the viewer
- **When** it is shown
- **Then** the original file is shown at full resolution, fitted into the window, and loaded from the library (not from the thumbnail cache)

### REQ-009-05 · Zoom and pan photos

- **Given** a photo in the viewer
- **When** the user turns the mouse wheel, double-clicks, presses `+` / `-` / `0`, or drags
- **Then** the wheel zooms around the pointer, a double-click switches between fitted and actual pixels (at least 2×) around the pointer, `+` / `-` zoom around the center, `0` fits it again, and dragging pans a zoomed photo
- **And** zoom stays between fitted (1×) and 8×, and a zoomed photo always covers the area it can fill (no panning into empty space)

### REQ-009-06 · Play videos

- **Given** a video in the viewer
- **When** it is shown
- **Then** it starts playing with standard controls, and seeking works before the whole file has loaded

### ~~REQ-009-07 · Preload neighbours~~

Deprecated 2026-10-26: one neighbour each way isn't enough on network shares, where an original takes seconds to load. Replaced by REQ-009-13.

- **Given** a photo before or after the current item in the gallery
- **When** the viewer shows the current item
- **Then** the neighbouring photos start loading, so stepping to them is quick

### REQ-009-08 · Info panel

- **Given** the viewer
- **When** the user presses `i` or clicks _Info_
- **Then** a side panel toggles, showing file name, full path (selectable), library, date taken (if known), date modified, dimensions (if known), duration (videos), file size, rating (if rated) and the item's tags, grouped into People, Places and Tags

### REQ-009-09 · Filter by a tag from the info panel

- **Given** the info panel or info sheet
- **When** the user clicks a tag
- **Then** the viewer closes and the gallery is filtered by that tag (added as included, keeping the other filters)

### REQ-009-10 · Show in Explorer

- **Given** the viewer or a tile's context menu
- **When** the user chooses _Show in Explorer_
- **Then** Explorer opens the item's folder with the file selected. If the item no longer exists, an error toast is shown

### REQ-009-11 · Context menu

- **Given** a tile, a list row or the media area of the viewer
- **When** the user right-clicks it
- **Then** a menu offers _Show in Explorer_ and _Information_
- **And** _Information_ opens the info sheet beside the gallery (same content as REQ-009-08) without opening the viewer, or opens the info panel inside the viewer

### REQ-009-12 · Missing items

- **Given** an item that is no longer indexed (e.g. removed by a scan while the viewer shows it), or whose file vanished
- **When** the viewer shows it
- **Then** it shows "Media not found", or the media fails to load, instead of showing something else

### REQ-009-13 · Preload ahead

- **Given** the viewer showing an item
- **When** photos lie up to two places before or after it in the gallery
- **Then** they start loading and are decoded ahead, so stepping to them shows the original at once
- **And** photos more than two places away from the current item stop loading, so stepping quickly through a slow library doesn't queue up downloads

### REQ-009-14 · Instant preview

- **Given** a photo whose original hasn't loaded yet
- **When** the viewer shows it
- **Then** its thumbnail is shown right away, at the size and place the original will take: the sharpest one already loaded (e.g. by its tile), else the largest
- **And** nothing else is shown before or over it: no placeholder and no loading indicator. The placeholder only stands in for a photo without a thumbnail
- **And** once the original has loaded and decoded, it fades in over the thumbnail without the photo moving or changing size
- **And** an original that is already loaded (e.g. preloaded, REQ-009-13) is shown directly, without a fade

### REQ-009-15 · Original fails to load

- **Given** a photo in the viewer
- **When** its original fails to load (e.g. the network share dropped)
- **Then** a message says so, and the thumbnail stays

### REQ-009-16 · Click to zoom

- **Given** a photo in the viewer
- **When** the user points at it
- **Then** the pointer shows a magnifier over a fitted photo and a hand over a zoomed one, and the normal pointer over the area around it, which closes the viewer (REQ-009-03)
- **When** the user clicks the photo without dragging
- **Then** a fitted photo zooms to actual pixels (at least 2×) around the pointer, and a zoomed photo fits again
- **And** a double-click on the photo switches only once, like a single click

## Out of scope

- Slideshows, rotating, cropping, editing, deleting, copying or sharing files.
- Opening a file in another app (other than revealing it in Explorer).
- Fullscreen mode and comparing items side by side. The filmstrip is specified in [015](../015-filmstrip/spec.md).
- Showing EXIF details beyond REQ-009-08.
- A larger cached preview between thumbnail and original, and previews for videos (they show their own loading state).

## Assumptions

- The video backend (libmpv, see `docs/architecture.md`) plays all supported containers and their common codecs, including HEVC.
- Originals are read straight from the library, so viewing needs the library to be reachable (thumbnails don't).

## Verification

| REQ        | Test                                                                                                                                                         | Result                                                              |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------- |
| REQ-009-01 | `crates/app/tests/viewer.rs` › req_009_01_req_009_04_opens_the_clicked_item_with_its_original, req_009_01_opens_the_focused_tile_with_enter, req_009_01_keys_dont_reach_the_window_below | pass |
| REQ-009-02 | `crates/app/tests/viewer.rs` › req_009_02_steps_through_the_gallery; `crates/domain/src/video.rs` › req_009_02_seeks_within_the_video; `crates/app/tests/viewer_video.rs` › req_009_02_arrows_seek_while_the_video_controls_have_focus | pass |
| REQ-009-03 | `crates/app/tests/viewer.rs` › req_009_03_req_007_13_closes_in_every_way_and_keeps_the_search, req_009_03_req_009_16_clicks_on_the_photo_and_double_clicks_dont_close, req_009_03_a_click_beside_a_zoomed_photo_closes | pass |
| REQ-009-04 | `crates/domain/src/viewer.rs` › req_009_04_fits_large_content_keeping_its_aspect_ratio, req_009_04_never_enlarges_content_beyond_its_own_size; `crates/media/src/original.rs` › req_009_04_decodes_the_whole_photo_upright, req_009_04_shrinks_only_what_exceeds_the_texture_limit; `crates/media/src/ffmpeg.rs` › req_009_04_loads_avif_originals_at_full_size; `crates/app/src/originals.rs` › req_009_04_req_009_13_loads_the_wanted_originals_at_full_size; `crates/app/tests/viewer.rs` › req_009_01_req_009_04_opens_the_clicked_item_with_its_original; `crates/app/tests/viewer_video.rs` › req_009_06_req_009_04_plays_the_shown_video_and_stops_it | pass (photos capped at the GPU's texture size, 8192 px; videos are rendered at the size they take on screen, never larger than decoded) |
| REQ-009-05 | `crates/domain/src/viewer.rs` › req_009_05_zooms_around_the_point_under_the_pointer, req_009_05_limits_the_scale_between_fitted_and_8x, req_009_05_keeps_zoomed_content_covering_the_viewport, req_009_05_centers_content_smaller_than_the_viewport; `crates/app/tests/viewer.rs` › req_009_05_zooms_and_pans | pass |
| REQ-009-06 | `crates/domain/src/video.rs` › req_009_06_keeps_the_volume_between_0_and_100, req_009_06_steps_the_volume_in_5_percent_steps, req_009_06_turns_rotations_into_quarters_and_swaps_sides, req_009_06_renders_at_the_size_on_screen_but_never_larger_than_the_video; `crates/app/tests/video.rs` › req_009_06_plays_all_four_containers_and_seeks, req_009_06_says_why_a_file_cant_play; `crates/app/tests/viewer_video.rs` › req_009_06_req_009_04_plays_the_shown_video_and_stops_it, req_009_06_controls_play_pause_seek_mute_and_volume, req_009_06_play_at_the_end_starts_over, req_009_06_shows_frames_fitted_and_turned_upright, req_009_06_says_when_a_video_cant_play | pass (`video.rs` needs libmpv and ffmpeg, skipped without them unless `GG_REQUIRE_HELPERS` is set; audio and display-synced playback checked in the running app) |
| ~~REQ-009-07~~ | — | no test (deprecated) |
| REQ-009-08 | `crates/db/src/media.rs` › req_009_08_returns_the_details_with_tags_by_category; `crates/app/tests/viewer.rs` › req_009_08_req_013_13_toggles_the_info_panel_with_the_details | pass |
| REQ-009-09 | `crates/app/tests/viewer.rs` › req_009_09_filters_by_a_tag_from_the_info_panel | pass |
| REQ-009-10 | `crates/app/tests/viewer.rs` › req_009_10_shows_the_file_in_explorer_or_says_it_is_gone, req_009_11_context_menus_show_in_explorer_and_information | pass (Explorer itself checked by hand: `explorer.exe /select` outside the app's job object) |
| REQ-009-11 | `crates/app/tests/viewer.rs` › req_009_11_context_menus_show_in_explorer_and_information | pass |
| REQ-009-12 | `crates/app/tests/viewer.rs` › req_009_12_shows_media_not_found_for_a_removed_item, req_009_15_says_when_the_original_fails | pass |
| REQ-009-13 | `crates/domain/src/viewer.rs` › req_009_13_lists_the_items_up_to_two_places_around_nearest_first, req_009_13_stops_at_the_ends_of_the_gallery; `crates/app/src/originals.rs` › req_009_13_frees_originals_that_are_no_longer_wanted, req_009_13_drops_photos_that_stopped_being_wanted_before_they_loaded, req_009_13_loads_a_photo_wanted_again_while_its_cancelled_load_finishes; `crates/app/tests/viewer.rs` › req_009_13_req_009_14_preloads_two_places_around | pass |
| REQ-009-14 | `crates/app/tests/viewer.rs` › req_009_13_req_009_14_preloads_two_places_around | partial: a preloaded original shows without a fade; the thumbnail preview needs the GPU streamer, checked in the running app |
| REQ-009-15 | `crates/media/src/original.rs` › req_009_15_fails_for_missing_and_broken_files_and_avif_without_ffmpeg; `crates/app/src/originals.rs` › req_009_15_remembers_failures_until_wanted_anew; `crates/app/tests/viewer.rs` › req_009_15_says_when_the_original_fails | pass |
| REQ-009-16 | `crates/domain/src/viewer.rs` › req_009_16_toggles_between_fitted_and_actual_pixels_at_least_2x; `crates/app/tests/viewer.rs` › req_009_16_click_toggles_between_fitted_and_actual_pixels, req_009_03_req_009_16_clicks_on_the_photo_and_double_clicks_dont_close | pass |
