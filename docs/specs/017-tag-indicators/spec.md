# 017 · Tag indicators

**Status:** Implemented

## Goal

Users who rate and tag their photos in other apps want to see at a glance which media items are already rated, placed or tagged with people, and which still need work. Small icons over each tile show this while browsing, without opening the viewer. The indicators cover part of the photo, so they are off until the user turns them on, and the user picks which of them are shown.

## Requirements

### REQ-017-01 · Indicators on tiles

- **Given** tag indicators are on
- **When** a tile is shown in the Masonry, Justified or Grid view (REQ-006-01)
- **Then** its bottom-left corner shows, in this order and only for the indicators the user picked:
  - _Rating_: as many filled stars as the rating (1–5). Nothing for unrated media items
  - _Places_: a pin when the media item has a tag of the _Places_ category
  - _People_: a person when the media item has a tag of the _People_ category
  - _Tags_: a tag when the media item has a tag outside both categories
- **And** a tile with none of the picked indicators shows nothing, and the indicators don't cover the video badge

### REQ-017-02 · Indicator names

- **Given** an indicator on a tile
- **When** the user hovers it or a screen reader reads the tile
- **Then** it is named "Rated N out of 5", "Has places", "Has people" or "Has tags"

### REQ-017-03 · Choose indicators

- **Given** the gallery
- **When** the user switches _Tag indicators_ under _View_ in the toolbar, or picks _Rating_, _Places_, _People_ and _Tags_ below it
- **Then** the tiles show or hide the indicators right away. Picking indicators is disabled while _Tag indicators_ is off and the picks are kept for when it is on again
- **And** both choices are remembered across sessions. Tag indicators are off until the user turns them on, and all four are picked by default

## Out of scope

- Indicators in the List view (it has a rating column) and in the viewer's filmstrip (015).
- Showing tag names or counts on tiles, and indicators for other metadata (date taken, location from GPS).
- Editing ratings or tags from a tile (the app is read-only).

## Assumptions

- Categories are decided as in the sidebar (REQ-008-03, glossary _Tag category_). A tag hierarchy whose root is a category root counts for that category only.
- Indicators come with the tile data of the media items near the viewport (REQ-006-08), so they cost no extra request while scrolling.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-017-01 | `crates/domain/src/view.rs` › req_017_01_req_017_02_shows_the_picked_indicators_in_a_fixed_order_with_stars_and_names, req_017_01_leaves_out_what_the_media_item_lacks; `crates/db/src/gallery.rs` › req_017_01_loads_items_with_the_categories_of_their_tags; `crates/app/tests/gallery.rs` › req_017_01_req_017_03_shows_the_picked_tag_indicators_once_turned_on | pass (the badge leaves the video badge's corner free; checked in the app) |
| REQ-017-02 | `crates/domain/src/view.rs` › req_017_01_req_017_02_shows_the_picked_indicators_in_a_fixed_order_with_stars_and_names; `crates/app/tests/gallery.rs` › req_017_01_req_017_03_shows_the_picked_tag_indicators_once_turned_on | pass (names in the tile's accessible name and as tooltips) |
| REQ-017-03 | `crates/domain/src/view.rs` › req_017_03_shows_nothing_while_tag_indicators_are_off, req_017_03_is_off_with_all_indicators_picked_until_the_user_changes_it, req_017_03_round_trips_keeping_the_picks_while_off_and_dropping_unknown_ones, req_017_03_toggles_picks_in_their_fixed_order; `crates/app/tests/gallery.rs` › req_017_01_req_017_03_shows_the_picked_tag_indicators_once_turned_on | pass |
