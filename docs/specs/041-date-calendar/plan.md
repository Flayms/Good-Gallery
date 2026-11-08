# 041 · Date calendar · Plan

## Approach

- **Pure logic (`domain::calendar`):** the weeks a month's grid shows (Monday first, whole weeks), what a click on a day does to the range (`pick`, with the anchor of a range being picked), the quick ranges for a given today, and a year's counts per month from date groups. Tests pin today.
- **Data:** a new loader request, `Calendar { search, from, to }`, runs `db::gallery::layout` with the gallery's search, its date range replaced by the grid's weeks (or the shown year) and sorted by date taken. Its date groups (spec 016) give each day's photos, videos and first item, whose thumbnail key comes from `db::thumbs::thumb_source`. The gallery keeps the last answer and asks again when the search, the month or the year view changes.
- **Thumbnails:** the texture streamer takes one wanted list per frame and the gallery's tiles set it. The calendar hands its days' thumbnails to the gallery (`want_calendar_thumbnails`), which puts them first in its next request, or asks for them at once when no tiles asked for theirs in that frame (a range without matches shows none).
- **UI (`toolbar.rs`, `calendar.rs`):** replaces the two `egui_extras` date pickers in the filters menu, which gets two columns: the date and its calendar at the left, the media type, rating and actions at the right, so it fits an 800 × 600 window (REQ-013-08). `jiff` and egui_extras' `datepicker` go with the pickers. The quick ranges sit above the month. Day cells are buttons named with the day and its count; the year view is a 3 × 4 grid of month buttons. The calendar's month, year view and anchor live in the gallery, so they survive the menu closing.

## Tasks

- [x] `domain::calendar`: grid, pick, quick ranges, months of a year (REQ-041-01, -03, -04)
- [x] Loader request and the gallery's calendar data (REQ-041-02, -03)
- [x] Calendar thumbnails first in the gallery's request (REQ-041-02)
- [x] The calendar in the filters menu, replacing the date pickers (REQ-041-01 … -04)

## Decisions

### 2026-11-08 · Every day can be picked

**Decided:** days without photos are dimmed but stay clickable, as agreed in the session: a range often starts or ends on a day without photos, such as the day a trip started.
**Rejected:** days without photos disabled.

### 2026-11-08 · Two clicks, no drag

**Decided:** the first click picks a day (a range of one day, applied at once), the second makes it a range; whichever day is earlier becomes the start.
**Rejected:** dragging across days, which is hard in a menu that scrolls and impossible with a keyboard; separate _From_ and _To_ calendars, which take twice the room.

### 2026-11-08 · The counts ignore the range itself

**Decided:** the days show what the search shows apart from its date range, so narrowing to a week still shows the photos of the days around it.
**Rejected:** counting only within the range, which would leave every other day dimmed once a range is set.
