# 007 · Search · Plan

Records the approach that was taken. Decisions that outlive this spec are in `docs/architecture.md`.

## Approach

- **Search state** (`domain::search::GallerySearch`): library, folder, media kind, file name, included and excluded tags (display names), date range (local days, both inclusive), ratings (0 = unrated), sort field and direction. Helpers: include / exclude / remove a tag (normalized comparison, never in both lists), active filter count, the day range as `[start of from, start of the day after to)` in local time (a day without a local midnight falls back to UTC midnight), folder ignored without a library. Kept in `AppState`, so the viewer never touches it (REQ-007-13).
- **Query** (`db::gallery::layout`): one `WHERE` built as SQL text with bound parameters: library, folder range on the generated `dir` column (`'a/' ≤ dir < 'a0'`), kind, file name through `instr(file_name_lower, lower(?))`, sort date range, ratings (`IS NULL` for 0). Sort columns have composite indexes with `id` as tie-breaker.
- **Tag conditions:** each tag expands to its subtree (recursive CTE, `UNION`, so parent cycles terminate); the estimate is the sum of `tags.media_count`. Up to 200,000 estimated matches the rarest tag drives (`media.id IN (…)`: rows are read in id order, then sorted, which beat walking the sort index at every size measured except a name sort over most of the index); beyond, the sort index is walked with a primary key probe per row (`EXISTS` with `+tag_id`). The unary `+` operators pin the query plans (see `docs/known-pitfalls.md`). Tag ids go into the SQL as numbers.
- **Suggestions** (`db::tags`): range scan on the unique `name_norm` index for the matching tags; the whole `tags` table (2k rows) is read once and their subtrees are summed in Rust, each tag once (a parent cycle ends), unused tags left out. The recursive query that summed them in SQL took 12 ms for all tags.
- **Kind** (REQ-007-14): `media_kind_date_idx (kind, sort_date, id)` (migration 0013), so a kind filter, with or without a date range, is a range scan in date order instead of checking the kind of every row in the range (videos of one year: 85 → 5 ms).
- **Read connections** use memory-mapped reads (512 MB) and a 64 MB page cache: searches walk a sort index and look up each row, which with the default cache costs one read call per row (4–8× slower on 500k items).
- **Search bar** (`app::search_bar`): an egui `TextEdit` with chips for included tags, excluded tags (struck through) and the file name filter; suggestions in a foreground `Area` while focused (up to 10, `-` prefix excludes, _File name contains "…"_). Enter and Escape are handled in the frame the input gives up focus. Suggestions only count for the current input.
- **_Filters_ menu:** kind, date range (two `egui_extras` date pickers; moving one day past the other moves the other along), ratings, _Clear_ per section, _Clear filters_, count on the button. **Sort** in the _View_ menu as toggles (a combo box inside a popup closed the popup).
- **File name:** its matches come from the name index (`media.id IN (SELECT id FROM media INDEXED BY media_name_idx WHERE instr(file_name_lower, …))`), which stores `file_name_lower`; walking the rows computed the virtual column's `lower()` for each. With a library selected, `+media.root_id` keeps the library's date index from being walked instead (not with a folder, whose range needs `(root_id, dir)`).
- **Speed** (REQ-007-14): `cargo xtask search-bench` builds a synthetic index of 500k items, 2k tags and ≈ 1M links and prints the 95th percentile of each search against its budget: 50 ms for results of up to 20,000 items, 1 s for larger ones and file names. Result (release, 2026-11-01): rare tag 0.5 ms, tag with 3k items 6 ms, two tags 9 ms, tag + exclusion 13 ms, videos of one year 5 ms, rating 5 (30k items) 42 ms, a tag root with 39k items 107 ms, a tag with 94k items 180 ms, file name over all items 90 ms, all items 776 ms; suggestions 3.9 ms. Earlier (2026-10-27 / 2026-10-31): 94k tag 459 / 224 ms, file name 692 / 65 ms, videos of one year 77 / 63 ms, suggestions 13.9 / 12.3 ms.
- **Why broad results have their own budget:** SQLite costs ≈ 1.5 µs per result row (reading it and sorting), so 94k rows can't come back in 50 ms; only a copy of the sort columns in memory, kept in step with every index write, could. Searches run on the gallery's loader thread and the window keeps drawing, so a broad search taking a few hundred ms is a short wait, not a stall; the budget was changed instead (#13).

## Differences

- Ratings are shown in words ("3 and 5 stars"): the app's font has no `★`, and egui no longer bundles an emoji font.

## Tasks

- [x] Search state, tag helpers, filter count, day range (REQ-007-05, -08, -10, -13)
- [x] Filters and sorts in the layout query; tag conditions with plan pins (REQ-007-01 … -03, -06 … -12)
- [x] Suggestions (REQ-007-04)
- [x] Search bar with chips and suggestions (REQ-007-04 … -06)
- [x] _Filters_ menu and sort (REQ-007-07 … -09, -12)
- [x] `cargo xtask search-bench` (REQ-007-14, measured)
- [x] Cheaper query shapes for broad tags and file names (REQ-007-14, #13)
- [x] Suggestions summed in Rust, kind index, budget for broad results (REQ-007-14, #13)
