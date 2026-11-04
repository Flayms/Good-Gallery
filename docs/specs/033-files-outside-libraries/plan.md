# 033 · Files outside libraries · Plan

## Approach

The viewer is built on a `Gallery`: positions, ids, tile data, details and originals all come through it (`viewer.rs` reaches the gallery in about a hundred places). Instead of teaching the viewer a second source, a file outside every library gets a **scratch index**: a temporary SQLite file of the same schema, holding one library (the file's folder), filled by the existing scan code and read by a second `Gallery`. The viewer runs on that gallery unchanged; what needs the real index (rating, tagging, editing, faces, filmstrip) is switched off while it does.

- **Scratch index.** `scratch/<random>.db` in the data folder, created when such a file is opened and deleted when the viewer closes or the app starts (a crash leaves at most one). It never touches `gallery.db` (REQ-033-01).
- **Filling it.** A worker thread with its own `IndexWriter` (not the indexer's queue, so a running library scan doesn't hold it up) scans in two steps: the opened file alone first, so the viewer can show it after one metadata read; then the rest of the folder. A new `ScanScope::Folder` lists one folder without walking into subfolders (REQ-033-02); `Changes` with one path covers the first step. Housekeeping rules are the scan's own (REQ-002-01, REQ-002-02).
- **The second gallery.** `Gallery::new` over the scratch file's connection, searched by `SortBy::Name` ascending (REQ-033-02), without a thumbnail service (no tiles are drawn, the filmstrip is off). `AppState` keeps it in a `Loose { gallery, folder, file, scratch }`; the viewer code paths in `lib.rs` that take `state.gallery` take "the viewer's gallery" instead.
- **Capabilities.** `Viewer` gets an `indexed: bool`: false hides rating, tags, edit, faces and the filmstrip, and the info panel shows the "not in a library" line with _Add folder as library_ (REQ-033-03).
- **Adding the folder.** `add_library` as today; the viewer keeps the scratch gallery until the library's first scan has the file (the `pending_open` path of REQ-027-01, with REQ-002-18's head start), then reopens on the real gallery at the file and drops the scratch index (REQ-033-04).
- **Skipped folders.** `find_pending_open`'s "wasn't indexed" branch hands the file to the scratch path instead of the error (REQ-033-06).

## Tasks

- [x] 1. `ScanScope::Folder`: one folder, no subfolders; scan tests (REQ-033-02)
- [x] 2. Scratch index: create, fill in two steps on a worker, delete on close and at startup; tests in `app` with a temp data folder (REQ-033-01, REQ-033-05)
- [x] 3. `AppState::open_file` outside every library opens the scratch gallery and the viewer at the file; `lib.rs` routes the viewer to its gallery; kittest: shown at once, nothing in `gallery.db` (REQ-033-01)
- [x] 4. Stepping in name order through the folder; kittest (REQ-033-02)
- [x] 5. `Viewer::indexed`: what is hidden, the info panel's line and button; snapshot (REQ-033-03)
- [x] 6. _Add folder as library_ and the switch to the indexed item; kittest (REQ-033-04)
- [x] 7. Close returns to the previous gallery and deletes the scratch file (REQ-033-05)
- [x] 8. Skipped folders use the scratch path (REQ-033-06)
- [x] 9. Deprecate REQ-027-03, update its tests; README, glossary (_scratch index_), architecture

## Decisions

### 2026-11-03 · A scratch index instead of a second viewer source

**Decided:** files outside every library are read into a temporary index of the same schema, shown by a second `Gallery`, so the viewer, its originals, video playback and info panel work unchanged. The scan code that fills it is the indexer's own, so the same files are skipped and the same metadata is read.
**Rejected:** a `ViewerSource` trait with a folder-listing implementation: every place the viewer reads the gallery would need a second implementation, and they would drift apart. A hidden library in `gallery.db`: it writes the user's index, survives a crash, and would show up in searches, counts and the sidebar unless every query excluded it.

### 2026-11-03 · The folder only, in name order

**Decided:** stepping goes through the file's folder without subfolders, by file name (the gallery's _Name_ sort), as Windows' Photos app does by default. Its natural order of numbers (`IMG_2` before `IMG_10`) is left out: the _Name_ sort uses an index on the lowercase name, and a natural collation can't use it, which slows sorting a large library.
**Rejected:** Explorer's current sort order (Windows doesn't pass it to the app opened with a file); date order (a folder of downloads has no meaningful dates; name order is what users see in Explorer by default); subfolders (a file on a drive's top level would pull in the whole drive).
