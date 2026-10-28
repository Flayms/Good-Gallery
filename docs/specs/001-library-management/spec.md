# 001 · Library management

**Status:** Implemented

## Goal

The user decides which folders make up the gallery: folders on local disks or network shares, including UNC paths that aren't mapped as drives. Libraries can be added, rescanned and removed from the Settings page. The files themselves are never touched.

## Requirements

### REQ-001-01 · Add a library by path

- **Given** an absolute path to a reachable folder that isn't a library yet
- **When** the user enters it on the Settings page and clicks _Add_
- **Then** the folder appears in the library list as `online`, labelled with the folder's name (the full path for a drive root), a full scan is queued and a toast confirms "Added <label>, indexing…"

### REQ-001-02 · Add a library with the folder picker

- **Given** the Settings page
- **When** the user clicks _Browse…_ and picks a folder
- **Then** the folder is added as in REQ-001-01
- **And** cancelling the dialog adds nothing

### REQ-001-03 · Reject invalid paths

- **Given** a relative path, or a folder that doesn't exist or isn't reachable within 5 seconds
- **When** the user tries to add it
- **Then** nothing is added and an error explains why ("Path must be absolute", "Folder not found or not reachable")

### REQ-001-04 · Reject duplicate libraries

- **Given** a library `D:\Photos`
- **When** the user adds the same path in any letter case (e.g. `d:\photos`)
- **Then** nothing is added and the error "Library already added" is shown
- **And** on Linux, whose paths are case-sensitive, only the same path counts: `/media/photos` is another folder than `/media/Photos`

### REQ-001-05 · Remove a library

- **Given** a library, possibly being scanned or waiting in the scan queue
- **When** the user clicks _Remove_ and confirms that files on disk aren't touched
- **Then** the library and all its media items disappear from the gallery and the sidebar
- **And** its queued scan is dropped, a running scan is cancelled without writing anything further, and its watcher stops
- **And** no file in the library is modified or deleted

### REQ-001-06 · Rescan a library

- **Given** a library
- **When** the user clicks _Rescan_
- **Then** a full scan is queued, which also finds files that changed in place while the app wasn't running, and a toast confirms "Rescan queued"

### REQ-001-07 · Library list

- **Given** one or more libraries
- **When** the user opens the Settings page
- **Then** each library shows its label, status (`online`, `offline`, `unknown`), full path and the time it was last indexed, sorted by label
- **And** without libraries the list says "No libraries yet."

### REQ-001-08 · Existing index

- **Given** an index written by an earlier version of Good Gallery
- **When** the app starts with it
- **Then** it opens without re-indexing, with all libraries, media items, tags and settings

### REQ-001-09 · Data folder

- **Given** the installed or portable app of a release
- **When** it starts without a data folder given on the command line
- **Then** it uses `%APPDATA%\good-gallery`, the folder every earlier version used, so an update picks up the index, settings and thumbnail cache as they are
- **And** builds made for development use a folder of their own (`%APPDATA%\good-gallery-rust`), so they never touch it

## Out of scope

- Renaming a library's label in the UI (the label is always derived from the folder name).
- Detecting nested or overlapping libraries (a library inside another one is indexed twice).
- Moving a library to a new path while keeping its index.
- Deleting a removed library's cached thumbnails right away (they are evicted from the thumbnail cache over time, see 005).
- Non-Windows paths.

## Assumptions

- Windows paths are case-insensitive, so duplicates are detected case-insensitively.
- UNC paths that aren't mapped in Explorer can't always be picked in the folder dialog, which is why typing a path stays possible.

## Verification

| REQ        | Test                                                                                                                                          | Result                                                                              |
| ---------- | --------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| REQ-001-01 | `crates/indexer/src/libraries.rs` › adds_a_reachable_folder…, normalizes_the_path; `crates/app/tests/libraries.rs` › adds_a_typed_path_and_confirms | pass |
| REQ-001-02 | `crates/app/tests/libraries.rs` › adds_the_picked_folder_and_nothing_when_cancelled (fake dialog) | pass |
| REQ-001-03 | `crates/indexer/src/libraries.rs` › rejects_relative_and_missing_paths; `reachability.rs` › gives_up_after_the_timeout; `crates/app/tests/libraries.rs` › explains_invalid_paths | pass |
| REQ-001-04 | `crates/indexer/src/libraries.rs` › rejects_duplicates_case_insensitively (Windows), rejects_the_same_path_but_tells_letter_cases_apart (Linux); `crates/db/src/libraries.rs` › finds_only_the_same_path_outside_windows; `crates/app/tests/libraries.rs` › rejects_a_duplicate_in_another_letter_case | pass |
| REQ-001-05 | `crates/indexer/src/libraries.rs` › removes_a_library…; `crates/indexer/src/indexer.rs` › removing_a_library_drops_its_queued_scan_and_cancels_the_running_one, watches_after_a_completed_scan_and_scans_reported_changes (watcher stopped); `crates/indexer/src/scan.rs` › writes_nothing_when_cancelled; `crates/app/tests/libraries.rs` › removes_a_library_after_confirmation… | pass |
| REQ-001-06 | `crates/indexer/src/libraries.rs` › rescan_queues_a_full_scan; `crates/app/tests/libraries.rs` › rescan_queues_a_full_scan_and_confirms | pass |
| REQ-001-07 | `crates/app/tests/libraries.rs` › lists_libraries_with_status_path_and_last_indexed_time, says_when_there_are_no_libraries | pass |
| REQ-001-08 | `crates/db/src/lib.rs` › req_001_08_opens_an_index_written_by_an_earlier_version, req_001_08_keeps_the_earlier_bookkeeping, req_001_08_migrates_an_earlier_index_to_the_current_schema | pass |
| REQ-001-09 | `crates/app/src/startup.rs` › packaged_builds_use_good_gallery_and_development_builds_dont; `packaging/smoke-test.ps1` (installed app run on a copy of the real index) | pass |
