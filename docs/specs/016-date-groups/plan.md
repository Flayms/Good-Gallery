# 016 · Date groups · Plan

Records the approach that was taken. Decisions that outlive this spec are in `docs/architecture.md`.

## Approach

- **Groups** (`domain::date_groups`): local calendar days of the sort date, through `chrono::Local`. `db::gallery::layout` returns the day groups of a date sort with the ids, so the layout places headers before any item is loaded.
- **Headers:** 40 px, bottom-aligned, with the day ("Monday, 26 October 2026") and the counts ("42 photos · 3 videos") in every view, including the List view as header rows (REQ-016-02, -05). Headers are accessible headings.
- **Choice** (REQ-016-03, -04): _Group by day_ in the _View_ menu, stored as `gallery.groupByDay` (`"off"` turns it off); disabled with "Only when sorted by date" for other sorts.
- **Closing the viewer** lands on the shown item: the gallery keeps it in view while the viewer is open (REQ-016-06).

## Tasks

- [x] Date groups and layout with headers (REQ-016-01, -05)
- [x] Headers in every view, _Group by day_ remembered (REQ-016-02, -03)
- [x] Disabled for other sorts (REQ-016-04)
- [x] Shown item kept in view (REQ-016-06)
