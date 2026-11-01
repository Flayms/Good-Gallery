# 026 · Tagging

**Status:** Implemented

## Goal

Users add the tags, People and Places a photo lacks, and take back wrong ones, while they look at it, without another program. Like ratings (spec 024), the changes go into the file's metadata, in the fields Lightroom, digiKam and Windows read, so they survive a rebuilt index and show in other tools. This is the second part of manual tagging (#32); tagging a selection follows.

## Requirements

### REQ-026-01 · Add a tag

- **Given** the info panel or info sheet of an item, in the desktop app
- **When** the user clicks _Add_ under People, Places or Tags, types a name and presses Enter or picks a suggestion
- **Then** the item gets that tag in that group: a suggested existing tag with its hierarchy (`Places › Italy › Rome`), a new one directly under People or Places, or, under Tags, on its own
- **And** suggestions are the existing tags of that group whose names contain what was typed; Escape closes the field without a change

### REQ-026-02 · Remove a tag

- **Given** a tag of the item in the info panel or info sheet, in the desktop app
- **When** the user clicks its remove button (×)
- **Then** the item no longer has that tag
- **And** a person named only by a face region (REQ-004-09) has no remove button: the region stays as other programs drew it, and the tag's tooltip says why

### REQ-026-03 · Written into the metadata

- **Given** a tag added or removed as in REQ-026-01 or REQ-026-02
- **When** it is saved
- **Then** an added tag goes where a rating would (REQ-024-03): into a photo, and into a video's sidecar unless the video has keywords of its own; it is written as the hierarchy (`XMP-lr:HierarchicalSubject`, `Places|Italy|Rome`) and as its name (`XMP-dc:Subject`), and into IPTC keywords if the file has them
- **And** a removed tag is taken out of every keyword field it is read from (REQ-004-06, REQ-004-07, REQ-004-09), in the file and its sidecar, together with the flat keywords of its hierarchy's levels that no other hierarchy of the item still uses (Lightroom writes every level as a flat keyword)
- **And** the date modified stays, and the index reads the file again, so the sidebar, the search and the gallery's tag indicators follow

### REQ-026-04 · The file can't be written

- **Given** a file that can't be written (read-only, an offline or read-only share, exiftool missing or failing)
- **When** the user adds or removes a tag
- **Then** the file and the index keep their tags, and a toast says why

### REQ-026-05 · Only the desktop app

- **Given** the browser version (spec 019)
- **When** the info panel shows an item's tags
- **Then** they show as before, without _Add_ or remove buttons

## Out of scope

- Renaming, moving or merging tags across the library.
- Tagging a selection (next part of #32).
- Drawing or naming face regions.
- `XMP-mwg-kw` keyword structures.

## Assumptions

- The keyword fields read are those of REQ-004-06, REQ-004-07 and REQ-004-09; each file has at most a few dozen keywords, so writing the whole list back is cheap.
- Lightroom treats a hierarchy's levels as flat keywords too; removing a nested tag also removes the levels that no other hierarchy needs, so they don't surface as tags of their own.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-026-01 | `crates/domain/src/tagging.rs` › req_026_01_suggests_and_finds_the_tags_of_a_group; `crates/app/tests/tagging.rs` › req_026_01_req_026_03_adds_suggested_and_new_tags, req_026_01_suggests_in_the_info_sheet, req_026_01_escape_closes_the_field; snapshot `tag_suggestions` | pass |
| REQ-026-02 | `crates/app/tests/tagging.rs` › req_026_02_removes_a_tag_but_not_a_face_region | pass |
| REQ-026-03 | `crates/domain/src/tagging.rs` › req_026_03_adds_the_hierarchy_and_the_name, req_026_03_removes_the_tag_from_every_field; `crates/media/src/tagging.rs` › req_026_03_…; `crates/db/src/media.rs` › req_026_03_marks_a_retagged_file_for_reading; `crates/app/tests/tagging.rs` › req_026_01_req_026_03_adds_suggested_and_new_tags | pass |
| REQ-026-04 | `crates/media/src/tagging.rs` › req_026_04_leaves_a_read_only_file_as_it_was; `crates/app/tests/tagging.rs` › req_026_04_keeps_the_tags_of_a_file_it_cant_write | pass |
| REQ-026-05 | by inspection: `crate::WEB` turns off _Add_ and the remove buttons (`info.rs`); `cargo clippy -p app --target wasm32-unknown-unknown` | no test |
