# 005 · Thumbnails · Plan

Records the approach that was taken. Decisions that outlive this spec are in `docs/architecture.md`.

## Approach

- **Widths** (`domain::thumbs`): `THUMB_WIDTHS = [400, 800]`; `pick_thumb_width(points, pixels_per_point)` picks the smallest that covers the tile's physical width (REQ-005-02).
- **Rendering** (`media::thumbnail::Renderer`): one decode per file, then every width and the placeholder from it.
  - JPEGs decode through `jpeg-decoder` with DCT scaling (shrink-on-load: the smallest scale still covering the 800 × 2400 box), truncated JPEGs partially. PNG, GIF (first frame) and WebP through `image`, with a decode limit of 268 402 689 pixels. Orientation from EXIF (REQ-005-03).
  - AVIF and video frames come from ffmpeg (`-ss <10 % of the duration> … -frames:v 1 -vf scale='min(800,iw)':-2 -f image2pipe -c:v png pipe:1`), which applies rotation itself; `image` would need dav1d (a C library with a meson build) for AVIF. 60 s timeout. Found like exiftool: `GOOD_GALLERY_FFMPEG`, `ffmpeg\ffmpeg.exe` next to the app, `PATH`.
  - Embedded previews (REQ-005-04): for `.jpg` / `.jpeg`, exiftool's `PreviewImage` replaces the full decode when it decodes, is at least `min(800, upright width)` wide and its aspect ratio is within 2 % of the original's.
  - Resize with Lanczos3 (`fast_image_resize`, alpha premultiplied) into 800 × 2400 without enlarging, then each width; WebP quality 80 through libwebp; ThumbHash of the image fitted into 100 × 100, base64 with padding.
- **Cache** (`indexer::thumb_cache::ThumbCache`): `<data>/thumbs/<h0h1>/<h2h3>/<sha1>.webp` with `sha1` over `[rootId, relPath, size, mtime, width]` serialized as `JSON.stringify` does (`serde_json` matches it for integers and strings; a test pins hashes computed with Node), the layout existing caches use (REQ-005-15). Size measured on a thread at startup, leftover `*.tmp` removed (REQ-005-13); writes are atomic (temp file + rename, retried while Defender holds the file); eviction by file mtime down to 90 % of the limit on its own thread (REQ-005-11); reading bumps the mtime at most once an hour.
- **Service** (`indexer::thumbnails::ThumbnailService`): `io_concurrency` render threads on one priority queue. Requests come first, newest first (REQ-005-07); background batches of 100 pending items (newest first, offline libraries skipped) are queued whenever the queue is shorter than the number of threads (REQ-005-08). The indexer kicks it after each written batch and scan. A failure marks the item `error` only while its library is reachable, else the library goes offline (REQ-005-09). Files are stored when the item was requested or the cache has room (REQ-005-12). Each render runs under `catch_unwind`. Requests are answered through a callback, which the gallery's texture streamer waits for. Own write connection; SQLite's busy timeout serializes it with the scan.

## Spec changes and deviations

- **REQ-005-01 corrected:** thumbnails fit into 800 × 2400 px and are never cropped (a 1000 × 10 000 strip becomes 240 × 2400); the tile crops.
- **No ICC profiles:** colors aren't converted through embedded ICC profiles, so wide-gamut photos may come out slightly less saturated.
- **Failed items** aren't requested again by the gallery; the service renders whatever it is asked.

## Tasks

- [x] Widths and `pick_thumb_width` (REQ-005-02)
- [x] Cache key and path, pinned against Node's hashing (REQ-005-10, -15)
- [x] `ThumbCache`: measure, temp cleanup, atomic writes, eviction, lowered limit, clear, read with view clock (REQ-005-11, -13, REQ-010-05, -06)
- [x] Decode, orientation, resize, WebP, ThumbHash (REQ-005-01, -03)
- [x] JPEG shrink-on-load, truncated files, embedded previews (REQ-005-04)
- [x] Video frames and AVIF through ffmpeg (REQ-005-05)
- [x] `ThumbnailService`: priority queue, background feeding, failures, full cache (REQ-005-07, -08, -09, -12)
- [ ] Automated tile tests for placeholders and failed items (REQ-005-06, -14; need the GPU texture streamer)
- [ ] Apply ICC profiles, if real photos show it matters
