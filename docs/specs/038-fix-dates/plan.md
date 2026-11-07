# 038 · Fix dates · Plan

## Approach

- **`domain::date_fix`** (pure): the three sources (another field, the filename, a shift), the built-in filename patterns and a user pattern in the rename date format (shared with `domain::rename`'s parser), and the checks (no value, impossible date, before 1900, in the future, a video). In: a row per photo with the dates the index and the read metadata have. Out: a new date or the reason it fails (REQ-038-02 … -07).
- **Reading:** the index has only the resolved date taken. When the dialog opens, a thread reads the EXIF date tags a fix replaces, `XMP-photoshop:DateCreated` and `GPSDateTime` for the selection through a small exiftool `-stay_open` pool of its own (two processes, started when the dialog first opens), 100 files per run, so the preview fills as they come. Rows of photos not read yet say _Reading…_.
- **Writing:** `media::dates::write`, one exiftool per file through `replace::write_metadata` like ratings and tags, so a failed write leaves the original. It writes into the file only: the index reads the file's date before the sidecar's (REQ-004-03), and _Undo_ couldn't restore sidecar values it never read.
- **Journal and undo:** `date-journal.json` in the data folder, a line per event like the rename journal (spec 035). The first line lists each change with the old values of the six EXIF tags written (`null` for a missing one) and its date modified; a line per written file records its size and date modified afterwards. _Undo_ writes the old values and date modified back where the file still has the recorded size and date, skips the others (REQ-038-09), and removes the journal.
- **Index:** written files are marked for reading (`db::media::mark_for_reading`) and their paths go to a path scan, as for tags. The gallery reloads when the scan wrote them, so sorting and date groups follow.
- **UI:** `fix_dates.rs`, a dialog like the rename dialog: the three sources as radio rows with their fields (a pattern, a date field picker, days / hours / minutes), the counts row and a virtualized preview list (name, old date, new date or why not). Entries: a calendar icon beside _Rename…_ in the selection bar, _Fix dates…_ in the context menu (gallery and viewer), and _Undo_ in the toast.

## Tasks

- [x] `domain::date_fix`: sources, filename patterns, user pattern, checks, wall-clock and offsets (REQ-038-02, -03, -04, -06, -07)
- [x] Share the date format tokens between `domain::rename` and `domain::date_fix` (REQ-038-03)
- [x] `media::dates`: read the date fields of many files per exiftool run; write into the file (REQ-038-02, -08)
- [x] Journal and undo, skipping files changed since (REQ-038-09)
- [x] Dialog, selection bar and context menu entries, toast with _Undo_ (REQ-038-01, -05, -08, -09)
- [x] Preview of 50,000 photos within a debug-build budget (REQ-038-05)
- [ ] Write many files per exiftool run (an argfile through the pool) instead of one process per file
- [ ] Share the dialog frame, counts row and list with the rename dialog

## Decisions

### 2026-11-07 · Photos first

**Decided:** videos are left out, and their rows say so. QuickTime's dates are UTC by its standard, but many cameras and phones write local time there. The app reads zoneless video dates as UTC (REQ-004-03), so a fix would need to know per camera which one it is.
**Rejected:** writing videos the same way as photos. It would shift some of them by the time zone's offset without saying so.

### 2026-11-07 · Undo stores the old values

**Decided:** date fixes are undoable like renames (open question 2 in #163). The journal is small: a few tags per photo.
**Rejected:** no undo, with the edit backups of spec 021 as the safety net. They exist only for edited photos.

### 2026-11-07 · No time zone conversion

**Decided:** dates stay the wall-clock time they show (default in #163). _Shift_ corrects a camera left on another zone.
**Rejected:** a time zone picker that converts. It needs to know which zone the camera was on, which the user rarely knows better than the hours to shift by.

### 2026-11-07 · One exiftool per file, for now

**Decided:** writing uses the same one-process-per-file path as ratings and tags (`replace::write_metadata`), which keeps the original whenever a write fails. A thousand photos take a minute or two in the background.
**Rejected for now:** an argfile through the `-stay_open` pool: it writes in place, so it needs its own way to keep the original on failure. Left as an open task.

### 2026-11-07 · The file only, no sidecar

**Decided:** the dates go into the file. The index reads the file's date before its sidecar's (REQ-004-03), so the gallery follows, and _Undo_ restores exactly what it read.
**Rejected:** writing an existing sidecar too, as ratings do: the dialog doesn't read sidecars, so _Undo_ couldn't put their values back.

### 2026-11-07 · Its own dialog, not shared with rename yet

**Decided:** `fix_dates.rs` repeats the dialog frame and the list layout of `rename.rs` (about 60 lines) rather than splitting them out in the same change.
**Rejected for now:** a shared module, which would touch the rename dialog and its snapshots in a feature that doesn't need it. Left as an open task.
