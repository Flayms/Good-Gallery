# 030 · Masks

**Status:** Implemented

## Goal

Users who edit a photo (spec 021) sometimes need to change a part that isn't its subject or background (spec 022): darken a bright sky with a gradient, lift only the shadows, or calm one color. Masks select such a part by its position, its brightness or its color, with adjustable parameters, and give it its own light and color. This is the first phase of #139; combining masks and painted masks come later.

## Requirements

### REQ-030-01 · Add and remove masks

- **Given** editing a photo
- **When** the user picks _Add gradient_, _Add luminance_ or _Add color_
- **Then** a mask of that kind is added to the photo's list, up to 8, and is chosen, so the panel shows its controls with _Light_ and _Color_
- **When** the user picks _Remove mask_
- **Then** it is gone and the panel shows the photo again
- **And** each of these is one undo step (REQ-021-07)

### REQ-030-02 · Adjustments of a mask

- **Given** a mask with its own light and color
- **When** the photo is shown
- **Then** they apply fully where the mask selects, not at all where it doesn't, and softly in between, on top of the photo's and its parts' edits
- **And** several masks apply one after another, in the list's order

### REQ-030-03 · Linear gradient

- **Given** a chosen gradient mask, with the whole photo shown
- **When** the user drags one of its two handles on the photo
- **Then** the mask is full at the first handle and fades to nothing at the second, and the drag is one undo step

### REQ-030-04 · Luminance range

- **Given** a chosen luminance mask
- **When** the user changes _From_, _To_ or _Softness_
- **Then** the mask selects what is that bright in the unedited photo, so it doesn't change when the sliders of the photo or of other masks do

### REQ-030-05 · Color range

- **Given** a chosen color mask
- **When** the user changes _Hue_, _Range_ or _Softness_
- **Then** the mask selects those colors in the unedited photo, and greys belong to no color

### REQ-030-06 · Invert

- **Given** a chosen mask
- **When** the user turns on _Invert_
- **Then** it selects what it left out, and the other way round

### REQ-030-07 · Show the mask

- **Given** a chosen mask
- **When** the user turns on _Show selection_
- **Then** what it selects is tinted, as for the subject (REQ-022-04); the tint isn't saved

### REQ-030-08 · Saved and editable again

- **Given** a photo with masks
- **When** it is saved
- **Then** the saved file shows what the preview showed (within two levels, as REQ-021-13), with the original's format and metadata as for any edit (REQ-021-09, REQ-021-10)
- **And** when it is edited again, all masks and their parameters come back; a gradient stays on the same part of the photo when it is cropped or turned (spec 029), because it is stored in the original's coordinates
- **And** values from a file are brought into their ranges and a mask kind only a newer version knows is left out (REQ-011-10)

### REQ-030-09 · Only for photos on the desktop

- **Given** a video or the browser version (spec 019)
- **When** it is shown
- **Then** there are no masks, as there is no editing

## Out of scope

- Combining masks (add, subtract, intersect), and using the subject or background as part of a mask: the next phase of #139.
- Brush and freeform masks: the phase after that.
- Radial gradients, depth or object masks, an eyedropper for the color mask, moving masks within the list, renaming them.
- Moving a gradient's handles by keyboard (the crop's frame has none either), or with the photo zoomed in: a chosen gradient shows the whole photo.
- Curves, looks and effects per mask, as for the subject and background.

## Assumptions

- Eight masks are enough, and they keep the preview's uniform fixed. Each costs a few operations per pixel, and only while it exists.
- Masks don't blur or crop the mask's own input: luminance and color are read after a blur (spec 023), from what is developed, in the preview and when saving alike.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-030-01 | `crates/app/tests/edit.rs` › req_030_01_req_030_06_req_030_07_adds_changes_and_removes_masks | pass |
| REQ-030-02 | `crates/domain/src/edit.rs` › req_030_02_a_mask_changes_only_what_it_selects, req_030_02_masks_apply_in_order | pass |
| REQ-030-03 | `crates/domain/src/edit.rs` › req_030_03_a_gradient_fades_between_its_handles; `crates/app/tests/edit.rs` › req_030_03_drags_a_gradients_handle | pass |
| REQ-030-04 | `crates/domain/src/edit.rs` › req_030_04_a_luminance_mask_reads_the_unedited_photo | pass |
| REQ-030-05 | `crates/domain/src/edit.rs` › req_030_05_a_color_mask_selects_hues_and_no_greys | pass |
| REQ-030-06 | `crates/domain/src/edit.rs` › req_030_06_invert_swaps_the_selection; `crates/app/tests/edit.rs` › req_030_01_req_030_06_req_030_07_adds_changes_and_removes_masks | pass |
| REQ-030-07 | `crates/app/tests/edit.rs` › req_030_01_req_030_06_req_030_07_adds_changes_and_removes_masks; `crates/app/src/edit.rs` › req_030_07_req_030_08_the_gpu_preview_of_masks_matches_the_cpu_render | pass |
| REQ-030-08 | `crates/domain/src/edit.rs` › req_030_08_masks_are_saved_and_values_stay_in_range; `crates/media/src/edit.rs` › req_030_08_saving_applies_the_masks; `crates/app/src/edit.rs` › req_030_07_req_030_08_the_gpu_preview_of_masks_matches_the_cpu_render | pass |
| REQ-030-09 | by inspection: masks are part of editing, which is photos on the desktop only (REQ-021-01, REQ-019-13) | no test |
