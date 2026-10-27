# M05 · Thumbnails

**Status:** Done (approved 2026-10-27, self-approved: the user asked to work through the milestones without their input; done 2026-10-27)
**Specs:** 005 (REQ-005-06 and -14 only their data side: the tiles that show placeholders come in M6)

One part: ≈ 400 lines of reference code (`thumbnail.ts`, `thumb-cache.ts`, `thumbnails.ts`) plus their tests, the oracle extension and app wiring. Estimated ≈ 2 000 lines, 12 tasks.

## Approach

**Rendering (`media::thumbnail`).** One decode per file, then every width and the placeholder from it, as `renderThumbnails` does:

- *Decode.* JPEGs through `jpeg-decoder` with DCT scaling (shrink-on-load, what libvips does for sharp): the smallest scale that stays at least as large as the 800 × 2400 box, so a 24 MP photo decodes at ¼ instead of full size. Truncated JPEGs decode partially (the missing part comes out as decoder fill, as with sharp's `failOn: 'none'`). PNG, GIF (first frame) and WebP through the `image` crate. Orientation from the file's EXIF (`image::metadata::Orientation::from_exif_chunk` on the JPEG's APP1, the `image` decoders' `orientation()` otherwise), applied with `DynamicImage::apply_orientation` (REQ-005-03, the reference's pinned 2 × 3 test ported with sharp's expected pixels).
- *AVIF and video through ffmpeg.* The `image` crate decodes AVIF only through dav1d (a C library with a meson build), so AVIF images take the same path as video frames: `ffmpeg -hide_banner -loglevel error -nostdin -ss <t> -i <file> -frames:v 1 -vf scale='min(800,iw)':-2 -f image2pipe -c:v png pipe:1` (the reference's arguments; `t` = 10 % of the duration, 0 for images and unknown durations), PNG decoded with `image`. ffmpeg applies rotation (display matrix, AVIF `irot` / `imir`) itself. 60 s timeout, then the process is killed. Arguments are passed as an argument vector, the path always absolute (REQ-011-09). Found like exiftool: `GOOD_GALLERY_FFMPEG`, `ffmpeg\ffmpeg.exe` next to the app, `PATH`. The process joins the app's job object, so it ends with the app.
- *Embedded previews (REQ-005-04).* For `.jpg` / `.jpeg`, exiftool extracts `PreviewImage` (`-charset filename=utf8 -json -b -PreviewImage <file>`, a `base64:` value, as exiftool-vendored's `BinaryToBufferTask`). It is used when it decodes, is at least `min(800, original's upright width)` wide (upright: swapped for orientations 5–8) and its aspect ratio is within 2 % of the original's; it gets the original's orientation applied. The original's size and orientation come from the JPEG header only.
- *Resize.* `fast_image_resize` with Lanczos3 (sharp's default kernel) into the box 800 × 2400 (`fit: inside`, never enlarged), then each width (400, 800) from that, never enlarged (REQ-005-01). Images with alpha keep it.
- *Encode.* WebP quality 80 through libwebp (`webp` crate, already used by the spike), which uses libwebp's defaults like sharp (method 4).
- *Placeholder.* ThumbHash of the 800-wide image resized to fit 100 × 100 (RGBA), with the `thumbhash` crate (Evan Wallace's own Rust port of the JavaScript package the reference uses), base64 with padding like `Buffer.toString('base64')`.
- *Colors.* sharp converts to sRGB through the embedded ICC profile; this port doesn't read ICC profiles (CMYK JPEGs are converted by the decoder). Wide-gamut photos may look slightly less saturated than the reference's thumbnails. Recorded as a deviation; revisit if the oracle or real photos show it matters.

**Cache (`indexer::thumb_cache`).** The reference's layout unchanged, so its cache is read as is (REQ-005-15): `<data>/thumbs/<h0h1>/<h2h3>/<sha1>.webp` with `sha1(JSON.stringify([rootId, relPath, size, mtime, width]))`. Our key has integers (mtime in ms is an integer in both indexes) and a string; `serde_json` escapes strings like `JSON.stringify` (`\"`, `\\`, `\b \f \n \r \t`, other control characters as lowercase `\u00xx`, everything else raw UTF-8). A test pins a hash computed by Node. `ThumbCache`: size measured on a thread at startup (leftover `*.tmp` removed, REQ-005-13); `has_room` only once measured; atomic writes (`<path>.<unique>.tmp`, then rename); eviction on its own thread by file mtime down to 90 % of the limit (REQ-005-11), one eviction at a time; `set_max_bytes`, `usage`, `clear` (REQ-010-05 / -06's storage side). Reading (`ThumbCache::read`, for M6's texture loader) bumps the mtime at most once per hour (REQ-005-11's view clock).

**Decision on the open cache layout question (Defender).** Keep the per-file layout. A pack file would read cold thumbnails ≈ 100× faster under Defender, but it breaks REQ-005-15 and the oracle-compatible cache, needs a migration of 66k files, and the cost only shows while scrolling fast through thumbnails not read for minutes. M6 measures cold scrolling with the real cache; if the budget fails there, a pack file (with a one-time import) becomes an M6 / M10 decision. Recorded in `docs/architecture.md`.

**Service (`indexer::thumbnails::Thumbnails`).** A port of `ThumbnailService` with threads: `io_concurrency` render workers take jobs from one priority queue; requests from visible tiles get rising priorities (the newest first, REQ-005-07), background jobs priority 0. A job already queued is moved up by a new request; a running one keeps running. Background feeding (`kick`): when the queue is shorter than the number of workers, the next 100 pending media items (newest `sort_date` first, offline libraries skipped, in-flight ones excluded) are queued (REQ-005-08); `kick` is called at start, after every batch a scan writes and after every scan. Only once the cache is measured, so a cache that looks empty isn't overfilled. Per job: the media item's source row; render; on failure the library's reachability decides between `thumb_status = 'error'` (REQ-005-09) and the library going `offline` (the item stays pending); on success the files are stored if the item was requested or the cache has room (REQ-005-12), a storage failure (disk full) isn't the file's fault; the placeholder is written unless the file changed meanwhile (`WHERE size = ? AND mtime = ?`). Every request is answered with an event (`ThumbnailReady { media, stored }`), which M6's texture loader waits for. Each job runs under `catch_unwind` (REQ-002-13 for decoders). The service has its own write connection: its writes are single-row updates, and SQLite's busy timeout serializes them with the scan's batches.

**Index queries (`db::thumbs`).** `thumb_source(id)`, `pending_thumbs(limit, exclude)`, `set_thumbnail(source, Option<thumbhash>)` as in the reference's `IndexWriter`; library status through `db::libraries`.

**Width selection (`domain::thumbs`).** `THUMB_WIDTHS = [400, 800]`, `pick_thumb_width(points, pixels_per_point)` (REQ-005-02), with the reference's cases.

**App.** The app starts the service with the data folder's `thumbs`, the cache limit from the settings and `io_concurrency` workers, hands it to the indexer (kicks) and keeps it for M6. No tiles yet, so the visible result is background rendering: the index's `thumb_status` and the cache folder fill while the app runs. The Settings page's cache section (usage, clear, limit) stays M10.

**Oracle (exit criterion).** `cargo xtask oracle` also renders every fixture with the reference's `renderMedia` (sharp, bundled like the indexer, with the reference's exiftool source for previews) and with `media::thumbnail`, and compares each width: same size, mean absolute difference per channel ≤ 3 levels, largest ≤ 48 (lossy WebP and different resamplers never match exactly); placeholders by their average color (±0.05). Fixtures added for this: an embedded-preview JPEG, a truncated JPEG, a PNG with alpha, a tall strip (> 3 : 1), an image smaller than 400 px and an AVIF.

## Differences and spec changes

- **REQ-005-01 corrected:** it said thumbnails are "at most 3 times as tall as wide"; both apps fit them into 800 × 2400 px and never crop (a 1000 × 10 000 strip becomes 240 × 2400), the tile crops. The spec now says "at most 2400 px (3 × the largest width) tall".
- **The oracle has no embedded-preview fixture:** exiftool can't write `PreviewImage` into a plain JPEG. The preview rules are covered by unit tests with a fake preview source and a real binary-tag read (`ThumbnailImage`).
- **Requests for failed items:** the service renders whatever it is asked; not re-requesting `error` items (as the reference's protocol handler does) is the caller's job (M6).

## Results

- `cargo xtask oracle` (2026-10-27): index 5 documented, 0 unexpected differences; thumbnails of 11 fixtures (3000 × 2000 JPEG, EXIF-rotated JPEG, truncated JPEG, PNG with alpha, 600 × 4000 strip, 300 × 200 PNG, GIF, WebP, AVIF, broken JPEG, 10 s video) all the same size as the reference's, mean difference ≤ 2.72 levels (premultiplied), 99th percentile ≤ 38, placeholder average colors within 0.016.
- `cargo xtask thumbs` on a copy of the real index and cache: 17,618 of 20,295 400-px and 532 800-px thumbnails found, the same counts as with Node's own hashing (the rest were evicted from the 5 GB cache).
- Running app with the fixture library in a new data folder: 15 items `ready`, `plain/broken.jpg` `error`, 30 WebP files; no helper process left behind.

## Reference

- `src/indexer/thumbnail.ts` (+ test): decode paths, preview rules (tolerance 2 %, `min(800, autoOrient.width)`), orientation table, ffmpeg arguments, resize box 800 × 2400, WebP quality 80, ThumbHash at 100 px. Tests ported: orientation 1–8 (sharp's expected pixels pinned), sizes without upscaling, preview used / too small / not an image, video frame.
- `src/indexer/thumb-cache.ts` (+ test): key JSON and SHA-1 fan-out, `.tmp` suffix, evict to 90 %, startup measurement removing temp files, `hasRoom` only once measured. Tests ported: stable / fanned-out path, every key part changes the path, measure + temp removal, eviction, lowered cap, clear.
- `src/indexer/thumbnails.ts` (+ test): priority queue, background batches of 100 when the queue is shorter than the concurrency, requested-or-room storage rule, reachability on failure. Tests ported: newest first, latest request first (with a running job), failed vs offline, full cache.
- `src/indexer/writer.ts`: `thumbSource`, `pendingThumbs`, `setThumbnail`.
- `src/main/protocols.ts`: reading touches at most once per hour; failed items aren't re-requested.
- `src/indexer/index.ts`: kicks after progress and after each scan, kick at start.
- `src/shared/media-urls.ts` (+ test): widths, `pickThumbWidth`.

## Tasks

- [x] 1. `domain::thumbs`: widths, `pick_thumb_width` (REQ-005-02)
- [x] 2. `indexer::thumb_cache`: key and path, pinned against Node's hash (REQ-005-10, REQ-005-15)
- [x] 3. `ThumbCache`: measure, temp cleanup, atomic write, eviction, lowered limit, clear, read with touch (REQ-005-11, -13, REQ-010-05, -06)
- [x] 4. `media::thumbnail`: decode + orientation, resize, WebP, ThumbHash (REQ-005-01, -03)
- [x] 5. JPEG shrink-on-load and truncated files (REQ-005-04 second part)
- [x] 6. Embedded previews through exiftool (REQ-005-04)
- [x] 7. ffmpeg: video frames and AVIF (REQ-005-05)
- [x] 8. `db::thumbs`: source, pending, set_thumbnail
- [x] 9. `indexer::thumbnails`: service with priority queue, background feeding, failures, full cache (REQ-005-07, -08, -09, -12)
- [x] 10. Indexer kicks; app starts the service; check in the running app with the fixture library and with a copy of the real index + cache (REQ-005-15)
- [x] 11. Oracle: thumbnails rendered by the reference vs ours within tolerance
- [x] 12. Docs: spec 005 verification, architecture (cache layout decision, rendering, ICC deviation), pitfalls, progress

## Exit criteria

From `docs/rewrite/plan.md`: reads the reference cache; images rendered from the same input match within a pixel tolerance; 005 REQs pass.

Added here: REQ-005-06 and -14 pass on their data side (placeholder stored, failed status kept, cached files readable while offline); their tile side is M6's. Background rendering runs in the app without stalling scans.
