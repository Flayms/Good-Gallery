# 024 · Rating · Plan

## Approach

### Decisions

#32 left these open. I decided them as follows (the user said to go ahead with my own judgment):

1. **Where the rating goes: where the reader takes it from.** `meta` takes the rating from the first source that has a `Rating` tag, the file before its sidecar (REQ-004-05). So a photo is written in place (all supported image formats are writable by exiftool). A video goes into its sidecar, because rewriting a large video over a network share is slow and exiftool can't write WebM or Matroska. The exception is a video that carries a rating itself, which is written in place, since a sidecar rating would never be read. An existing sidecar with a rating gets the same value, so Lightroom or digiKam don't keep showing the old one.
2. **New sidecars are named `<file name>.xmp`** (`clip.mp4.xmp`, the darktable / digiKam form). The Lightroom form `clip.xmp` would also be read as the sidecar of a `clip.mov` beside it, and our reader prefers the long form anyway (REQ-004-10). An existing sidecar is used whatever its form.
3. **exiftool writes, with `-Rating=N -XMP-xmp:Rating=N -P -overwrite_original`.** `-Rating` updates every group that already has a rating (EXIF `IFD0:Rating` from Windows, XMP), and `-XMP-xmp:Rating` makes sure the XMP one exists. `-P` keeps the date modified to the second, and the app then restores the exact time, which the index compares. exiftool writes to a temp file and renames it, so a failed write leaves the file as it was. A removed rating is written as 0, the XMP value for "unrated", not deleted, so that a sidecar's rating doesn't take its place.
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
