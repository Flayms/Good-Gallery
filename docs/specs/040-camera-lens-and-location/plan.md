# 040 · Camera, lens and location · Plan

## Approach

- **Extraction (pure, `meta::rules`):** `MediaMetadata` gets `camera`, `lens` and `location: Option<Location>` (`domain::media::Location { latitude, longitude }`). `extract_metadata` reads `Make` / `Model`, `LensModel` / `Lens` / `LensID` and the signed `GPSLatitude` / `GPSLongitude`, the file before its sidecar. The camera name rule (drop the make when the model starts with its first word) lives in `domain::media::camera_name`.
- **Browser reader (`meta::tiff`):** reads `Model` (IFD0 0x0110), `LensModel` (EXIF 0xa434) and the GPS IFD's latitude and longitude with their references, and writes the signed composite as exiftool `-n` does, so `rules` needs no second path.
- **Index:** migration `0020_media_camera_location` adds `camera`, `lens`, `latitude`, `longitude` to `media`, with indexes on `camera` and `lens` so the editor's lists and the conditions don't scan. `IndexWriter` writes them.
- **Read again:** `METADATA_VERSION` goes to 4; the scheduler already rescans libraries whose rows are behind it (REQ-004-01).
- **Info panel:** `db::media::detail` returns the three; `info.rs` shows them after _Size_. `format::coordinates` formats the position. The row is _Coordinates_, as _Location_ already names the file's path.
- **Conditions:** `Condition::Camera { name, is }`, `Condition::Lens { name, is }`, `Condition::HasLocation(bool)`, stored as `{"type": "camera", ...}`. Older versions of the app leave out conditions they can't read, so there an album with them matches more. `rule_condition` matches the exact name through `param`. The cameras and lenses the editor offers come with the sidebar's data (`db::media::cameras`, `db::media::lenses`), loaded on the loader's thread.

## Tasks

- [x] `Location`, `camera_name`, the new `MediaMetadata` fields, `METADATA_VERSION` 4 (REQ-040-01, -02, -03)
- [x] `extract_metadata` reads them (REQ-040-01, -02)
- [x] The TIFF reader reads them (REQ-040-06)
- [x] Migration, writer, `detail` (REQ-040-01, -02)
- [x] Info panel rows (REQ-040-04)
- [x] Conditions, storage, SQL, the editor's lists (REQ-040-05)

## Decisions

### 2026-11-08 · One spec for the three

**Decided:** camera, lens and location share a spec because they share the re-read of every library, which should happen once. The location is stored now, though only the info panel and _Has a location_ use it, so #259 and #260 don't need a second re-read.
**Rejected:** a column per make and model. Nobody searches by make alone, and one name is what the panel and the editor show.

### 2026-11-08 · The file before its sidecar

**Decided:** as for dates (REQ-004-03), the file's camera, lens and position come first. A sidecar fills in what the file lacks, which covers raw files whose position was added in Lightroom.
**Rejected:** the sidecar first. A position written into a sidecar while the file has its own is rare, and dates already work the other way.
