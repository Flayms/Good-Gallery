# 038 · Fix dates

**Status:** Implemented

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
- **When** the user chooses _From another date_ and one of _Date modified_, _Date created_ (EXIF `CreateDate`, also called DateTimeDigitized), _XMP date created_ (`XMP-photoshop:DateCreated`) or _GPS time_
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
- **And** a source with an offset keeps it, written to `OffsetTimeOriginal`; GPS time, which is UTC, is shown and written in the photo's own offset, else the computer's
- **And** a photo's existing offset stays when the new date has none

### REQ-038-08 · Writing

- **Given** a preview with changed rows
- **When** the user chooses _Fix_
- **Then** each photo's `DateTimeOriginal` and `CreateDate` are written into the file (which the index reads before its sidecar, REQ-004-03), in the background, and the dialog closes; subseconds the old date had are removed, and an offset is written only as REQ-038-07 says
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
| REQ-038-01 | `crates/app/tests/fix_dates.rs` › req_038_01_req_038_05_the_preview_shows_old_and_new_dates, req_038_01_the_context_menu_opens_the_dialog (snapshot `fix_dates_dialog`); `crates/domain/src/date_fix.rs` › req_038_06_dates_that_cant_be_used | pass |
| REQ-038-02 | `crates/domain/src/date_fix.rs` › req_038_02_req_038_07_takes_another_date; `crates/media/src/dates.rs` › req_038_02_reads_the_dates_of_several_photos (exiftool) | pass |
| REQ-038-03 | `crates/domain/src/date_fix.rs` › req_038_03_finds_dates_in_names, req_038_03_reads_a_pattern_of_the_users_own; `crates/app/tests/fix_dates.rs` › req_038_01_req_038_05_the_preview_shows_old_and_new_dates | pass |
| REQ-038-04 | `crates/domain/src/date_fix.rs` › req_038_04_shifts_the_date_taken; `crates/app/tests/fix_dates.rs` › req_038_01_req_038_05_the_preview_shows_old_and_new_dates | pass |
| REQ-038-05 | `crates/domain/src/date_fix.rs` › req_038_05_previews_50000_photos_quickly; `crates/db/src/media.rs` › req_038_05_lists_the_items_to_fix; `crates/app/tests/fix_dates.rs` › req_038_01_req_038_05_the_preview_shows_old_and_new_dates | pass |
| REQ-038-06 | `crates/domain/src/date_fix.rs` › req_038_06_dates_that_cant_be_used; `crates/app/tests/fix_dates.rs` › req_038_01_req_038_05_the_preview_shows_old_and_new_dates | pass |
| REQ-038-07 | `crates/domain/src/date_fix.rs` › req_038_02_req_038_07_takes_another_date, req_038_07_reads_exiftool_dates; `crates/media/src/dates.rs` › req_038_08_req_038_09_writes_and_restores_the_date_taken | pass |
| REQ-038-08 | `crates/media/src/dates.rs` › req_038_08_req_038_09_writes_and_restores_the_date_taken (exiftool); `crates/app/tests/fix_dates.rs` › req_038_08_req_038_09_fixes_and_undoes | partial: the gallery moving the photos to their new date follows from the path scan, which the app tests record but don't run |
| REQ-038-09 | `crates/media/src/dates.rs` › req_038_08_req_038_09_writes_and_restores_the_date_taken, req_038_09_the_journal_keeps_what_undo_needs; `crates/app/tests/fix_dates.rs` › req_038_08_req_038_09_fixes_and_undoes, req_038_09_undo_leaves_files_changed_since | pass |
