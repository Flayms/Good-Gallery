# 011 · Security

**Status:** Approved (ported from the reference, not yet implemented in Rust)

## Goal

Media files may come from anywhere and contain hostile metadata. The app's window is treated like an untrusted web page: it can reach only the app's own API and only the media items in the index, never arbitrary files, other sites or system capabilities.

## Requirements

### REQ-011-01 · Isolated window

- **Given** the app window
- **When** its page runs
- **Then** it has no access to Node.js or Electron APIs, runs sandboxed with context isolation, and sees only the app's API transport

### REQ-011-02 · No foreign pages

- **Given** the app window
- **When** anything tries to navigate it to another page, redirect it, attach a webview or open a new window
- **Then** that is blocked. Only `https:` links open, in the default browser

### REQ-011-03 · Only the app's own page calls the API

- **Given** an API message
- **When** it comes from anything but the main frame of the app's own page, or is malformed
- **Then** it is ignored
- **And** API inputs are validated, and invalid ones are rejected with a `BAD_REQUEST` error

### REQ-011-04 · Files only by media id

- **Given** a request for a thumbnail or an original
- **When** it reaches the app
- **Then** it is served only for the exact forms `gg-thumb://media/<id>/<400|800>` and `gg-media://media/<id>`, where `<id>` is an indexed media item. Every other form, unknown id or vanished file gets a 404
- **And** the window never sends file paths. _Show in Explorer_ and originals resolve the path from the index by id

### REQ-011-05 · Permissions

- **Given** a permission request (camera, microphone, notifications, …)
- **When** the app's page or anything else asks
- **Then** only fullscreen is granted, and only to the app's own page

### REQ-011-06 · Content policy

- **Given** the app's page
- **When** it loads content
- **Then** scripts come only from the app itself. Images and media come only from the app, `data:` / `blob:` URLs and the two media protocols. Plugins, frames, form submissions and `<base>` changes are blocked

### REQ-011-07 · API over IPC

- **Given** the app's page calling the API
- **When** it runs queries, mutations or subscriptions
- **Then** results keep their types (e.g. dates), errors keep their codes, and stopping a subscription, closing or reloading the window aborts its pending work in the main process

## Out of scope

- Code signing and auto-updates.
- Protection against a malicious user with access to the user profile or the libraries.
- Sandboxing the indexer process (it needs file system access to read libraries).

## Assumptions

- Media decoding (sharp, ffmpeg, exiftool) runs in the separate indexer process, so crashes there don't take down the window.
- Development builds load the page from a local dev server. Production builds load it from the app files, and the trusted-page check accepts exactly one of those.

## Verification

| REQ        | Test                                                                                                                                                         | Result                                                                |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------- |
| REQ-011-01 | — | no test |
| REQ-011-02 | — | no test |
| REQ-011-03 | — | no test |
| REQ-011-04 | — | no test |
| REQ-011-05 | — | no test |
| REQ-011-06 | — | no test |
| REQ-011-07 | — | no test |
