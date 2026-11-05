# 034 · Smart albums

**Status:** Implemented

## Goal

Users return to the same searches: a trip's tag, five-star photos of one person, last summer's videos in one folder. Building them again each time takes several clicks and is easy to get slightly wrong. A smart album keeps a search under a name in the sidebar, and it stays up to date as media items are added, tagged or rated, because it is the search and not a list of files.

## Requirements

### REQ-034-01 · Save a search

- **Given** a search that isn't the default one (any library, folder, tag, file name or filter)
- **When** the user chooses _Save as smart album…_ in the _Filters_ menu
- **Then** a dialog asks for a name, suggested from the search (its tags and file name, else _Smart album_), and _Save_ keeps the search with its sort order under that name
- **And** the name is trimmed and can't be empty
- **And** with the default search the entry is disabled, as there is nothing to keep

### REQ-034-02 · Smart albums in the sidebar

- **Given** smart albums
- **When** the sidebar shows
- **Then** they are listed under _Smart albums_ by name, each with the number of media items it holds now
- **And** the group can be collapsed, moved and switched off like the other groups (REQ-008-07, REQ-008-10)
- **And** without any smart album the group doesn't show

### REQ-034-03 · Open a smart album

- **Given** a smart album
- **When** the user clicks it
- **Then** the gallery shows its search in its sort order, and the row is marked as active as long as the search is unchanged
- **And** the search can be changed from there like any other, which doesn't change the smart album

### REQ-034-04 · Up to date

- **Given** a smart album
- **When** media items matching it are added, removed, tagged or rated
- **Then** its count and what it shows follow, as they do for the search it holds

### REQ-034-05 · Rename and delete

- **Given** a smart album
- **When** the user chooses _Rename…_ or _Delete_ in its context menu
- **Then** it gets the new name (trimmed, not empty) or is removed from the sidebar
- **And** neither touches any file

### REQ-034-06 · A removed library or folder

- **Given** a smart album of a library that was removed, or of a folder without media items any more
- **When** the sidebar shows
- **Then** its row has a warning icon and says why on hover ("Its library was removed")
- **And** opening it shows that search, which finds nothing: it never widens to other libraries or folders

### REQ-034-07 · Kept

- **Given** smart albums
- **When** the app starts again
- **Then** they are there, in the index, with their searches unchanged

## Out of scope

- Conditions the search doesn't have, matching any instead of all conditions, and relative dates (#203).
- Camera, lens and GPS (#204).
- Exporting smart albums, or keeping them when the index is deleted.
- Albums of chosen media items (static albums).

## Assumptions

- A user keeps a few dozen smart albums at most, so counting each one when the sidebar loads costs about as much as a few searches.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-034-01 | `crates/domain/src/smart_albums.rs` › req_034_01_suggests_a_name_from_tags_and_file_name, req_034_01_req_034_05_trims_names_and_refuses_empty_ones; `crates/db/src/smart_albums.rs` › req_034_01_req_034_05_req_034_07_keeps_renames_and_deletes_albums_by_name; `crates/app/tests/smart_albums.rs` › req_034_01_req_034_02_saves_a_search_and_lists_it_with_its_count (snapshot `smart_album_save`), req_034_01_needs_a_name | pass |
| REQ-034-02 | `crates/db/src/smart_albums.rs` › req_034_02_req_034_04_req_034_06_counts_albums_and_finds_what_is_gone; `crates/app/tests/smart_albums.rs` › req_034_01_req_034_02_saves_a_search_and_lists_it_with_its_count (snapshot `smart_album_sidebar`) | pass |
| REQ-034-03 | `crates/app/tests/smart_albums.rs` › req_034_03_opens_a_smart_album | pass |
| REQ-034-04 | `crates/db/src/smart_albums.rs` › req_034_02_req_034_04_req_034_06_counts_albums_and_finds_what_is_gone | pass |
| REQ-034-05 | `crates/db/src/smart_albums.rs` › req_034_01_req_034_05_req_034_07_keeps_renames_and_deletes_albums_by_name; `crates/domain/src/smart_albums.rs` › req_034_01_req_034_05_trims_names_and_refuses_empty_ones; `crates/app/tests/smart_albums.rs` › req_034_05_renames_and_deletes_a_smart_album | pass |
| REQ-034-06 | `crates/db/src/smart_albums.rs` › req_034_02_req_034_04_req_034_06_counts_albums_and_finds_what_is_gone | partial (the warning icon and its text: by inspection) |
| REQ-034-07 | `crates/domain/src/smart_albums.rs` › req_034_07_reads_back_every_part_of_a_search, req_034_07_keeps_defaults_for_what_it_cant_read; `crates/db/src/smart_albums.rs` › req_034_01_req_034_05_req_034_07_keeps_renames_and_deletes_albums_by_name | pass |
