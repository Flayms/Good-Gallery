# 033 · Files outside libraries

**Status:** Approved

## Goal

Users who open a photo or video with Good Gallery from Explorer (spec 027) want to see it, also when it isn't in a library: a download, a USB stick, a folder they never meant to index. Today the app asks them to add its folder as a library first (REQ-027-03), which indexes a folder they may not want in their gallery and makes them wait for the scan. They see the file at once instead, step through its folder like in any photo viewer, and add the folder as a library only if they want to (#162).

## Requirements

### REQ-033-01 · Show a file outside every library

- **Given** a photo or video that is in no library
- **When** it is opened with the app (REQ-027-01's ways: _Open with_, a double-click on a file type it's the default app for, `good-gallery <file>`), also while the app runs (REQ-027-02)
- **Then** the viewer shows it at once, as it shows an indexed file: original quality, zoom and pan, video playback
- **And** nothing is added to the index, the libraries or the thumbnail cache

### REQ-033-02 · Step through its folder

- **Given** the viewer showing a file outside every library
- **When** the user steps to the previous or next item (←/→, the viewer's buttons)
- **Then** it moves through the photos and videos of the file's folder, without its subfolders, in the gallery's _Name_ order (REQ-007-12)
- **And** files the app doesn't index (REQ-002-01) and housekeeping files (REQ-002-02) are skipped, as in a library

### REQ-033-03 · What the viewer shows for it

- **Given** the viewer showing a file outside every library
- **When** the user opens the info panel
- **Then** it shows what the file itself says: name, folder, date taken, size, dimensions, camera, place, and the rating, People, Places and tags written in it
- **And** a line says the file isn't in a library, with _Add folder as library_ (REQ-033-04)
- **And** rating, tagging, editing, faces and the filmstrip, which need the index, aren't offered for it

### REQ-033-04 · Add its folder from the viewer

- **Given** the viewer showing a file outside every library
- **When** the user clicks _Add folder as library_
- **Then** the folder is added as a library (REQ-001-02) and the viewer keeps showing the file
- **And** once the library's first scan has indexed the file, the viewer shows it as an indexed item: rating, tagging, editing and the filmstrip become available, and stepping moves through the library (REQ-009-02)

### REQ-033-05 · Close

- **Given** the viewer showing a file outside every library
- **When** the user closes it (Escape, the close button)
- **Then** the gallery shows what it showed before the file was opened; nothing of the folder stays in the app

### REQ-033-06 · Files a library skips

- **Given** a photo or video inside a library, in a folder the scan skips (a hidden or housekeeping folder, REQ-002-02)
- **When** it is opened with the app
- **Then** it is shown as a file outside every library (REQ-033-01), instead of the message that it wasn't indexed (REQ-027-01)

## Changes to other specs

- ~~REQ-027-03~~ (a file outside every library offers to add its folder before it is shown) is deprecated in favour of REQ-033-01 and REQ-033-04. REQ-027-04 (files that can't be opened) stays.

## Out of scope

- Rating, tagging, editing, faces and the filmstrip for files outside a library; they need the index. Adding the folder (REQ-033-04) gives all of them.
- Subfolders, other sort orders than by file name, and Explorer's own sort order or selection (Windows doesn't hand it to the app).
- Natural order of numbers in names (`IMG_2` before `IMG_10`): the gallery's _Name_ sort compares letters, and a natural order for it is a change of its own.
- Opening several files at once, or a folder.
- The browser version: it can't be started with a file.

## Assumptions

- A folder opened this way is small enough to list at once (thousands of files, not hundreds of thousands); listing a folder of 10,000 files takes well under a second on a local disk.
- Reading one file's metadata with exiftool takes about 100 ms, so it can be read when the file is shown, not ahead for the whole folder.

## Verification

| REQ        | Test | Result  |
| ---------- | ---- | ------- |
| REQ-033-01 | —    | no test |
| REQ-033-02 | —    | no test |
| REQ-033-03 | —    | no test |
| REQ-033-04 | —    | no test |
| REQ-033-05 | —    | no test |
| REQ-033-06 | —    | no test |
