# 043 · LUTs

**Status:** Implemented

## Goal

Colorists, film emulation packs and other editors share looks as LUTs: tables that map every color to another. People who have LUTs they like want to put them on their photos in Good Gallery, at a strength, with the rest of their edit on top, and keep the photo editable (#249).

## Requirements

### REQ-043-01 · Import a LUT

- **Given** the editor's _Look_ section
- **When** the user clicks _Import LUT…_ and picks a `.cube` file
- **Then** the app keeps a copy among its own data and offers it by its title, else its file name, also after a restart
- **And** the same file imported again is the same LUT
- **And** a file that isn't a 3D `.cube` LUT, or one of more than 65 points a side, isn't kept, and a notice says why

### REQ-043-02 · Apply a LUT

- **Given** a photo being edited and an imported LUT
- **When** the user picks it
- **Then** it applies to the photo's colors first, interpolated between its points, and every other adjustment, a film look included, works on what it made
- **And** _LUT strength_, 0 to 100 %, blends it with the photo's own colors; a double-click goes back to 100 %
- **And** the preview and the saved file look alike (within two levels)

### REQ-043-03 · Kept with the edit

- **Given** a saved edit with a LUT
- **When** the photo is edited again
- **Then** the LUT and its strength come back, as the rest of the edit does (REQ-021-12)
- **And** on a computer without that LUT, the photo shows without it, the editor says so, and the choice stays in the edit until changed

## Out of scope

- 1D LUTs and formats other than `.cube` (`.3dl`, `.look`, Hald images).
- Removing an imported LUT in the app; deleting its file from the data folder's `luts` removes it.
- LUTs bundled with the app, and exporting the film looks as LUTs.
- A LUT on the subject, the background or a mask alone.

## Assumptions

- A creative LUT maps sRGB-encoded colors, as LUTs for photos and Rec. 709 video mostly do.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-043-01 | `crates/domain/src/lut.rs` › req_043_01_reads_a_cube_file, req_043_01_says_why_a_file_cant_be_used; `crates/app/src/luts.rs` › req_043_01_keeps_imported_luts; `crates/app/tests/edit.rs` › req_043_01_req_043_02_req_043_03_imports_and_picks_a_lut | pass |
| REQ-043-02 | `crates/domain/src/lut.rs` › req_043_02_maps_colors_and_interpolates_between_points, req_043_02_req_043_03_an_edit_applies_its_lut_first_at_its_strength; `crates/media/src/edit.rs` › req_043_02_req_043_03_saving_applies_the_lut; `crates/app/src/edit.rs` › req_043_02_the_gpu_preview_of_a_lut_matches_the_cpu_render; `crates/app/tests/edit.rs` › req_043_01_req_043_02_req_043_03_imports_and_picks_a_lut | pass |
| REQ-043-03 | `crates/domain/src/lut.rs` › req_043_03_the_same_file_has_the_same_id, req_043_02_req_043_03_an_edit_applies_its_lut_first_at_its_strength; `crates/media/src/edit.rs` › req_043_02_req_043_03_saving_applies_the_lut; `crates/app/tests/edit.rs` › req_043_01_req_043_02_req_043_03_imports_and_picks_a_lut | pass (the editor's note without the table by inspection) |
