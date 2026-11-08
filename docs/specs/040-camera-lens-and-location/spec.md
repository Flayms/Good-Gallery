# 040 · Camera, lens and location

**Status:** Implemented

## Goal

People who shoot with several cameras or lenses want to find what they took with one of them, and see in the info panel what a photo was taken with and where. Their files already say so, but the index doesn't keep it yet. This spec adds the camera, the lens and the GPS position to the index, reads them for libraries indexed before, and lets smart albums and the info panel use them. Place names and a map from the position are #259 and #260 (#204).

## Requirements

### REQ-040-01 · Camera and lens from the file

- **Given** a photo or video whose metadata names the camera's make and model, or the lens
- **When** it is indexed
- **Then** the index keeps the camera as the make followed by the model, without the make twice when the model already starts with it ("Canon EOS R5", not "Canon Canon EOS R5"; "NIKON D750", not "NIKON CORPORATION NIKON D750"), or whichever of the two is there
- **And** it keeps the lens as the file's lens model, else its lens name, else the lens exiftool identifies, but not one exiftool reports as unknown
- **And** a sidecar's camera or lens counts when the file names none, as for dates (REQ-004-03)

### REQ-040-02 · Location from the file

- **Given** a photo or video with a GPS position in the file or its sidecar
- **When** it is indexed
- **Then** the index keeps its latitude and longitude in degrees, south and west negative
- **And** a position outside ±90° latitude or ±180° longitude, or exactly 0° / 0°, which cameras write when they have no fix, counts as no location

### REQ-040-03 · Libraries indexed before

- **Given** a library indexed before cameras, lenses and locations were read
- **When** the app starts
- **Then** the indexer reads its files' metadata again in the background, as after any change to what is read (REQ-004-01), and the gallery stays usable meanwhile

### REQ-040-04 · In the info panel

- **Given** the info panel of an item (REQ-009-08)
- **When** the item has a camera, a lens or a location
- **Then** the panel shows _Camera_, _Lens_ and _Coordinates_ after _Size_, the coordinates as degrees with four decimals and the hemisphere ("33.4500° S, 70.6600° W"); _Location_ stays the file's path
- **And** it leaves out each of them the item doesn't have

### REQ-040-05 · Smart album conditions

- **Given** the smart album editor (REQ-039-02)
- **When** the user adds a condition
- **Then** they can choose _Camera is_, _Camera is not_, _Lens is_, _Lens is not_, _Has a location_ and _Has no location_
- **And** the camera and lens conditions offer the cameras and lenses in the index, most used first, with how many items have each
- **And** _Camera is not_ and _Lens is not_ also match items without a camera or lens

### REQ-040-06 · Browser version

- **Given** the browser version (spec 019)
- **When** it scans a JPEG, PNG, WebP or AVIF photo
- **Then** it reads the camera, the lens and the location from the photo's EXIF as REQ-040-01 and REQ-040-02 describe

## Out of scope

- Place names from the position, and Places from GPS (#259).
- A map (#260).
- Camera, lens or location as filters in the search bar or under _Filters_; smart albums carry them.
- Editing the camera, lens or location.

## Assumptions

- exiftool gives GPS positions as signed decimal degrees (its composite `GPSLatitude` / `GPSLongitude` with `#`), also for videos and XMP sidecars.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-040-01 | `crates/domain/src/media.rs` › req_040_01_names_the_camera_with_its_make_once; `crates/meta/src/rules.rs` › req_040_01_reads_the_camera_and_the_lens; `crates/db/src/writer.rs` › req_040_01_req_040_02_stores_the_camera_the_lens_and_the_location; `crates/indexer/tests/end_to_end.rs` › req_040_01_req_040_02_indexes_the_camera_the_lens_and_the_location | pass |
| REQ-040-02 | `crates/domain/src/media.rs` › req_040_02_keeps_only_positions_on_earth_with_a_fix; `crates/meta/src/rules.rs` › req_040_02_reads_the_location_from_the_file_else_the_sidecar; `crates/db/src/writer.rs` › req_040_01_req_040_02_stores_the_camera_the_lens_and_the_location; `crates/indexer/tests/end_to_end.rs` › req_040_01_req_040_02_indexes_the_camera_the_lens_and_the_location | pass |
| REQ-040-03 | `crates/indexer/src/scan.rs` › req_040_03_rereads_libraries_indexed_before_cameras_lenses_and_locations | pass (the gallery staying usable is the scan's usual background work, REQ-002-05) |
| REQ-040-04 | `crates/app/src/format.rs` › req_040_04_writes_coordinates_with_their_hemispheres; `crates/app/tests/viewer.rs` › req_040_04_shows_the_camera_the_lens_and_the_coordinates | pass |
| REQ-040-05 | `crates/domain/src/album_rules.rs` › req_040_05_stores_camera_lens_and_location_conditions; `crates/db/src/smart_albums.rs` › req_040_05_rule_albums_match_camera_lens_and_location; `crates/app/tests/smart_albums.rs` › req_040_05_the_editor_offers_the_cameras_in_the_index | pass |
| REQ-040-06 | `crates/meta/src/tiff.rs` › req_040_06_reads_the_model_the_lens_and_the_position | pass |
