# 007 · Search · Plan

Records the approach that was taken. Decisions that outlive this spec are in `docs/architecture.md`.

## Approach

- **Search state** (`domain::search::GallerySearch`): library, folder, media kind, file name, included and excluded tags (display names), date range (local days, both inclusive), ratings (0 = unrated), sort field and direction. Helpers: include / exclude / remove a tag (normalized comparison, never in both lists), active filter count, the day range as `[start of from, start of the day after to)` in local time (a day without a local midnight falls back to UTC midnight), folder ignored without a library. Kept in `AppState`, so the viewer never touches it (REQ-007-13).
- **Query** (`db::gallery::layout`): one `WHERE` built as SQL text with bound parameters: library, folder range on the generated `dir` column (`'a/' ≤ dir < 'a0'`), kind, file name through `instr(file_name_lower, lower(?))`, sort date range, ratings (`IS NULL` for 0). Sort columns have composite indexes with `id` as tie-breaker.
- **Tag conditions:** each tag expands to its subtree (recursive CTE, `UNION`, so parent cycles terminate); the estimate is the sum of `tags.media_count`. Up to 5,000 estimated matches the rarest tag drives (`media.id IN (…)`); up to 100,000 the sort index is walked against materialized id lists (`+media.id IN (…)`); beyond, a primary key probe per row (`EXISTS` with `+tag_id`). The unary `+` operators pin the query plans (see `docs/known-pitfalls.md`). Tag ids go into the SQL as numbers.
- **Suggestions** (`db::tags`): range scan on the unique `name_norm` index, counts summed over the subtree, unused tags left out.
- **Read connections** use memory-mapped reads (512 MB) and a 64 MB page cache: searches walk a sort index and look up each row, which with the default cache costs one read call per row (4–8× slower on 500k items).
- **Search bar** (`app::search_bar`): an egui `TextEdit` with chips for included tags, excluded tags (struck through) and the file name filter; suggestions in a foreground `Area` while focused (up to 10, `-` prefix excludes, _File name contains "…"_). Enter and Escape are handled in the frame the input gives up focus. Suggestions only count for the current input.
- **_Filters_ menu:** kind, date range (two `egui_extras` date pickers; moving one day past the other moves the other along), ratings, _Clear_ per section, _Clear filters_, count on the button. **Sort** in the _View_ menu as toggles (a combo box inside a popup closed the popup).
- **Speed** (REQ-007-14): `cargo xtask search-bench` builds a synthetic index of 500k items, 2k tags and ≈ 1M links and prints 95th percentiles. Result (release, dates unrelated to insertion order): rare tag 1.0 ms, tag with 3k items 12 ms, two tags 13 ms, tag + exclusion 18 ms, rating 5 (30k items) 71 ms, a tag root with 39k items 255 ms, a tag with 94k items 459 ms, file name over all items 692 ms, all items 695 ms; suggestions 13.9 ms.

## Differences

- Ratings are shown in words ("3 and 5 stars"): the app's font has no `★`, and egui no longer bundles an emoji font.

## Tasks

- [x] Search state, tag helpers, filter count, day range (REQ-007-05, -08, -10, -13)
- [x] Filters and sorts in the layout query; tag conditions with plan pins (REQ-007-01 … -03, -06 … -12)
- [x] Suggestions (REQ-007-04)
- [x] Search bar with chips and suggestions (REQ-007-04 … -06)
- [x] _Filters_ menu and sort (REQ-007-07 … -09, -12)
- [x] `cargo xtask search-bench` (REQ-007-14, measured)
- [ ] Meet REQ-007-14 for broad searches and suggestions: a covering index for the sort orders, or rows stored in sort order. A schema change with a migration; it no longer has to stay readable by older versions.
