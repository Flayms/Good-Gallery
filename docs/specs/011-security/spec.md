# 011 · Security

**Status:** Approved

## Goal

Media files may come from anywhere and contain hostile names and metadata. Good Gallery reads them without letting them steer the app: files are only ever opened through the index, helper programs can't be tricked into running something else, malformed metadata can't take the app down, and the app doesn't talk to the network.

## Requirements

### ~~REQ-011-01 · Isolated window~~

Deprecated 2026-10-26: the Rust app has no web page, renderer process or Node.js / Electron APIs to isolate.

- **Given** the app window
- **When** its page runs
- **Then** it has no access to Node.js or Electron APIs, runs sandboxed with context isolation, and sees only the app's API transport

### ~~REQ-011-02 · No foreign pages~~

Deprecated 2026-10-26: a native window can't be navigated to other pages or open webviews.

- **Given** the app window
- **When** anything tries to navigate it to another page, redirect it, attach a webview or open a new window
- **Then** that is blocked. Only `https:` links open, in the default browser

### ~~REQ-011-03 · Only the app's own page calls the API~~

Deprecated 2026-10-26: there is no IPC API between a page and a main process; the UI calls the database and indexer in-process.

- **Given** an API message
- **When** it comes from anything but the main frame of the app's own page, or is malformed
- **Then** it is ignored
- **And** API inputs are validated, and invalid ones are rejected with a `BAD_REQUEST` error

### ~~REQ-011-04 · Files only by media id~~

Deprecated 2026-10-26: there are no custom protocols (`gg-thumb://`, `gg-media://`) in the Rust app. The rule behind it, files are resolved from the index by id, is REQ-011-08.

- **Given** a request for a thumbnail or an original
- **When** it reaches the app
- **Then** it is served only for the exact forms `gg-thumb://media/<id>/<400|800>` and `gg-media://media/<id>`, where `<id>` is an indexed media item. Every other form, unknown id or vanished file gets a 404
- **And** the window never sends file paths. _Show in Explorer_ and originals resolve the path from the index by id

### ~~REQ-011-05 · Permissions~~

Deprecated 2026-10-26: no web permissions (camera, microphone, notifications) exist in a native window.

- **Given** a permission request (camera, microphone, notifications, …)
- **When** the app's page or anything else asks
- **Then** only fullscreen is granted, and only to the app's own page

### ~~REQ-011-06 · Content policy~~

Deprecated 2026-10-26: there is no HTML content to restrict.

- **Given** the app's page
- **When** it loads content
- **Then** scripts come only from the app itself. Images and media come only from the app, `data:` / `blob:` URLs and the two media protocols. Plugins, frames, form submissions and `<base>` changes are blocked

### ~~REQ-011-07 · API over IPC~~

Deprecated 2026-10-26: there is no IPC transport. Cancelling indexer work when a library is removed is REQ-001-05.

- **Given** the app's page calling the API
- **When** it runs queries, mutations or subscriptions
- **Then** results keep their types (e.g. dates), errors keep their codes, and stopping a subscription, closing or reloading the window aborts its pending work in the main process

### REQ-011-08 · Files only from the index

- **Given** a media item to show, reveal in Explorer or render a thumbnail for
- **When** the app opens its file
- **Then** the path is built from the item's library and its relative path in the index, looked up by media id
- **And** a relative path that would leave its library (absolute, a drive or UNC prefix, or `..` components) is refused, and the item is treated as missing

### REQ-011-09 · Helpers can't be steered by file names

- **Given** files whose names contain spaces, quotes, leading dashes, `;`, `&`, newlines or non-ASCII characters
- **When** exiftool or ffmpeg read them
- **Then** they are started without a shell and the names are passed so they can never be taken as options or commands
- **And** such files are indexed and rendered like any other

### REQ-011-10 · Hostile metadata

- **Given** a file with malformed metadata (invalid UTF-8, unexpected types, very long strings or lists, very deep tag hierarchies)
- **When** it is indexed and shown
- **Then** the app keeps running and stays responsive, and the file is indexed as in REQ-002-12

### REQ-011-11 · No network access

- **Given** the app running
- **When** it indexes, renders, searches or shows media
- **Then** it opens no network connections of its own. Libraries on network shares are read through the operating system's file access only

## Out of scope

- Code signing and auto-updates.
- Protection against a malicious user with access to the user profile or the libraries.
- Sandboxing the indexer (it needs file system access to read libraries).
- Isolating crashes inside native decoders (C code) in a separate process; see the indexer decision in `docs/architecture.md`.

## Assumptions

- exiftool and ffmpeg are bundled with the app and run as separate processes; a crash there doesn't take down the app.
- In-process decoding (thumbnails, viewer) uses mature libraries; a panic in Rust code during indexing is contained to that job (REQ-002-13).

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| ~~REQ-011-01~~ | — | no test (deprecated) |
| ~~REQ-011-02~~ | — | no test (deprecated) |
| ~~REQ-011-03~~ | — | no test (deprecated) |
| ~~REQ-011-04~~ | — | no test (deprecated) |
| ~~REQ-011-05~~ | — | no test (deprecated) |
| ~~REQ-011-06~~ | — | no test (deprecated) |
| ~~REQ-011-07~~ | — | no test (deprecated) |
| REQ-011-08 | — | no test |
| REQ-011-09 | `crates/media/src/exiftool.rs` › reads_files_with_awkward_names (exiftool) | pass |
| REQ-011-10 | `crates/media/src/metadata.rs` › survives_hostile_values | partial: extraction only; indexing such files in M4b |
| REQ-011-11 | — | no test |
