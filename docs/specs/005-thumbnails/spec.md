# 005 · Thumbnails

**Status:** Approved

## Goal

Tiles appear instantly and stay sharp, even for libraries on slow network shares and while those shares are offline. Thumbnails are rendered once, cached locally within a size limit the user controls, and the tiles currently on screen are always rendered first.

## Requirements

### REQ-005-01 · Thumbnail sizes

- **Given** a media item
- **When** its thumbnails are rendered
- **Then** WebP thumbnails 400 and 800 px wide are created, never larger than the original, at most 3 times as tall as wide, together with a placeholder

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
| REQ-005-01 | — | no test |
| REQ-005-02 | — | no test |
| REQ-005-03 | — | no test |
| REQ-005-04 | — | no test |
| REQ-005-05 | — | no test |
| REQ-005-06 | — | no test |
| REQ-005-07 | — | no test |
| REQ-005-08 | — | no test |
| REQ-005-09 | — | no test |
| REQ-005-10 | — | no test |
| REQ-005-11 | — | no test |
| REQ-005-12 | — | no test |
| REQ-005-13 | — | no test |
| REQ-005-14 | — | no test |
| REQ-005-15 | — | no test |
