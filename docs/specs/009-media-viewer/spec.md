# 009 · Media viewer

**Status:** Implemented

## Goal

The user looks at one media item at full quality without losing their place in the gallery: steps through the current result with the keyboard, zooms into photos, plays videos, checks a file's details and tags, and jumps to the file in Explorer.

## Requirements

### REQ-009-01 · Open an item

- **Given** the gallery in any view
- **When** the user clicks a tile or list row with nothing selected (REQ-025-10), or presses Enter on it
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
- **When** the user turns the mouse wheel, with or without Ctrl, scrolls or pinches on a touchpad, double-clicks, presses `+` / `-` / `0`, or drags
- **Then** the wheel, scrolling and pinching zoom around the pointer, a double-click switches between fitted and actual pixels (at least 2×) around the pointer, `+` / `-` zoom around the center, `0` fits it again, and dragging pans a zoomed photo
- **And** zoom stays between fitted (1×) and 8×, and a zoomed photo always covers the area it can fill (no panning into empty space; the area is the window, REQ-009-20)

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
- **Then** a side panel toggles, showing file name, full path (selectable), library, date taken, date modified, dimensions, duration (videos), file size, rating, whether it is an edit (edited items only) and the item's tags, grouped into People, Places and Tags
- **And** a field the item has no value for (no date taken, dimensions, duration, people, places or tags) keeps its title and place with nothing under it, so the fields are always in the same order
- **And** the rating shows as five stars, outlined when unrated, which change it (REQ-024-02); in the desktop app People, Places and Tags offer _Add_ and remove buttons (REQ-026-01, REQ-026-02)

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
- **And** an item that is still indexed but no longer matches the search isn't missing (REQ-009-18)

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

### REQ-009-17 · Face regions

- **Given** a photo with face regions (REQ-004-13) in the viewer
- **When** the user turns face regions on (`P` or the control bar's _Faces_ button, shown only for photos with face regions, or for all photos while faces are looked for, REQ-032-09)
- **Then** each region is drawn as a box with the person's name, on the person, upright and at any zoom and pan
- **And** the choice is remembered across items and restarts, and turning it off hides the boxes

### REQ-009-18 · An item leaves the search

- **Given** the viewer showing an item that is still indexed
- **When** the gallery reloads and the item no longer matches the search (a tag or rating changed in the viewer, or a scan changed them)
- **Then** the viewer keeps showing it, with the info panel, rating, tagging and editing working as before
- **When** the user steps to the next or the previous item, or picks one in the filmstrip
- **Then** the viewer goes there, to the neighbours of the place the item had, and the item can't be stepped back to
- **When** the user closes the viewer
- **Then** it fades, with no tile to shrink into, and the gallery shows the search's result, even an empty one
- **When** the item matches the search again
- **Then** the viewer follows it to its new position

### REQ-009-19 · The photo in the middle

- **Given** the viewer, with or without the filmstrip and the info panel
- **When** the item is shown, and when the controls fade out (REQ-013-10)
- **Then** the photo or video is centered in the area beside the info panel, with as much room above it as below, and it doesn't move when the controls fade
- **And** the control bar with the file name and the buttons sits above it, in the title bar's strip, clear of the window controls

### REQ-009-20 · A zoomed photo fills the window

- **Given** a photo zoomed in the viewer (REQ-009-05) beyond the media area
- **When** it is shown or panned
- **Then** it spreads over the whole window beside the info panel, under the control bar, the arrows and the filmstrip, which stay on top of it
- **And** panning stops where its edges reach the window's or the info panel's, not the media area's
- **And** fitted (1×) it stays in the media area, in the middle (REQ-009-19)

### REQ-009-21 · Rename, rate and rotate from the menu

- **Given** a tile's, a list row's or the viewer's context menu (REQ-009-11) in the desktop app
- **When** the user chooses _Rename…_, a rating under _Rating_ (_No rating_, 1 to 5 stars), _Rotate left_ or _Rotate right_
- **Then** it applies to the selection if the item is in it, else to that item alone
- **And** _Rename…_ opens the rename dialog for them (REQ-035-01), a rating is written as from the keyboard (REQ-024-03, REQ-025-07), and rotating turns each photo by a quarter (REQ-009-22)
- **And** _Rotate_ is off for videos and for photos whose format has no orientation (GIF, AVIF), and the browser version has none of these items

### REQ-009-22 · Rotate without re-encoding

- **Given** photos rotated from the context menu
- **When** they are turned
- **Then** only their orientation tag changes, turned a quarter from the one they have (a mirrored photo stays mirrored), and their pixels are written as they were; the date modified stays
- **And** their tile, thumbnail and the viewer show them turned
- **And** a file that can't be written stays as it was, and a message names it and says why

### REQ-009-23 · Delete

- **Given** a tile's, a list row's or the viewer's context menu (REQ-009-11) in the desktop app, a selection in the gallery, or an item in the viewer
- **When** the user chooses _Delete…_ or presses Delete
- **Then** it applies to the selection if the item is in it, else to that item alone (Delete in the gallery applies to the selection)
- **And** a dialog always asks first, naming how many items go and where: to the Recycle Bin (the Trash on Linux), or deleted permanently when one of their libraries is on a network drive, which has no Recycle Bin; its button then says _Delete permanently_
- **When** confirmed, **then** each file goes there with its sidecars (`IMG_1.jpg.xmp`, and `IMG_1.xmp` unless another file of that name shares it)
- **And** a file that can't be deleted stays, the others still go, and a message says how many failed, names the first and says why
- **And** the browser version and files outside every library have no _Delete…_

### REQ-009-24 · Deleted items leave the gallery

- **Given** items deleted (REQ-009-23)
- **When** their files are gone
- **Then** they leave the index, the gallery, the selection and the counts right away, and their cached thumbnails are removed
- **And** the viewer showing one moves on to the item that takes its place, or closes when none is left

### REQ-009-25 · Copy a photo

- **Given** a photo's context menu, on its tile, its row or in the viewer, in the desktop app
- **When** the user chooses _Copy_
- **Then** the photo goes on the clipboard as an image, upright as the viewer shows it, at full size up to 8192 pixels on its longer side, without unsaved edits
- **And** _Copy_ says so with a short notice, or why it couldn't copy; videos and the browser version have no _Copy_

## Out of scope

- Face regions in videos. Adding, editing and removing face regions and finding faces are spec 032.
- Slideshows, copying or sharing files. Undoing a delete in the app (the Recycle Bin is the undo), and deleting the backups of edited originals (spec 021), which stay. Rotating with new pixels, cropping and editing are specs 021 and 029.
- Opening a file in another app (other than revealing it in Explorer).
- Fullscreen mode and comparing items side by side. The filmstrip is specified in [015](../015-filmstrip/spec.md).
- Showing EXIF details beyond REQ-009-08.
- A larger cached preview between thumbnail and original, and previews for videos (they show their own loading state).

## Assumptions

- The video backend (libmpv, see the plan) plays all supported containers and their common codecs, including HEVC.
- Originals are read straight from the library, so viewing needs the library to be reachable (thumbnails don't).

## Verification

| REQ        | Test                                                                                                                                                         | Result                                                              |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------- |
| REQ-009-01 | `crates/app/tests/viewer.rs` › req_009_01_req_009_04_opens_the_clicked_item_with_its_original, req_009_01_opens_the_focused_tile_with_enter, req_009_01_keys_dont_reach_the_window_below | pass |
| REQ-009-02 | `crates/app/tests/viewer.rs` › req_009_02_steps_through_the_gallery; `crates/domain/src/video.rs` › req_009_02_seeks_within_the_video; `crates/app/tests/viewer_video.rs` › req_009_02_arrows_seek_while_the_video_controls_have_focus | pass |
| REQ-009-03 | `crates/app/tests/viewer.rs` › req_009_03_req_007_13_closes_in_every_way_and_keeps_the_search, req_009_03_req_009_16_clicks_on_the_photo_and_double_clicks_dont_close, req_009_03_a_click_beside_a_zoomed_photo_closes | pass |
| REQ-009-04 | `crates/domain/src/viewer.rs` › req_009_04_fits_large_content_keeping_its_aspect_ratio, req_009_04_never_enlarges_content_beyond_its_own_size; `crates/media/src/original.rs` › req_009_04_decodes_the_whole_photo_upright, req_004_12_req_009_04_turns_photos_oriented_only_in_xmp_upright, req_009_04_shrinks_only_what_exceeds_the_texture_limit; `crates/media/src/ffmpeg.rs` › req_009_04_loads_avif_originals_at_full_size; `crates/app/src/originals.rs` › req_009_04_req_009_13_loads_the_wanted_originals_at_full_size; `crates/app/tests/viewer.rs` › req_009_01_req_009_04_opens_the_clicked_item_with_its_original; `crates/app/tests/viewer_video.rs` › req_009_06_req_009_04_plays_the_shown_video_and_stops_it | pass (photos capped at the GPU's texture size, 8192 px; videos are rendered at the size they take on screen, never larger than decoded) |
| REQ-009-05 | `crates/domain/src/viewer.rs` › req_009_05_zooms_around_the_point_under_the_pointer, req_009_05_limits_the_scale_between_fitted_and_8x, req_009_05_keeps_zoomed_content_covering_the_viewport, req_009_05_centers_content_smaller_than_the_viewport; `crates/app/tests/viewer.rs` › req_009_05_zooms_and_pans, req_009_05_a_touchpad_pinch_zooms | partial: touchpad events are simulated; winit reports no pinch on Linux |
| REQ-009-06 | `crates/domain/src/video.rs` › req_009_06_keeps_the_volume_between_0_and_100, req_009_06_steps_the_volume_in_5_percent_steps, req_009_06_turns_rotations_into_quarters_and_swaps_sides, req_009_06_renders_at_the_size_on_screen_but_never_larger_than_the_video; `crates/app/tests/video.rs` › req_009_06_plays_all_four_containers_and_seeks, req_009_06_says_why_a_file_cant_play; `crates/app/tests/viewer_video.rs` › req_009_06_req_009_04_plays_the_shown_video_and_stops_it, req_009_06_controls_play_pause_seek_mute_and_volume, req_009_06_play_at_the_end_starts_over, req_009_06_shows_frames_fitted_and_turned_upright, req_009_06_says_when_a_video_cant_play | pass (`video.rs` needs libmpv and ffmpeg, skipped without them unless `GG_REQUIRE_HELPERS` is set; audio and display-synced playback checked in the running app) |
| ~~REQ-009-07~~ | — | no test (deprecated) |
| REQ-009-08 | `crates/db/src/media.rs` › req_009_08_returns_the_details_with_tags_by_category; `crates/app/tests/viewer.rs` › req_009_08_req_013_13_toggles_the_info_panel_with_the_details, req_009_08_shows_the_titles_of_details_the_item_lacks | pass |
| REQ-009-09 | `crates/app/tests/viewer.rs` › req_009_09_filters_by_a_tag_from_the_info_panel | pass |
| REQ-009-10 | `crates/app/tests/viewer.rs` › req_009_10_shows_the_file_in_explorer_or_says_it_is_gone, req_009_11_context_menus_show_in_explorer_and_information | pass (Explorer itself checked by hand: `explorer.exe /select` outside the app's job object) |
| REQ-009-11 | `crates/app/tests/viewer.rs` › req_009_11_context_menus_show_in_explorer_and_information | pass |
| REQ-009-12 | `crates/app/tests/viewer.rs` › req_009_12_shows_media_not_found_for_a_removed_item, req_009_15_says_when_the_original_fails, req_009_18_says_when_a_kept_item_is_gone | pass |
| REQ-009-13 | `crates/domain/src/viewer.rs` › req_009_13_lists_the_items_up_to_two_places_around_nearest_first, req_009_13_stops_at_the_ends_of_the_gallery; `crates/app/src/originals.rs` › req_009_13_frees_originals_that_are_no_longer_wanted, req_009_13_drops_photos_that_stopped_being_wanted_before_they_loaded, req_009_13_loads_a_photo_wanted_again_while_its_cancelled_load_finishes; `crates/app/tests/viewer.rs` › req_009_13_req_009_14_preloads_two_places_around | pass |
| REQ-009-14 | `crates/app/tests/viewer.rs` › req_009_13_req_009_14_preloads_two_places_around | partial: a preloaded original shows without a fade; the thumbnail preview needs the GPU streamer, checked in the running app |
| REQ-009-15 | `crates/media/src/original.rs` › req_009_15_fails_for_missing_and_broken_files_and_avif_without_ffmpeg; `crates/app/src/originals.rs` › req_009_15_remembers_failures_until_wanted_anew; `crates/app/tests/viewer.rs` › req_009_15_says_when_the_original_fails | pass |
| REQ-009-16 | `crates/domain/src/viewer.rs` › req_009_16_toggles_between_fitted_and_actual_pixels_at_least_2x; `crates/app/tests/viewer.rs` › req_009_16_click_toggles_between_fitted_and_actual_pixels, req_009_03_req_009_16_clicks_on_the_photo_and_double_clicks_dont_close | pass |
| REQ-009-18 | `crates/domain/src/viewer.rs` › req_009_18_steps_from_the_place_the_item_had; `crates/app/tests/viewer.rs` › req_009_18_keeps_an_item_that_left_the_search, req_009_18_steps_back_from_an_item_that_left_the_search, req_009_18_closes_with_an_empty_result, req_009_18_follows_the_item_back_into_the_search, req_009_18_says_when_a_kept_item_is_gone | pass |
| REQ-009-19 | `crates/app/src/viewer/mod.rs` › req_009_19_centers_the_photo_with_the_bar_above_it, req_009_19_the_bar_keeps_clear_of_the_window_controls | pass |
| REQ-009-20 | `crates/app/tests/viewer.rs` › req_009_20_a_zoomed_photo_pans_to_the_windows_edges, req_009_20_stops_at_the_info_panel (snapshot `viewer_zoomed`); req_009_05_zooms_and_pans, req_009_16_click_toggles_between_fitted_and_actual_pixels, req_009_03_a_click_beside_a_zoomed_photo_closes | pass |
| REQ-009-21 | `crates/domain/src/orientation.rs` › req_009_21_only_formats_with_an_orientation_turn; `crates/app/tests/context_menu.rs` › req_009_21_the_menu_renames_rates_and_rotates_the_selection, req_009_21_rotate_is_off_for_videos_and_gifs, req_009_21_the_viewers_menu_rates_and_rotates (snapshot `context_menu`) | pass |
| REQ-009-22 | `crates/domain/src/orientation.rs` › req_009_22_turns_a_quarter_and_keeps_the_mirror; `crates/media/src/rotate.rs` › req_009_22_changes_only_the_orientation_tag, req_009_22_a_mirrored_photo_stays_mirrored, req_009_22_a_read_only_file_stays; `crates/app/tests/context_menu.rs` › req_009_21_the_menu_renames_rates_and_rotates_the_selection | pass |
| REQ-009-23 | `crates/domain/src/deletion.rs` › req_009_23_network_paths_have_no_recycle_bin, req_009_23_the_question_says_where_the_files_go; `crates/media/src/delete.rs` › req_009_23_the_file_goes_with_its_sidecars, req_009_23_a_shared_sidecar_stays, req_009_23_a_file_that_cant_go_stays_with_its_sidecars, req_009_23_a_gone_file_counts_and_paths_stay_inside, req_009_23_permanently_removes_the_files, req_009_23_a_local_library_has_a_recycle_bin; `crates/app/tests/context_menu.rs` › req_009_23_req_009_24_the_menu_deletes_the_selection_after_asking, req_009_23_the_delete_key_and_an_item_outside_the_selection (snapshot `context_menu`) | partial: tests use a stand-in for the Recycle Bin; moving to the real one (`trash` crate) and finding mapped network drives on Windows have no test |
| REQ-009-24 | `crates/db/src/media.rs` › req_009_24_removes_a_deleted_item; `crates/app/tests/context_menu.rs` › req_009_23_req_009_24_the_menu_deletes_the_selection_after_asking, req_009_24_the_viewer_moves_on_from_a_deleted_item | pass |
| REQ-009-17 | `crates/domain/src/faces.rs` › req_009_17_face_regions_are_hidden_until_shown; `crates/app/tests/viewer.rs` › req_009_17_shows_face_regions_and_remembers_the_choice (snapshot `viewer_faces`) | pass |
| REQ-009-25 | `crates/app/tests/context_menu.rs` › req_009_25_copies_a_photo_from_its_tile_and_the_viewer | pass (pasting into another app by hand) |
