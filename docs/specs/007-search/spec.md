# 007 · Search

**Status:** Approved

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
- **And** the section shows "Any rating", "Unrated" or the selected stars (e.g. "3★, 5★"), and _Clear_ removes the filter

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
| REQ-007-01 | — | no test |
| REQ-007-02 | — | no test |
| REQ-007-03 | — | no test |
| REQ-007-04 | — | no test |
| REQ-007-05 | — | no test |
| REQ-007-06 | — | no test |
| REQ-007-07 | — | no test |
| REQ-007-08 | — | no test |
| REQ-007-09 | — | no test |
| REQ-007-10 | — | no test |
| REQ-007-11 | — | no test |
| REQ-007-12 | — | no test |
| REQ-007-13 | — | no test |
| REQ-007-14 | — | no test |
