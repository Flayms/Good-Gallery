# 044 · World map · Plan

## Approach

- **Data:** `cargo xtask map` downloads Natural Earth's 1:50m land and land borders (GeoJSON), triangulates the land with `earcut` (egui fills only convex shapes) and writes `crates/app/data/world.deflate` (700 KB): polygons with bounds, vertices, triangles and rings, then border lines. `app::map` inflates and projects it once.
- **`domain::map`** (pure): Web Mercator, `MapView` (center on the world square, zoom; pan, zoom around a point, fit to the page), and `clusters` at a whole zoom level (`cluster_level`): a grid counted from the world's corner, so clusters don't change while panning or zooming within the level, then neighbors closer than the spacing merged. The gallery keeps the clusters until the points or the level change. Points come newest first, so a cluster's first id is its thumbnail.
- **Data for the map:** `db::gallery::located_items` runs the gallery's filters with `latitude IS NOT NULL`. The loader answers `Request::Located` with the search's points and `Request::ThumbKeys` with the thumbnail keys of the clusters shown, each asked for once.
- **Page:** `Page::Map`, led to from a _Map_ row at the top of Places. It draws ocean, land (one mesh of the polygons in view) and borders, then the clusters with their newest thumbnails through the gallery's texture streamer (`want_thumbnails`, as the People page does).
- **Clicked:** `AppState::show_map_photos` sets the search's `media` to the cluster's ids and shows the gallery; the filters bar shows them as a chip.

## Tasks

- [x] `cargo xtask map` and the bundled data (REQ-044-01)
- [x] `domain::map`: projection, view, clusters (REQ-044-01, -02)
- [x] Located items and thumbnail keys through the loader (REQ-044-01)
- [x] The map page, its row in Places, zoom and pan (REQ-044-01, -02)
- [x] A cluster's photos in the gallery, the map chip (REQ-044-03)

## Decisions

### 2026-11-08 · Bundled vector data, no tiles

**Decided:** Natural Earth's land and borders, bundled and drawn by the app, as refined in the session.
**Rejected:** map tiles from a tile server, which sends where the user looks (spec 011) and needs the network. Bundled raster tiles, which take hundreds of megabytes for a few zoom levels.

### 2026-11-08 · A cluster opens in the gallery

**Decided:** a click shows a cluster's photos in the gallery, with the search's other filters, through `GallerySearch.media`, which _Unconfirmed faces_ already uses (REQ-032-26).
**Rejected:** an area filter (latitude and longitude bounds), which matches photos outside the cluster at its edges and needs a filter of its own in search and smart albums.

### 2026-11-09 · Clusters grouped at whole zoom levels, merged when too close

**Decided:** group at the zoom level below the view's and merge clusters closer than their spacing at that level; zooming in only moves them apart, so they never overlap. All are one size, the count in the badge (#293, #295, #296).
**Rejected:** grouping at the view's own zoom, whose grid moves with every wheel step and regroups the photos each frame. Sizes by count, which made neighbors look unrelated.
