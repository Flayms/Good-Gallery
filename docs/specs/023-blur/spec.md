# 023 · Blur

**Status:** Implemented

## Goal

A blur draws the eye to what stays sharp. Users who edit a photo (spec 021) blur the background behind a portrait as a fast lens would, fake a miniature with a tilt-shift band, or keep one spot sharp and let the rest fall away, with live feedback and the same result in the saved file.

## Requirements

### REQ-023-01 · Choose a blur

- **Given** editing a photo, with the whole photo chosen (spec 022)
- **When** the user picks a blur under _Effects_: _None_, _Behind subject_, _Tilt-shift_ or _Radial_
- **Then** the photo is blurred that way by _Strength_ (0 to 100), and _None_ or _Strength_ at 0 leaves it unchanged
- **And** choosing _Behind subject_ looks for the subject first (REQ-022-02); until it is found, or if there is none, nothing is blurred

### REQ-023-02 · Blur behind the subject

- **Given** _Behind subject_ chosen
- **When** the user raises _Strength_
- **Then** the background gets blurrier while the subject stays sharp, with a soft transition at the subject's edge

### REQ-023-03 · Tilt-shift and radial blur

- **Given** _Tilt-shift_ or _Radial_ chosen
- **When** the user sets _Focus size_ and _Transition_ (0 to 100 each), or chooses _Place focus_ and clicks a spot in the photo
- **Then** tilt-shift keeps a horizontal band through the focus sharp, radial a round spot around it, and the blur grows smoothly over the transition beyond it
- **And** the focus starts in the middle of the photo

### REQ-023-04 · Preview and saved file alike

- **Given** a blur
- **When** the user changes it, at any zoom, or saves the edit
- **Then** the preview follows from the next frame on, and the saved file shows the same blur (within a few levels per channel), relative to the photo's size

## Out of scope

- Turning the tilt-shift band, elliptic radial spots, several focus spots.
- Lens-shaped bokeh (discs, cat's eyes), depth estimation, blur of the subject only.
- Blur per part (spec 022): blur applies to the whole photo.

## Assumptions

- A Gaussian-like blur is what users expect from "blur"; strong blurs reach about 1.5 % of the photo's longer side (a 4000 px photo: about 60 px).

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-023-01 | `crates/domain/src/edit.rs` › req_023_01_no_blur_or_blur_at_zero_changes_nothing; `crates/app/tests/edit.rs` › req_023_01_req_023_02_req_023_03_chooses_a_blur_and_places_its_focus | pass |
| REQ-023-02 | `crates/domain/src/edit.rs` › req_023_02_background_blur_spares_the_subject; `crates/app/tests/edit.rs` › req_023_01_req_023_02_req_023_03_chooses_a_blur_and_places_its_focus; `crates/app/src/edit.rs` › req_023_04_the_gpu_blur_matches_the_cpu_blur | pass |
| REQ-023-03 | `crates/domain/src/edit.rs` › req_023_03_tilt_shift_and_radial_keep_the_focus_sharp; `crates/app/tests/edit.rs` › req_023_01_req_023_02_req_023_03_chooses_a_blur_and_places_its_focus | pass |
| REQ-023-04 | `crates/domain/src/edit.rs` › req_023_04_the_pyramid_blurs_smoothly_and_keeps_flat_areas; `crates/media/src/edit.rs` › req_023_04_saving_blurs_away_from_the_focus; `crates/app/src/edit.rs` › req_023_04_the_gpu_blur_matches_the_cpu_blur | pass |
