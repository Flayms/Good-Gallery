# 027 · Open with

**Status:** Implemented

## Goal

Users open a photo or video with Good Gallery from the file manager (_Open with_ on Windows, the file manager's equivalent on Linux) and see it in the viewer at once, in the window they already have open. A file outside every library is shown without indexing it (spec 033).

## Requirements

### REQ-027-01 · Open a file from a library

- **Given** a photo or video inside a library
- **When** the app is started with the file's path (_Open with_, a double-click on a file type it's the default app for, or `good-gallery <file>`)
- **Then** the gallery shows that library, without a search or filters, and the viewer shows the file; stepping moves through the library's order
- **And** a file in a library that isn't indexed yet (new since the last scan, or not reached by a running scan) is indexed first, ahead of other scans (REQ-002-18), and then shown; a message says it opens once it is indexed

### REQ-027-02 · One window

- **Given** the app running
- **When** another file is opened with it
- **Then** the running window comes to the front and shows that file as in REQ-027-01; no second window opens, and the second start exits
- **And** with a different data folder (`--data-dir`) it is a separate app and opens its own window

### ~~REQ-027-03 · A file outside every library~~

Deprecated (#162): such a file is shown at once from a scratch index, and its folder can be added from the viewer (REQ-033-01, REQ-033-04), instead of asking to add the folder before anything is shown.

### REQ-027-04 · Files that can't be opened

- **Given** a path that doesn't exist, isn't a file, or isn't a photo or video the app indexes (REQ-002-01)
- **When** it is opened with the app
- **Then** the app opens as usual and an error message says it couldn't open that file and why

### REQ-027-05 · Registered as a handler

- **Given** the Windows installer has run with _Show under "Open with"_ on (REQ-014-15), or the setting is on (REQ-027-06)
- **When** the user opens _Open with_ on a photo or video of a format the app indexes
- **Then** Good Gallery is listed, without making itself the default app; uninstalling removes the entries
- **And** on Linux the installed `.desktop` file lists those formats' MIME types and passes the file to the app, so file managers offer it the same way; the portable Windows zip registers nothing

### REQ-027-06 · Open with in the settings

- **Given** the app installed with the Windows installer
- **When** the user turns _Show under "Open with"_ on or off in the settings
- **Then** the app adds or removes the same entries as the installer (REQ-027-05) at once, and the next update keeps that choice (REQ-014-15)
- **And** the switch shows whether the entries are there now; if they can't be changed, an error message says why and the switch stays as it was
- **And** the portable app, Linux and the browser version don't show the switch: they register nothing (REQ-027-05)

## Out of scope

- Making Good Gallery the default app (Windows only lets the user choose that), the Explorer context menu beyond _Open with_, opening several files at once (the first one is shown), folders.
- macOS (#37) and the browser version.

## Assumptions

- The data folder is on a local disk, so two starts of the app see each other's files in it at once.

## Verification

| REQ        | Test | Result  |
| ---------- | ---- | ------- |
| REQ-027-01 | `crates/domain/src/media.rs` › req_027_01_finds_a_files_path_inside_its_library, req_027_01_matches_windows_paths_in_any_letter_case; `crates/app/tests/open_with.rs` › req_027_01_shows_a_file_from_a_library_in_the_viewer, req_027_01_indexes_a_new_file_first, req_027_01_says_it_opens_once_indexed, req_027_01_says_so_when_the_scan_finds_nothing | pass |
| REQ-027-02 | `crates/app/src/instance.rs` › req_027_02_a_second_start_hands_its_file_to_the_first, req_027_02_apps_on_different_data_folders_are_separate | partial: the window coming to the front by hand |
| ~~REQ-027-03~~ | — | no test (deprecated) |
| REQ-027-04 | `crates/app/tests/open_with.rs` › req_027_04_says_why_a_file_cant_be_opened | pass |
| REQ-027-05 | `xtask/tests/packaging.rs` › req_027_05_the_installer_registers_every_indexed_format, req_027_05_the_desktop_file_lists_every_indexed_format | partial: _Open with_ in Explorer and a Linux file manager by hand |
| REQ-027-06 | `crates/app/src/open_with.rs` › req_027_06_adds_and_removes_the_installers_entries; `crates/app/tests/settings.rs` › req_027_06_switches_open_with_in_the_settings | partial: the app's own registry in Explorer by hand |
