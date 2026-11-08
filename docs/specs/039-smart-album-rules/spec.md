# 039 · Smart album rules

**Status:** Implemented

## Goal

A smart album (spec 034) keeps a search as the filters bar builds it: all of its filters must match. Albums people actually want are often "this year's videos longer than a minute", "unrated photos from the last 30 days" or "anything tagged Beach or Sea". The user builds such an album from a list of conditions, matching all or any of them, and its dates can move with time. Everything the editor offers is already in the index, so no file is read again.

## Requirements

### REQ-039-01 · Edit a smart album's rules

- **Given** a smart album
- **When** the user chooses _Edit…_ in its context menu
- **Then** an editor opens with its name, _Match all_ or _Match any_ of its conditions, its conditions as a list, and its sort order
- **And** _Save as smart album…_ (REQ-034-01) opens the same editor, filled with the current search's filters as conditions
- **And** the browser version offers it too, as it has smart albums

### REQ-039-02 · Conditions

- **Given** the editor
- **When** the user adds a condition
- **Then** they can choose from:
  - _Tag_ is or is not (a tag matches its descendants, REQ-007-01), _has People_, _has Places_, _has no tags_
  - _File name_ contains
  - _Kind_ is photo or video
  - _Rating_ is one of, or at least
  - _Library_ is, _Folder_ is (with subfolders)
  - _Date taken_ or _Date modified_: between two days, or in the last N days, weeks, months or years
  - _Size_, _Duration_ or _Megapixels_: at least or at most
  - _Orientation_ is portrait, landscape or square
  - _Shows_ words, by content search (REQ-036-05), at most one per album
  - _Camera_ and _Lens_ is or is not, _has a location_ or _has no location_ (REQ-040-05)
- **And** each condition can be removed, and an incomplete one (no value yet) is marked and keeps _Save_ off

### REQ-039-03 · All or any

- **Given** conditions
- **When** _Match all_ is chosen
- **Then** an item is in the album if every condition matches it
- **When** _Match any_ is chosen
- **Then** an item is in the album if at least one matches it
- **And** without conditions the album holds every item

### REQ-039-04 · Relative dates move

- **Given** a smart album with _in the last N …_
- **When** a new day starts while the app runs, or the app starts on a later day
- **Then** its count and what it shows follow the new day, in local time

### REQ-039-05 · Opening an album with rules

- **Given** a smart album with conditions the filters bar can't show (_Match any_, a relative date, size, duration, megapixels, orientation, _has People_, _has Places_, _has no tags_, _at least_ ratings)
- **When** the user opens it
- **Then** the gallery shows it, and the filters bar shows a chip with the album's name instead of its filters; removing the chip leaves the album
- **And** an album the filters bar can show opens as it does now (REQ-034-03)

### REQ-039-06 · Old albums

- **Given** smart albums saved before this
- **When** the app starts
- **Then** they open, count and show as before, and _Edit…_ shows their filters as _Match all_ conditions

### REQ-039-07 · Fast

- **Given** an index of 500,000 items
- **When** a smart album of five conditions with _Match any_ is counted or opened
- **Then** it stays within the budgets of a search (REQ-007-14): 50 ms for results of up to 20,000 items, 1 s for larger ones, measured by `cargo xtask search-bench`

## Out of scope

- Nested groups of all and any: hard to read, and hard to keep fast at 500,000 items.
- Camera, lens and GPS conditions (#204): they need new columns and reading every file again.
- The new conditions in the filters bar itself.
- Albums of chosen items (static albums), and exporting albums.

## Assumptions

- A day starts at local midnight, as in date groups (spec 016).
- Megapixels are width × height as stored (as displayed, REQ-004-02).

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-039-01 | `crates/domain/src/album_rules.rs` › req_039_01_req_039_06_a_search_becomes_rules_and_back; `crates/app/tests/smart_albums.rs` › req_039_01_req_039_03_req_039_05_saves_rules_matching_any (snapshot `album_editor`), req_039_01_req_039_06_edits_an_album | pass |
| REQ-039-02 | `crates/domain/src/album_rules.rs` › req_039_02_incomplete_conditions; `crates/db/src/smart_albums.rs` › req_039_02_req_039_03_rule_albums_match_all_or_any; `crates/app/tests/smart_albums.rs` › req_039_01_req_039_06_edits_an_album | pass |
| REQ-039-03 | `crates/db/src/smart_albums.rs` › req_039_02_req_039_03_rule_albums_match_all_or_any; `crates/app/tests/smart_albums.rs` › req_039_01_req_039_03_req_039_05_saves_rules_matching_any | pass |
| REQ-039-04 | `crates/domain/src/album_rules.rs` › req_039_04_relative_dates_end_today; `crates/db/src/smart_albums.rs` › req_039_02_req_039_03_rule_albums_match_all_or_any (the last 30 days) | partial: refreshing when a new day starts while the app runs (`AppState::notice_new_day`) has no test |
| REQ-039-05 | `crates/domain/src/album_rules.rs` › req_039_05_rules_the_filters_bar_cant_show; `crates/app/tests/smart_albums.rs` › req_039_01_req_039_03_req_039_05_saves_rules_matching_any | pass |
| REQ-039-06 | `crates/domain/src/album_rules.rs` › req_039_06_stores_and_reads_rules, req_039_01_req_039_06_a_search_becomes_rules_and_back; `crates/app/tests/smart_albums.rs` › req_039_01_req_039_06_edits_an_album; the tests of spec 034 pass unchanged | pass |
| REQ-039-07 | `cargo xtask search-bench` (500k items, 2k tags, ≈ 1M links; dev build with optimized dependencies, 2026-11-07) | no test (measured by the benchmark, p95: any of five conditions, 38,634 matches, 765 ms of 1,000; all of three, 353 matches, 5 ms of 50; the last 5 years and videos, 5,246 matches, 11 ms of 50) |
