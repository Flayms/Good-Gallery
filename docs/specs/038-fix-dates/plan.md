# 038 · Fix dates · Plan

## Approach

- **`domain::date_fix`** (pure): the three sources (another field, the filename, a shift), the built-in filename patterns and a user pattern in the rename date format (shared with `domain::rename`'s parser), and the checks (no value, impossible date, before 1900, in the future, a video). In: a row per photo with the dates the index and the read metadata have. Out: a new date or the reason it fails (REQ-038-02 … -07).
- **Reading:** the index has only the resolved date taken. When the dialog opens, a thread reads the extra fields (`CreateDate`, `DateTimeDigitized`, `GPSDateTime`, `OffsetTimeOriginal`) for the selection through the exiftool `-stay_open` pool, in batches, so the preview fills as they come. _Shift_ and _From the filename_ need none of this and show at once.
- **Writing:** `media::dates::write` through the same pool, one argfile per batch, not one exiftool per file (#163 notes: a thousand files take a minute or two otherwise). It writes into the file or the sidecar by the same rule as ratings (`media::rating`), and through `replace::write_metadata`, so a failed write leaves the original.
- **Journal and undo:** `date-journal.json` in the data folder, kept until the next fix, like the rename journal (spec 035). Each row records the old values of the tags written (or that they were missing) and the file's size and date modified after writing. _Undo_ writes the old values back where the file still has the size and date the journal recorded, and otherwise skips it (REQ-038-09).
- **Index:** written files are marked for reading (`db::media::mark_for_reading`) and their paths go to a path scan, as for tags. The gallery reloads when the scan wrote them, so sorting and date groups follow.
- **UI:** a dialog like the rename dialog: a source picker with each source's fields, and the same virtualized preview list with old and new dates. It shares the dialog frame, the counts row and the list with `rename.rs`, which are split out for this.

## Tasks

- [ ] `domain::date_fix`: sources, filename patterns, user pattern, checks, wall-clock and offsets (REQ-038-02, -03, -04, -06, -07)
- [ ] Share the date format parser between `domain::rename` and `domain::date_fix` (REQ-038-03)
- [ ] `media::dates`: read the extra fields in batches; write in batches into file or sidecar (REQ-038-02, -08)
- [ ] Journal and undo, skipping files changed since (REQ-038-09)
- [ ] Split the rename dialog's frame, counts and list out for both dialogs (REQ-038-05)
- [ ] Dialog, selection bar and context menu entries, toast with _Undo_ (REQ-038-01, -05, -08, -09)
- [ ] Benchmark the preview on 50,000 photos (REQ-038-05)

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
