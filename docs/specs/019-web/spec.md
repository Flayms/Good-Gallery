# 019 · Web

**Status:** Approved

## Goal

People who want to try Good Gallery, or who are on a computer where they can't install it, open a web page and browse a folder of their own photos and videos with the same gallery, search, sidebar and viewer as the desktop app. There is no server: the page is static files on GitHub Pages, the index and thumbnails stay in the browser, and nothing leaves the user's machine.

## Requirements

### REQ-019-01 · Static page, no server

- **Given** the page's files served by any static host (GitHub Pages)
- **When** the user opens it in a current desktop browser (REQ assumptions)
- **Then** Good Gallery starts in the page, with the empty state of REQ-006-10 and a button to pick a folder
- **And** in a browser that lacks what the page needs, it says so and names the browsers that work, instead of a blank page

### REQ-019-02 · Pick a folder

- **Given** the page open
- **When** the user picks a folder through the browser's folder picker
- **Then** it becomes a library (REQ-001-02), named after the folder, and its first scan starts
- **And** the page asks the browser for read access only; picking the same folder again doesn't add a second library (REQ-001-04)

### REQ-019-03 · Indexing in the browser

- **Given** a library picked in the page
- **When** it is scanned
- **Then** every photo and video the browser can show (JPEG, PNG, WebP, GIF, AVIF; MP4, M4V, WebM, and MOV where the browser plays it) is indexed, housekeeping folders skipped (REQ-002-02), with the progress shown in the sidebar (REQ-002-08)
- **And** the gallery fills while the scan runs, and the page keeps responding to scrolling and typing during the scan

### REQ-019-04 · Metadata without helper programs

- **Given** media files with embedded EXIF, IPTC or XMP metadata, or XMP sidecars
- **When** they are indexed in the page
- **Then** their dimensions, date taken, duration, rating and tags (keywords, tag hierarchies, face regions) are the same as the desktop app reads from the same files (REQ-004-02 to REQ-004-11)
- **And** malformed metadata costs at most that file's metadata (REQ-011-10)

### REQ-019-05 · Thumbnails and placeholders

- **Given** indexed media in the page
- **When** their tiles come into view
- **Then** they show placeholders, then thumbnails, rendered on demand first and in the background after (REQ-005); a video's thumbnail is its frame at 10 % of its duration

### REQ-019-06 · Gallery, search and sidebar

- **Given** an indexed library in the page
- **When** the user browses it
- **Then** the gallery views, zoom, date groups and tag indicators (specs 006, 016, 017), the search and filters (spec 007) and the sidebar (spec 008) behave as in the desktop app

### REQ-019-07 · Viewer

- **Given** the gallery in the page
- **When** the user opens a media item
- **Then** the viewer, its info panel and the filmstrip behave as in the desktop app (specs 009, 015): photos show in original quality with zoom and pan, and videos play with sound, seeking and volume through the browser

### REQ-019-08 · Index kept between visits

- **Given** a library indexed in the page, and the page closed
- **When** the user opens the page again in the same browser
- **Then** the gallery, sidebar and thumbnails show at once from the browser's storage, without picking the folder again
- **And** the page asks the browser to keep its storage, so the browser doesn't clear it when space runs low unless the user agrees

### REQ-019-09 · Back to the folder

- **Given** a library from an earlier visit
- **When** the user opens a photo in original quality, plays a video or rescans
- **Then** the page asks for access to the folder again: one confirmation where the browser can remember a picked folder, otherwise by picking the folder again (it must be the same folder by name)
- **And** without access, browsing the index, its thumbnails and its search works, and the library shows as needing access instead of offline errors

### REQ-019-10 · Rescan finds changes

- **Given** a library whose folder changed since its last scan (files added, removed or changed)
- **When** the user rescans it (REQ-001-06), or the page regains access to it at the start of a visit
- **Then** the index matches the folder afterwards, re-reading only changed files (size or date modified)

### REQ-019-11 · Remove a library

- **Given** a library in the page
- **When** the user removes it (REQ-001-05)
- **Then** its index entries and its thumbnails are deleted from the browser's storage, and the page no longer has access to its folder

### REQ-019-12 · Nothing leaves the machine

- **Given** the page open and a library indexed, browsed and viewed
- **When** anything happens in the page
- **Then** the only requests it makes are for its own files from the host it was loaded from, and none of them carries data from the user's files or index
- **And** the page forbids itself any other connection (a content security policy), so a mistake in the code can't send data out (the web counterpart of REQ-011-11)

### REQ-019-13 · Desktop-only features hidden

- **Given** the page
- **When** the user looks for _Show in Explorer / folder_, libraries on network shares, adding a library by typing a path, watchers, scheduled checks, parallel file reads or helper settings
- **Then** they aren't offered; the thumbnail cache limit, usage and clearing (REQ-010-05, REQ-010-06) work

### REQ-019-14 · Published from CI

- **Given** a release (spec 014)
- **When** the release workflow runs
- **Then** it builds the page and publishes it to the repository's GitHub Pages site, and CI builds the page and runs its tests on every pull request

## Out of scope

- A server, accounts, sharing or syncing between browsers and devices.
- Opening the desktop app's index or thumbnail cache in the page, or the other way round.
- Formats the browser can't decode (HEIC, RAW), MKV, writing metadata.
- Phones and tablets: touch, small screens, mobile browsers. The page targets desktop browsers.
- Installing the page as an app (PWA), working offline before the first visit.
- Several tabs of the page at once: a second tab says the page is already open elsewhere.
- Libraries of 50,000 files and more: they work, but there's no speed target for them in the page.
- Glass where the browser has no WebGPU: surfaces are opaque there (REQ-013-06's fallback).

## Assumptions

- Current desktop browsers: the last two releases of Chrome, Edge, Firefox and Safari. Chrome and Edge can remember a picked folder between visits (File System Access API); Firefox and Safari can't, so there REQ-019-09 means picking the folder again.
- WebGPU is available in Chrome and Edge on Windows and macOS, in Safari 26 and in Firefox on Windows; elsewhere the page draws with WebGL 2.
- The browser's storage quota for the page is large enough for the index and the thumbnail cache of the libraries in scope (a few GB for 50,000 files); the cache size limit (REQ-005-11, REQ-010-05) applies.
- Libraries in scope have up to 20,000 files on a local disk. Larger ones are slower to scan than in the desktop app, which reads files in parallel.
- The page's host serves it over HTTPS (browsers offer folder access and storage only there).

## Verification

| REQ        | Test | Result  |
| ---------- | ---- | ------- |
| REQ-019-01 | `web/tests/web.spec.js` › pick, scan, browse, …; a browser without what the page needs… | pass |
| REQ-019-02 | `web/tests/web.spec.js` › pick, scan, browse, … | partial: the folder upload input (Firefox, Safari); the directory picker (Chrome, Edge) by hand |
| REQ-019-03 | `web/tests/web.spec.js` › pick, scan, browse, … | partial: formats and skipped folders of the fixture; responsiveness during a scan by hand |
| REQ-019-04 | `crates/media/src/metadata.rs` › req_019_04_meta_reads_the_same_metadata_as_exiftool; `crates/meta/src` › req_019_04_… | pass |
| REQ-019-05 | `web/tests/web.spec.js` › pick, scan, browse, … | pass |
| REQ-019-06 | `web/tests/web.spec.js` › pick, scan, browse, … | partial: the sidebar and a tag search; views, zoom and filters are the desktop's code and tests |
| REQ-019-07 | `web/tests/web.spec.js` › pick, scan, browse, … | partial: a photo in original quality; video playback by hand |
| REQ-019-08 | `web/tests/web.spec.js` › pick, scan, browse, … | partial: the reload; the persistent storage request by hand |
| REQ-019-09 | `web/tests/web.spec.js` › pick, scan, browse, … | partial: picking the folder again (upload input); the browser's permission prompt by hand |
| REQ-019-10 | `web/tests/web.spec.js` › pick, scan, browse, … | pass |
| REQ-019-11 | `web/tests/web.spec.js` › pick, scan, browse, … | pass |
| REQ-019-12 | `web/tests/web.spec.js` › pick, scan, browse, …; `xtask/tests/no_network.rs` › req_019_12_…; `xtask/src/web.rs` › req_019_12_… | pass |
| REQ-019-13 | `web/tests/web.spec.js` › pick, scan, browse, … | partial: the settings page; the gallery's and viewer's menus by inspection |
| REQ-019-14 | `xtask/tests/workflows.rs` › req_019_14_ci_tests_the_page_and_the_release_publishes_it | pass |
