# 003 · Change detection

**Status:** Implemented

## Goal

Files that are added, changed, renamed or deleted in a library show up in the gallery within seconds, without full scans. A full scan of a large network share takes hours, so changes are detected by watching the libraries and by cheap quick scans that only look at folders whose contents changed.

## Requirements

### REQ-003-01 · Watch libraries

- **Given** a library whose last scan completed
- **When** files or folders in it change
- **Then** the changes are picked up without user action
- **And** watching stops when the library goes offline or is removed

### REQ-003-02 · Settle before reporting

- **Given** a burst of changes, e.g. copying many files into a library
- **When** changes keep coming
- **Then** they are handled together once 2 seconds pass without further changes, but no later than 10 seconds after the first change

### REQ-003-03 · Ignore housekeeping changes

- **Given** changes to `Thumbs.db`, `desktop.ini`, dotfiles or anything in a housekeeping folder
- **When** they are reported
- **Then** they don't cause a scan

### REQ-003-04 · Changes scan

- **Given** reported changed paths
- **When** the changes scan runs
- **Then** the folders containing them are listed again, so new, renamed and deleted files and folders are found, and new subfolders are scanned completely
- **And** of the files already indexed, only the reported ones (or those whose reported path is their sidecar) are checked again, and other files in those folders aren't

### REQ-003-05 · Fall back to a quick scan

- **Given** the reported changes are unknown (change events were lost) or exceed 5,000 paths
- **When** they are handled
- **Then** a quick scan of the library runs instead of a changes scan

### REQ-003-06 · Watcher failure

- **Given** a watched library
- **When** watching fails (e.g. the share disconnects)
- **Then** watching stops and a quick scan is queued, which also detects whether the library went offline
- **And** the next completed scan starts watching again
- **And** when watching can't start after a completed scan, nothing is queued (nothing was missed yet); the next scheduled quick scan (REQ-003-09) tries again

### REQ-003-07 · Changes during a scan

- **Given** a scan of a library is running
- **When** changes in that library are reported
- **Then** a changes scan is queued after it, because the running scan may already have passed the changed folders

### REQ-003-08 · Quick scan

- **Given** a library with stored folder data from an earlier scan
- **When** a quick scan runs
- **Then** only folders whose contents (added, removed or renamed entries) changed since they were last listed are listed again, and their new and changed files are read
- **And** without stored folder data, the quick scan runs as a full scan

### REQ-003-09 · Scheduled quick scans

- **Given** the quick check interval (default every hour)
- **When** the app starts, and then whenever the interval elapses
- **Then** every library gets a quick scan, which also restarts watchers that stopped silently
- **And** with the interval set to "Only at startup", only the startup scan runs

### REQ-003-10 · Reconnecting offline libraries

- **Given** an offline library
- **When** it becomes reachable again
- **Then** within about a minute it is scanned, becomes `online` and is watched again

### REQ-003-11 · Shares that are still reconnecting

- **Given** a library that isn't `offline`, on a network share that takes a while to answer (it is reconnecting after the computer started or woke up)
- **When** a scan finds it unreachable
- **Then** the library stays as it is and is checked again for up to 30 seconds; it only becomes `offline` if it doesn't answer in that time
- **And** an `offline` library is checked once per retry (REQ-003-10), so retries don't hold up other scans

## Out of scope

- Detecting in-place edits (same file name, folder contents unchanged) made while the app wasn't running. Quick scans can't see them, so the user runs a manual rescan (REQ-001-06).
- Watching libraries while the app is closed.
- Configuring the debounce times or the offline re-check interval.

## Assumptions

- Network shares (SMB) deliver change notifications for recursive watches on Windows. Where they don't, the scheduled quick scans still catch up.
- A folder's modification time changes only when direct children are added, removed or renamed, not when files inside it are edited or when something deeper down changes.
- Windows also reports last-access changes, so the indexer's own reads cause change events. That's why a changes scan only checks reported files and doesn't re-read everything it sees.

## Verification

| REQ        | Test                                                                                                                                                             | Result                                                          |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| REQ-003-01 | `crates/indexer/src/watcher.rs` › reports_new_files_in_a_watched_folder; `crates/indexer/src/indexer.rs` › watches_after_a_completed_scan_and_scans_reported_changes, stops_watching_an_offline_library_and_watches_it_again_after_a_reconnect | pass |
| REQ-003-02 | `crates/indexer/src/watcher.rs` › reports_the_changed_paths_once_changes_settle, reports_within_the_maximum_delay… | pass |
| REQ-003-03 | `crates/indexer/src/watcher.rs` › normalizes_separators_and_skips_housekeeping_paths | pass |
| REQ-003-04 | `crates/indexer/src/scan.rs` › checks_only_reported_files_of_changed_folders; `crates/indexer/src/indexer.rs` › watches_after_a_completed_scan_and_scans_reported_changes | pass |
| REQ-003-05 | `crates/indexer/src/watcher.rs` › reports_unknown_changes_when_events_were_lost; `crates/indexer/src/indexer.rs` › more_than_5000_changed_paths_queue_a_quick_scan | pass |
| REQ-003-06 | `crates/indexer/src/indexer.rs` › a_failed_watch_stops_and_queues_a_quick_scan_that_watches_again | pass |
| REQ-003-07 | `crates/indexer/src/indexer.rs` › changes_during_a_scan_are_scanned_after_it | pass |
| REQ-003-08 | `crates/indexer/src/scan.rs` › quick_scans_only_relist_folders_whose_entries_changed, falls_back_to_a_full_scan_without_stored_folders | pass |
| REQ-003-09 | `crates/indexer/src/schedule.rs` › scans_at_startup_and_offline_libraries…, quick_checks_every_interval…; started by `crates/app/src/main.rs` (checked in the running app) | pass |
| REQ-003-10 | `crates/indexer/src/schedule.rs` › scans_at_startup_and_offline_libraries_again_and_again; `crates/indexer/src/indexer.rs` › stops_watching_an_offline_library_and_watches_it_again_after_a_reconnect | pass |
| REQ-003-11 | `crates/indexer/src/scan.rs` › req_003_11_waits_for_a_library_that_was_online_to_reconnect, req_003_11_an_offline_library_is_checked_once; `crates/indexer/src/reachability.rs` › req_003_11_probes_until_the_folder_answers_or_the_grace_is_over | pass |
