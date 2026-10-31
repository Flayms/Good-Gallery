# 007 · Search · Plan

Records the approach that was taken. Decisions that outlive this spec are in `docs/architecture.md`.

## Approach

- **Search state** (`domain::search::GallerySearch`): library, folder, media kind, file name, included and excluded tags (display names), date range (local days, both inclusive), ratings (0 = unrated), sort field and direction. Helpers: include / exclude / remove a tag (normalized comparison, never in both lists), active filter count, the day range as `[start of from, start of the day after to)` in local time (a day without a local midnight falls back to UTC midnight), folder ignored without a library. Kept in `AppState`, so the viewer never touches it (REQ-007-13).
- **Query** (`db::gallery::layout`): one `WHERE` built as SQL text with bound parameters: library, folder range on the generated `dir` column (`'a/' ≤ dir < 'a0'`), kind, file name through `instr(file_name_lower, lower(?))`, sort date range, ratings (`IS NULL` for 0). Sort columns have composite indexes with `id` as tie-breaker.
- **Tag conditions:** each tag expands to its subtree (recursive CTE, `UNION`, so parent cycles terminate); the estimate is the sum of `tags.media_count`. Up to 200,000 estimated matches the rarest tag drives (`media.id IN (…)`: rows are read in id order, then sorted, which beat walking the sort index at every size measured except a name sort over most of the index); beyond, the sort index is walked with a primary key probe per row (`EXISTS` with `+tag_id`). The unary `+` operators pin the query plans (see `docs/known-pitfalls.md`). Tag ids go into the SQL as numbers.
- **Suggestions** (`db::tags`): range scan on the unique `name_norm` index, counts summed over the subtree, unused tags left out.
- **Read connections** use memory-mapped reads (512 MB) and a 64 MB page cache: searches walk a sort index and look up each row, which with the default cache costs one read call per row (4–8× slower on 500k items).
- **Search bar** (`app::search_bar`): an egui `TextEdit` with chips for included tags, excluded tags (struck through) and the file name filter; suggestions in a foreground `Area` while focused (up to 10, `-` prefix excludes, _File name contains "…"_). Enter and Escape are handled in the frame the input gives up focus. Suggestions only count for the current input.
- **_Filters_ menu:** kind, date range (two `egui_extras` date pickers; moving one day past the other moves the other along), ratings, _Clear_ per section, _Clear filters_, count on the button. **Sort** in the _View_ menu as toggles (a combo box inside a popup closed the popup).
- **File name:** its matches come from the name index (`media.id IN (SELECT id FROM media INDEXED BY media_name_idx WHERE instr(file_name_lower, …))`), which stores `file_name_lower`; walking the rows computed the virtual column's `lower()` for each. With a library selected, `+media.root_id` keeps the library's date index from being walked instead (not with a folder, whose range needs `(root_id, dir)`).
- **Speed** (REQ-007-14): `cargo xtask search-bench` builds a synthetic index of 500k items, 2k tags and ≈ 1M links and prints 95th percentiles. Result (release, dates unrelated to insertion order, 2026-10-31): rare tag 0.5 ms, tag with 3k items 7 ms, two tags 7 ms, tag + exclusion 12 ms, rating 5 (30k items) 49 ms, videos of one year 63 ms, a tag root with 39k items 79 ms, a tag with 94k items 224 ms, file name over all items 65 ms, all items 773 ms; suggestions 12.3 ms. Before the query shapes above (2026-10-27 / same machine 2026-10-31): 94k tag 459 / 437 ms, tag root 255 / 228 ms, file name 692 / 771 ms.

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
- [ ] Meet REQ-007-14 for broad searches and suggestions. What is left is reading and sorting tens of thousands of rows per search; a covering index doesn't help a driven search (it reads rows by id). Likely an in-memory copy of the sort columns, filtered in Rust (Tier 3).
