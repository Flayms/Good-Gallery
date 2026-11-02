# 024 · Rating

**Status:** Implemented

## Goal

Users sort out a shoot by rating while they look through it: a key press in the viewer, or a click on the stars in the info panel or sheet. The rating goes into the file's metadata, where Lightroom, digiKam, darktable and Windows also look, so it isn't locked into Good Gallery and survives a rebuilt index. This is the first part of manual tagging (#32); tags, People and Places and changes to a selection of items follow.

## Requirements

### REQ-024-01 · Rate with the keyboard

- **Given** a photo or video open in the viewer
- **When** the user presses `1` to `5`
- **Then** the item gets that many stars, and the info panel, the tile's rating indicator, the Ratings in the sidebar and a search by rating show it
- **And** pressing the key of the rating it already has removes the rating

### REQ-024-02 · Rate with the stars

- **Given** the info panel or the info sheet of an item, rated or not
- **When** the user clicks the _n_-th star
- **Then** the item gets _n_ stars; clicking the star of the rating it already has removes the rating
- **And** hovering a star previews the rating it would set, and the stars are named for screen readers (`Rate 3 stars`)

### REQ-024-03 · Written into the metadata

- **Given** a rating set as in REQ-024-01 or REQ-024-02
- **When** it is saved
- **Then** a photo gets it in its own metadata (`XMP-xmp:Rating`, and every other `Rating` tag the file already has), and a video in its sidecar, an existing one or a new `<file name>.xmp`, unless the video file itself already carries a rating, which is then changed in place
- **And** a sidecar that already holds a rating gets the same one, so other programs don't see an older rating
- **And** a removed rating is written as 0 (unrated), so a rating elsewhere doesn't take its place when the file is read again (REQ-004-05)
- **And** the file's date modified stays as it was, so the gallery's order doesn't change

### REQ-024-04 · The file can't be written

- **Given** a file that can't be written (read-only, an offline or read-only share, exiftool missing or failing)
- **When** the user rates it
- **Then** the file and the index keep the old rating, and a toast says the rating couldn't be saved and why

### REQ-024-05 · Only the desktop app

- **Given** the browser version (spec 019), which reads files but never writes them
- **When** an item is shown
- **Then** its rating shows as before and can't be changed: the keys do nothing and the stars aren't buttons

### REQ-024-06 · A failed write leaves the file as it was

- **Given** a rating written into an existing file or sidecar
- **When** exiftool fails or is ended, the app is ended part-way, a temporary file of an earlier write was left behind, or the file system loses the new version while it reports it as written (an rclone mount on Windows, #140)
- **Then** the file has the new rating with all of its other content, or is byte for byte as before; it is never empty or partial
- **And** a failure shows a toast that says why, and leaves no temporary file behind that would block the next write
- **And** the new version is on disk before it takes the original's place, and the date modified (to the sub-second), the date created on Windows and the file's permissions stay

### REQ-024-07 · Changes in quick succession

- **Given** several changes to one file in quick succession: ratings, tags (spec 026), changes to a selection (spec 025), an edit saved or reverted (spec 021)
- **When** they are written
- **Then** they are written one after another, in the order they were made, and the file has all of them afterwards

## Out of scope

- Tags, People and Places (later parts of #32).
- Rating a selection of several items (#31, then #32).
- Flags, colour labels and rejecting (rating −1).
- Undo; rating again sets the previous value.

## Assumptions

- Writing a rating is a small metadata change: exiftool rewrites the file with the same pixels. Photos are written in place, like saved edits (spec 021): the new version is written beside the file, checked, and only then takes its place.
- Videos get sidecars because rewriting large video files over a network share is slow and exiftool can't write WebM and Matroska.
- The rating is written to the file first and to the index only once the file has it, so the index never shows a rating the file doesn't have.

## Verification

| REQ        | Test                                                                                                                                                                                                                                                               | Result  |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------- |
| REQ-024-01 | `crates/domain/src/rating.rs` › req_024_01_req_024_02_sets_the_stars_or_removes_the_same_rating; `crates/app/tests/rating.rs` › req_024_01_req_024_03_rates_with_the_keys                                                                                          | pass    |
| REQ-024-02 | `crates/domain/src/rating.rs` › req_024_01_req_024_02_sets_the_stars_or_removes_the_same_rating; `crates/app/tests/rating.rs` › req_024_02_rates_with_the_stars; snapshots `info_sheet`, `viewer_info_without_details` (outlined stars)                              | pass    |
| REQ-024-03 | `crates/domain/src/rating.rs` › req_024_03_writes_photos_in_place_and_videos_beside_them; `crates/media/src/rating.rs` › req_024_03_…; `crates/db/src/media.rs` › req_024_03_stores_the_rating_set_by_hand; `crates/app/tests/rating.rs` › req_024_01_req_024_03_rates_with_the_keys | pass    |
| REQ-024-04 | `crates/media/src/rating.rs` › req_024_04_leaves_files_it_cant_write_as_they_were; `crates/app/tests/rating.rs` › req_024_04_keeps_the_rating_of_a_file_it_cant_write, req_024_04_says_so_without_exiftool                                                        | pass    |
| REQ-024-05 | by inspection: `crate::WEB` turns off the keys (`viewer.rs`) and the star buttons (`info.rs`); `cargo clippy -p app --target wasm32-unknown-unknown`                                                                                                              | no test |
| REQ-024-06 | `crates/media/src/replace.rs` › req_024_06_*; `crates/media/src/rating.rs` › req_024_06_*; syncing before the rename by inspection | pass |
| REQ-024-07 | `crates/app/tests/rating.rs` › req_024_07_writes_quick_changes_one_after_another | pass |
