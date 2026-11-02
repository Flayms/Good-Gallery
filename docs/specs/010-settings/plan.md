# 010 · Settings · Plan

Records the approach that was taken. Lasting decisions are under _Decisions_ at the end.

## Approach

- **Storage** (`db::settings`): `Settings` (`thumbCacheGiB`, `quickCheckIntervalMinutes`, `ioConcurrency`) as JSON values in the `settings` table under camelCase keys, with defaults and ranges. Invalid stored values fall back to defaults; invalid updates are rejected (REQ-010-02). Other keys (view choices) are kept as they are.
- **Page** (`app::settings_page`): below _Libraries_ come _Indexing_ (_Check for missed changes_: presets 0, 15, 30, 60, 180, 360, 1440 minutes plus a stored non-preset value; _Parallel file reads_: a 1–16 slider that saves on release) and _Thumbnail cache_ (_Maximum size_: presets 1, 2, 5, 10, 20, 50, 100 GB plus a stored value; _Used_, "…" until measured; _Clear_ with confirmation and a toast). Rows are label, description and control in a bordered group.
- **Applied at once** (REQ-010-03 … -06): `AppState::update_settings` validates and stores, then hands the changes to a `SettingsTargets` (the app's running parts: `Scheduler::set_interval`; `Indexer::set_concurrency` + `ExifTool::set_max_processes` + `ThumbnailService::set_workers`; `ThumbCache::set_max_bytes`). UI tests record the calls with a fake. Thumbnail threads above the set number wait; more are started when it grows. exiftool's pool only grows while a scan runs. Cache usage is read every 10 s (every second for 10 s after a new limit); clearing runs on a job thread.

## Tasks

- [x] Settings storage with defaults, ranges and fallbacks (REQ-010-01, -02)
- [x] `ThumbnailService::set_workers` (REQ-010-04)
- [x] `update_settings` with `SettingsTargets`, error toasts, cache usage and clear jobs (REQ-010-01 … -06)
- [x] _Indexing_ and _Thumbnail cache_ sections (REQ-010-01 … -06)

## Decisions

### 2026-10-28 · Settings applied at once

**Decided:** the Settings page stores through `db::settings::save` (validated; an invalid value is an error toast and nothing changes) and hands what changed to an `app::SettingsTargets` (the app's running parts in `main.rs`; UI tests record calls with a fake). Dates are formatted with ICU4X (`icu_datetime`, compiled data) in the locale `sys-locale` reports; tests pin `en-GB`. Relative paths from the index are checked by `domain::media::is_safe_rel_path` wherever a file is opened. `xtask/tests/no_network.rs` keeps network crates and socket code out of the app.
