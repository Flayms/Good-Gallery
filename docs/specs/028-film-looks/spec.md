# 028 · Film looks

**Status:** Implemented

## Goal

Users who edit a photo (spec 021) give it the color response and grain of a well-known analog film or instant camera in one click, then tune how strong it is, instead of building it from a dozen sliders. Looks are a starting point: every other adjustment still applies on top, and the photo stays editable (#121).

## Requirements

### REQ-028-01 · Pick a look

- **Given** editing a photo, with _Photo_ chosen (not its subject or background)
- **When** the user opens _Look_ and picks one of _Polaroid 600_, _Polaroid B&W_, _Portra 400_, _Velvia 50_, _Gold 200_, _Superia 200_ or _HP5_
- **Then** the photo takes on that look's colors, contrast, grain and vignette at full strength, and _Strength_ and _Grain_ appear under the looks
- **And** _None_ takes the look away again; each pick is one undo step (REQ-021-07)

### REQ-028-02 · Strength

- **Given** a look picked
- **When** the user changes _Strength_ (0 to 100 %)
- **Then** the photo moves between unedited by the look (0 %) and the full look (100 %), every part of the look alike

### REQ-028-03 · Grain of a look

- **Given** a look picked
- **When** the user changes _Grain_ (0 to 200 %)
- **Then** only the look's grain changes: none at 0 %, the look's own at 100 %, twice as much at 200 %; the colors stay as they were

### REQ-028-04 · Other adjustments on top

- **Given** a look picked
- **When** the user changes other sliders or curves
- **Then** they apply on top of the look: a slider adds to what the look sets, the curves bend the look's result, and the sliders show the user's own values, not the look's
- **And** the look, its strength and grain are saved with the edit and come back when it is edited again (REQ-021-11)

### REQ-028-05 · Preview and saved file alike

- **Given** a look with any strength and grain, and any other adjustments
- **When** the photo is saved (REQ-021-09, REQ-021-10)
- **Then** the saved file shows what the preview showed (within two levels, as REQ-021-13)

### REQ-028-06 · Grain like film's

- **Given** a look with grain
- **When** it is applied
- **Then** the grain shows most in the midtones and less towards black and white
- **And** a color film's grain differs between the color channels; a black and white film's doesn't
- **And** _Grain_ among the _Effects_, without a look, stays as it is (REQ-021-06)

### REQ-028-07 · Colors per hue

- **Given** a color look
- **When** it is applied
- **Then** it changes hues differently, as its film does: Velvia 50 strengthens greens and blues more than skin, and Portra 400 mutes greens and keeps skin
- **And** greys stay grey, the change follows _Strength_, and a black and white look has none

### REQ-028-08 · Halation

- **Given** a color look that has it (_Portra 400_, _Velvia 50_, _Gold 200_, _Superia 200_)
- **When** a photo has bright lights
- **Then** they glow red into what lies beside them, following _Strength_; an evenly bright area keeps its color

## Out of scope

- Previews of each look on the photo itself (the looks are named buttons).
- Looks the user saves (presets), importing LUTs (`.cube`), more looks than these seven.
- Instant frames (the white Polaroid border), light leaks, dust and scratches.
- Looks for the subject or background alone, for videos, in the browser version, or for many photos at once.

## Assumptions

- The looks are tuned by eye against reference scans, with the editor's own adjustments; they recreate the character of each film, not a measured color response.
- Brand names identify which film a look recreates; the app makes no claim of affiliation.

## Verification

| REQ        | Test | Result  |
| ---------- | ---- | ------- |
| REQ-028-01 | `crates/domain/src/look.rs` › req_028_01_every_look_changes_the_photo_and_black_and_white_ones_drop_color, req_028_01_superia_is_cooler_and_greener_than_gold; `crates/app/tests/edit.rs` › req_028_01_req_028_02_req_028_03_picks_a_look_and_sets_its_strength_and_grain | pass |
| REQ-028-02 | `crates/domain/src/look.rs` › req_028_02_strength_moves_between_none_and_the_full_look; `crates/app/tests/edit.rs` › req_028_01_req_028_02_req_028_03_… | pass |
| REQ-028-03 | `crates/domain/src/look.rs` › req_028_03_grain_changes_only_the_grain; `crates/app/tests/edit.rs` › req_028_01_req_028_02_req_028_03_… | pass |
| REQ-028-04 | `crates/domain/src/look.rs` › req_028_04_adjustments_add_to_the_look_and_curves_bend_its_result, req_028_04_the_look_is_saved_with_the_edit | pass |
| REQ-028-05 | `crates/app/src/edit/tests.rs` › req_028_05_the_gpu_preview_of_a_look_matches_the_cpu_render | pass |
| REQ-028-06 | `crates/domain/src/look.rs` › req_028_06_grain_is_strongest_in_the_midtones, req_028_06_color_film_has_grain_per_channel; `crates/app/src/edit/tests.rs` › req_028_05_the_gpu_preview_of_a_look_matches_the_cpu_render | pass |
| REQ-028-07 | `crates/domain/src/look.rs` › req_028_07_looks_treat_hues_differently, req_028_07_greys_stay_and_strength_scales_it; `crates/app/src/edit/tests.rs` › req_028_05_the_gpu_preview_of_a_look_matches_the_cpu_render | pass |
| REQ-028-08 | `crates/domain/src/look.rs` › req_028_08_bright_lights_glow_red_around_them; `crates/media/src/edit.rs` › req_028_08_saving_lets_bright_lights_glow; `crates/app/src/edit/tests.rs` › req_028_08_the_gpu_halation_matches_the_cpu_render | pass |
