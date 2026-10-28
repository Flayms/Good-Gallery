# 002 · Indexing · Plan

Records the approach that was taken. Decisions that outlive this spec are in `docs/architecture.md`.

## Approach

- **Walk and retry** (`indexer::walk`, `indexer::retry`): depth-first listing per folder with the folder's mtime taken before listing; housekeeping folders, dotfiles and symlinks are skipped (`read_dir` + `file_type()`, which doesn't follow links); sidecars are mapped to their media; `skip` / `from` / `descend` hooks serve quick and changes scans. Transient errors (anything but not found, access denied, not a directory, invalid name, name too long) are retried 3 times with 250 ms × 2ⁿ backoff. Windows error codes are mapped to these categories.
- **`scan_root`** (`indexer::scan`): the walking thread owns all index access (lookups, batched writes, removals at the end); `io_concurrency` scoped worker threads stat files and read metadata, connected by a bounded channel (backpressure) and a result channel. Batches of ≤ 250 files or every 2 s are written while the scan runs (REQ-002-06). An `AtomicBool` cancels (library removed), a second one marks "offline" (a failure plus a failed reachability check). Outcome `Completed | Offline | Cancelled | Failed`. Removals (vanished files and folders, folder bookkeeping, tag pruning) happen only after a completed scan.
- **Writer** (`db::writer::IndexWriter`): upserts media (thumbnail reset only when size or mtime changed), replaces tag links, resolves tag paths to leaf ids (first parent wins, cycles refused), prunes unused tags bottom-up, keeps the folder bookkeeping (`'/'` … `'0'` range trick for subtrees) and the library status.
- **Scan queue and indexer thread** (`indexer::Indexer`): one queued scan per library, merged by thoroughness; changes merged by path up to 5,000; a running scan covers new requests. One thread owns the write connection and runs scans one at a time. Commands are method calls from any thread, results come back as `IndexerEvent`s over a channel and wake the UI through a callback.
- **Failures** (REQ-002-13): a panic while reading one file costs that file (`catch_unwind` per job); a panic anywhere else ends the scan as `Failed`, and the next scan runs.
- **Progress** (REQ-002-08): "Indexing <library>" / "Checking <library>" with counts and waiting scans, drawn over the bottom-left corner of the window; the library's sidebar entry shows a spinner. Gallery counts refresh after the first written files, every 5 s, and at the end.

## Tasks

- [x] Walk and retry (REQ-002-01, -02)
- [x] Full and quick scans, batches, removals, offline, unreadable files (REQ-002-03 … -06, -09, -10, -12, -14)
- [x] Scan queue and indexer thread with `catch_unwind` (REQ-002-07, -13)
- [x] Progress in the app, counts refreshed (REQ-002-08)
- [ ] Test for folders that can't be listed (REQ-002-11; hard to set up on Windows without admin rights)
- [ ] Test for files that can't be accessed at all (REQ-002-12)
- [ ] Show the progress as a sidebar footer row and test the library's spinner (REQ-002-08)
