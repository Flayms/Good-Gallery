# 004 · Metadata

**Status:** Approved

## Goal

Search and sorting rely on the tags, ratings and dates that photo tools (Lightroom, digiKam, darktable, Picasa, Windows Photos, cameras) already wrote into the files or their sidecars. Good Gallery reads them as they are, in all the common places, and never modifies a file.

## Requirements

### REQ-004-01 · Read-only

- **Given** any library
- **When** it is indexed, browsed or viewed
- **Then** no media file or sidecar is created, modified, renamed or deleted

### REQ-004-02 · Dimensions as displayed

- **Given** an image with an EXIF orientation that rotates it by 90° or 270° (orientations 5–8), or a video with a 90° or 270° rotation
- **When** it is indexed
- **Then** its width and height are stored as displayed, i.e. swapped
- **And** unknown or invalid dimensions are stored as unknown

### REQ-004-03 · Date taken

- **Given** a file with capture dates (`SubSecDateTimeOriginal`, `DateTimeOriginal`, `CreateDate`, `MediaCreateDate`, in this order of preference)
- **When** it is indexed
- **Then** the first valid date from the file becomes its date taken, else the first valid one from its sidecar
- **And** zeroed or pre-1970 dates (cameras without a clock) count as missing

### REQ-004-04 · Video duration

- **Given** a video
- **When** it is indexed
- **Then** its duration in seconds is stored. A missing or non-numeric duration counts as unknown

### REQ-004-05 · Rating

- **Given** a file with a rating in the file or its sidecar
- **When** it is indexed
- **Then** the first valid rating (file before sidecar) is stored, rounded to whole stars and capped at 5
- **And** ratings of 0 or below (unrated, rejected) are stored as unrated

### REQ-004-06 · Flat keywords

- **Given** keywords in `Keywords` (IPTC), `Subject` (XMP) or `XPKeywords` (Windows, `;`-separated)
- **When** the file is indexed
- **Then** each becomes a tag, deduplicated case-insensitively with the first spelling kept, empty entries dropped and numeric keywords (e.g. `2024`) kept as text

### REQ-004-07 · Tag hierarchies

- **Given** `HierarchicalSubject` entries such as `Places|Germany|Berlin`
- **When** the file is indexed
- **Then** each entry becomes a tag path. The media item is linked to its leaf, and the leaf is linked to its ancestors
- **And** flat keywords that also occur in one of the file's hierarchies are dropped (Lightroom writes every level as a flat keyword too)

### REQ-004-08 · One hierarchy per tag name

- **Given** the same tag name in different hierarchies across files (e.g. `Places|Berlin` and `Berlin|Places`)
- **When** they are indexed
- **Then** a tag name exists once (case-insensitive), the first parent seen for it wins, and links that would make a tag its own ancestor are ignored

### REQ-004-09 · Face regions become People tags

- **Given** names of people in `PersonInImage`, `RegionPersonDisplayName`, or `RegionName` with `RegionType` `Face`
- **When** the file is indexed
- **Then** each name becomes the tag `People|<name>`, without duplicating a name already in a keyword hierarchy or flat keyword
- **And** regions of other types (e.g. pets) are ignored

### REQ-004-10 · Sidecars

- **Given** a media file `IMG_1.jpg` with a sidecar `IMG_1.jpg.xmp` (darktable, digiKam) or `IMG_1.xmp` (Lightroom), in any letter case
- **When** it is indexed
- **Then** `IMG_1.jpg.xmp` is preferred over `IMG_1.xmp`. Its tags are merged with the file's, and its date and rating fill in where the file has none
- **And** a changed sidecar alone makes the file be read again (REQ-002-04)

### REQ-004-11 · Tag normalization

- **Given** tags that differ only in letter case, surrounding or repeated whitespace, or Unicode composition
- **When** they are indexed or searched
- **Then** they are the same tag, displayed with collapsed whitespace in the spelling first seen

## Out of scope

- Editing, adding or removing tags or ratings, and writing anything back to files.
- Automatic tagging (AI, face recognition, reverse geocoding of GPS data).
- GPS coordinates, camera and lens data, captions and titles.
- Color labels, flags and picks.

## Assumptions

- When metadata extraction changes, the metadata version is increased so already indexed files are read again (REQ-002-05).
- People and places are recognized by the names of their top-level tags (see 008), not by dedicated metadata fields. Only face regions get a `People` tag added by Good Gallery.

## Verification

| REQ        | Test                                                                                                                                                                | Result                                                                    |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| REQ-004-01 | `crates/indexer/tests/end_to_end.rs` › indexes_a_photo_with_its_sidecar_without_touching_files (exiftool) | pass |
| REQ-004-02 | `crates/media/src/metadata.rs` › swaps_dimensions_for_rotated_images_and_videos | pass |
| REQ-004-03 | `crates/media/src/metadata.rs` › prefers_embedded_capture_dates…, takes_the_first_valid_date…, uses_offset_tags…, infers_the_zone…, reads_zoneless_video_dates_as_utc, reads_zoneless_photo_dates_in_local_time | partial: zone from the GPS position (geo-tz) not ported, see `docs/architecture.md` |
| REQ-004-04 | `crates/media/src/metadata.rs` › reads_video_duration | pass |
| REQ-004-05 | `crates/media/src/metadata.rs` › normalizes_the_rating_falling_back_to_the_sidecar | pass |
| REQ-004-06 | `crates/media/src/metadata.rs` › merges_keyword_fields…, accepts_numeric_keywords | pass |
| REQ-004-07 | `crates/media/src/metadata.rs` › keeps_hierarchies…; `crates/db/src/writer.rs` › stores_hierarchies_links_the_leaf_and_refuses_cycles | pass |
| REQ-004-08 | `crates/db/src/writer.rs` › stores_hierarchies…refuses_cycles, the_first_parent_seen_wins | pass |
| REQ-004-09 | `crates/media/src/metadata.rs` › adds_face_region_names…, does_not_duplicate_a_face_region… | pass |
| REQ-004-10 | `crates/domain/src/media.rs` › prefers_the_full_name_sidecar…; `crates/media/src/metadata.rs` › reads_dimensions_from_the_file_and_tags_from_its_sidecar; `crates/indexer/src/scan.rs` › rescans_incrementally; `tests/end_to_end.rs` | pass |
| REQ-004-11 | `crates/domain/src/tags.rs` › trims_collapses…, applies_nfkc…; `crates/db/src/writer.rs` › one_tag_per_normalized_name_with_the_first_spelling | pass |
