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
- **When** the user looks at the sidebar
- **Then** its footer shows "Indexing <library>" (full scan) or "Checking <library>" (quick or changes scan), the number of files seen and updated, and how many scans are waiting
- **And** the library's entry shows a spinner

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
- **Then** it is still indexed, without metadata
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

## Out of scope

- File types beyond those in REQ-002-01: HEIC, RAW, PSD and similar.
- Following symbolic links or junctions.
- Pausing or cancelling a scan by hand (only removing the library cancels it).
- Running scans of several libraries in parallel.

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
| REQ-002-08 | `crates/app/tests/indexing.rs` › shows_which_library_is_scanned_with_counts_and_waiting_scans | partial: drawn over the window's bottom-left corner (the sidebar's foot) rather than as a footer row; the library's spinner is drawn but not tested |
| REQ-002-09 | `crates/indexer/src/scan.rs` › keeps_the_index_of_unreachable_libraries | pass |
| REQ-002-10 | `crates/indexer/src/scan.rs` › stops_and_keeps_everything_when_the_library_goes_offline_during_a_scan | pass |
| REQ-002-11 | `crates/indexer/src/scan.rs` (failed folders keep their media and are stored as unlisted) | no test |
| REQ-002-12 | `crates/indexer/src/scan.rs` › indexes_files_with_unreadable_metadata_without_it | partial: files that can't be accessed at all aren't tested |
| REQ-002-13 | `crates/indexer/src/indexer.rs` › a_panicking_scan_counts_as_finished…, a_panic_while_reading_one_file_costs_only_that_file | pass |
| REQ-002-14 | `crates/indexer/src/scan.rs` › rescans_incrementally; `crates/db/src/writer.rs` › prunes_unused_tags_bottom_up | pass |
