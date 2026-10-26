# M03 · Database and libraries

**Status:** Done (approved 2026-10-26, self-approved: the user asked to work through the milestones without their input)
**Specs:** 001 (REQ-001-01 … 08); settings storage of 010 (REQ-010-01, REQ-010-02, storage part only)

## Approach

**Migrations, byte-compatible with the reference.** The reference's `drizzle/*.sql` and `meta/_journal.json` are copied unchanged to `crates/db/migrations/` (`.gitattributes` keeps their bytes, since the bookkeeping hashes them). `db::migrate` does what drizzle's SQLite migrator does: `CREATE TABLE IF NOT EXISTS __drizzle_migrations (id SERIAL PRIMARY KEY, hash text NOT NULL, created_at numeric)`, read the newest `created_at`, and in one transaction run every journal entry with a newer `when`, chunk by chunk (split at `--> statement-breakpoint`, each chunk as one batch), inserting `(sha256(file), when)`. An index created by either app therefore looks migrated to the other one. SQL files are embedded with `include_str!`; a test fails if the journal and the embedded list disagree, so a new reference migration can't be forgotten.

**Connections.** `Database::open(path)` opens the write connection with the reference's pragmas (`journal_mode = WAL`, `synchronous = NORMAL`, `foreign_keys = ON`, `busy_timeout = 5000`) and migrates; `Database::open_reader(path)` opens a read-only connection for the UI. In M3 the UI does its few writes (add, remove library) itself; from M4 on the indexer's writer thread owns the write connection and the UI sends it commands. rusqlite with the `bundled` SQLite.

**Fixture from the reference.** `crates/db/tests/fixtures/make-reference-db.mjs` creates `reference.db` with the reference's own migrator (drizzle + better-sqlite3 from `../good-gallery/node_modules`) and a few rows in every table. Tests open a copy of it (REQ-001-08, Electron → Rust) and compare a fresh Rust-created database with it: same `__drizzle_migrations` rows and the same `sqlite_master` schema (Rust → Electron). A copy of the user's real index is checked by hand once.

**Library operations** live in `indexer::libraries` (they need reachability, which belongs to the indexer, and they queue scans): `add(db, path, scans)`, `remove`, `rescan`, `list`. Path handling follows the reference: must be absolute (Windows rules: drive + root, or UNC), normalized like Node's `path.resolve` (separators, `.`/`..`, no trailing separator except at a drive root), reachable as a directory within 5 seconds (stat on a helper thread with a timeout, so a dead share can't hang the caller), duplicate check with `lower(path) = lower(?)` in SQL (as the reference, ASCII case folding), label = folder name or the full path for a drive root, status `online`. Scans are requested through a small trait `ScanRequests` (`request_scan(root, kind)`, `remove_root(root)`); M3 has a no-op implementation, M4 implements the queue. Errors are a `thiserror` enum whose messages are the reference's ("Path must be absolute", "Folder not found or not reachable", "Library already added", "Library not found").

**Settings storage.** `db::settings::Settings` (`thumb_cache_gib`, `quick_check_interval_minutes`, `io_concurrency`) with the reference's defaults and ranges; values are JSON in the `settings` table under the reference's camelCase keys. Invalid stored values fall back to defaults; invalid updates are rejected. The Settings page controls for them come in M10.

**App.** The data folder is `%APPDATA%\good-gallery-rust` (override: `--data-dir <folder>`), not the reference's `%APPDATA%\good-gallery`: during the rewrite, running the app must never touch the user's real index. Switching to the shared folder is an M11 task (recorded in `docs/architecture.md`). The window gets two pages, Gallery and Settings, with a temporary *Settings* / *Back to gallery* button until the toolbar and sidebar exist (M6, M7). The Settings page has the *Libraries* section of the reference: path field, *Add*, *Browse…* (native folder dialog via `rfd`), the list (label, status badge, path, last indexed, *Rescan*, *Remove* with a confirmation dialog), and toasts. Slow work (reachability check, folder dialog) runs on threads and reports back over a channel; tests swap the folder dialog for a fake and wait for background jobs before stepping the UI. The gallery shows "No libraries yet" (button opens Settings) or, with libraries, "No media found" until M6 brings the real gallery.

**Icons.** lucide SVGs, generated from the reference's `lucide-react` by `crates/app/assets/icons/make-icons.mjs` (white stroke, 1.75), embedded and rendered through `egui_extras`' SVG loader (resvg), tinted per use. Lasting decision, recorded in `docs/architecture.md`.

**Dates.** "Last indexed" uses the medium format without seconds from `docs/design.md` ("2 Jul 2025, 07:12") in the local time zone (`chrono`). Real locale-dependent formatting (REQ-013-13) is left for M10; this format is the reference's in English locales.

**CLI.** `cargo xtask libraries --db <file> list | add <path> | remove <id> | rescan <id>` drives the same API without a window (scans are only printed until M4).

## Reference

- `drizzle/*.sql`, `drizzle/meta/_journal.json`: copied unchanged.
- `node_modules/drizzle-orm/sqlite-core/dialect.js` (`migrate`), `drizzle-orm/migrator.js` (`readMigrationFiles`): bookkeeping table, hash, ordering, transaction.
- `src/main/db/index.ts` (pragmas), `schema.ts` (columns, timestamps in ms).
- `src/main/trpc/routers/libraries.ts`: validation order, messages, label rule, status, scan requests.
- `src/main/reachability.ts`: 5 s stat timeout.
- `src/main/settings.ts`, `src/shared/settings.ts`: keys, defaults, ranges, fallback on invalid stored values.
- `src/renderer/src/components/library-settings.tsx`: texts, toasts, confirmation, list layout.
- `src/main/trpc/router.test.ts` (libraries, settings): test cases.

## Tasks

- [x] 1. `db`: migrations (embedded, drizzle-compatible bookkeeping), open with pragmas, reader; fixture generator and `reference.db` (REQ-001-08).
- [x] 2. `db`: library rows (list, insert, find by path, delete, get) and settings storage (REQ-010-01, REQ-010-02).
- [x] 3. `indexer::libraries`: path rules, reachability with timeout, add / remove / rescan, `ScanRequests` (REQ-001-01, -03, -04, -05, -06).
- [x] 4. `cargo xtask libraries` CLI; manual check with a copy of the real index.
- [x] 5. `app`: data folder, database at startup, pages, toasts, icons.
- [x] 6. `app`: Settings page *Libraries* section with add, browse, list, rescan, remove (REQ-001-01 … 07 in `egui_kittest`).
- [x] 7. Docs: spec 001 Verification, architecture decisions, pitfalls, progress.

## Exit criteria

From `docs/rewrite/plan.md`: a copy of the user's real `gallery.db` opens and lists its libraries; 001 REQs pass.
