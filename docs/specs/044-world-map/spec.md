# 044 · World map

**Status:** Implemented

## Goal

People want to see where they took their photos: a trip's route, the countries they have been to, the photos of one place. With positions in the index (spec 040), a map can show them without anything leaving the computer (#260).

## Requirements

### REQ-044-01 · The map

- **Given** media items with a GPS position (REQ-040-02) among those the search shows
- **When** the user clicks _Map_ at the top of Places
- **Then** a world map of land and country borders, bundled with the app, shows them as clusters: the photos close together on screen as one round thumbnail of the newest, with how many it holds
- **And** a cluster's tip and name say how many photos it holds and near which town ("2 photos near Munich", spec 042)
- **And** _Map_ shows in Places once any media item has a position; when the search shows none of them, the map says so

### REQ-044-02 · Zoom and pan

- **Given** the map
- **When** the user drags it, turns the wheel, pinches, double-clicks, or clicks _Zoom in_ or _Zoom out_
- **Then** it pans, or zooms around the pointer (the buttons around its center), from the whole world filling the page down to a town's streets
- **And** clusters split into smaller ones as it zooms in, and the map remembers where it was while the user is elsewhere

### REQ-044-03 · A place's photos

- **Given** a cluster on the map
- **When** the user clicks it
- **Then** the gallery shows its photos, with the rest of the search, and a chip in the filters bar ("2 photos from the map") whose × shows the whole search again

## Out of scope

- Street maps and satellite images, which would need map tiles from the internet (spec 011).
- Drawing a trip's route, heat maps.
- Setting a photo's position on the map.

## Assumptions

- Natural Earth's 1:50m land and country borders (public domain) are detailed enough down to a region; closer in, the clusters matter more than the coastline.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-044-01 | `crates/domain/src/map.rs` › req_044_01_projects_onto_web_mercators_square, req_044_01_req_044_02_clusters_split_as_the_map_zooms_in; `crates/db/src/gallery.rs` › req_044_01_lists_the_located_items_of_a_search_newest_first; `crates/app/src/map.rs` › req_044_01_reads_the_bundled_world, req_044_01_land_triangles_cover_each_polygon_once; `crates/app/tests/map.rs` › req_044_01_req_044_03_shows_clusters_and_opens_their_photos | pass (the thumbnails in clusters by looking at the running app's map) |
| REQ-044-02 | `crates/domain/src/map.rs` › req_044_02_zooms_around_the_pointer_and_pans, req_044_02_the_world_fills_the_view, req_044_01_req_044_02_clusters_split_as_the_map_zooms_in; `crates/app/tests/map.rs` › req_044_02_zooming_in_splits_clusters | pass (dragging, the wheel and pinching by hand) |
| REQ-044-03 | `crates/app/tests/map.rs` › req_044_01_req_044_03_shows_clusters_and_opens_their_photos | pass |
