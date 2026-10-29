# 019 · Web · Plan

## Approach

The page is the same UI compiled to `wasm32-unknown-unknown`: eframe 0.37 has a web backend on wgpu (WebGPU, WebGL 2 as fallback), so `app`'s gallery, sidebar, search bar, viewer, settings page and glass run in a `<canvas>`. What changes is everything below the UI: no threads, no file paths, no helper programs, no SQLite file on disk. `app` keeps its desktop half behind `cfg(not(target_arch = "wasm32"))` and gets a browser half in `app::web`.

### Constraints that shape it

- **No threads, one instance.** Shared-memory wasm threads need nightly (`-Z build-std`) and cross-origin isolation (COOP / COEP headers), which GitHub Pages can't send; the toolchain stays pinned to stable. Everything runs in **one wasm instance on the main thread**. The work that is slow (decoding, encoding, reading files) is the browser's and runs off our thread; our own CPU work (metadata parsing, index writes, ThumbHash) is sliced by `web::Slice`, which yields to the browser after 8 ms, so frames and input keep going during a scan (REQ-019-03). The existing message seams stay: the scan queue (`web::scan::WebIndexer`) implements `ScanRequests` and sends `IndexerEvent`s, the thumbnail service reports `ThumbnailReady`s, both over `std::sync::mpsc` channels the UI drains each frame.
- **Storage.** The index is SQLite (`db`'s schema, migrations and queries, unchanged) on `sqlite-wasm-rs`'s in-memory file system, shared by name between connections. A snapshot of it is kept in the Origin Private File System (OPFS): restored before the first connection opens, saved through `serialize` a few seconds after `PRAGMA data_version` changes and when the page is hidden (REQ-019-08). Thumbnails are files in OPFS, `thumbs/<library>/<h0h1>/<h2h3>/<sha1>.webp` (the desktop cache's naming, `domain::thumbs::thumb_hash`, plus the library, so removing a library is one folder delete, REQ-019-11), with the desktop's size limit and LRU eviction (`web::thumbs::ThumbCache`). `navigator.storage.persist()` at the first library. A Web Lock keeps a second tab out.
- **File access.** Chrome and Edge: `showDirectoryPicker({ mode: "read" })`, the handle kept in IndexedDB and asked for again with `requestPermission` on the next visit (REQ-019-09). Firefox and Safari: `<input type="file" webkitdirectory>`, a snapshot for this visit; picking the same folder (by name) again gives access back. Both become `web::folders::Source` (a `Directory` handle or a `Files` map). A library whose folder isn't readable at the start of a visit is reported offline and labelled "needs access".
- **No exiftool.** The new crate `meta` (no IO, both targets) holds the rules that turn exiftool-named tags into `MediaMetadata` (moved from `media`, so REQ-004 lives in one place) and pure-Rust readers that produce the same tags from file bytes: JPEG, PNG, WebP, GIF, AVIF (HEIF boxes), MP4 / M4V / MOV (`mvhd`, `tkhd`), with EXIF / TIFF, XMP (`quick-xml`), IPTC IIM and Photoshop resources. Readers return `Missing { at, len }` when they need more bytes, so the browser reads slices of a `File` instead of whole files. They mirror exiftool's `-use MWG` reconciliation (XMP keywords over IPTC's unless the IPTC digest changed). `media`'s test runs exiftool and `meta` on the same tagged files and compares the results (REQ-019-04).
- **Decoding and thumbnails.** `createImageBitmap(file, { imageOrientation: "from-image" })` decodes every format the browser shows, an `OffscreenCanvas` resizes, `convertToBlob` encodes WebP (JPEG where the browser can't encode WebP). ThumbHash runs in Rust on the 100 px level. Video frames come from a hidden `<video>` seeked to 10 % (REQ-019-05). Stored thumbnails become textures through `createImageBitmap` → RGBA → `textures.rs`'s upload, with its per-frame budget and LRU; the texture budget is 256 MB in the browser.
- **Video.** `web::video::WebPlayback` implements `video::Playback`: a `<video>` element plays the `File` (an object URL), and each UI frame copies its current frame into a texture with `copy_external_image_to_texture`, so the viewer's chrome and glass draw over it as on the desktop.
- **Time and the rest of std.** `web-time` instead of `std::time::Instant`, `chrono`'s `wasmbind` for local time, `sys-locale`'s `js` feature, and `sqlite-wasm-rs`'s `wasm-bindgen` feature for SQLite's clock, randomness and local time.

### Code layout

```
crates/
  meta      new: rules (exiftool tags → MediaMetadata) and EXIF / XMP / IPTC / container readers. No IO, both targets.
  media     exiftool, ffmpeg, image decoding: desktop only; uses meta's rules
  indexer   status types for both targets; scans, watchers and the thumbnail service desktop only
  app       the UI for both targets; desktop.rs is the desktop entry point, app::web the browser's services
web/        index.html (CSP), check.js (unsupported browsers), page.css, Trunk.toml; tests/ (Playwright)
```

`cfg(target_arch = "wasm32")` sits at the seams: `state.rs` (folder picking, background jobs), `textures.rs` (decoding), `originals.rs`, `video`, `settings_page.rs` and `gallery` (desktop-only actions, REQ-019-13). The desktop app's whole test suite, snapshots included, guards it.

### Privacy (REQ-019-12)

- `index.html`'s policy: `default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; …; form-action 'none'`. `cargo xtask web` moves Trunk's inline start script into a file, so scripts need no `'unsafe-inline'`.
- `xtask/tests/no_network.rs`: the `web-sys` features enabled for wasm include no network API (`Request`, `XmlHttpRequest`, `WebSocket`, …), the page's scripts call no `fetch` and the like, and the policy is in place.
- The Playwright test records every request of a full scenario and fails on any that isn't a GET for the page's own files.

### Browser tests

Logic stays native-tested (`domain`, `db`, `meta`, `app`'s kittest UI tests). `web/tests/web.spec.js` (Playwright, `cargo xtask web-test`) loads the built page from a local static server in Chromium with SwiftShader (the WebGL 2 path), picks `web/tests/fixtures/Holiday` through the `webkitdirectory` input, and checks the gallery, a search, the viewer, a reload, picking a changed copy again, the rescan and removing the library, by screenshots (clock, time zone and locale fixed) and the app's log lines. The directory-picker path (Chromium's File System Access API) can't be driven headless and is checked by hand.

### Build and deploy (REQ-019-14)

- `cargo xtask web` builds the page with Trunk (`web/Trunk.toml`, release, `wasm-opt -Oz`) into `target/web/`: about 13 MB of `.wasm` before compression. The wasm target is in `rust-toolchain.toml`; clang compiles SQLite.
- CI: a `web` job on `ubuntu-latest` (clippy for wasm, `cargo xtask web`, Playwright's Chromium, `cargo xtask web-test`), part of the required "Test" check.
- The release workflow builds the page and publishes it with `actions/upload-pages-artifact` and `actions/deploy-pages`. **The repository's Pages source must be set to "GitHub Actions" once, by hand.**

### Deviations from the first draft

- **No index worker.** The draft ran the index in a dedicated worker (a second wasm instance speaking `postMessage`), with SQLite on OPFS through `sahpool`. Built instead: one instance, with the index in memory and a snapshot in OPFS. It drops a whole message protocol and keeps `app`'s channels as they are. The cost: our own scan work shares the main thread (sliced), and the index lives in memory, which is fine for the libraries in scope (up to 20,000 files).
- **No separate `web` / `native` crates.** The browser half is the module `app::web`; the desktop entry point moved from `main.rs` to `desktop.rs`.
- **Thumbnail textures** go through RGBA (`createImageBitmap` → canvas) instead of `copy_external_image_to_texture`, which only the video uses, so `textures.rs`'s mip upload stays one path.

### Rejected

- **Wasm threads (`wasm-bindgen-rayon`, `-Z build-std`):** needs nightly and COOP / COEP, which GitHub Pages can't serve (a service-worker shim like `coi-serviceworker` reloads the page and breaks in some browsers).
- **A separate, simpler web UI:** two UIs drift apart, and the point is to try _this_ app.
- **sql.js / IndexedDB instead of SQLite:** `db`'s queries, migrations and search plans would have to be rewritten.
- **`image` + libwebp in wasm for decoding:** slower than the browser's decoders, on our only thread, and libwebp is C. AVIF would still be missing.
- **A DOM `<video>` overlay over the canvas:** the viewer's chrome and glass couldn't draw over it.
- **Chromium-only (File System Access API everywhere):** Firefox and Safari users couldn't try it at all; the input-element path is cheap to keep.

## Tasks

- [x] 1. ~~Spike: `sahpool` in a worker~~ replaced by the in-memory index with an OPFS snapshot (see Deviations) (REQ-019-08)
- [x] 2. eframe web build with the glass pass on WebGL 2; `.wasm` size (REQ-019-01)
- [x] 3. Desktop-only code behind target cfgs, `web-time`, `desktop.rs`; native app unchanged, full suite and snapshots green (Tier 0)
- [x] 4. Crate `meta` with the rules and the EXIF, XMP, IPTC and container readers; comparison test against exiftool (REQ-019-04)
- [x] 5. `app::web`: start, index snapshot, scan queue, thumbnails in OPFS (REQ-019-03, REQ-019-05, REQ-019-08)
- [x] 6. Folders: directory picker + IndexedDB handles, `webkitdirectory` input; scan and rescan over them; libraries without access shown as needing access (REQ-019-02, REQ-019-03, REQ-019-09, REQ-019-10)
- [x] 7. Thumbnails: browser decoding, WebP / JPEG encoding, ThumbHash, video frames; texture streaming (REQ-019-05)
- [x] 8. Viewer: originals from `File`s, `WebPlayback`; desktop-only features hidden (REQ-019-07, REQ-019-13)
- [x] 9. Removing a library deletes its storage and handle; persistent storage; second-tab message; unsupported-browser message (REQ-019-01, REQ-019-08, REQ-019-11)
- [x] 10. CSP, `no_network.rs` web half (REQ-019-12)
- [x] 11. `cargo xtask web`, `cargo xtask web-test` with Playwright end to end; CI job (REQ-019-01 to REQ-019-14)
- [x] 12. Release workflow deploys to Pages; README, architecture decision, pitfalls (REQ-019-14)
- [ ] 13. By hand in Chrome / Edge with a GPU (WebGPU): the directory picker, permission on the next visit, video playback with sound; Firefox and Safari smoke test
- [ ] 14. Set the repository's Pages source to "GitHub Actions"; close #45 after the first deploy
