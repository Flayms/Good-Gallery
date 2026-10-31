# 021 · Photo editing

**Status:** Implemented

## Goal

Users who find a photo that is too dark, has a color cast or lacks punch fix it in the viewer, without exporting it to another program. They adjust light, curves, color and a few effects with live feedback, then either keep the original and save an edited copy, or replace the original. Either way the edit can be taken back: the unedited original is never lost, and reopening an edited photo shows the sliders where they were left. Colors, bit depth and metadata survive the round trip.

## Requirements

### REQ-021-01 · Edit a photo

- **Given** a JPEG, PNG or WebP photo open in the viewer, in a library that is online
- **When** the user chooses _Edit_ (control bar button, `E`, or the context menu)
- **Then** the viewer switches to editing: the photo stays in view and an adjustments panel replaces the info panel
- **And** videos, GIFs, AVIF photos and media of offline libraries offer no _Edit_

### REQ-021-02 · Live preview

- **Given** editing
- **When** the user changes an adjustment
- **Then** the photo shows the result from the next frame on, at any zoom level, without the file being touched

### REQ-021-03 · Light

- **Given** editing
- **When** the user changes _Exposure_ (−5 to +5 EV), _Contrast_, _Highlights_, _Shadows_, _Whites_ or _Blacks_ (each −100 to +100)
- **Then** exposure scales the light by 2^EV; contrast steepens or flattens the tones around the midtone; highlights and shadows lift or lower only the bright or dark tones; whites and blacks move the white and black points
- **And** each at 0 leaves the photo unchanged

### REQ-021-17 · Brightness

- **Given** editing
- **When** the user changes _Brightness_ (−100 to +100)
- **Then** the midtones get lighter or darker while black stays black and white stays white
- **And** at 0 it leaves the photo unchanged

### REQ-021-04 · Curves

- **Given** editing
- **When** the user adds, drags or removes points on the tone curve, for all channels together or for red, green or blue alone
- **Then** the tones are mapped through a smooth, monotonic curve through those points
- **And** a straight curve from black to white leaves the photo unchanged

### REQ-021-18 · Histogram behind the curve

- **Given** editing with _Curves_ open
- **When** the user changes an adjustment that comes before the curves (light, white balance) or picks a channel
- **Then** the curve editor shows, behind the curve, the histogram of the tones the curve receives: the red, green and blue values together for _RGB_, one channel for _Red_, _Green_ or _Blue_
- **And** changing the curve itself doesn't move the histogram

### REQ-021-05 · Color

- **Given** editing
- **When** the user changes _Temperature_, _Tint_, _Vibrance_ or _Saturation_ (each −100 to +100), or picks a neutral point with the white balance picker
- **Then** temperature shifts the colors towards blue or yellow, tint towards green or magenta; saturation scales all colors alike, vibrance mostly the muted ones and spares skin tones; the picker sets temperature and tint so the picked spot turns neutral grey
- **And** each at 0 leaves the photo unchanged

### REQ-021-06 · Effects

- **Given** editing
- **When** the user changes _Grain_ (amount, size) or _Vignette_ (amount −100 to +100, midpoint)
- **Then** grain adds film-like noise, a negative vignette darkens and a positive one lightens the edges
- **And** grain looks the same in the preview and in the saved file, and the same each time the photo is saved

### REQ-021-07 · Compare, reset and undo

- **Given** editing
- **When** the user holds `\` (or the _Compare_ button), double-clicks a slider, chooses _Reset all_, or presses `Ctrl+Z` / `Ctrl+Shift+Z`
- **Then** the original shows while held; that adjustment returns to 0; all adjustments return to 0; the last change is undone or redone

### REQ-021-08 · Leave without saving

- **Given** editing with changes that were not saved
- **When** the user presses `Esc`, steps to another item or closes the viewer
- **Then** a dialog offers _Save_, _Discard_ and _Cancel_; without changes, editing just ends

### REQ-021-09 · Save as copy

- **Given** editing with changes
- **When** the user chooses _Save as copy_
- **Then** a new file `<name> (edited).<ext>` (numbered if taken) is written next to the original, and the original and its sidecar stay unchanged
- **And** the copy appears in the gallery next to the original, and the viewer shows it

### REQ-021-10 · Save over the original

- **Given** editing with changes
- **When** the user chooses _Save_, and confirms the first time a photo is replaced
- **Then** the unedited original is first kept as a backup, then the file is replaced with the edited photo
- **And** the gallery's thumbnail and the viewer show the edited photo

### REQ-021-11 · Edits stay editable

- **Given** a photo that was saved with an edit (over the original or as a copy) whose unedited original still exists
- **When** the user edits it again
- **Then** the sliders and curves start where they were saved, and the result is rendered from the unedited original, so repeated edits never stack or lose quality

### REQ-021-12 · Revert to original

- **Given** a photo that was saved over its original
- **When** the user chooses _Revert to original_ in the info panel or the context menu
- **Then** the file is the unedited original again, byte for byte, and the backup is removed
- **And** a copy saved with _Save as copy_ has no _Revert_; the original is still there, and the copy can be deleted with other tools

### REQ-021-13 · Faithful colors and depth

- **Given** a photo with an embedded color profile, or a PNG with 16 bits per channel
- **When** it is edited and saved
- **Then** the saved file keeps the color profile and the bit depth, and adjustments at 0 give the same colors as the original (within one level per channel)
- **And** smooth gradients don't band (no 8-bit intermediate steps)

### REQ-021-14 · Metadata kept

- **Given** a photo with metadata (dates, rating, tags, face regions, GPS)
- **When** it is saved as a copy or over the original
- **Then** the saved file carries the same metadata as the original, and its orientation is upright in the pixels (orientation tag 1)
- **And** a copy gets a copy of the original's sidecar under its own name, so it has the same rating and tags

### REQ-021-15 · Date modified on save

- **Given** the setting _When saving edits_ with _Keep date modified_ (default) or _Update date modified_
- **When** an edit is saved (as a copy or over the original), or an original is reverted
- **Then** the file's date modified is the original's, or the time of saving
- **And** the date taken never changes

### REQ-021-16 · Safe writes

- **Given** saving or reverting
- **When** the library goes offline, the folder is read-only, the disk is full or the app is ended part-way
- **Then** either the old or the new file is in place, never a partial one, the backup is never lost, and a failure shows an error toast while editing stays open

## Out of scope

- Crop and rotate (#34), which will be more adjustments of the same edit.
- Editing videos, GIFs and AVIF photos, raw files, and in the browser version.
- Local adjustments (brushes, masks, gradients; the subject and background of spec 022 aside), presets, copying an edit to other photos, batch editing.
- Reading or writing other tools' edit settings (Lightroom's `crs:` tags, darktable's history); see the plan.
- Managing backups of originals whose edited file was moved or deleted with other tools.

## Assumptions

- The libraries the user edits in are writable by them; the app needs no elevated rights.
- exiftool is available (as for indexing) to copy metadata into saved files.
- Adjustments are global (the same for every pixel apart from vignette and grain), so the preview of a zoomed-out photo matches the saved file.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-021-01 | `crates/app/tests/edit.rs` › req_021_01_edit_is_offered_for_jpeg_png_and_webp_photos_in_online_libraries; `crates/domain/src/edit.rs` › req_021_01_jpeg_png_and_webp_are_editable | pass |
| REQ-021-02 | `crates/app/src/edit.rs` › req_021_02_req_021_06_the_gpu_preview_matches_the_cpu_render; `crates/app/tests/edit.rs` › req_021_02_req_021_07_previews_compares_undoes_and_resets | pass |
| REQ-021-03 | `crates/domain/src/edit.rs` › req_021_03_* (exposure, contrast, shadows and highlights, whites and blacks, neutral values) | pass |
| REQ-021-17 | `crates/domain/src/edit.rs` › req_021_17_brightness_lifts_or_lowers_the_midtones_only; `crates/app/src/edit.rs` › req_021_02_req_021_06_the_gpu_preview_matches_the_cpu_render | pass |
| REQ-021-04 | `crates/domain/src/edit.rs` › req_021_04_a_curve_maps_tones_through_its_points_without_overshooting, req_021_04_curve_points_are_added_moved_and_removed_within_bounds | pass |
| REQ-021-18 | `crates/domain/src/edit.rs` › req_021_18_the_histogram_shows_the_tones_the_curve_receives; `crates/app/tests/edit.rs` › req_021_18_the_curve_editor_shows_the_histogram | pass |
| REQ-021-05 | `crates/domain/src/edit.rs` › req_021_05_* (temperature and tint, picker, saturation and vibrance); `crates/app/src/edit.rs` › req_021_05_the_picker_neutralizes_the_picked_spot | pass |
| REQ-021-06 | `crates/domain/src/edit.rs` › req_021_06_vignette_darkens_or_lightens_the_edges_only, req_021_06_grain_is_the_same_for_the_same_position_and_follows_the_photo_not_the_screen; `crates/app/src/edit.rs` › req_021_02_req_021_06_the_gpu_preview_matches_the_cpu_render | pass |
| REQ-021-07 | `crates/domain/src/edit.rs` › req_021_07_history_undoes_and_redoes_changes; `crates/app/src/edit.rs` › req_021_07_req_021_08_tracks_changes_undo_and_asks_before_leaving; `crates/app/tests/edit.rs` › req_021_02_req_021_07_previews_compares_undoes_and_resets | pass |
| REQ-021-08 | `crates/app/tests/edit.rs` › req_021_08_asks_before_leaving_with_unsaved_changes; `crates/app/src/edit.rs` › req_021_07_req_021_08_tracks_changes_undo_and_asks_before_leaving, req_021_08_discarding_after_the_preview_was_queued_still_draws_it | pass |
| REQ-021-09 | `crates/media/src/edit.rs` › req_021_09_req_021_14_saves_a_copy_beside_the_original_with_its_metadata_and_sidecar (exiftool); `crates/app/tests/edit.rs` › req_021_09_saves_a_copy (exiftool); `crates/domain/src/edit.rs` › req_021_09_copies_are_named_after_the_original_and_numbered | pass |
| REQ-021-10 | `crates/media/src/edit.rs` › req_021_10_req_021_12_replacing_keeps_a_backup_and_reverting_restores_the_original_byte_for_byte (exiftool); `crates/app/tests/edit.rs` › req_021_10_req_021_12_saves_over_the_original_and_reverts (exiftool); `crates/db/src/media.rs` › req_021_10_a_rewritten_file_is_read_again_and_gets_a_new_thumbnail | pass |
| REQ-021-11 | `crates/app/tests/edit.rs` › req_021_11_edits_again_from_the_original_with_the_saved_recipe; `crates/domain/src/edit.rs` › req_021_11_*; `crates/db/src/writer.rs` › req_021_11_stores_the_saved_edit_and_drops_it_when_the_file_has_none; `crates/meta/src/rules.rs` › req_021_11_reads_the_saved_edit_from_the_file_but_not_from_its_sidecar | pass |
| REQ-021-12 | `crates/media/src/edit.rs` › req_021_10_req_021_12_replacing_keeps_a_backup_and_reverting_restores_the_original_byte_for_byte (exiftool); `crates/app/tests/edit.rs` › req_021_10_req_021_12_saves_over_the_original_and_reverts (exiftool) | pass |
| REQ-021-13 | `crates/media/src/edit.rs` › req_021_13_* (16 bits and profile, banding, lossless WebP) (exiftool); `crates/domain/src/edit.rs` › req_021_13_dither_stays_within_one_step_and_repeats | pass |
| REQ-021-14 | `crates/media/src/edit.rs` › req_021_09_req_021_14_saves_a_copy_beside_the_original_with_its_metadata_and_sidecar, req_021_14_bakes_the_orientation_into_the_pixels (exiftool) | pass |
| REQ-021-15 | `crates/media/src/edit.rs` › req_021_15_keeps_or_updates_the_date_modified (exiftool); `crates/db/src/settings.rs` › req_021_15_stores_the_date_modified_choice_and_falls_back_to_keep; `crates/app/tests/edit.rs` › req_021_15_the_date_modified_setting_is_on_the_settings_page | pass |
| REQ-021-16 | `crates/media/src/edit.rs` › req_021_16_a_failed_save_leaves_the_original_and_no_temporary_file, req_021_16_a_read_only_folder_fails_and_changes_nothing (exiftool); `crates/app/tests/edit.rs` › req_021_16_a_failed_save_keeps_editing_open | pass |
