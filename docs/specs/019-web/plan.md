# 019 · Web · Plan

## Approach

The page is the same UI compiled to `wasm32-unknown-unknown`: eframe 0.37 has a web backend on wgpu (WebGPU, WebGL 2 as fallback), so `app`'s gallery, sidebar, search bar, viewer, settings page and glass run in a `<canvas>`. What changes is everything below the UI: no threads, no file paths, no helper programs, no SQLite file on disk. The work is mostly drawing that boundary in `app`, then filling it a second time for the browser.

Checked in this session: `domain` builds for wasm as it is. `db` builds once clang is installed. rusqlite 0.41 compiles SQLite for wasm through `sqlite-wasm-rs`, which needs clang to compile SQLite's C (missing on the dev box, present on GitHub's runners).

### Constraints that shape it

- **No threads.** Shared-memory wasm threads need nightly (`-Z build-std`) and cross-origin isolation (COOP / COEP headers), and GitHub Pages can't send those headers. The toolchain stays pinned to stable. So the page runs **two wasm instances without shared memory**: the **UI instance** on the main thread and an **index worker** (a dedicated Web Worker running the same `.wasm` with another entry point), talking by `postMessage`. The architecture is already message-based (indexer commands and events, the gallery loader's requests and replies, texture requests and `Loaded`s), so the messages become a serialized enum instead of `crossbeam-channel` values.
- **Storage.** The index is a SQLite database in the Origin Private File System (OPFS) through `sqlite-wasm-rs`'s `sahpool` VFS. Its synchronous access handles exist only in workers, which is one more reason the database lives in the index worker. The schema and migrations are `db`'s, unchanged. Thumbnails are files in OPFS with the cache's layout (`thumbs/<h0h1>/<h2h3>/<sha1>.webp`), so `ThumbCache`'s naming, LRU and size limit carry over behind a small storage trait. `navigator.storage.persist()` at the first library (REQ-019-08).
- **File access.** Chrome and Edge: `showDirectoryPicker({ mode: "read" })`. The `FileSystemDirectoryHandle` is stored in IndexedDB (handles are structured-cloneable, not storable in SQLite), and `requestPermission` asks again on later visits (REQ-019-09). Firefox and Safari: `<input type="file" webkitdirectory>`, which gives a flat list of `File`s with `webkitRelativePath`. It's a snapshot for this visit only, so picking again is how access comes back. Both become one `FolderSource` trait in the worker: list entries (relative path, size, mtime), read bytes, and `File` objects for originals and video. `File`s can be passed to the worker by `postMessage`, so the input element's list reaches it too.
- **No exiftool.** A new pure-Rust reader produces the **same exiftool-shaped tag map** (`Tags`: `DateTimeOriginal`, `OffsetTimeOriginal`, `Orientation`, `ImageWidth`, `Rating`, `Subject`, `HierarchicalSubject`, `Keywords`, `XPKeywords`, `RegionInfo` / `RegionInfoMP`, `PersonInImage`, `CreateDate`, `Duration`, …) that `media::metadata::extract_metadata` already maps to `MediaMetadata`. So the rules (REQ-004) stay in one place. `extract_metadata` and its helpers move from `media` to `domain` (they are pure: `serde_json` + `chrono`). The reader parses EXIF (`kamadak-exif`, pure Rust), XMP packets and sidecars (`quick-xml`), IPTC IIM keywords, and MP4 / MOV `mvhd` / `tkhd` (creation date, duration, dimensions, rotation) with a small box parser. It lives in a new crate `meta` with no IO, so it builds for both targets. Because it's native-testable, REQ-019-04's test runs it **against exiftool** on a fixture set in CI (`GG_REQUIRE_HELPERS=1`) and compares the resulting `MediaMetadata`. This is also the first step of #46 (Rust-native alternatives to exiftool).
- **Decoding and thumbnails.** In the worker, `createImageBitmap(file, { imageOrientation: "from-image" })` decodes every format the browser shows (incl. AVIF) on the browser's own threads. An `OffscreenCanvas` resizes, and `convertToBlob({ type: "image/webp", quality: 0.8 })` encodes, with JPEG where the browser can't encode WebP (Safari). ThumbHash from the 100 px level runs in Rust (`thumbhash`). This uses the browser's decoders, which are off our thread and faster than `image` in wasm, and avoids compiling libwebp (C) for wasm. Video frames: `<video>` elements exist only on the main thread, so the UI instance seeks a hidden video to 10 % and hands the frame (`ImageBitmap`, transferable) to the worker.
- **Textures.** Thumbnails reach the UI instance as `ImageBitmap`s and become `wgpu::Texture`s with `Queue::copy_external_image_to_texture` (no CPU copy; mips with a small blit pass or `generateMipmap`-style downsampling in the shader). The 3 ms per frame registration budget and the LRU stay `textures.rs`'s; only the decode side differs. The texture budget drops to 256 MB (wasm32's 4 GB address space, browser GPU process limits).
- **Video.** A `<video>` element playing the `File` (an object URL), with frames copied into a texture each animation frame with `copy_external_image_to_texture`, so the viewer's chrome and glass draw over it as on the desktop. It implements the existing `video::Playback` trait (`WebPlayback`), so the viewer doesn't change.
- **Time and the rest of std.** `std::time::Instant` and `SystemTime` panic on `wasm32-unknown-unknown`. `app` switches to `web-time` (same API, `std` re-export on native). `chrono`'s `wasmbind` feature for `Local`. `sys-locale` works on wasm (`navigator.language`).

### Code layout

```
crates/
  meta      new: EXIF / XMP / IPTC / MP4 readers → exiftool-shaped Tags. No IO, both targets.
  domain    + extract_metadata (moved from media)
  app       UI, target-agnostic: depends on a `Backend` boundary instead of indexer / media directly
  native    new home of what `app` does with threads, files and helpers today (or a `native` module, see task 3)
  web       new: wasm entry points (UI instance, index worker), FolderSource, OPFS storage, browser decoding, WebPlayback
web/        index.html, worker bootstrap, CSP, unsupported-browser message; built with Trunk
```

`app`'s modules that touch threads, files or helpers (`state.rs`, `gallery/mod.rs`'s loader, `textures.rs`'s workers, `originals.rs`, `video/mpv.rs`, `titlebar.rs`, `startup.rs`) get their platform half behind traits that the native and web builds implement. `cfg(target_arch = "wasm32")` sits only at those seams. The native app must behave exactly as before: its whole test suite, snapshots included, is the safety net for the refactor (Tier 0 inside a Tier 3).

### Privacy (REQ-019-12)

- `index.html` carries `<meta http-equiv="Content-Security-Policy" content="default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; img-src 'self' blob: data:; media-src blob:; worker-src 'self'; style-src 'self' 'unsafe-inline'">`. Trunk's inline bootstrap script moves to a file (or gets a hash), so `script-src` needs no `'unsafe-inline'`. No `fetch` of anything but the page's own `.wasm` / `.js`.
- `xtask/tests/no_network.rs` grows a web half: the `web-sys` features enabled in the tree must not include `Request`, `XmlHttpRequest`, `WebSocket`, `EventSource`, `RtcPeerConnection`, `Navigator.sendBeacon`. Source scan for `fetch(` in `web/` scripts other than the wasm bootstrap.
- A Playwright test (below) records every request during a full scenario and fails on any that isn't a GET for one of the page's own files.

### Browser tests

Most logic stays native-tested: `domain`, `db`, `meta` and `app`'s kittest UI tests, with the backend faked. The browser half gets:

- `wasm-bindgen-test` in headless Chromium for the worker pieces (OPFS storage, `FolderSource` over a generated folder, decode and thumbnail).
- One Playwright end-to-end suite (`web/tests/`, Node, run by `cargo xtask web-test`). It loads the built page from a local static server, picks a fixture folder through the `webkitdirectory` input (`setInputFiles` works headless), waits for the scan, then checks the gallery, a search, the viewer and a reload (REQ-019-03, 05–08, 10, 11), records requests (REQ-019-12) and screenshots the canvas. Chromium is the CI browser; Firefox and WebKit run there too if their Playwright builds handle the canvas (WebGL 2 path), otherwise by hand.

Playwright is a dev-only tool like git-cliff. It isn't part of the page, and `npm` is used only under `web/tests/`.

### Build and deploy (REQ-019-14)

- `cargo xtask web` builds the page with Trunk (`trunk build --release`, `wasm-opt -Oz`) into `target/web/`. Trunk handles the worker as a second `data-type="worker"` target of the same crate. Fonts and icons are already `include_bytes!`, so nothing else needs fetching.
- CI: a `web` job on `ubuntu-latest`: `rustup target add wasm32-unknown-unknown`, clang (preinstalled), `cargo clippy --target wasm32-unknown-unknown -p web`, `cargo xtask web`, the browser tests.
- The release workflow uploads `target/web/` with `actions/upload-pages-artifact` and `actions/deploy-pages`. **The repository's Pages source must be set to "GitHub Actions" once, by hand.**

### Risks, checked first

1. `sqlite-wasm-rs` + `sahpool` in a worker with rusqlite 0.41's API, WAL off (OPFS has no shared memory; journal mode `TRUNCATE`). `db::open`'s pragmas get a wasm branch.
2. `app`'s glass pass and `textures.rs` on eframe's web wgpu (WebGPU and WebGL 2): render targets, mip uploads, `copy_external_image_to_texture`.
3. Bundle size: egui + wgpu + SQLite + ICU data. Target ≤ 15 MB `.wasm` before compression. ICU4X compiled data is the likely big part. Trim to the locales' date skeletons we use, or fall back to `Intl.DateTimeFormat` on the web.
4. OPFS speed for many small thumbnail files. A pack file would be the fallback (rejected on desktop for compatibility, which doesn't apply here).

### Rejected

- **Wasm threads (`wasm-bindgen-rayon`, `-Z build-std`):** needs nightly and COOP / COEP, which GitHub Pages can't serve (a service-worker shim like `coi-serviceworker` reloads the page and breaks in some browsers).
- **A separate, simpler web UI:** two UIs drift apart, and the point is to try *this* app.
- **sql.js / IndexedDB instead of SQLite in OPFS:** `db`'s queries, migrations and search plans would have to be rewritten or loaded fully into memory.
- **`image` + libwebp in wasm for decoding:** slower than the browser's decoders, on our only worker thread, and libwebp is C (another clang build). AVIF would still be missing.
- **A DOM `<video>` overlay over the canvas:** the viewer's chrome and glass couldn't draw over it.
- **Chromium-only (File System Access API everywhere):** Firefox and Safari users couldn't try it at all; the input-element path is cheap to keep.

## Tasks

- [ ] 1. Spike: rusqlite + `sqlite-wasm-rs` `sahpool` in a worker opens, migrates and queries an index (risk 1) (REQ-019-08)
- [ ] 2. Spike: eframe web build of a minimal `app` shell with the glass pass and one `copy_external_image_to_texture` texture on WebGPU and WebGL 2; measure the `.wasm` size (risks 2, 3) (REQ-019-01)
- [ ] 3. Refactor `app` behind a `Backend` boundary (index queries, indexer commands and events, thumbnails, originals, video, OS services); `web-time`; native app unchanged, full suite and snapshots green (Tier 0)
- [ ] 4. Move `extract_metadata` to `domain`; crate `meta` with EXIF, XMP, IPTC and MP4 readers; comparison test against exiftool on fixtures (REQ-019-04)
- [ ] 5. `web` crate: UI instance and index worker, message protocol, OPFS-backed `db` and thumbnail storage (REQ-019-03, REQ-019-08)
- [ ] 6. `FolderSource`: directory picker + IndexedDB handles, `webkitdirectory` input; scan and rescan over it (REQ-019-02, REQ-019-03, REQ-019-09, REQ-019-10)
- [ ] 7. Thumbnails: browser decoding, WebP / JPEG encoding, ThumbHash, video frames from the UI instance; texture streaming (REQ-019-05)
- [ ] 8. Viewer: originals from `File`s, `WebPlayback`; hide desktop-only features (REQ-019-07, REQ-019-13)
- [ ] 9. Remove a library deletes its storage and handle; persistent storage request; second-tab message; unsupported-browser message (REQ-019-01, REQ-019-08, REQ-019-11)
- [ ] 10. CSP, `no_network.rs` web half (REQ-019-12)
- [ ] 11. `cargo xtask web`, `cargo xtask web-test` with Playwright end to end; CI job (REQ-019-01 to REQ-019-14)
- [ ] 12. Release workflow deploys to Pages; README (link, browser support), architecture decision, glossary, pitfalls; close #45 (REQ-019-14)
