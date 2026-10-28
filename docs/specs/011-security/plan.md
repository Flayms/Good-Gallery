# 011 · Security · Plan

Records the approach that was taken. Decisions that outlive this spec are in `docs/architecture.md`.

## Approach

- **Files only from the index** (REQ-011-08): `domain::media::is_safe_rel_path` refuses absolute paths, drive or UNC prefixes, `..`, `.` and empty components (and, on Windows, `:` and `\` inside components). `db::media::absolute_path` and `ThumbSource::file` return `None` for such paths, so the viewer treats the item as missing, Explorer isn't opened and the thumbnail fails.
- **Helpers can't be steered** (REQ-011-09): exiftool gets its arguments over stdin, one per line, with absolute paths, so no file name can start with `-`; ffmpeg gets an argument vector, never a shell. Both run in the app's job object and end with it.
- **Hostile metadata** (REQ-011-10): extraction ignores values of unexpected types instead of panicking; a panic while reading one file costs only that file (`catch_unwind` per job).
- **No network** (REQ-011-11): `xtask/tests/no_network.rs` fails on known network crates in `Cargo.lock` (HTTP clients, async network runtimes, TLS) and on `std::net` or socket use in the workspace's sources. The helper downloads for packaging live in a PowerShell script, so no HTTP client enters the workspace.

## Tasks

- [x] Safe relative paths in `domain` and `db`; callers treat refused paths as missing (REQ-011-08)
- [x] exiftool and ffmpeg without a shell, absolute paths (REQ-011-09)
- [x] Hostile metadata survives (REQ-011-10)
- [x] No-network test (REQ-011-11)
