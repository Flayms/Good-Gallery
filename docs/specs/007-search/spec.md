# 007 · Search

**Status:** Implemented

## Goal

The user finds media items by the tags already embedded in their files, combined with file name, media kind, date, rating, library and folder, and orders the result as needed. Search stays fast for 500k media items, and the search survives opening and closing the viewer.

## Requirements

### REQ-007-01 · Include tags

- **Given** one or more included tags
- **When** the gallery is filtered
- **Then** only media items that carry every included tag, or one of its descendant tags, are shown (e.g. `Places` matches media items tagged `Places|France|Paris`)

### REQ-007-02 · Exclude tags

- **Given** one or more excluded tags
- **When** the gallery is filtered
- **Then** media items carrying any excluded tag, or one of its descendants, are hidden

### REQ-007-03 · Unknown tags

- **Given** a tag in the search that doesn't exist in the index (e.g. removed by a scan while it was in the search)
- **When** the gallery is filtered
- **Then** an unknown included tag matches nothing, and an unknown excluded tag is ignored

### REQ-007-04 · Tag suggestions

- **Given** the focused search bar
- **When** the user types
- **Then** up to 10 tags whose name starts with the input (case-, whitespace- and Unicode-insensitive) are suggested, most used first, each with its count of media items including descendants
- **And** with empty input, the most used tags are suggested. Tags already in the search and tags without media items aren't suggested

### REQ-007-05 · Edit tags in the search bar

- **Given** the search bar
- **When** the user picks a suggestion
- **Then** the tag is added as included, or as excluded if the input started with `-`, and the input is cleared while the suggestions stay open
- **And** clicking a tag chip switches it between included and excluded (excluded chips are struck through) and its × removes it. A tag is never included and excluded at the same time
- **And** Backspace in the empty input removes the file name filter, otherwise the last tag. Escape clears the input, or leaves the search bar if it is empty

### REQ-007-06 · File name filter

- **Given** text typed into the search bar (not starting with `-`)
- **When** the user picks _File name contains "<text>"_
- **Then** only media items whose file name contains the text, ignoring letter case, are shown, and the filter appears as a chip that replaces any earlier file name filter

### REQ-007-07 · Media kind

- **Given** _Filters_ in the toolbar (013)
- **When** the user picks _All_, _Photos_ or _Videos_
- **Then** the gallery shows all media items, only images, or only videos

### REQ-007-08 · Date range

- **Given** the _Date_ section under _Filters_
- **When** the user sets a start day, an end day or both
- **Then** only media items whose date taken (else date modified) falls on or between those local calendar days are shown
- **And** the section shows the range (e.g. "From 1 Jan 2024", "Until …", one day or a span), the date pickers don't offer an end before the start, and _Clear_ removes the range

### REQ-007-09 · Rating

- **Given** the _Rating_ section under _Filters_
- **When** the user selects any combination of _Unrated_ and 1–5 stars
- **Then** only media items with exactly one of the selected ratings are shown
- **And** the section shows "Any rating", "Unrated" or the selected stars (e.g. "3 and 5 stars"), and _Clear_ removes the filter

### REQ-007-10 · Library and folder

- **Given** a library or a folder of a library chosen in the sidebar (008)
- **When** the gallery is filtered
- **Then** only media items of that library, or of that folder and its subfolders, are shown
- **And** a folder without a library is ignored

### REQ-007-11 · Combined filters

- **Given** several filters at once
- **When** the gallery is filtered
- **Then** media items must match all of them

### REQ-007-12 · Sort order

- **Given** the sort controls
- **When** the user picks _Date taken_, _Date modified_, _Name_, _Full path_ or _Size_, and ascending or descending
- **Then** the gallery is ordered accordingly: date taken falls back to date modified, names compare without letter case, full path orders by library first
- **And** the default is date taken, newest first, and items with equal values keep a stable order

### REQ-007-13 · Search kept while viewing

- **Given** any combination of filters and sort order
- **When** the user opens and closes the viewer
- **Then** the same search is still applied

### REQ-007-14 · Search speed

- **Given** an index of 500k media items with about 2k tags and 1M tag links
- **When** the user searches with typical filter combinations
- **Then** the result is computed in under 50 ms (95th percentile), and tag suggestions in under 10 ms

## Out of scope

- OR combinations of tags, saved searches and search history.
- Full-text search in captions, titles or paths beyond the file name.
- Searching by GPS location, camera or any metadata not in 004.
- Filtering by duration, dimensions or file size ranges.

## Assumptions

- Tag names are unique regardless of hierarchy (REQ-004-08), so a tag in the search is identified by its name alone.
- Dates are compared in the computer's local time zone.

## Verification

| REQ        | Test                                                                                                                                                         | Result                                                                                       |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------- |
| REQ-007-01 | `crates/db/src/gallery.rs` › req_007_01_requires_all_included_tags_matching_descendants, req_007_01_common_tags_take_the_other_query_shapes_with_the_same_result; `crates/app/tests/search.rs` › req_007_04_req_007_05_suggests_tags_and_adds_them_as_included_or_excluded; `cargo xtask oracle` (30 searches against the reference) | pass |
| REQ-007-02 | `crates/db/src/gallery.rs` › req_007_02_excludes_tags_with_their_descendants; `crates/app/tests/search.rs` › req_007_04_req_007_05_suggests_tags_and_adds_them_as_included_or_excluded; `cargo xtask oracle` (30 searches against the reference) | pass |
| REQ-007-03 | `crates/db/src/gallery.rs` › req_007_03_matches_nothing_for_unknown_included_tags_and_ignores_unknown_exclusions; `cargo xtask oracle` (30 searches against the reference) | pass |
| REQ-007-04 | `crates/db/src/tags.rs` › req_007_04_matches_normalized_prefixes_ordered_by_usage, req_002_14_req_007_04_counts_descendants_and_follows_deletions; `crates/app/tests/search.rs` › req_007_04_req_007_05_suggests_tags_and_adds_them_as_included_or_excluded (snapshot `search_suggestions`); `cargo xtask oracle` (30 searches against the reference) | pass |
| REQ-007-05 | `crates/domain/src/search.rs` › req_007_05_req_008_06_adds_moves_and_removes_tags_case_insensitively; `crates/app/tests/search.rs` › req_007_04_req_007_05_suggests_tags_and_adds_them_as_included_or_excluded | pass (Escape clears the input or leaves it; checked in the app) |
| REQ-007-06 | `crates/db/src/gallery.rs` › req_007_06_req_007_11_matches_a_case_insensitive_file_name_substring_combined_with_other_filters; `crates/app/tests/search.rs` › req_007_06_filters_by_file_name_from_the_search_bar; `cargo xtask oracle` (30 searches against the reference) | pass |
| REQ-007-07 | `crates/db/src/gallery.rs` › req_007_07_req_007_10_falls_back_to_mtime_and_filters_by_kind_and_library; `crates/app/tests/search.rs` › req_007_07_req_007_09_req_013_08_filters_by_kind_and_rating_and_counts_active_filters; `cargo xtask oracle` (30 searches against the reference) | pass |
| REQ-007-08 | `crates/domain/src/search.rs` › req_007_08_turns_inclusive_local_days_into_a_half_open_range; `crates/db/src/gallery.rs` › req_007_08_filters_by_local_days_both_inclusive; `crates/app/tests/search.rs` › req_007_08_sets_a_date_range_from_the_filters; `cargo xtask oracle` (30 searches against the reference) | pass (the end is kept from the start by moving the other day) |
| REQ-007-09 | `crates/db/src/gallery.rs` › req_007_09_filters_by_exact_rating_0_meaning_unrated; `crates/app/tests/search.rs` › req_007_07_req_007_09_req_013_08_filters_by_kind_and_rating_and_counts_active_filters; `cargo xtask oracle` (30 searches against the reference) | pass |
| REQ-007-10 | `crates/domain/src/search.rs` › req_007_10_drops_the_folder_without_a_library; `crates/db/src/gallery.rs` › req_007_10_filters_by_folder_including_subfolders, req_007_07_req_007_10_falls_back_to_mtime_and_filters_by_kind_and_library; `crates/app/tests/search.rs` › req_008_01_req_008_02_req_007_10_limits_the_gallery_to_a_library_and_its_folders; `cargo xtask oracle` (30 searches against the reference) | pass |
| REQ-007-11 | `crates/db/src/gallery.rs` › req_007_06_req_007_11_matches_a_case_insensitive_file_name_substring_combined_with_other_filters; `cargo xtask oracle` (30 searches against the reference) | pass |
| REQ-007-12 | `crates/db/src/gallery.rs` › req_007_12_orders_by_date_taken_newest_first_with_the_id_as_tie_breaker, req_007_12_sorts_by_every_field_in_both_directions; `crates/app/tests/search.rs` › req_007_12_sorts_from_the_view_menu; `cargo xtask oracle` (30 searches against the reference) | pass |
| REQ-007-13 | — | no test (the search lives in the app state; the viewer comes in M8) |
| REQ-007-14 | `cargo xtask search-bench` (500k items, 2k tags, ≈ 1M links, release build, 2026-10-27) | no test (measured: searches matching up to a few thousand items 1–18 ms p95; a tag on 94k items 459 ms, a file name over all items 692 ms, all items 695 ms; suggestions 13.9 ms. Broad searches miss the budget, open issue in docs/progress.md) |
