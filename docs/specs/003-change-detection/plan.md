# 003 · Change detection · Plan

Records the approach that was taken. Decisions that outlive this spec are in `docs/architecture.md`.

## Approach

- **Watcher** (`indexer::watcher::LibraryWatcher`): `notify` (ReadDirectoryChangesW, which SMB shares support), recursive per library. Housekeeping paths, `Thumbs.db` and `desktop.ini` are filtered out; reports are debounced (2 s, at most 10 s). Reports become a changes scan, or a quick scan for unknown changes and more than 5,000 paths. When a watcher fails, it is dropped and a quick scan is queued.
- **Lifecycle:** the indexer owns the watchers. A completed scan starts one for its library; an offline result or a removed library stops it. Watchers are never started or dropped under the state lock.
- **Schedule** (`indexer::schedule::Scheduler`): its own thread with a read connection. Quick scans of all libraries at startup and every quick check interval (full scans for libraries with rows behind `METADATA_VERSION`, REQ-002-05); offline libraries are checked every minute. Queued scans merge per library, so there is never more than one reachability check per library in flight. `Scheduler::set_interval` applies a changed setting at once (REQ-010-03).

## Spec changes

- **REQ-003-06:** it said a watcher that _can't start_ queues a quick scan. Since watchers start after every completed scan, a library whose watch can't start would be scanned forever. The spec now says the next scheduled quick scan tries again.

## Known limits

- Watching starts after the first completed scan, so changes during a library's first scan in folders already walked wait for the next quick scan.

## Tasks

- [x] Watcher with debounce and filtering (REQ-003-01 … -03, -05, -06)
- [x] Changes scans and changes during a scan (REQ-003-04, -07)
- [x] Schedule: startup and periodic quick scans, metadata version upgrades, offline re-checks (REQ-003-08 … -10)
