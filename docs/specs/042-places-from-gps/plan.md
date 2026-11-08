# 042 · Places from GPS · Plan

## Approach

- **Data:** `cargo xtask places` downloads GeoNames' `cities5000`, `admin1CodesASCII` and `countryInfo` and writes `crates/domain/data/places.deflate` (about 70,000 towns, 1 MB): tab-separated countries, regions and towns, deflated. `domain::places` inflates it once (`miniz_oxide`, pure Rust, so the browser version has it too) into a one-degree grid and finds the nearest town within 100 km in the position's and the neighbouring cells, across the antimeridian.
- **Index:** the places are tag links like any other, so Places, search, counts and smart albums need no change. Migration `0021` adds `media_tags.from_position`. `IndexWriter` links `Places › Country › Region › Town` with `from_position = 1` when the file and sidecar have no tag under a Places root. Writing a tag reads the file's own keywords (`media::tagging`), so a place from the position never reaches a file; after a Places tag is written, the file is read again and the link goes.
- **Read again:** `METADATA_VERSION` 5. Version 4 (spec 040) isn't released yet, so people updating from 0.6 read their libraries again once, not twice.
- **Info panel:** `DetailTag.from_position` marks the chip ("From its position", no remove button). `MediaDetail.position_place` is the position's town when the item's own Places tags don't name it, for the hint.

## Tasks

- [x] `cargo xtask places` and the bundled data (REQ-042-01)
- [x] `domain::places::place_at` (REQ-042-01, -05)
- [x] Migration, writer, `detail` (REQ-042-01, -02, -03)
- [x] `METADATA_VERSION` 5 (REQ-042-04)
- [x] Info panel chip and hint (REQ-042-02, -03)
- [x] GeoNames' attribution in the README and both third-party notices

## Decisions

### 2026-11-08 · Bundled towns, nearest one

**Decided:** the nearest town of 5,000 people or more within 100 km, from bundled GeoNames data, as refined in the session.
**Rejected:** an online reverse geocoder, which would send every position away (spec 011). Country borders, which would name the country right even far from towns but add tens of megabytes. Smaller towns (`cities1000`, 150,000 places, 2 MB), which name villages people don't think of as the place.

### 2026-11-08 · Tags win, positions never written

**Decided:** a Places tag of the item's own replaces the position's place, and the position's place is never written, as refined in the session. The info panel points out a disagreement instead of resolving it.
**Rejected:** showing both, which puts a photo in two places in the sidebar.

### 2026-11-08 · Places from positions as tag links

**Decided:** links in `media_tags`, marked `from_position`, so every query on tags works for them.
**Rejected:** a table of its own, which every Places query, count and smart album condition would have to join as well.
