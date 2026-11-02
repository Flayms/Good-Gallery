# 024 · Rating · Plan

## Approach

### Decisions

#32 left these open. I decided them as follows (the user said to go ahead with my own judgment):

1. **Where the rating goes: where the reader takes it from.** `meta` takes the rating from the first source that has a `Rating` tag, the file before its sidecar (REQ-004-05). So a photo is written in place (all supported image formats are writable by exiftool). A video goes into its sidecar, because rewriting a large video over a network share is slow and exiftool can't write WebM or Matroska. The exception is a video that carries a rating itself, which is written in place, since a sidecar rating would never be read. An existing sidecar with a rating gets the same value, so Lightroom or digiKam don't keep showing the old one.
2. **New sidecars are named `<file name>.xmp`** (`clip.mp4.xmp`, the darktable / digiKam form). The Lightroom form `clip.xmp` would also be read as the sidecar of a `clip.mov` beside it, and our reader prefers the long form anyway (REQ-004-10). An existing sidecar is used whatever its form.
3. **exiftool writes, with `-Rating=N -XMP-xmp:Rating=N -P -overwrite_original`.** `-Rating` updates every group that already has a rating (EXIF `IFD0:Rating` from Windows, XMP), and `-XMP-xmp:Rating` makes sure the XMP one exists. `-P` keeps the date modified to the second, and the app then restores the exact time, which the index compares. exiftool writes to a temp file and renames it, so a failed write leaves the file as it was. (Superseded on 2026-11-02, #140: exiftool's rename lost files on an rclone mount; see the decision below.) A removed rating is written as 0, the XMP value for "unrated", not deleted, so that a sidecar's rating doesn't take its place.
4. **The index is updated directly, after the file.** With the date modified kept and XMP padding absorbing the change, the file's size may not change either, so a changes scan might not read the file again. The app writes the rating into the `media` row once exiftool succeeded, then refreshes. A later scan reads the same value from the file.
5. **Keys `1`–`5`, pressing the same one again removes the rating.** `0` already resets the zoom in the viewer (REQ-009-05), so there is no "unrated" key; toggling is how Lightroom's and Windows Photos' star widgets work too.
6. **The stars become buttons in both info views** (viewer panel and sheet), always shown, outlined when unrated; the hovered star and those before it fill in as a preview. Not in the browser version, which never writes files (REQ-024-05).

### Layers

- `domain::rating`: `toggled(current, stars) -> Option<i64>` (REQ-024-01/02) and `Target::of(kind, file_has_rating) -> Target { File, Sidecar }` (REQ-024-03); `sidecar_name(file_name)`.
- `media::rating::write(exiftool, library, rel_path, kind, rating)`: safe path (REQ-011-08), reads the file's rating to decide, finds an existing sidecar in the folder (`domain::media::find_sidecar`), runs exiftool with the arguments on stdin (REQ-011-09). Shares the exiftool runner with `media::edit`.
- `db::media::set_rating(conn, id, rating)`.
- `app`: `AppState::rate(media, rating)` writes in the background (`Event::Rated`), then `set_rating`, drops the cached detail and refreshes; on failure a toast (REQ-024-04). The viewer turns keys into `Action::Rate`, the info views return `InfoAction::Rate`.

### Changes to other specs

- **REQ-004-01** (read-only): rating an item writes, besides saving and reverting edits.
- **REQ-009-08**: the rating in the info panel can be changed (spec 024).
- Glossary: _Rating_ mentions where it is written.

## Tasks

- [x] `domain::rating` with tests (REQ-024-01, REQ-024-02, REQ-024-03)
- [x] `media::rating::write` with exiftool tests: photo, photo with a sidecar rating, video, video with its own rating, removal, read-only file (REQ-024-03, REQ-024-04)
- [x] `db::media::set_rating`
- [x] App: keys, stars, background write, index update, toast; UI tests and snapshots (REQ-024-01, REQ-024-02, REQ-024-04, REQ-024-05)
- [x] Spec changes to 004 and 009, glossary, architecture note
- [x] `media::replace`: new versions written beside the file, moved in without renaming over it, read back; used by ratings, tags, saving and reverting edits (REQ-024-06, REQ-026-07, REQ-021-16, #140)
- [x] App: one writer thread for every file write (REQ-024-07, REQ-026-07)

## Decisions

### 2026-11-02 · Files are replaced without renaming over them, and read back (#140)

**Decided:** every write into a library file goes through `media::replace`. exiftool writes the new version into a new dot-named file beside the original (`-o`, never `-overwrite_original`); it gets the original's date modified (to the sub-second), date created (Windows) and permissions and is synced. Then the original is moved aside (`.<name>.gg-old.<ext>`), the new version renamed into the free name, read back to the end and compared with what was written (length and hash); only then is the original deleted. If the comparison fails, the original goes back, again into a free name, and the user is told. A write the app was ended in is finished by the next write to that file: a missing file gets its original back, a leftover original beside a file is dropped. All writes (ratings, tags, a selection's changes, saving and reverting edits) run on one writer thread, in the order they were made.
**Why:** an rclone mount on Windows lost 41 photos (#140): renaming over an existing file failed in its cache, was reported as done, and an empty file was uploaded. Right after such a rename the file even reports the new size; only reading it shows the loss, so a size check isn't enough. A rename into a free name worked every time on the same mount. Separate threads per write let two changes to one file collide (exiftool's "Temporary file already exists") or overwrite each other's tags, since a tag write reads the keywords and writes the whole list back.
**Rejected:** `-overwrite_original` (renames over the file, and deletes the original when that rename fails); `-o` followed by a rename over the original (lost 3 of 3 files on the same mount); checking only the size (the lost file reports the right size); checking with exiftool (another process start, ≈ 0.3 s on Windows, per write); a lock per file instead of one writer (doesn't keep the order); syncing only (doesn't help against a file system that drops data it acknowledged).
**Trade-off:** each write reads the new version twice more (before and after the rename); on a share that is one more transfer of the file per write. The original is missing from its name for the moment between the two renames.

### 2026-11-01 · Ratings written where they are read

**Decided:** a rating set by hand (spec 024) is written by a one-shot exiftool (`media::rating`, sharing `exiftool::run_once` with saving edits) where `meta` reads it first: into photos, and into a video's sidecar (an existing one, else `<file name>.xmp`) unless the video has a rating of its own; an existing sidecar with a rating is kept in agreement. `-Rating=N -XMP-xmp:Rating=N` updates every rating the file has and makes sure the XMP one exists, the date modified is restored to the exact time afterwards (exiftool's `-P` keeps whole seconds), and 0 means unrated, so no other source takes its place. The index is updated (`db::media::set_rating`) only after the file, because with the date kept the file's size may not change and a scan wouldn't read it again; the row is marked for reading again anyway, since a new sidecar is only noted then.
**Rejected:** writing into videos (slow over a network, impossible for WebM and Matroska), the Lightroom sidecar name `clip.xmp` (also read as the sidecar of a `clip.mov` beside it), keeping ratings in the index only (lost with the index, unseen by other tools).
