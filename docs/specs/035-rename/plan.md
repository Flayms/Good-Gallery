# 035 · Rename many files · Plan

## Approach

- **`domain::rename`** (pure): the pattern (plain or regex, case), placeholders and their date formats, applying it to a stem, and checking each new name (REQ-035-02 … -05). It turns the accepted rows into an ordered list of steps: a name that is still in use by another row goes through a temporary name (`.<name>.gg-rename`) first, which handles swaps, chains and case-only changes.
- **`db::media::rename`**: one transaction updates `media.rel_path` of the renamed rows, and `media_edits.source` wherever it pointed at a renamed file or its backup. `(root_id, rel_path)` is unique, so swaps go through the same temporary names inside the transaction.
- **Order of a rename:** the journal is written, then the index rows, then the files. When the watcher reports the change, the old path is in neither the index nor on disk, and the new one is in the index with the same size and date modified, so the path scan has nothing to do (REQ-035-07). A file that can't be renamed gets its row back.
- **Thumbnails:** the cache names a thumbnail by a hash of `(root_id, rel_path, size, mtime, width)` (REQ-005-15). After a file is renamed, its cached thumbnails are renamed to the new key, for every width the cache holds. A miss only costs a new render.
- **Sidecars and backups:** `IMG_1.jpg.xmp` and `IMG_1.xmp` follow the file. The backup at `.good-gallery/originals/<rel_path>` follows too, with its own sidecar.
- **Journal:** `rename-journal.json` in the data folder lists each step's old and new path and whether it is done. It is written before the first step and kept until the next rename, which gives _Undo_ (REQ-035-08). On startup, or when the library is online again, an unfinished journal is completed for steps whose new file exists, and taken back for the rest (REQ-035-09).
- **UI:** a dialog over the gallery, with the search, replacement, two switches and a virtualized preview list (the List view's row approach). The preview is computed on the UI thread: 50,000 regex replacements measure in milliseconds, which is to be checked in the benchmark task.

## Tasks

- [ ] `domain::rename`: pattern, placeholders, checks, steps through temporary names (REQ-035-02, -03, -05, -06)
- [ ] `db::media::rename` with `media_edits.source` (REQ-035-07)
- [ ] Thumbnail cache: move a key's thumbnails (REQ-035-07)
- [ ] `media::rename`: files, sidecars, backups; failures per file (REQ-035-06, -07)
- [ ] Journal, undo, recovery at startup and on reconnect (REQ-035-08, -09)
- [ ] Watcher test: a rename by the app keeps the media id and doesn't render thumbnails (REQ-035-07)
- [ ] Dialog, preview, selection bar entry, toast with _Undo_ (REQ-035-01, -04, -06, -08)
- [ ] Benchmark the preview on 50,000 names (REQ-035-04)

## Decisions

### 2026-11-05 · The index changes before the files

**Decided:** rows are updated first, files second, so the watcher never sees a known file vanish and a new one appear. That would delete the row and lose what exists only in the index: face decisions, an edit's recipe, the thumbnail.
**Rejected:** renaming files first and matching a removed and an added file by size and date afterwards. It guesses, and it fails for identical copies. Pausing the watcher during a rename: events from a share arrive late, and other programs' changes in that time would be lost.

### 2026-11-05 · A journal, not only the index

**Decided:** a file in the data folder records each step before it happens, so a crash between index and file can be repaired, and _Undo_ survives a restart (agreed in #163).
**Rejected:** undo only until the app closes. A crash mid-batch would leave rows and files disagreeing with no record of which.

### 2026-11-05 · The name only

**Decided:** a rename changes the stem; extensions stay (default in #163). A wrong extension would make the file unreadable for the app and others.
**Rejected:** an option for the case of extensions, until someone asks for it.
