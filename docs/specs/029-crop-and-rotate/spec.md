# 029 · Crop and rotate

**Status:** Implemented

## Goal

Users who find a sideways photo or one with something distracting at its edge fix it in the viewer, without exporting it to another program. Crop and rotation are more adjustments of the same edit (spec 021): live, undoable, saved like every other edit, and always reversible.

## Requirements

### REQ-029-01 · Rotate by quarter turns

- **Given** a photo in editing mode
- **When** the user chooses _Rotate left_ or _Rotate right_
- **Then** the photo shows turned by 90° that way from the next frame on, fitted into the media area, without the file being touched

### REQ-029-02 · Crop

- **Given** a photo in editing mode
- **When** the user chooses _Crop_
- **Then** the whole photo shows, rotated as chosen, with the crop as a frame over it: the area outside is darkened and a grid of thirds divides the area inside
- **And** dragging an edge or corner resizes the frame, dragging inside moves it, and it stays on the photo
- **And** the frame can't get smaller than 16 pixels of the photo on either side
- **And** _Esc_ or _Done_ leaves _Crop_, staying in the editor
- **When** the user leaves _Crop_
- **Then** only the cropped area shows, fitted into the media area

### REQ-029-03 · Aspect ratio

- **Given** _Crop_
- **When** the user picks _Free_, _Original_, _1:1_, _4:3_ or _16:9_
- **Then** the frame takes that ratio, centered and as large as fits inside the current frame, and keeps it while it is resized
- **And** _4:3_ and _16:9_ are landscape or portrait by the current frame's shape, and _Swap_ turns them to the other one

### REQ-029-04 · Other adjustments stay on the content

- **Given** a photo with a crop or rotation
- **When** other adjustments are used or were set before
- **Then** they look the same on the part that remains: the subject (spec 022), the blur's focus (spec 023), the white balance picker and the grain stay on the same spot of the photo
- **And** the vignette follows the crop: its center and edges are those of the cropped, straightened photo

### REQ-029-05 · Like any adjustment

- **Given** a crop or rotation
- **When** the user undoes, redoes, resets or leaves without saving
- **Then** it behaves as in REQ-021-07 and REQ-021-08
- **And** _Compare_ shows the photo as editing started (REQ-021-07): the whole photo without an earlier edit, else with its saved crop and turns

### REQ-029-06 · Saved file

- **Given** a crop or rotation
- **When** it is saved, as a copy or over the original
- **Then** the file holds only the cropped, turned pixels, with their new dimensions and orientation tag 1, at the same bit depth and with the same color profile (REQ-021-13)
- **And** the gallery's tile, thumbnail and the viewer show the new shape

### REQ-029-07 · Reversible

- **Given** a photo saved with a crop or rotation
- **When** the user edits it again
- **Then** the whole photo is available again with the crop and rotation where they were left, rendered from the unedited original (REQ-021-11)
- **And** _Revert to original_ brings back the whole photo (REQ-021-12)

### REQ-029-08 · Face regions

- **Given** a photo with face regions (REQ-004-13), in the file or its sidecar
- **When** a crop or rotation is saved and the regions are shown (REQ-009-17)
- **Then** they mark the same faces as before; on a straightened photo a region is the box around the turned one
- **And** a region entirely outside the crop isn't shown, and a region partly outside is cut at the crop's edge
- **And** People tags stay as they are, also for a person cropped away

### REQ-029-09 · Smaller size

- **Given** a photo in editing mode
- **When** the user sets its long edge in pixels, or picks 75 %, 50 % or 25 %, in the _Size_ section
- **Then** the saved photo has that long edge, after the crop and turns, with its aspect ratio kept, and the section shows the size it will have ("1920 × 1440")
- **And** it can't be larger than the photo after its crop: there is no enlarging, and a crop made smaller afterwards saves at the crop's own size
- **And** like any adjustment it can be undone, reset and changed later (REQ-029-05, REQ-029-07)

### REQ-029-10 · Straighten

- **Given** a photo in editing mode
- **When** the user drags _Straighten_ in _Crop and rotate_, from −100° to +100° in steps of 0.1°
- **Then** the photo turns by that angle clockwise (counterclockwise below 0°) from the next frame on, as well as by its quarter turns, and a fine grid lies over it while the slider is held
- **And** the crop becomes the largest of its shape, centered, that holds only the photo, so no empty corner shows or is saved
- **And** in _Crop_ the whole turned photo shows with its empty corners, and the frame can't be dragged or shaped beyond the photo: it stops at its edge
- **And** a double-click on the slider sets it back to 0°, and like any adjustment it can be undone, reset and changed later (REQ-029-05, REQ-029-07)

### REQ-029-11 · Flip

- **Given** a photo in editing mode
- **When** the user chooses _Flip horizontal_ or _Flip vertical_
- **Then** the photo shows mirrored left to right or top to bottom, as it shows on screen, with its crop and straightening mirrored too
- **And** choosing it again, undo or reset takes it back, and it is saved like a crop (REQ-029-06)

## Out of scope

- Straightening by drawing a line along the horizon, and finding the angle automatically.
- Enlarging, and scaling videos (#205).
- Rotating without opening the editor, or several selected photos at once.
- A lossless save for a rotate-only edit (writing the orientation tag instead of new pixels). A JPEG is re-encoded once at the edit's quality (REQ-021-21), like any edit.
- Videos and the formats spec 021 doesn't edit, and the browser version.
- Rewriting another tool's sidecar.

## Assumptions

- Crop and rotation apply to the upright photo, after its EXIF orientation.
- Most photos have face regions in the file or none; a sidecar's regions are correct in this app, but other tools that read that sidecar see the positions of the unedited photo. A sidecar's regions on a photo whose orientation only the original's EXIF tells are placed as if it were upright, since saving replaces that tag.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-029-01 | `crates/domain/src/edit.rs` › req_029_01_quarter_turns_swap_the_sides_and_move_points_clockwise; `crates/app/tests/edit.rs` › req_029_01_req_029_05_turns_the_photo_and_takes_it_back | pass |
| REQ-029-02 | `crates/domain/src/crop.rs` › req_029_02_edges_and_corners_resize_and_inside_moves, req_029_02_the_frame_stays_on_the_photo_and_above_the_smallest_size, req_029_02_finds_what_a_press_takes_hold_of; `crates/domain/src/edit.rs` › req_029_02_the_frame_on_screen_and_the_crop_agree; `crates/app/tests/edit.rs` › req_029_02_req_029_05_crops_by_dragging_the_frame | pass |
| REQ-029-03 | `crates/domain/src/crop.rs` › req_029_03_a_ratio_gives_the_largest_centered_frame, req_029_03_a_ratio_is_kept_while_resizing, req_029_03_swap_turns_the_shape_around_and_back; `crates/app/tests/edit.rs` › req_029_03_picks_the_frames_shape | pass |
| REQ-029-04 | `crates/domain/src/edit.rs` › req_029_04_the_mapping_to_the_output_and_back_agree, req_029_04_the_vignette_follows_the_crop; `crates/app/src/edit/tests.rs` › req_029_04_the_gpu_preview_of_crop_and_turns_matches_the_cpu_render; `crates/app/tests/edit.rs` › req_029_04_a_pick_lands_on_the_same_spot_of_the_photo | pass |
| REQ-029-05 | `crates/app/tests/edit.rs` › req_029_01_req_029_05_turns_the_photo_and_takes_it_back, req_029_02_req_029_05_crops_by_dragging_the_frame; `crates/app/src/edit/tests.rs` › req_021_07_req_029_05_compare_shows_the_photo_as_editing_started | pass |
| REQ-029-06 | `crates/domain/src/edit.rs` › req_029_06_the_crop_in_pixels_stays_inside_the_photo; `crates/media/src/edit.rs` › req_029_06_saves_the_cropped_turned_pixels, req_029_06_keeps_16_bits (exiftool); `crates/app/tests/edit.rs` › req_029_06_saves_what_the_frame_holds; the tile's new shape checked in the running app | pass |
| REQ-029-07 | `crates/domain/src/edit.rs` › req_029_07_crop_and_turns_round_trip_and_hostile_ones_are_tamed, req_029_07_the_originals_size_follows_from_the_outputs; `crates/media/src/edit.rs` › req_029_07_editing_again_and_reverting_bring_back_the_whole_photo (exiftool) | pass |
| REQ-029-08 | `crates/domain/src/edit.rs` › req_029_08_places_rectangles_on_the_output; `crates/meta/src/rules.rs` › req_029_08_moves_a_sidecars_regions_by_the_saved_edit, req_029_08_places_regions_on_the_output_and_names_who_is_gone; `crates/media/src/edit.rs` › req_029_08_saved_regions_mark_the_same_faces (exiftool) | pass |
| REQ-029-10 | `crates/domain/src/edit.rs` › req_029_10_straightening_maps_both_ways_and_the_crop_shows_only_the_photo, req_029_10_straightening_fits_the_crop_and_round_trips, req_029_08_regions_follow_flips_and_straightening; `crates/app/src/edit/tests.rs` › req_029_10_req_029_11_the_gpu_preview_of_flips_and_straightening_matches_the_cpu_render; `crates/media/src/edit.rs` › req_029_10_req_029_11_saves_straightened_and_flipped_pixels (exiftool); `crates/app/tests/edit.rs` › req_029_10_straightens_with_the_crop_inside_the_photo (snapshot `edit_straighten`) | pass |
| REQ-029-11 | `crates/domain/src/edit.rs` › req_029_11_flips_mirror_the_output, req_029_08_regions_follow_flips_and_straightening; `crates/app/src/edit/tests.rs` › req_029_10_req_029_11_the_gpu_preview_of_flips_and_straightening_matches_the_cpu_render; `crates/media/src/edit.rs` › req_029_10_req_029_11_saves_straightened_and_flipped_pixels (exiftool); `crates/app/tests/edit.rs` › req_029_11_flips_the_photo | pass |
| REQ-029-09 | `crates/domain/src/edit.rs` › req_029_09_scales_the_long_edge_down_and_never_up, req_029_09_req_021_21_size_and_quality_are_changes_and_round_trip; `crates/media/src/edit.rs` › req_029_09_saves_at_the_smaller_size; `crates/app/tests/edit.rs` › req_029_09_req_021_21_the_size_section_sets_the_long_edge_and_the_quality (snapshot `edit_size`) | pass |
