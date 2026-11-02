# 029 · Crop and rotate

**Status:** Draft

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
- **And** the vignette follows the crop: its center and edges are those of the cropped photo

### REQ-029-05 · Like any adjustment

- **Given** a crop or rotation
- **When** the user undoes, redoes, resets or leaves without saving
- **Then** it behaves as in REQ-021-07 and REQ-021-08
- **And** _Compare_ shows the whole unedited photo

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
- **Then** they mark the same faces as before
- **And** a region entirely outside the crop isn't shown, and a region partly outside is cut at the crop's edge
- **And** People tags stay as they are, also for a person cropped away

## Out of scope

- Straightening by a free angle and flipping.
- Rotating without opening the editor, or several selected photos at once.
- A lossless save for a rotate-only edit (writing the orientation tag instead of new pixels). A JPEG is re-encoded once at quality 95, like any edit.
- Videos and the formats spec 021 doesn't edit, and the browser version.
- Rewriting another tool's sidecar.

## Assumptions

- Crop and rotation apply to the upright photo, after its EXIF orientation.
- Most photos have face regions in the file or none; a sidecar's regions are correct in this app, but other tools that read that sidecar see the positions of the unedited photo. A sidecar's regions on a photo whose orientation only the original's EXIF tells are placed as if it were upright, since saving replaces that tag.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-029-01 | `crates/domain/src/edit.rs` › req_029_01_quarter_turns_swap_the_sides_and_move_points_clockwise | partial: no UI yet |
| REQ-029-02 | | no test |
| REQ-029-03 | | no test |
| REQ-029-04 | `crates/domain/src/edit.rs` › req_029_04_the_mapping_to_the_output_and_back_agree, req_029_04_the_vignette_follows_the_crop | partial: no preview yet |
| REQ-029-05 | | no test |
| REQ-029-06 | `crates/domain/src/edit.rs` › req_029_06_the_crop_in_pixels_stays_inside_the_photo; `crates/media/src/edit.rs` › req_029_06_saves_the_cropped_turned_pixels, req_029_06_keeps_16_bits (exiftool) | partial: the tile's new shape not yet |
| REQ-029-07 | `crates/domain/src/edit.rs` › req_029_07_crop_and_turns_round_trip_and_hostile_ones_are_tamed; `crates/media/src/edit.rs` › req_029_07_editing_again_and_reverting_bring_back_the_whole_photo (exiftool) | pass |
| REQ-029-08 | `crates/domain/src/edit.rs` › req_029_08_places_rectangles_on_the_output; `crates/meta/src/rules.rs` › req_029_08_moves_a_sidecars_regions_by_the_saved_edit, req_029_08_places_regions_on_the_output_and_names_who_is_gone; `crates/media/src/edit.rs` › req_029_08_saved_regions_mark_the_same_faces (exiftool) | pass |
