# M07 · Search and sidebar

**Status:** Approved (2026-10-27, self-approved: the user asked to work through the milestones without their input)
**Specs:** 007, 008 (and REQ-006-11 tag suggestions, REQ-016-04 the toggle for other sorts)

Too large for one part. Split:

- **M7a · Search model and queries:** the search state and its rules, every query behind search and sidebar, result sets compared with the reference, search speed measured.
- **M7b · Search bar, filters and sidebar:** the UI on top of it.

## Approach

### M7a · Search model and queries

**Search state (`domain::search`).** `GallerySearch` as the reference's URL state: library, folder, media kind, file name, included and excluded tags (display names), date range (local days, both inclusive), ratings (0 = unrated), sort field and direction. Helpers ported with their tests: `with_tag` / `without_tag` / `tag_mode` (normalized comparison, never in both lists), `active_filter_count`, the day range as `[start of from, start of the day after to)` in local time, folder ignored without a library. `SortBy` moves from `db::gallery` to `domain::search`.

**Trees (`domain::folders`, `domain::tag_tree`).** `build_folder_tree` from folders with direct counts (intermediate folders added, natural order `2` before `10` ignoring case), `folder_ancestors`; `build_tag_tree` from a category's flat list.

**Queries (`db::search`, `db::tags`).** A port of the reference's `media.layout` filters into `db::gallery::layout`: library, folder range on `dir` (`'a/' ≤ dir < 'a0'`), kind, file name substring through `instr(file_name_lower, lower(?))`, sort date range, ratings (`IS NULL` for 0), and the tag conditions of `tag-search.ts` with its plan pins kept exactly (`+media.id`, `+tag_id`) and its thresholds (`DRIVING_MAX` 5,000, `MATERIALIZE_MAX` 100,000). The SQL is built as text with bound parameters (no query builder). Tag queries: `autocomplete(prefix, limit)` (range scan on `name_norm`, counts with descendants, unused tags left out), `category(people|places)`, `popular(limit)` without category tags, `rating_counts`, `library_folders(library)`.

**Oracle (exit criterion).** `cargo xtask oracle` also runs a list of searches on the fixture index, with the reference's `appRouter` (`media.layout`, `tags.autocomplete`, `tags.category`, `tags.popular`, `media.ratingCounts`, `libraries.folders`) through `xtask/oracle/reference-search.ts`, and with this app's queries, and compares the results by path / tag name. The fixture library gets a few more tagged and rated files so every filter has something to match.

**Speed (REQ-007-14).** `cargo xtask search-bench` builds a synthetic index of 500k media items, 2k tags (with hierarchies) and ≈ 1M links in a temporary folder, runs typical searches (no filter, a common tag, a rare tag, two tags, tag + exclusion, name, rating, date range, sorts) many times each and prints the 95th percentiles; suggestions likewise. Budget: searches < 50 ms, suggestions < 10 ms.

### M7b · Search bar, filters and sidebar

**Search bar** in the toolbar (left of _Filters_ and _View_): chips for included tags, excluded tags (struck through, minus) and the file name filter; a text input; suggestions under it while focused (up to 10, `-` prefix excludes, _File name contains "…"_ entry), keyboard as REQ-007-05 (Backspace in the empty input removes the name filter, else the last tag; Escape clears or leaves). Clicking a chip toggles include / exclude, its × removes it.

**Filters menu** (REQ-007-07 … -09, REQ-013-08): a button with the number of active filters; kind (_All_, _Photos_, _Videos_), date range with two date pickers (`egui_extras`' date picker; the end can't be before the start) and its summary ("From 1 Jan 2024", …), ratings (_Unrated_, 1–5 stars as toggles, summary "Any rating" / "3★, 5★"), _Clear_ per section and _Clear filters_.

**Sort** in the _View_ menu (REQ-007-12): field and direction. _Group by day_ disabled with its hint for other sorts (REQ-016-04).

**Sidebar** (`app::sidebar`, Recess, left of the gallery, 256 px): _All libraries_ and each library (path as tooltip, offline badge with the unplugged icon, spinner while scanned), the selected library's folder tree (chevrons, tooltips with path and count, ancestors of the active folder expanded), _People_ and _Places_ trees (10 top-level tags, _Show all (n)_ / _Show less_), _Ratings_ (5 … Unrated with counts, toggling the rating filter), _Tags_ (15 most used), tags clickable to include / remove (included highlighted, excluded with a minus). Collapsed groups are stored as `sidebar.collapsed` in the `settings` table. Sidebar data reloads with the gallery (REQ-006-11). Settings moves to the sidebar's bottom as in the reference.

**Search state.** Kept in `AppState` (the viewer of M8 won't touch it, REQ-007-13), applied to the gallery with a reload; the suggestions query runs on the gallery's loader thread.

## Results (M7a)

- `cargo xtask oracle`: 30 searches (every filter, combinations, every sort, unknown tags), 6 suggestion prefixes and the 5 sidebar lists give the same results as the reference's router, both run on the reference's index of the fixture library. `cargo xtask oracle-search --index <reference index>` runs this part alone.
- `cargo xtask search-bench` (release, 500k items, 2k tags, 995,814 links, dates in random order to the ids): p95 rare tag 1.0 ms, tag with 3k items 12 ms, two tags 13 ms, tag + exclusion 18 ms, tag by name 6.5 ms, rating 5 (30k) 71 ms, videos of a year 52 ms, tag root with 39k items 255 ms, tag with 94k items 459 ms, file name over all items 692 ms, all items 695 ms; suggestions 13.9 ms. Before the reader pragmas (memory-mapped reads, 64 MB cache) the broad ones took 1.2–4.5 s; the reference's unconfigured connection takes ≈ 5 s for all items on the same file.
- **Budget missed for broad searches** (REQ-007-14: 50 ms, suggestions 10 ms). Their cost is one table lookup per matching row while walking the sort index. A covering index or rows stored in sort order would change the shared schema, which needs the user's decision (open issue).

## Reference

- `src/main/db/tag-search.ts` (plan pins, thresholds), `src/main/db/tag-categories.ts`, `src/main/trpc/routers/{media,tags,libraries}.ts`, `src/main/trpc/router.test.ts` (layout, tags, ratings, name, folders tests ported).
- `src/renderer/src/lib/{search,folders,tag-tree}.ts` (+ tests), `components/{tag-search,filters-menu,date-range-filter,rating-filter,app-sidebar,folder-tree,tag-tree,sidebar-ratings,sidebar-tags,popular-tags,collapsible-sidebar-group}.tsx`, `components/view-menu.tsx` (sort).

## Tasks

### M7a

- [x] 1. `domain::search`: search state, tag helpers, filter count, day range (REQ-007-05, -08, -10)
- [x] 2. `domain::folders` and `domain::tag_tree` (REQ-008-02, -03)
- [x] 3. `db::gallery::layout` filters and sorts; tag conditions with the plan pins (REQ-007-01 … -03, -06 … -12)
- [x] 4. `db::tags`: autocomplete, category, popular, rating counts, library folders (REQ-007-04, REQ-008-02 … -05)
- [x] 5. Oracle: searches and sidebar queries against the reference
- [x] 6. `cargo xtask search-bench` (REQ-007-14)
- [x] 7. M7a docs, review, commit

### M7b

- [ ] 8. Search bar with chips and suggestions (REQ-007-04 … -06)
- [ ] 9. Filters menu (REQ-007-07 … -09) and sort in the View menu (REQ-007-12, REQ-016-04)
- [ ] 10. Sidebar: libraries and folder tree (REQ-008-01, -02, REQ-007-10)
- [ ] 11. Sidebar: People, Places, Ratings, Tags, clicking tags, collapsed groups (REQ-008-03 … -07)
- [ ] 12. M7b docs, review, commit

## Exit criteria

From `docs/rewrite/plan.md`: result sets match the reference for a list of queries on the fixture library; REQs pass. Added: search speed measured on 500k items.
