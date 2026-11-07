# 038 · Fix dates

**Status:** Draft

## Goal

A camera with a wrong clock, a phone that set no date, scans and messenger copies that lost their metadata: such photos end up in the wrong place in a gallery sorted by date taken. Their filename or another field often still has the right date. The user fixes the date taken of a selection at once, from another field, from the filename, or by shifting it, sees every old and new date before anything is written, and can undo it.

## Requirements

### REQ-038-01 · Fix the dates of a selection

- **Given** a selection of photos in the desktop app
- **When** the user chooses _Fix dates…_ in the selection bar or the context menu (REQ-009-21)
- **Then** a dialog opens for them
- **And** videos in the selection are listed as left out, with the reason (REQ-038-06), and the browser version doesn't offer it

### REQ-038-02 · From another date

- **Given** the dialog
- **When** the user chooses _From another date_ and one of _Date modified_, _Date created_ (`CreateDate`), _Digitized_ (`DateTimeDigitized`) or _GPS time_
- **Then** each photo's new date taken is that date

### REQ-038-03 · From the filename

- **Given** the dialog
- **When** the user chooses _From the filename_
- **Then** a date and time found in the name becomes the new date taken: `yyyyMMdd_HHmmss` (`IMG_20230512_101500`, `PXL_20230512_101500123`), `yyyy-MM-dd HH.mm.ss`, `yyyy-MM-dd`, and the same with `-`, `_`, `.` or nothing between the parts
- **And** the user can enter a pattern of their own instead, with the same format as the rename placeholders (`yyyy-MM-dd HH-mm-ss`, REQ-035-03), where the rest of the name is ignored
- **And** a date without a time gets 12:00, so it stays on its day in every time zone

### REQ-038-04 · Shift

- **Given** the dialog
- **When** the user chooses _Shift_ and enters days, hours and minutes, forwards or back
- **Then** each photo's date taken moves by that amount, which corrects a camera clock that was set wrong or left on another time zone

### REQ-038-05 · Preview

- **Given** the dialog
- **When** anything in it changes
- **Then** a list shows each photo with its old and new date taken, changed rows marked, and the number of changed, unchanged and failing rows above it
- **And** it stays responsive for 50,000 photos

### REQ-038-06 · Dates that can't be used

- **Given** a photo whose new date can't be worked out (no such field, no date in its name, a date that doesn't exist such as 31 February), or would be before 1900 or in the future, or a video
- **When** the preview shows
- **Then** its row says why, and it is left out, while the other rows can still be fixed

### REQ-038-07 · Wall-clock time

- **Given** a photo date without a time zone (REQ-004-03)
- **When** its new date is worked out and written
- **Then** the time shown stays what it was read as, and no zone offset is added
- **And** a source with an offset (`OffsetTimeOriginal`, GPS time) keeps it, written to `OffsetTimeOriginal`

### REQ-038-08 · Writing

- **Given** a preview with changed rows
- **When** the user chooses _Fix_
- **Then** each photo's `DateTimeOriginal` and `CreateDate` (with their subseconds and offset where it has them) are written, in the file or in its sidecar as ratings are (REQ-024-03), in the background, and the dialog closes
- **And** the date modified follows the setting _When saving edits_ (REQ-021-15)
- **And** a file that can't be written stays as it was, the rest carry on, and a toast counts them and names the first with the reason
- **And** the gallery shows the photos at their new place, and their date groups follow (spec 016)

### REQ-038-09 · Undo

- **Given** a date fix that finished
- **When** the user chooses _Undo_ in its toast, or _Undo last fix_ in the dialog until the next fix
- **Then** every photo it changed gets the dates it had back, also after the app was closed
- **And** a photo whose dates were changed by something else since then keeps those, and the toast says how many were skipped

## Out of scope

- Videos: QuickTime stores its dates in UTC, and many cameras write local time there anyway, so fixing them needs its own rules. Left for a later step.
- Fixing date modified alone, GPS positions or other metadata.
- Saved fixes to apply again (#207).
- Fixing dates automatically on new files.

## Assumptions

- exiftool writes `DateTimeOriginal`, `CreateDate` and `OffsetTimeOriginal` in every format the app writes ratings to (JPEG, PNG, WebP, TIFF, HEIC, and the sidecar of raw files).
- 1900 to now covers every real photo, scans of old prints included. A scan dated earlier is rare enough to fix in another tool.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-038-01 | —    | no test |
| REQ-038-02 | —    | no test |
| REQ-038-03 | —    | no test |
| REQ-038-04 | —    | no test |
| REQ-038-05 | —    | no test |
| REQ-038-06 | —    | no test |
| REQ-038-07 | —    | no test |
| REQ-038-08 | —    | no test |
| REQ-038-09 | —    | no test |
