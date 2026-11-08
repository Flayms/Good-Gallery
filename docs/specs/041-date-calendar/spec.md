# 041 · Date calendar

**Status:** Implemented

## Goal

Picking a date range under _Filters_ meant typing or stepping two plain date pickers, without knowing on which days there are photos. A calendar that shows a photo on each day the library has one lets people find the day of a trip or a party by looking, and pick a range with two clicks (#263).

## Requirements

### REQ-041-01 · One calendar for the range

- **Given** the _Date_ section under _Filters_ (REQ-007-08)
- **When** it is shown
- **Then** it shows one month as a calendar, weeks from Monday, opening at the month of the range's start, else the current month
- **And** a click on a day with no range being picked sets the range to that day alone, and a click on a second day makes the range run between the two, whichever comes first; the next click starts a new range
- **And** every day can be picked, also days without photos
- **And** the range's first and last days are marked, the days between them too, and today has a ring; weekends look like other days

### REQ-041-02 · Photos on the days

- **Given** the calendar
- **When** the library has media items on a day, among those the search shows apart from its date range
- **Then** the day shows a thumbnail of the first of them, and its tip and name say how many photos and videos there are ("3 July 2025, 4 photos")
- **And** days without such items are dimmed

### REQ-041-03 · Months and years

- **Given** the calendar
- **When** the user clicks the arrows beside the month's name
- **Then** it shows the month before or after
- **And** a click on the month's name shows the twelve months of its year with how many items each has, months without any dimmed, with arrows to the year before or after; a click on a month shows that month

### REQ-041-04 · Quick ranges

- **Given** the calendar
- **When** the user clicks _This month_, _Last 30 days_, _This year_ or _Last year_
- **Then** the range becomes that span of days, up to today where it ends in the future, and the calendar shows its first month

## Out of scope

- Times of day.
- Picking several separate ranges.
- A date range for date modified; smart albums have it (REQ-039-02).

## Assumptions

- Days are local calendar days, as for the date range (REQ-007-08) and date groups (spec 016).

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-041-01 | `crates/domain/src/calendar.rs` › req_041_01_shows_whole_weeks_from_monday, req_041_01_two_clicks_make_a_range_either_way_round; `crates/app/tests/search.rs` › req_041_01_req_041_02_picks_a_range_on_a_calendar_of_the_photos | pass (the rings and fills by the `date_calendar` snapshot) |
| REQ-041-02 | `crates/app/tests/search.rs` › req_041_01_req_041_02_picks_a_range_on_a_calendar_of_the_photos, req_041_02_shows_the_first_photo_of_each_day | pass |
| REQ-041-03 | `crates/domain/src/calendar.rs` › req_041_03_counts_a_years_months; `crates/app/tests/search.rs` › req_041_03_req_041_04_steps_through_years_and_quick_ranges | pass |
| REQ-041-04 | `crates/domain/src/calendar.rs` › req_041_04_quick_ranges_end_today_at_the_latest; `crates/app/tests/search.rs` › req_041_03_req_041_04_steps_through_years_and_quick_ranges | pass |
