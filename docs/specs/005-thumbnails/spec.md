# 005 · Thumbnails

**Status:** Implemented

## Goal

Tiles appear instantly and stay sharp, even for libraries on slow network shares and while those shares are offline. Thumbnails are rendered once, cached locally within a size limit the user controls, and the tiles currently on screen are always rendered first.

## Requirements

### REQ-005-01 · Thumbnail sizes

- **Given** a media item
- **When** its thumbnails are rendered
- **Then** WebP thumbnails 400 and 800 px wide are created, never larger than the original and at most 2400 px (3 × the largest width) tall, together with a placeholder

### REQ-005-02 · Sharp tiles

- **Given** a tile of a certain width on a display with a certain pixel density
- **When** it is shown
- **Then** it loads the smallest thumbnail at least as wide as the tile's physical pixels, else the largest

### REQ-005-03 · Correct orientation

- **Given** an image with any EXIF orientation (1–8)
- **When** its thumbnail is rendered
- **Then** the thumbnail is upright, the same as the image would be when auto-oriented

### REQ-005-04 · Embedded previews

- **Given** a JPEG with an embedded preview at least as wide as the largest thumbnail (or the original, if smaller) and with the original's aspect ratio (±2 %)
- **When** its thumbnail is rendered
- **Then** the preview is used instead of decoding the whole file, oriented like the original
- **And** smaller, differently shaped or invalid previews are ignored and the full image is decoded, partially if the file is truncated

### REQ-005-05 · Video thumbnails

- **Given** a video
- **When** its thumbnail is rendered
- **Then** it shows the frame at 10 % of the video's duration (the first frame if the duration is unknown)

### REQ-005-06 · Placeholder while loading

- **Given** a tile whose thumbnail isn't loaded yet
- **When** it is shown
- **Then** it shows the item's placeholder (blurred colors), and fades in the thumbnail once loaded
- **And** a failed thumbnail shows a "no image" icon instead

### REQ-005-07 · Visible tiles first

- **Given** tiles on screen whose thumbnails don't exist yet
- **When** they request their thumbnails
- **Then** those are rendered right away, ahead of background rendering, the most recent request first (earlier ones have likely scrolled away)

### REQ-005-08 · Background rendering

- **Given** media items without thumbnails
- **When** the app runs (also while a scan is still writing new media items)
- **Then** their thumbnails and placeholders are rendered in the background, newest date taken first
- **And** media items of offline libraries are skipped until the library is back

### REQ-005-09 · Failed thumbnails

- **Given** a file in a reachable library that can't be rendered (corrupt, unsupported codec)
- **When** rendering fails
- **Then** the item is marked as failed and isn't retried until the file changes
- **And** if the library is unreachable instead, it becomes `offline` and the item is rendered later

### REQ-005-10 · Stale thumbnails

- **Given** an indexed media item with a thumbnail
- **When** the file's content changes (size or modification time)
- **Then** a new thumbnail and placeholder are rendered and the old ones are never shown again
- **And** a change of only the sidecar keeps the existing thumbnail

### REQ-005-11 · Thumbnail cache size limit

- **Given** the thumbnail cache's size limit (setting, default 5 GB)
- **When** the cache grows beyond it, or the limit is lowered
- **Then** the least recently viewed thumbnails are removed until the cache is at 90 % of the limit
- **And** showing a thumbnail counts as a view (recorded at most once per hour per thumbnail)

### REQ-005-12 · Full cache

- **Given** a full thumbnail cache
- **When** background rendering continues
- **Then** it only stores placeholders and no thumbnail files, so it never pushes out thumbnails that were viewed
- **And** thumbnails requested by visible tiles are still stored

### REQ-005-13 · No broken files after a crash

- **Given** the app crashed or was killed while writing thumbnails
- **When** thumbnails are served or the app starts again
- **Then** a partially written thumbnail is never served, and leftover temporary files are removed at startup

### REQ-005-14 · Browsing offline libraries

- **Given** an offline library with cached thumbnails
- **When** the user browses it
- **Then** the cached thumbnails are shown
- **And** items without a cached thumbnail show the "no image" icon over their placeholder, if one was rendered before

### REQ-005-15 · Existing thumbnail cache

- **Given** thumbnails cached by Good Gallery's Electron version
- **When** the app shows those media items
- **Then** the cached thumbnails are shown without rendering them again, as long as the files didn't change

## Out of scope

- Thumbnails for formats beyond the supported media types (002).
- Animated thumbnails for GIFs or videos.
- Choosing thumbnail sizes or quality.
- Storing the thumbnail cache somewhere other than the user profile.

## Assumptions

- The thumbnail cache is on a local disk. Rendering reads from the library once per file version.
- A cleared or evicted thumbnail is simply rendered again when next needed (on-demand rendering), as long as the library is reachable.

## Verification

| REQ        | Test                                                                                                                                                                 | Result                                                                     |
| ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| REQ-005-01 | `crates/media/src/thumbnail.rs` › req_005_01_renders_every_width_as_webp_without_upscaling_plus_a_thumbhash, req_005_01_fits_tall_strips_into_2400_pixels; `crates/indexer/tests/end_to_end.rs` › req_005_08_renders_thumbnails_of_scanned_media; `cargo xtask oracle` (thumbnails) | pass |
| REQ-005-02 | `crates/domain/src/thumbs.rs` › req_005_02_picks_the_smallest_thumbnail_covering_the_physical_width | pass (the gallery picks each tile's width with it) |
| REQ-005-03 | `crates/media/src/thumbnail.rs` › req_005_03_matches_sharps_auto_orientation_for_every_orientation, req_005_03_turns_jpegs_upright_by_their_exif_orientation; `cargo xtask oracle` (portrait.jpg) | pass |
| REQ-005-04 | `crates/media/src/thumbnail.rs` › req_005_04_uses_a_large_enough_embedded_preview_with_the_original_orientation, req_005_04_ignores_small_differently_shaped_or_invalid_previews, req_005_04_decodes_a_truncated_jpeg_partially, req_005_04_shrinks_large_jpegs_while_decoding; `crates/media/src/exiftool.rs` › req_005_04_reads_embedded_images | pass |
| REQ-005-05 | `crates/media/src/ffmpeg.rs` › req_005_05_shows_the_frame_at_a_tenth_of_the_duration | pass |
| REQ-005-06 | `crates/indexer/src/thumbnails.rs` › req_005_08_req_005_06_renders_pending_media_in_the_background_newest_first | partial: placeholders stored; tiles draw the ThumbHash, fade in and show the "no image" icon (checked in the app, M6a), without an automated tile test since thumbnails need the GPU streamer |
| REQ-005-07 | `crates/indexer/src/thumbnails.rs` › req_005_07_runs_the_latest_request_first | pass |
| REQ-005-08 | `crates/indexer/src/thumbnails.rs` › req_005_08_req_005_06_renders_pending_media_in_the_background_newest_first, req_005_08_picks_up_media_written_later, req_005_08_renders_more_than_one_batch; `crates/db/src/thumbs.rs` › req_005_08_lists_pending_media_newest_first_without_offline_libraries; `crates/indexer/tests/end_to_end.rs` › req_005_08_renders_thumbnails_of_scanned_media | pass |
| REQ-005-09 | `crates/indexer/src/thumbnails.rs` › req_005_09_req_005_14_marks_unrenderable_media_as_failed_but_only_while_the_library_is_reachable, req_005_09_a_panic_marks_the_item_failed_and_rendering_goes_on, req_005_09_skips_queued_items_of_a_library_found_offline; `crates/db/src/thumbs.rs` › stores_the_outcome_unless_the_file_changed | pass |
| REQ-005-10 | `crates/db/src/writer.rs` › req_005_10_rewriting_resets_the_thumbnail_only_when_the_content_changed; `crates/indexer/src/thumb_cache.rs` › req_005_10_path_changes_with_every_part_of_the_key; `crates/db/src/thumbs.rs` › stores_the_outcome_unless_the_file_changed | pass |
| REQ-005-11 | `crates/indexer/src/thumb_cache.rs` › req_005_11_evicts_least_recently_viewed_thumbnails_once_over_the_cap, evicts_down_to_a_lowered_cap, req_005_11_reading_counts_as_a_view_at_most_once_an_hour, req_005_11_counts_replaced_and_concurrent_writes_once | pass (M6's texture loader must read through `ThumbCache::read`) |
| REQ-005-12 | `crates/indexer/src/thumbnails.rs` › req_005_12_stores_only_requested_thumbnails_once_the_cache_is_full | pass |
| REQ-005-13 | `crates/indexer/src/thumb_cache.rs` › req_005_13_measures_existing_thumbnails_and_drops_leftover_temp_files, req_005_13_writes_leave_no_temp_files | pass |
| REQ-005-14 | `crates/indexer/src/thumbnails.rs` › req_005_09_req_005_14_marks_unrenderable_media_as_failed_but_only_while_the_library_is_reachable | partial: cached thumbnails are read without touching the library; tiles whose render failed show the "no image" icon over the placeholder (checked in the app, M6a) |
| REQ-005-15 | `crates/indexer/src/thumb_cache.rs` › req_005_15_matches_the_electron_versions_cache_paths; `cargo xtask thumbs` on a copy of the real index and cache (17,618 of 20,295 found, as with Node's hashing) | pass |
