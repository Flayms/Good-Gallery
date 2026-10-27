# M04 · Indexing, change detection, metadata

**Status:** Approved (2026-10-26, self-approved: the user asked to work through the milestones without their input)
**Specs:** 002, 003, 004 (and REQ-011-08 … 10, REQ-001-05 rest)

Too large for one part (≈ 2 000 lines of reference code plus the oracle). Split:

- **M4a · Metadata:** tags, exiftool process, extraction rules, index writer.
- **M4b · Scanning:** walk, retries, full and quick scans, scan queue, indexer thread, offline libraries, progress in the app.
- **M4c · Change detection and oracle:** watcher, changes scans, scheduled quick scans, reconnects, oracle diff against the reference.

## Approach

### M4a · Metadata

**Tags (`domain::tags`).** `clean_tag` (NFKC, trim, collapse whitespace) and `normalize_tag` (+ lower case) as in `src/shared/tags.ts`; category roots for later (People, Places). Pure, in `domain` (`unicode-normalization`). JavaScript's `toLowerCase` and Rust's `to_lowercase` agree for everything but a few context-dependent cases (final sigma); the tests pin the reference's examples.

**exiftool.** `media::exiftool::ExifTool`: a pool of long-running `exiftool -stay_open True -@ -` processes (pool size = parallel file reads). Per read, the arguments exiftool-vendored's `ReadTask` (v38) builds from its default options: `-charset filename=utf8 -json -fast -api <UTF-8 repair filter> -api struct=1 -use MWG -api keepUTCTime -*Duration*# -GPSAltitude# -GPSLatitude# -GPSLongitude# -GPSPosition# -GeolocationPosition# -Orientation# -all <file> -ignoreMinorErrors`, then `-execute<n>`; the reply ends with `{ready<n>}`. (The first draft used `struct=0` and missed `-fast`; the M4a review caught it.) Arguments go through stdin one per line, never through a shell; paths are always absolute (drive or UNC), so a file name can't start with `-` and be taken as an option, and Windows file names can't contain newlines (REQ-011-09). A read that takes longer than 60 s kills and replaces the process. The executable is found via `GOOD_GALLERY_EXIFTOOL`, then `exiftool\exiftool.exe` next to the app, then `PATH` (bundling is M11). Tests that need exiftool are skipped with a message when it's missing, unless `GG_REQUIRE_HELPERS=1` (set in CI) makes them fail.

**Extraction (`media::metadata`).** A port of `extractMetadata` / `extractTags` working on exiftool's JSON (`serde_json::Value`): dimensions swapped for orientations 5–8 and rotations 90 / 270, first valid date of `SubSecDateTimeOriginal`, `DateTimeOriginal`, `CreateDate`, `MediaCreateDate` (file before sidecar), duration, rating, tag paths (hierarchies, face regions, flat keywords minus those already named; numbers kept as text). Values of unexpected types are ignored, never panics (REQ-011-10).

**Difference from the reference (face regions).** With `struct=1`, exiftool returns MWG and Microsoft face regions as structs; the reference looks for the flattened fields only and so never finds them. REQ-004-09 wins: the port reads `RegionInfo.RegionList[]` (type `Face`) and `RegionInfoMP.Regions[]` as well. The oracle shows this as extra `People` tags on files with regions.

**Dates.** exiftool-vendored turns date strings into milliseconds with time-zone inference. Ported: an offset in the string itself (`+02:00`, `Z`) wins; otherwise the file's zone from `OffsetTimeOriginal` / `OffsetTime` / `OffsetTimeDigitized` / `TimeZone`, or inferred from another capture date that carries an offset; otherwise QuickTime video dates are UTC (`defaultVideosToUTC`); otherwise the computer's local zone. **Not ported:** the GPS-based zone lookup (geo-tz, a 30 MB time-zone shape database). Photos with GPS but no offset and no other zone hint therefore get the local zone where the reference used the GPS position's zone. Recorded as a deviation in `docs/architecture.md`; the M4c oracle quantifies it on the fixtures.

**Writer (`db::writer`).** Port of `IndexWriter`: upsert media (thumbnail reset only when size or mtime changed), replace tag links, tag path → leaf id with the first parent winning and cycle refusal, caches cleared on rollback, `prune_tags` bottom-up, folder bookkeeping (`folders`, `existing`, `existing_in`, `save_folders`, `remove_folders` with the `'/'` … `'0'` range trick), `set_root_status`, `remove`. `METADATA_VERSION = 1`, the reference's value: both apps must agree, or each would re-read everything the other wrote.

### M4b · Scanning

**Walk and retry** (`indexer::walk`, `indexer::retry`): depth-first listing per folder with mtime taken before listing, housekeeping folders, dotfiles and symlinks skipped (`std::fs::read_dir` + `file_type()` which doesn't follow links), sidecar map, `skip` / `from` / `descend` hooks; retries for transient errors (anything but not found, access denied, not a directory, invalid name, name too long) with 250 ms × 2ⁿ backoff, 3 times. Windows error codes are mapped to the reference's categories.

**`scan_root`**: the reference's algorithm on threads instead of promises: the walking thread feeds a bounded channel of files to `io_concurrency` workers (stat + metadata); results go to a writer loop that flushes batches of ≤ 250 files or every 2 s; a shared `AtomicBool` cancels (library removed) and a second one marks "offline" (a failure plus a failed reachability check). Outcome `Completed | Offline | Cancelled | Failed`. Removals (vanished files, vanished folders, folder bookkeeping, tag pruning) only after a completed scan.

**Scan queue and indexer thread** (`indexer::Indexer`): a port of `IndexerController` (one queued scan per library, merged by thoroughness, changes merged by path up to 5 000, running scan covering new requests) plus the worker loop of `src/indexer/index.ts`. One thread owns the write connection and runs scans one at a time; each scan runs under `catch_unwind` (REQ-002-13). Commands in (`Scan`, `Remove`, `Configure`), events out (`Progress`, `Done`, `Changed`) over channels; `Indexer` implements `ScanRequests`, so M3's library operations queue real scans. The app shows "Indexing <library>" / "Checking <library>" with counts in the gallery until the sidebar exists (M7), and refreshes its counts after writes (REQ-006-11, partial).

### M4c · Change detection and oracle

**Watcher** (`indexer::watcher`): `notify` (ReadDirectoryChangesW) recursive per library, `changed_path` filtering (housekeeping, `Thumbs.db`, `desktop.ini`), debounce 2 s / max 10 s, unknown changes → quick scan, errors → quick scan and stop. Started after a completed scan, stopped when a library goes offline or is removed.

**Schedule** (`indexer::schedule`): quick scan of all libraries at startup and every quick check interval (full scan for libraries with rows behind `METADATA_VERSION`), offline libraries re-checked every minute (one check at a time per library, see `docs/known-pitfalls.md`).

**Oracle** (`cargo xtask oracle`): generates a fixture library (images with EXIF / IPTC / XMP keywords, hierarchies, face regions, ratings, dates with and without offsets, sidecars of both namings, rotated images, a video, housekeeping folders, dotfiles), indexes it with the reference (its `scanRoot` + `IndexWriter` + exiftool source, bundled with the reference's own vite into one script and run with Node) and with this indexer, and diffs the databases ignoring ids and timestamps. Exit: no differences, apart from documented deviations.

## Reference

- `src/shared/tags.ts` (+ test), `src/shared/metadata.ts`, `src/indexer/metadata.ts` (+ test), `media-types.ts`.
- `node_modules/exiftool-vendored/dist/{ReadTask,DefaultExifToolOptions,ExifDateTime,Timezones}.js`: arguments, numeric tags, date parsing and zone inference.
- `src/indexer/writer.ts`, `walk.ts`, `retry.ts`, `scan.ts` (+ test), `protocol.ts`, `index.ts`, `watcher.ts` (+ test).
- `src/main/indexer.ts` (+ test), `rescan.ts`, `reachability.ts`.
- `e2e/smoke.spec.ts`: fixture generation (sharp + exiftool writes).

## Tasks

### M4a

- [x] 1. `domain::tags`: clean / normalize, category roots (REQ-004-11).
- [x] 2. `media::exiftool`: process pool, arguments, timeouts, executable lookup (REQ-011-09).
- [x] 3. `media::metadata`: extraction rules ported with the reference's test cases (REQ-004-02 … 09, REQ-011-10).
- [x] 4. Dates and time zones (REQ-004-03).
- [x] 5. Sidecars: lookup rule and merge (REQ-004-10).
- [x] 6. `db::writer`: media upsert, tag paths, pruning, folders (REQ-004-07, REQ-004-08, REQ-002-14).

### M4b

- [x] 7. `indexer::walk` + `retry` (REQ-002-01, REQ-002-02).
- [x] 8. `indexer::scan`: full and quick scans, batches, removals, offline, unreadable files and folders (REQ-002-03 … 06, -09 … 12, REQ-003-08, REQ-001-05).
- [x] 9. Scan queue and indexer thread with `catch_unwind` (REQ-002-07, REQ-002-13).
- [x] 10. App: indexer started with the index, scan progress shown, counts refreshed (REQ-002-08 partial, REQ-006-11 partial).

### M4c

- [ ] 11. Watcher with debounce and filtering (REQ-003-01 … 03, -05, -06).
- [ ] 12. Changes scans and changes during a scan (REQ-003-04, REQ-003-07).
- [ ] 13. Schedule: startup and periodic quick scans, metadata-version upgrades, offline re-checks (REQ-003-09, REQ-003-10, REQ-002-05).
- [ ] 14. Oracle harness and diff on the fixture library.
- [ ] 15. Docs: spec Verification tables, architecture, pitfalls, progress.

## Exit criteria

From `docs/rewrite/plan.md`: oracle diff is empty on the fixture library; 002–004 REQs pass.

Here: empty apart from the documented GPS time-zone deviation (if the fixtures exercise it).
