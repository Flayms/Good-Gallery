# 042 · Places from GPS

**Status:** Implemented

## Goal

Places in the sidebar come from location tags only (REQ-008-03), so photos with a GPS position but no such tag are missing from it. Most phones and many cameras record the position. Naming the town nearest to it fills Places without anyone tagging, offline. A place tagged by hand always wins over the position (#259).

## Requirements

### REQ-042-01 · Places from the position

- **Given** a media item with a GPS position (REQ-040-02) and no Places tag of its own
- **When** it is indexed
- **Then** it counts under _Places › Country › Region › Town_ for the nearest town of 5,000 people or more within 100 km, from data bundled with the app, without going online
- **And** that place shows in the sidebar's Places, filters the gallery and counts like a Places tag (REQ-008-03, REQ-007-01)
- **And** a position farther than 100 km from every such town names no place

### REQ-042-02 · Tags win

- **Given** a media item with a Places tag of its own and a GPS position
- **When** it is indexed
- **Then** it counts under its own Places tags only
- **And** when the position's town isn't among them, the info panel says so under _Places_ ("Its position is near Munich, Bavaria, Germany")

### REQ-042-03 · Never written into files

- **Given** a place named from the position
- **When** the info panel shows it
- **Then** its chip says it comes from the position and has no button to remove it, and adding a Places tag by hand replaces it
- **And** the place is never written into the file or its sidecar

### REQ-042-04 · Libraries indexed before

- **Given** a library indexed before places were named from positions
- **When** the app starts
- **Then** the indexer reads its files again in the background (REQ-004-01), once

### REQ-042-05 · Fast

- **Given** a library of many photos with positions
- **When** it is indexed
- **Then** naming their places takes under 2 seconds per 20,000 photos, on top of reading them

### REQ-042-06 · One name per country

- **Given** Places tags of the user's own that name a country in another language than the bundled one (e.g. _Deutschland_ for _Germany_)
- **When** media items are indexed, in any order
- **Then** the places named from positions in that country take the user's name, so its photos are under one place, not under _Germany_ and _Deutschland_ apart
- **And** when the user's tags name it in several languages, the most used name wins; the tags in the files stay as they are

## Out of scope

- A map (#260).
- Writing a place from the position into the file on request.
- Names in the system's language; the bundled names are English where GeoNames has one, unless the user's own tags name the country (REQ-042-06). Regions and towns keep GeoNames' names.
- Places below towns of 5,000 people (villages, landmarks).

## Assumptions

- GeoNames' cities of 5,000 people or more, with their first-level regions and countries (CC BY 4.0), name the places; `cargo xtask places` rebuilds the bundled file from them.
- Unicode CLDR's country names in its 95 fully covered languages tell which tags name the same country; `cargo xtask countries` rebuilds them.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-042-01 | `crates/domain/src/places.rs` › req_042_01_names_the_nearest_town_with_its_region_and_country, req_042_01_far_from_every_town_names_no_place; `crates/db/src/writer.rs` › req_042_01_req_042_02_req_042_03_names_places_from_positions_unless_tagged | pass |
| REQ-042-02 | `crates/db/src/writer.rs` › req_042_01_req_042_02_req_042_03_names_places_from_positions_unless_tagged; `crates/app/tests/viewer.rs` › req_042_02_req_042_03_shows_places_from_the_position | pass |
| REQ-042-03 | `crates/db/src/writer.rs` › req_042_01_req_042_02_req_042_03_names_places_from_positions_unless_tagged; `crates/app/tests/viewer.rs` › req_042_02_req_042_03_shows_places_from_the_position | pass (never written: tagging reads the file's own keywords, `media::tagging`) |
| REQ-042-04 | `crates/indexer/src/scan.rs` › req_042_04_rereads_libraries_indexed_before_places_from_positions | pass |
| REQ-042-05 | `crates/domain/src/places.rs` › req_042_05_a_library_of_positions_is_named_quickly | pass |
| REQ-042-06 | `crates/domain/src/places.rs` › req_042_06_knows_a_country_by_its_name_in_any_language; `crates/db/src/writer.rs` › req_042_06_places_from_positions_take_the_users_country_name | pass |
