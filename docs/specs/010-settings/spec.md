# 010 · Settings

**Status:** Implemented

## Goal

The user adapts indexing and the thumbnail cache to their hardware and network: how often to check for missed changes, how hard to load a NAS, and how much disk space thumbnails may use. Changes take effect immediately, without restarting the app. Library management on the same page is covered by 001.

## Requirements

### REQ-010-01 · Persisted settings with defaults

- **Given** a fresh installation
- **When** the user opens the Settings page
- **Then** the quick check interval is "Every hour", parallel file reads is 4 and the thumbnail cache limit is 5 GB
- **And** changed values are kept across restarts

### REQ-010-02 · Valid values only

- **Given** a settings change outside the allowed ranges (thumbnail cache 0.5–1024 GB, quick check interval 0 minutes to 7 days, parallel file reads 1–16)
- **When** it is saved
- **Then** it is rejected with an error and nothing changes
- **And** stored values that are no longer valid (e.g. after an update narrowed a range) fall back to their defaults

### REQ-010-03 · Check for missed changes

- **Given** the _Check for missed changes_ setting
- **When** the user picks _Only at startup_, every 15 or 30 minutes, every hour, every 3 or 6 hours, or daily
- **Then** scheduled quick scans (REQ-003-09) follow the new interval from now on, without an immediate scan
- **And** a stored value that isn't one of these options is still shown and selectable

### REQ-010-04 · Parallel file reads

- **Given** the _Parallel file reads_ slider (1–16)
- **When** the user releases it at a new value
- **Then** reading metadata and rendering thumbnails use that many parallel reads from then on. A running scan keeps its value and the next scan uses the new one

### REQ-010-05 · Thumbnail cache limit

- **Given** the _Maximum size_ setting (1, 2, 5, 10, 20, 50 or 100 GB, plus a stored non-preset value)
- **When** the user picks a size
- **Then** the new limit applies right away. If the cache is larger than the new limit, it is reduced as in REQ-005-11, and the shown usage updates

### REQ-010-06 · Thumbnail cache usage and clearing

- **Given** the Settings page
- **When** it is open
- **Then** the current size of the thumbnail cache is shown and refreshed every 10 seconds ("…" while unknown, "unknown" if it can't be determined)
- **And** _Clear_ asks for confirmation, then deletes all cached thumbnails (except ones being read at that moment), shows the new size and confirms with a toast. Thumbnails are rendered again when next shown

## Out of scope

- Choosing the location of the index or the thumbnail cache.
- Settings for language or keyboard shortcuts. The appearance (light, dark or the system's) is in 020, the sidebar's groups in 008.
- Per-library settings.
- Importing or exporting settings.

## Assumptions

- Settings are per user and stored with the index in the user profile.
- One app instance runs at a time per user profile.

## Verification

| REQ        | Test                                                                                                                                                         | Result                                                       |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------ |
| REQ-010-01 | `crates/db/src/settings.rs` › req_010_01_starts_with_defaults_and_persists_updates; `crates/app/tests/settings.rs` › req_010_01_shows_the_defaults_and_keeps_changes_across_restarts | pass |
| REQ-010-02 | `crates/db/src/settings.rs` › req_010_02_rejects_out_of_range_values, req_010_02_falls_back_to_defaults_for_stored_values_that_no_longer_validate; `crates/app/tests/settings.rs` › req_010_02_rejects_invalid_values_with_an_error_and_changes_nothing | pass |
| REQ-010-03 | `crates/app/tests/settings.rs` › req_010_03_follows_the_new_interval_and_offers_a_stored_odd_value | pass (the schedule restarts its timer in `Scheduler::set_interval`) |
| REQ-010-04 | `crates/indexer/src/thumbnails.rs` › req_010_04_renders_with_as_many_threads_as_set; `crates/app/tests/settings.rs` › req_010_04_saves_parallel_reads_when_the_slider_is_released | pass (scans read the new value at their start, exiftool's pool resizes) |
| REQ-010-05 | `crates/indexer/src/thumb_cache.rs` › evicts_down_to_a_lowered_cap; `crates/app/tests/settings.rs` › req_010_05_applies_a_new_cache_limit_and_shows_the_new_usage | pass |
| REQ-010-06 | `crates/indexer/src/thumb_cache.rs` › req_010_06_clears_all_thumbnails; `crates/app/tests/settings.rs` › req_010_06_shows_usage_and_clears_after_confirmation, req_010_06_says_unknown_without_a_thumbnail_cache | pass |
