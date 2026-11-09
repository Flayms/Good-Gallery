# 002 · Indexing

**Status:** Implemented

## Goal

Every library is mirrored in a local index, so browsing and searching 50k–500k media items stays fast, even when the library lives on a slow or intermittently reachable network share. Scans are incremental, show results while they run, and never lose the index because a share went away.

## Requirements

### REQ-002-01 · Supported files

- **Given** a library containing files of various types
- **When** it is scanned
- **Then** files ending in `.jpg`, `.jpeg`, `.png`, `.webp`, `.gif`, `.avif` are indexed as images and `.mp4`, `.m4v`, `.mov`, `.webm`, `.mkv` as videos, regardless of letter case
- **And** all other files are ignored (`.xmp` files are read only as sidecars, see 004)

### REQ-002-02 · Recursive scan without housekeeping folders

- **Given** a library with nested folders
- **When** it is scanned
- **Then** media in all subfolders are indexed
- **And** housekeeping folders (dot folders, `$` folders, `@eaDir`, `#recycle`, `#snapshot`, `.@__thumb`, `System Volume Information`, in any letter case), dotfiles (e.g. macOS `._IMG_1.jpg`) and symbolic links are skipped

### REQ-002-03 · Full scan

- **Given** a library
- **When** a full scan completes
- **Then** every supported file is in the index with its metadata, the library is `online` and its last indexed time is set

### REQ-002-04 · Incremental scans

- **Given** a library that was scanned before
- **When** it is scanned again
- **Then** only new files and files whose size, modification time or sidecar modification time changed are read again
- **And** media items whose file vanished are removed, and so are vanished folders with everything below them

### REQ-002-05 · Re-read after a metadata version change

- **Given** media items indexed with an older metadata version
- **When** the library is next scanned
- **Then** those media items are read again even though their files are unchanged
- **And** the scheduled scan of such a library is a full scan instead of a quick scan until no outdated media items remain

### REQ-002-06 · Results while scanning

- **Given** a scan of a large or slow library
- **When** files have been read
- **Then** they are written to the index in batches of at most 250 files and at least every 2 seconds, so they appear in the gallery before the scan finishes (refresh: REQ-006-11)

### REQ-002-07 · Scan queue

- **Given** scan requests for several libraries
- **When** they arrive
- **Then** scans run one at a time, in request order
- **And** each library has at most one waiting scan. A new request for it merges into the waiting one, keeping the more thorough kind (full > quick > changes)
- **And** a full or quick request for the library being scanned is dropped if the running scan is at least as thorough

### REQ-002-08 · Scan progress

- **Given** a running scan
- **When** the user looks at any page
- **Then** the toolbar or page header shows a spinning processing icon, and nothing covers the window's corners (#175)
- **And** clicking the icon shows "Indexing <library>" (full scan) or "Checking <library>" (quick or changes scan), the number of files seen and updated, and how many scans are waiting, updated while it is open
- **And** the library's entry shows a spinner, and once nothing is running the icon is gone

### REQ-002-09 · Offline library at scan start

- **Given** a library that can't be reached within 5 seconds
- **When** a scan starts
- **Then** the scan stops, the library becomes `offline` and its media items stay in the index, browsable with their cached thumbnails

### REQ-002-10 · Library goes offline during a scan

- **Given** a scan in progress
- **When** reading fails with a network-type error and the library is then unreachable
- **Then** the scan stops, files read so far are written, nothing is removed and the library becomes `offline`
- **And** a failure that leaves the library reachable only affects that file or folder

### REQ-002-11 · Folders that can't be listed

- **Given** a folder that can't be listed (e.g. access denied)
- **When** the library is scanned
- **Then** media items indexed from that folder and below are kept, and the next quick scan tries the folder again

### REQ-002-12 · Unreadable files

- **Given** a file whose metadata can't be read
- **When** it is scanned
- **Then** it is still indexed: a new file without metadata, a file indexed before with the metadata it had
- **And** it counts as unreadable, with the reason the read failed, until a read succeeds
- **And** a file that can't be accessed at all keeps the state it was last indexed with

### REQ-002-13 · Indexer failure

- **Given** the indexer fails unexpectedly while scanning (a panic in a scan or while reading a file)
- **When** further scans are waiting or requested
- **Then** they still run, and the interrupted scan counts as finished
- **And** the window stays open and responsive

### REQ-002-14 · Remove unused tags

- **Given** tags that no media item (or descendant tag) uses anymore
- **When** a scan completes
- **Then** those tags are removed and no longer suggested

### REQ-002-15 · Read unreadable files again

- **Given** an unreadable file (REQ-002-12)
- **When** its library is next scanned, quick scans included
- **Then** its metadata is read again, even though the file didn't change
- **And** once a read succeeds, the file is no longer unreadable
- **And** it doesn't turn the library's scheduled scan into a full scan (unlike REQ-002-05)
- **And** media items an earlier version indexed without width and height (a read that failed unnoticed) count as unreadable once the index is updated, so they are read again too

### REQ-002-16 · Unreadable files in the toolbar

- **Given** unreadable files in any library
- **When** the gallery is shown
- **Then** the toolbar shows how many there are ("1 unreadable") with an alert icon
- **And** clicking it lists each file with its name, library and folder, and the reason its read failed
- **And** without unreadable files the toolbar shows nothing

### REQ-002-17 · Read again

- **Given** the list of unreadable files (REQ-002-16)
- **When** the user clicks _Read again_ on a file, or _Read all again_
- **Then** those files are read again right away, and their buttons show _Reading…_ until the scan is done
- **And** files read successfully leave the list, and the toolbar's count goes down

### REQ-002-18 · Files someone is waiting for go first

- **Given** scans running or waiting, e.g. a library's first scan
- **When** a file the user wants to see right away (one opened with the app, REQ-027-01) is asked to be indexed
- **Then** its changes scan runs next, before the waiting scans
- **And** a running full or quick scan pauses for it: what it has read so far is kept, and it continues right after, without reading those files again

### REQ-002-19 · Files still being written

- **Given** a new or changed file that another program is still writing (e.g. a copy to a network share): it is locked, or its metadata can't be read and it changed less than a minute ago
- **When** it is scanned
- **Then** it isn't indexed yet and doesn't count as unreadable (a file indexed before keeps its last indexed state)
- **And** it is read again after 5 seconds, 30 seconds, 2 minutes and 10 minutes, until a read succeeds; the last read takes it as it is (REQ-002-12)

## Out of scope

- File types beyond those in REQ-002-01: HEIC, RAW, PSD and similar.
- Following symbolic links or junctions.
- Pausing or cancelling a scan by hand (only removing the library cancels it).
- Running scans of several libraries in parallel.
- Unreadable files in the browser version (spec 019): its readers have no failures to report, a file they can't parse simply has no metadata.
- Opening an unreadable file from the toolbar's list.

## Assumptions

- The index lives in the user profile on a local disk. It is the only copy of indexed data, and it can always be rebuilt from the libraries.
- Network errors (anything but "not found", "access denied" and similar permanent errors) are transient: file operations retry 3 times with exponential backoff before giving up.

## Verification

| REQ        | Test                                                                                                                                                        | Result                                                                  |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| REQ-002-01 | `crates/domain/src/media.rs` › recognizes_supported_files…; `crates/indexer/src/scan.rs` › indexes_media_recursively_and_skips_housekeeping_files | pass |
| REQ-002-02 | `crates/domain/src/media.rs` › skips_dotfiles…; `crates/indexer/src/walk.rs` › walks_depth_first_and_skips_housekeeping, skips_symbolic_links; `scan.rs` › indexes_media_recursively… | pass |
| REQ-002-03 | `crates/indexer/src/scan.rs` › indexes_media_recursively_and_skips_housekeeping_files | pass |
| REQ-002-04 | `crates/indexer/src/scan.rs` › rescans_incrementally; `crates/db/src/writer.rs` › removes_vanished_folders_with_everything_below | pass |
| REQ-002-05 | `crates/indexer/src/scan.rs` › rereads_files_from_an_older_metadata_extractor; `crates/indexer/src/schedule.rs` › scans_libraries_with_outdated_rows_fully_and_others_quickly | pass |
| REQ-002-06 | `crates/indexer/src/scan.rs` › writes_read_files_while_the_scan_still_runs | pass |
| REQ-002-07 | `crates/indexer/src/indexer.rs` › merges_scopes_keeping_the_more_thorough_kind, runs_scans_one_at_a_time_in_request_order | pass |
| REQ-002-08 | `crates/app/tests/indexing.rs` › req_002_08_shows_which_library_is_scanned_with_counts_and_waiting_scans (snapshot `processing_menu`) | partial: the library's spinner is drawn but not tested |
| REQ-002-09 | `crates/indexer/src/scan.rs` › keeps_the_index_of_unreachable_libraries | pass |
| REQ-002-10 | `crates/indexer/src/scan.rs` › stops_and_keeps_everything_when_the_library_goes_offline_during_a_scan | pass |
| REQ-002-11 | `crates/indexer/src/scan.rs` › keeps_the_media_of_folders_that_cant_be_listed_and_retries_them (Unix only: a folder without the read permission) | pass |
| REQ-002-12 | `crates/indexer/src/scan.rs` › indexes_new_files_with_unreadable_metadata_without_it_and_with_the_reason, keeps_the_metadata_of_a_known_file_whose_read_fails | partial: files that can't be accessed at all aren't tested |
| REQ-002-13 | `crates/indexer/src/indexer.rs` › a_panicking_scan_counts_as_finished…, a_panic_while_reading_one_file_costs_only_that_file | pass |
| REQ-002-14 | `crates/indexer/src/scan.rs` › rescans_incrementally; `crates/db/src/writer.rs` › prunes_unused_tags_bottom_up | pass |
| REQ-002-15 | `crates/indexer/src/scan.rs` › quick_scans_read_unreadable_files_again, changes_scans_read_an_unreadable_file_again (the metadata version stays, so `schedule::scan_all` keeps quick scans); `crates/db/src/lib.rs` › marks_media_indexed_without_dimensions_unreadable | pass |
| REQ-002-16 | `crates/app/tests/indexing.rs` › lists_unreadable_files_with_their_library_folder_and_reason (snapshot `unreadable_files`), shows_nothing_without_unreadable_files; `crates/app/src/unreadable.rs` › names_the_library_and_folder | pass |
| REQ-002-17 | `crates/app/tests/indexing.rs` › reads_files_again_and_drops_those_read_successfully; `crates/indexer/src/scan.rs` › changes_scans_read_an_unreadable_file_again | pass |
| REQ-002-18 | `crates/indexer/src/indexer.rs` › urgent_paths_go_first_and_pause_the_running_scan; `crates/indexer/src/scan.rs` › a_paused_scan_removes_nothing_and_saves_no_folders | pass |
| REQ-002-19 | `crates/indexer/src/scan.rs` › req_002_19_skips_files_still_being_written_until_the_last_wait; `crates/indexer/src/indexer.rs` › req_002_19_reads_files_still_being_written_again_later | pass |
