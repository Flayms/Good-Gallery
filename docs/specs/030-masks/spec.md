# 030 · Masks

**Status:** Implemented

## Goal

Users who edit a photo (spec 021) sometimes need to change a part that isn't its subject or background (spec 022): darken a bright sky with a gradient, lift only the shadows, or calm one color. Masks select such a part by its position, its brightness or its color, with adjustable parameters, and give it its own light and color. Where no rule finds the part, users paint it with a brush or draw around it. Choosing what to edit comes first in the panel, because it decides what everything below it changes.

## Requirements

### REQ-030-01 · Add and remove masks

- **Given** editing a photo
- **When** the user picks _Add gradient_, _Add luminance_, _Add color_, _Add brush_ or _Add freeform_
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
- **When** it is chosen
- **Then** what it selects is tinted, as for the subject (REQ-022-04), and the tint isn't saved
- **When** the user turns off _Show selection_
- **Then** no selection is tinted until it is turned on again, while editing this photo

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

### ~~REQ-030-10 · A mask of several shapes~~

Deprecated 2026-11-02: adding, subtracting and intersecting shapes was too confusing to use (#152). A mask has one shape. It never reached a release.

### ~~REQ-030-11 · The subject and background in a mask~~

Deprecated 2026-11-02: only as shapes to combine had they a use, and they are always there as parts (REQ-030-15).

### REQ-030-12 · Brush

- **Given** a chosen brush shape, with the whole photo shown
- **When** the user drags over the photo, with a _Size_, a _Softness_ and _Paint_ or _Erase_ chosen
- **Then** the stroke selects what it covers, softly at its edge, or takes that out again; a click paints a dot
- **And** each stroke is one undo step, and _Clear_ takes all strokes away

### REQ-030-13 · Freeform

- **Given** a chosen freeform shape, with the whole photo shown
- **When** the user draws an outline over the photo and lets go
- **Then** it closes and selects what lies inside, softly at its edge by _Softness_; further outlines add to it, and with _Erase_ they take out what lies inside

### REQ-030-14 · Painted masks are saved

- **Given** a photo with painted shapes
- **When** it is saved and edited again
- **Then** the saved file shows what the preview showed (REQ-030-08), and the strokes and outlines come back, stored as points in the saved edit
- **And** at most 4 painted masks and 1,500 points fit a photo, so that the saved edit fits the file's metadata: strokes are thinned as the user lets go, and at the limit the panel says so and no more can be painted; values from a file are brought into these limits (REQ-011-10)

### REQ-030-15 · What to edit comes first

- **Given** editing a photo
- **When** the panel shows
- **Then** _Photo_, _Subject_, _Background_ and the masks come first, in one list, followed by the buttons that add a mask, and everything below them changes what is chosen
- **And** _Photo_, _Subject_ and _Background_ are always there and can't be removed

### REQ-030-16 · The tint steps aside while adjusting

- **Given** a selection tinted (REQ-030-07, REQ-022-04)
- **When** the user holds an adjustment's slider in _Light_ or _Color_
- **Then** the tint hides until the slider is let go, so that the colors can be judged
- **And** the sliders that change the selection itself (_From_, _Hue_ and the like) keep it shown

## Out of scope

- Combining shapes in a mask, or leaving the subject out of one.
- Radial gradients, depth or object masks, an eyedropper for the color mask, moving masks within the list, renaming them.
- Moving a gradient's handles or painting by keyboard (the crop's frame has none either), or with the photo zoomed in: a chosen gradient or painted mask shows the whole photo.
- Pen pressure, and editing an outline's points after drawing it.
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
| REQ-030-07 | `crates/app/tests/edit.rs` › req_030_01_req_030_06_req_030_07_adds_changes_and_removes_masks, req_030_07_tints_by_default_until_turned_off; `crates/app/src/edit.rs` › req_030_07_req_030_08_the_gpu_preview_of_masks_matches_the_cpu_render | pass |
| REQ-030-08 | `crates/domain/src/edit.rs` › req_030_08_masks_are_saved_and_values_stay_in_range, req_030_08_shapes_from_a_development_build_are_left_out; `crates/media/src/edit.rs` › req_030_08_saving_applies_the_masks; `crates/app/src/edit.rs` › req_030_07_req_030_08_the_gpu_preview_of_masks_matches_the_cpu_render | pass |
| ~~REQ-030-10~~ | — | no test (deprecated) |
| ~~REQ-030-11~~ | — | no test (deprecated) |
| REQ-030-12 | `crates/domain/src/paint.rs` › req_030_12_a_stroke_selects_what_it_covers_softly, req_030_12_erasing_takes_it_out_again, req_030_12_drawing_on_top_equals_drawing_at_once; `crates/domain/src/edit.rs` › req_030_12_a_painted_shape_reads_its_painting; `crates/app/src/edit.rs` › req_030_12_req_030_13_the_gpu_preview_of_paintings_matches_the_cpu_render, req_030_12_the_preview_adds_only_whats_new, req_030_12_the_texture_kept_up_to_date_equals_one_drawn_anew; `crates/app/tests/edit.rs` › req_030_12_paints_and_erases_on_the_photo | pass |
| REQ-030-13 | `crates/domain/src/paint.rs` › req_030_13_an_outline_selects_what_lies_inside; `crates/app/src/edit.rs` › req_030_12_req_030_13_the_gpu_preview_of_paintings_matches_the_cpu_render; `crates/app/tests/edit.rs` › req_030_13_draws_an_outline_on_the_photo | pass |
| REQ-030-14 | `crates/domain/src/paint.rs` › req_030_14_thinning_keeps_the_stroke, req_030_14_values_from_a_file_stay_in_the_limits; `crates/domain/src/edit.rs` › req_030_14_paintings_are_saved_and_tidied; `crates/media/src/edit.rs` › req_030_14_a_full_painting_fits_the_saved_file | pass |
| REQ-030-15 | `crates/app/tests/edit.rs` › req_030_15_what_to_edit_comes_first | pass |
| REQ-030-16 | `crates/app/tests/edit.rs` › req_030_16_the_tint_steps_aside_while_adjusting | pass |
| REQ-030-09 | by inspection: masks are part of editing, which is photos on the desktop only (REQ-021-01, REQ-019-13) | no test |
