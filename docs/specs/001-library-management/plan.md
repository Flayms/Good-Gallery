# 001 · Library management · Plan

Records the approach that was taken. Decisions that outlive this spec are in `docs/architecture.md`.

## Approach

- **Migrations** (`db::migrate`): the SQL files in `crates/db/migrations/` and `meta/_journal.json` are kept byte for byte (`.gitattributes`: `-text`), since the bookkeeping hashes them. `__drizzle_migrations` is created with drizzle's exact DDL text, the newest `created_at` is read, and every journal entry with a newer `when` runs chunk by chunk (split at `--> statement-breakpoint`) in one transaction, inserting `(sha256 of the file, when)`. Existing indexes carry this bookkeeping, so they open without re-indexing (REQ-001-08). A test fails if the journal and the embedded file list disagree. New migrations keep the journal order and increasing `when` values.
- **Fixture:** `crates/db/tests/fixtures/legacy-index.db` is an index created by an earlier version (2 libraries, 3 media items, settings, a few rows in every table). It is frozen: it stands for indexes already on users' disks, so never regenerate it; add another fixture for new cases. Tests open a copy, migrate it and compare it with a freshly created index (same bookkeeping rows, same `sqlite_master` schema).
- **Connections:** one write connection (WAL, `synchronous = NORMAL`, foreign keys, `busy_timeout = 5000`), read-only connections for readers.
- **Library operations** (`indexer::libraries`): `add`, `remove`, `rescan`, `list`. The path must be absolute (drive + root, or UNC), is normalized (separators, `.` / `..`, no trailing separator except at a drive root), must be reachable as a folder within 5 s (stat on a helper thread with a timeout, so a dead share can't hang the caller), and is checked for duplicates with `lower(path) = lower(?)`. The label is the folder name, or the full path for a drive root. Scans are requested through the `ScanRequests` trait, which the indexer implements.
- **Errors** are a `thiserror` enum with the UI's messages ("Path must be absolute", "Folder not found or not reachable", "Library already added", "Library not found").
- **UI:** the Settings page's _Libraries_ section: path field, _Add_, _Browse…_ (native dialog through `rfd`, behind the `FolderPicker` trait so tests use a fake), the list (label, status badge, path, last indexed, _Rescan_, _Remove_ with confirmation), toasts. Slow work (reachability, dialog) runs on job threads.
- **Data folder** (REQ-001-09): the packaged app (feature `packaged`, set by `cargo xtask dist`) uses `%APPDATA%\good-gallery`; every other build uses `%APPDATA%\good-gallery-rust`, so `cargo run` never touches the real index. `--data-dir` overrides both.
- **CLI:** `cargo xtask libraries --db <file> list | add <path> | remove <id> | rescan <id>` drives the same API without a window.

## Tasks

- [x] Migrations with drizzle-compatible bookkeeping, pragmas, reader connections; legacy index fixture (REQ-001-08)
- [x] Library rows and settings storage
- [x] Path rules, reachability with timeout, add / remove / rescan behind `ScanRequests` (REQ-001-01, -03 … -06)
- [x] `cargo xtask libraries` CLI; checked with a copy of the real index
- [x] Settings page _Libraries_ section (REQ-001-01 … -07)
- [x] Data folder per build (REQ-001-09)
