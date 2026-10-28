# Progress

The current milestone is the first row that isn't `Done`. Statuses: `Not started` → `Planning` (plan drafted, waiting for approval) → `In progress` → `Done`. `Blocked` needs a decision by the user (say which in the log).

| Milestone | Title                                  | Status      | Plan                                    |
| --------- | -------------------------------------- | ----------- | --------------------------------------- |
| M0a       | Spikes: grid and look                  | Done        | [M00](rewrite/milestones/M00-spikes.md) |
| M0b       | Spikes: video                          | Done        | [M00](rewrite/milestones/M00-spikes.md) |
| M1        | Foundation                             | Done        | [M01](rewrite/milestones/M01-foundation.md) |
| M2        | Spec audit                             | Done        | [M02](rewrite/milestones/M02-spec-audit.md) |
| M3        | Database and libraries                 | Done        | [M03](rewrite/milestones/M03-database-libraries.md) |
| M4a       | Indexing: metadata                     | Done        | [M04](rewrite/milestones/M04-indexing.md) |
| M4b       | Indexing: scanning                     | Done        | [M04](rewrite/milestones/M04-indexing.md) |
| M4c       | Indexing: change detection, oracle     | Done        | [M04](rewrite/milestones/M04-indexing.md) |
| M5        | Thumbnails                             | Done        | [M05](rewrite/milestones/M05-thumbnails.md) |
| M6a       | Gallery: tiles                         | Done        | [M06](rewrite/milestones/M06-gallery.md) |
| M6b       | Gallery: views and toolbar             | Done        | [M06](rewrite/milestones/M06-gallery.md) |
| M7a       | Search: model and queries              | Done        | [M07](rewrite/milestones/M07-search-sidebar.md) |
| M7b       | Search bar, filters and sidebar        | Done        | [M07](rewrite/milestones/M07-search-sidebar.md) |
| M8a       | Viewer core                            | Done        | [M08](rewrite/milestones/M08-viewer.md) |
| M8b       | Viewer panels and filmstrip            | Done        | [M08](rewrite/milestones/M08-viewer.md) |
| M9        | Video                                  | Done        | [M09](rewrite/milestones/M09-video.md) |
| M10a      | Settings and rules                     | Done        | [M10](rewrite/milestones/M10-settings-design.md) |
| M10b      | Glass and title bar                    | Done        | [M10](rewrite/milestones/M10-settings-design.md) |
| M11       | Packaging and release                  | Done        | [M11](rewrite/milestones/M11-packaging-release.md) |

## Log

Newest first. One entry per session: date, milestone, what was done, decisions (lasting ones also in `docs/architecture.md`), open issues, next step.

### 2026-10-28 · M11 · Packaging and release

- Plan `M11-packaging-release.md` written and self-approved, one part.
- `cargo xtask release`: the reference's `release.ts` in Rust with its tests (throwaway repo with a bare `origin` and a tiny Cargo workspace); version in `[workspace.package]`, `Cargo.lock` refreshed and committed with it; git-cliff with the reference's `cliff.toml`.
- `.github/workflows/release.yml` (tag → `ci.yml` → version check → helpers → `cargo xtask dist` → smoke test → git-cliff notes → `gh release create`); CI installs git-cliff and fetches the bundled exiftool and ffmpeg so their tests run (libmpv left out: Vulkan loader on the runners unknown). `xtask/tests/workflows.rs` guards both workflows.
- `packaging/fetch-helpers.ps1` (exiftool 13.59, ffmpeg-static b6.1.1, libmpv 20261021, SHA-256 pinned; fresh download checked), `cargo xtask dist` (staging from the shared `media::helpers::*_BUNDLED` layout, zip with Windows' tar, NSIS installer `packaging/windows/installer.nsi`), `packaging/smoke-test.ps1`. Local result: setup.exe 68.6 MB, portable zip 94.2 MB.
- App: the packaged app (feature `packaged`, set by `cargo xtask dist`) uses `%APPDATA%\good-gallery` (new REQ-001-09), other builds `good-gallery-rust`; no console when packaged, startup errors in a message box, panics logged, `logs\app.log` (+ `app.old.log`), helper paths logged; app icon (reference's) in the exe (`winresource`) and the window; `--info` for viewer screenshots.
- Exit criteria: the silent install, a run of the installed app on a copy of the user's real index (sidebar with People / Places, thumbnails rendered from the libraries, all three helpers found in the install folder, no errors in the log) and the uninstall (data folder kept, shortcut and registry entry gone) passed through the smoke test. The full test suite also passed with the bundled helper versions (`GG_REQUIRE_HELPERS=1`). Release dry run: see below.
- `cargo xtask screenshots` (the reference's demo library, on a `subst` drive) rendered `docs/screenshots/{masonry,justified,grid,list,viewer}.png`; README rewritten (features, install, development, releases).
- Specs: 014 `Implemented` (REQ-014-01, -03 … -07, -09 pass; -08 partial until the first published release; -02 is a repository setting), new REQ-014-09 (installer and portable app). Spec index statuses synced.
- Code review fixes: the installer's folder could be a shared one whose `ffmpeg` / `mpv` / `exiftool` folders it deleted (a non-empty picked folder now gets `Good Gallery` appended; checked with a silent install); a failed push left a local release commit and tag that blocked the next run (now undone, test with a rejecting hook); `cargo run --release` would have opened the real index (the shared folder now needs the `packaged` feature); panics and argument errors were invisible in the packaged app (logged, message box); a log held open no longer costs the run its log; `xtask screenshots --keep` indexed the library twice on reruns; the smoke test compared full paths that may differ in 8.3 form; helpers aren't cached twice in CI; one exit-code helper in xtask's dispatcher.
- Open: (1) publish the first release (`cargo xtask release`, pushes `main` and the tag; the workflow has never run on GitHub); (2) branch protection on `main` requiring CI (REQ-014-02); (3) a run on a really clean Windows machine (here: this machine, temp install folder) — especially whether `libmpv-2.dll` finds `vulkan-1.dll` without GPU drivers; (4) code signing (out of scope); (5) `thumb_cache::tests::req_005_13_writes_leave_no_temp_files` failed once in the first run after a rebuild (Defender, as M8b/M9) and passed in the next.
- Next: the rewrite's milestones are done. "After the rewrite" in `docs/rewrite/plan.md`: move the roadmap into `docs/roadmap.md`, archive the reference.

### 2026-10-28 · M10a + M10b · Settings, design polish, glass

- Plan `M10-settings-design.md` written and self-approved, split into M10a (settings and rules) and M10b (glass and title bar); both done in this session (several runs, interrupted by rate limits) and committed together.
- M10a: Settings page _Indexing_ (check interval with a stored odd value kept as an option, parallel reads slider saved on release) and _Thumbnail cache_ (size, usage every 10 s, _Clear_ with confirmation and toast), applied at once through `SettingsTargets` (new `ThumbnailService::set_workers`); `domain::media::is_safe_rel_path` refuses index paths that leave their library (REQ-011-08); `xtask/tests/no_network.rs` (REQ-011-11); glass, focus and scrollbar tokens with the reference's contrast tests; one focus ring for the focused widget; Windows' animation and transparency effects re-read every 2 s; dates in the user's locale through ICU4X (tests pin `en-GB`).
- M10b: `app::glass` (the M0 pass on the frame's recorded backdrop) on the toolbar, Settings header, menus, combo lists, context menus, suggestions and dialogs; the viewer's scrim blurs the window (tiles, sidebar, toolbar) and its chrome reuses that blur, tinted; opaque fallback for reduced transparency and without the pass (kittest); content in the title bar with the M0 window subclass (native hit tests for the caption buttons, egui-handled caption drags), 138 px kept free; scrollbars as the design's pill, starting below the toolbar.
- Specs 010, 011, 013 `Implemented`. 13 snapshots updated (reserved caption space, opaque fallbacks); new tests `crates/app/tests/settings.rs`.
- Checked in the running app on the dev data folder: caption buttons, toolbar glass picking up the red tiles scrolled under it (pixel values), blurred and darkened window behind the viewer with glass control bar, arrows and filmstrip; dates in the machine's en-US locale.
- Code review fixes: the info sheet's and the viewer info panel's tops lay under the caption buttons (a click on the sheet's X closed the app), now below the caption; caption space is kept free only where a bar reaches the window's right edge (the toolbar beside an open info sheet squeezed its search field); a lower parallel reads value no longer shrinks exiftool's pool under a running scan (it only grows now); parked thumbnail threads wait on their own condvar; tiles record their placeholder for glass only until the thumbnail covers it; the OS preferences no longer wake an idle window every 2 s (read while frames are drawn); the date formatter checks a version counter instead of locking per call; `:` and `\` count as unsafe path characters only on Windows; "Every minute"; the parallel reads knob snaps to whole steps; the sidebar's drag area uses the caption height.
- Not changed after review: glass rebuilds its per-tile bind groups and buffers every frame and keeps slot textures (e.g. the scrim's) until exit; worth measuring with `cargo xtask gallery-bench` before optimizing.
- Session note: the build failed with "paging file too small" (C: had 15 GB free, 15 GB in `target`); building with `-j 4` works.
- Open: (1) the user's approval of the side-by-side screenshots (exit criterion); (2) the viewer chrome blurs the scrim, not the photo under it (only visible when zoomed under the bar); (3) glass rebuilds tile bind groups every frame (≈ 300 for the scrim at 12 columns) — not measured against the frame budget yet; (4) the date pickers' popups stay Raised; (5) counts keep English grouping.
- Next: M11 (Packaging and release).

### 2026-10-27 · M9 · Video

- Resumed an interrupted session that had written the plan (self-approved), `domain::video` and `media::helpers::{find_mpv, display_refresh_rate}`.
- `app::video`: libmpv FFI (`video::mpv`, all of the video `unsafe`), `MpvPlayer` (render thread with GPU and egui-texture paths, display-synced timing, event thread, load counting so frames of a video stepped away from never show), the `Playback` trait, `FakePlayback` for tests, `Videos` (player made on first use, or why there is none). The app finds libmpv like ffmpeg.
- Viewer: plays the shown video from its library file, stops on step, close and missing items; the frame replaces the preview, fitted and turned by its rotation; glass video controls along the video's bottom (play / pause, time, seek slider with keyframe seeks while dragging and an exact one on release, mute, volume); Space plays / pauses, a click on the video too; ← / → seek 5 s and ↑ / ↓ change the volume while the last click was on the video or its controls; a paused video keeps the controls; play at the end starts over; messages when videos can't play here or a file fails.
- Exit criterion: generated MP4 H.264 + AAC, MOV HEVC + AAC (rotated), MKV H.264 + Opus and WebM VP9 + Opus play and seek exactly through the real player (`crates/app/tests/video.rs`, with the spike's libmpv). Checked in the running app on the M0b fixtures: 1080p and the rotated phone-style MOV play display-synced with controls (player ready in 24 ms).
- Specs: 009 `Implemented` (REQ-009-02, -04, -06 pass); REQ-013-10 passes. Snapshot `viewer_video`.
- Code review fixes: the controls no longer cover the message for a file that fails; mute toggles by what it says (it muted at volume 0); mpv properties are read only after the new file has loaded (they were the previous video's) and at most every 100 ms also for files without video; → seeks forward while the duration isn't known yet; ↑ while muted starts from the volume set before; stepping from video to video replaces the file without stopping first (that reopened mpv's outputs); one shared lookup for bundled helpers (`media::helpers::find_helper`, ffmpeg and libmpv).
- Not changed after review: the refresh rate is Windows' integer rate of the primary display (59 for 59.94 Hz; mpv's display-resample absorbs it), read once; while the window is minimized display-synced playback gets no presented frames (video waits, audio goes on) — revisit if it shows up in practice.
- Open: (1) audio checked only as "an audio device opened", not by ear, and the controls weren't clicked by hand in the real app (UI tests cover them); (2) `crates/app/tests/video.rs` needs `GOOD_GALLERY_MPV` (CI and packaging bundle libmpv in M11); (3) `thumb_cache::tests::replaces_a_thumbnail_that_is_open_for_a_moment` failed once again in the first run after a rebuild (Defender, see M8b).
- Next: M10 (Settings, design polish, glass).

### 2026-10-27 · M8b · Viewer panels and filmstrip

- Info panel in the viewer (`i`, _Info_) and info sheet beside the gallery (context menu _Information_) with file, location (selectable), library, dates, dimensions, duration, size, rating and tags grouped into People, Places and Tags; a click on a tag closes the viewer and filters by it. `db::media::detail` as the reference's `media.byId`.
- _Show in Explorer_ in the control bar and in the context menus of tiles, rows and the viewer's media; a missing file gets an error toast.
- Filmstrip (`f`, _Filmstrip_, remembered as `viewer.filmstrip`): one row of the gallery's thumbnails below the media, shown item outlined, toggled for assistive technology and centered, click or Enter jumps, the wheel scrolls sideways, play symbol on videos, fades with the controls.
- Chrome tinted by the item's ThumbHash average (OKLCH clamp as the reference); the photo grows from its tile and shrinks back on close (fades without a tile on screen), instant with Windows' animation effects off.
- Specs: 009 all pass but -02 / -04 / -14 (video parts, M9); 015 `Implemented`; REQ-013-11, -12 pass; REQ-013-10 partial (paused videos, M9); REQ-013-13 partial (locale, M10).
- Code review fixes: the info sheet hides while the viewer is open (sheet and panel took turns asking for their item's details every frame and never showed them); Show in Explorer checks and starts Explorer on a job thread (an offline share froze the window) and retries without breaking away from the app's job when a parent job forbids it; the filmstrip centers again when its width changes and keeps within the row; one lookup of an item's file (`db::media::file_in`; a missing library no longer falls back to a relative path); the tint is computed once per item; details are forgotten when no info view shows them; the play symbol stays inside the filmstrip.
- Open: `thumb_cache::tests::replaces_a_thumbnail_that_is_open_for_a_moment` fails now and then in the first full test run after a rebuild (Defender scanning new files holds the temporary thumbnail longer than the ≈ 1.4 s of rename retries); it passes on its own and in later runs.
- Next: M9 (Video).

### 2026-10-27 · M8a · Viewer core

- M8 split into M8a (viewer core) and M8b (info panel, Explorer, context menus, filmstrip, tinted chrome, tile ↔ viewer transition); plan `M08-viewer.md`, self-approved.
- Viewer over the darkened window: opens from a tile or row (click or Enter), steps with ← / → and _Previous_ / _Next_, closes with Escape, _Close_, Alt + ←, the mouse's back button or a click on the scrim (delayed so a double-click zooms instead). Control bar with file name, date and rating; controls and pointer step aside after 2.5 s.
- Originals decoded from the library on two worker threads straight into mipped wgpu textures (`app::originals`, `media::original`, AVIF through ffmpeg); the shown photo and two places either way preload, the rest is dropped. The thumbnail (else the placeholder) stands in until the original fades in; a preloaded one shows at once. Failures say "Can't load the original. Showing a preview."; a removed item says "Media not found".
- Zoom and pan as the reference (wheel around the pointer, `+` `-` `0`, click and double-click toggle fitted ↔ actual pixels, drag, 1×–8×, clamped), pure logic in `domain::viewer`.
- The gallery keeps the shown item in view, so closing lands on it (REQ-016-06 now passes); REQ-007-13 passes.
- Specs: 009-01, -03, -05, -12, -13, -15, -16 pass; -02, -04, -14 partial (video parts with M9; the thumbnail preview needs the GPU and was checked in the running app); REQ-013-10, -14 partial. New `--view <position>` option opens the viewer for screenshots.
- Code review fixes: a photo cancelled while it loaded and wanted again right after was taken as failed (now re-queued); a scrim click waits egui's whole double-click window (0.3 s), so a slower double-click zooms instead of closing; Tab, Enter and Space no longer reach the widgets under the viewer (they opened Settings); clicks where a zoomed photo is clipped away close instead of zooming; items taller than the gallery's view don't pin its scroll; failures are logged once; the originals' workers are detached, so a stalled network read can't hold up closing; the bar's text is laid out once per frame.
- Open: originals are turned upright by their EXIF orientation as the `image` crate reads it; a photo oriented only in XMP would show sideways (exiftool's orientation could be passed in).
- Session note: `target\debug\deps` and `incremental` had grown to 41 GB with 1.3 GB free on C:; deleted them (the user had OK'd it) and built with `CARGO_INCREMENTAL=0` afterwards.
- Next: M8b (viewer panels and filmstrip).

### 2026-10-27 · M7b · Search bar, filters and sidebar

- Search bar in the toolbar: chips for included (tag icon), excluded (struck through) tags and the file name filter, tag suggestions with counts while focused (`-` excludes, _File name contains "…"_), arrow keys, Enter, Backspace and Escape as REQ-007-05.
- _Filters_ (media type, date range with date pickers, ratings, _Clear_ per section, _Clear filters_, count on the button) and sort fields and direction in _View_; _Group by day_ disabled for other sorts.
- Sidebar: _All libraries_ and libraries (offline badge, spinner while scanned), the selected library's folder tree, People and Places trees with _Show all_, Ratings, the 15 most used Tags; clicking filters, collapsed groups remembered; Settings at the bottom (moved out of the toolbar).
- Specs 007, 008 `Implemented`; REQ-006-11 and REQ-016-04 now pass; REQ-007-13 waits for the viewer (M8), REQ-013-08 partial (minimum window size with M10). REQ-007-09's example reads "3 and 5 stars" (no star glyph in the fonts). Snapshots `search_suggestions`, `filters_menu`, `sidebar`.
- Found while testing: menus closed when their button's look or the widgets before it changed (fixed ids now), Enter and Escape were lost because the input drops focus in that frame, clicks on suggestions were lost to a clip-rect check, missing glyphs (★, 📆).
- Code review fixes: suggestions only count for the current input (an older answer could put the wrong tag under Enter); the sidebar doesn't block the UI thread on the database lock; a removed library leaves the search; the sidebar's data is polled on the Settings page too and reloads only when it can change (not on every filter click); ratings have one wording ("Unrated and 3 stars", "1 star") and are kept sorted (`GallerySearch::toggle_rating`); no per-frame copy of the sidebar data; the suggestions don't count as a menu's overlay.
- Session note: the C: drive filled up with temporary build folders (made because a running app window locked `target\debug\app.exe`); they were deleted.
- Next: M8 (Photo viewer and filmstrip).

### 2026-10-27 · M7a · Search: model and queries

- Plan written and self-approved (`docs/rewrite/milestones/M07-search-sidebar.md`), split into M7a (model and queries) and M7b (search bar, filters, sidebar).
- `domain::search` (search state, tag include / exclude / remove, filter count, local day range), `domain::folders` (folder tree with natural order, ancestors, tag trees), `db::gallery::layout` with every filter and the reference's tag conditions (plan pins kept), `db::tags` (suggestions, People / Places, popular, rating counts, library folders). The reference's router tests are ported.
- Oracle: 30 searches, 6 suggestion prefixes and 5 sidebar lists identical to the reference's router on the same index. `cargo xtask oracle-search` runs that part alone.
- REQ-007-14 measured with `cargo xtask search-bench` (500k items): narrow searches 1–18 ms p95, broad ones 0.25–0.7 s, suggestions 13.9 ms. Read connections now use memory-mapped reads and a 64 MB cache (4–8× faster).
- Also: the thumbnail cache's rename retry widened to ≈ 1.4 s after a full-suite flake.
- Code review fixes: `oracle-search` works on a temp copy (the reference migrates what it opens); natural order ignores accents like the reference's collator; tags whose parents form a cycle reach the tag tree; a day without a local midnight falls back to UTC midnight instead of 1970; ratings above 5 are ignored; searches without tags use the statement cache; the bench refuses a reused index that isn't complete. Kept: memory-mapped reads (an I/O error becomes a crash, but the index is on the local disk).
- **Open (user decision):** REQ-007-14's 50 ms for broad searches needs a covering index or rows stored in sort order, a change of the schema shared with the Electron version.
- Next: M7b (search bar, filters, sort, sidebar).

### 2026-10-27 · M6b · Gallery: views and toolbar

- Toolbar over the gallery (Raised until M10) with the _View_ menu: layout (Masonry, Justified, Grid, List), zoom out / slider / zoom in (disabled in the List view), _Group by day_, tag indicators with their picks; Settings moved from the corner into the toolbar. Choices stored under the reference's keys (`domain::view`).
- List view (`app::gallery::list`): thumbnail, name with play icon and duration, folder, date taken or the dimmed date modified with a tooltip, size, dimensions, rating stars; a column header below the toolbar; group header rows.
- Date group headers in every view ("Monday, 26 October 2026", "42 photos · 3 videos"); tag indicator badges (stars, pin, person, tag) that leave the video badge's corner free, named in tooltips and in the tile's accessible name.
- `gallery.rs` became `gallery/{mod, tiles, list}.rs`. Six lucide icons added (layout, list, rows, zoom).
- Spec fix: REQ-006-02's gap is 8 px, 4 px from 8 columns on (reference and design agree). Specs 006, 016, 017 `Implemented`; open: REQ-006-11 tag suggestions (M7), REQ-016-04 disabled toggle for other sorts (M7), REQ-016-06 scrolling to the viewed item (M8). Snapshots `view_menu`, `gallery_list`.
- Checked in the app: grouped Masonry with indicators, List view with groups, on the fixture library.
- Code review fixes: the toolbar and the List view's column header now take the pointer, so hidden tiles and rows under them don't react; the anchor's position is looked up once per new layout instead of every frame (500k-item scan); per-frame clones and label strings avoided (labels built only when AccessKit asks); headers index their group directly; the column header lines up with the rows' width; tag indicator controls are disabled in the List view, like zoom.
- Next: M7 (Search and sidebar).

### 2026-10-27 · M6a · Gallery: tiles

- Plan written and self-approved (`docs/rewrite/milestones/M06-gallery.md`), split into M6a (tiles) and M6b (views, toolbar).
- Done, tests seen red then green: tasks 1–7. `domain::{layout, date_groups, zoom}`, `db::gallery` (layout + items with tag categories), `db::settings::{value, set_value}`, `app::textures` (spike streamer, LRU budget), `app::gallery` (loader thread, Masonry tiles, ThumbHash placeholders, fade-in, video badge, failed icon, Ctrl + wheel zoom stored as `gallery.columns`, scroll anchoring, live reload, skeleton and error states), `crates/app/tests/gallery.rs` + snapshots `gallery`, `gallery_loading`. Checked in the app: fixture library, on-demand rendering, and a copy of the real index with the Electron cache.
- Frame-time budget (REQ-006-12): `cargo xtask gallery-bench` builds a data folder with 100,000 items whose thumbnails are hard links into a cache copy and runs the app with `--bench-scroll` (vsync off, a warming pass, then 20 s at 1,440 pt/s, 12 columns). Result on mains power: p50 3.0 ms, p95 4.4 ms, p99 5.3 ms, max 11.9 ms. The first run showed textures at 324 MB, over the planned 256 MB budget: at 12 columns the visible tiles and the prefetch band alone take ≈ 330 MB, so the budget is 512 MB (as in M0); the second run peaked at 511 MB.
- Specs: 006 REQ-006-02, -07, -08, -09, -13 pass, -03, -04, -06, -10, -11 partial (view choice, slider, List view in M6b; tag suggestions M7), -12 measured; 016 -01, -04 … -06 partial (headers in M6b); 017 -01 partial; 005 -06, -14 tiles checked in the app (no automated tile test without the GPU streamer).
- Code review fixes: chunks forgotten with the remembered items (or loaded from a stale layout during a reload) were never asked for again; a reload laid out twice and snapped the view to the old position, so the anchor is now the media item (test: items added above stay out of the way); the gallery `AppState::new` builds now loads lazily, so the one `main` replaces never queries; a window resize lays out once the width settles (120 ms); loaded textures keep their LRU protection when a larger width is missing; missing thumbnails are looked for again after a reload; a usage string gap.
- Decisions in `docs/architecture.md` (gallery data and textures); pitfalls (egui Ctrl + wheel, per-frame layout clone, hard links).
- Open: (1) the Settings button sits in the top-right corner until the toolbar (M6b). (2) Thumbnail rendering still runs at normal priority; the bench ran with all thumbnails cached, so rendering while scrolling isn't measured.
- Next: M6b (toolbar with the View menu, List view, date group headers, tag indicators).

### 2026-10-27 · M5 · Thumbnails

- Plan written and self-approved (`docs/rewrite/milestones/M05-thumbnails.md`), one part.
- `domain::thumbs` (widths, `pick_thumb_width`, `ThumbKey`), `media::thumbnail` (decode with JPEG shrink-on-load and EXIF orientation, Lanczos3 resize into 800 × 2400, WebP q80, ThumbHash, embedded previews through exiftool), `media::ffmpeg` (video frames at 10 %, AVIF), `db::thumbs`, `indexer::thumb_cache` (the Electron version's layout, atomic writes, LRU eviction, view clock), `indexer::thumbnails::ThumbnailService` (priority queue, background batches, failed vs offline, full cache). The indexer kicks it after writes and scans; the app starts it with the data folder's `thumbs` and the cache limit.
- Spec 005 `Implemented`: all REQs pass except REQ-005-06 and -14, partial until the tiles exist (M6). REQ-010-05 / -06 partial (cache side). REQ-005-01 corrected (800 × 2400 box, no 3:1 crop).
- Decisions in `docs/architecture.md`: cache layout kept (Defender question: revisit in M6 with cold-scroll measurements), rendering stack, AVIF through ffmpeg, ICC profiles not applied (deviation).
- Oracle extended with thumbnails: 11 fixtures, same sizes, mean ≤ 2.72 levels. `cargo xtask thumbs` finds the real cache's thumbnails exactly as Node's hashing does (17,618 of 20,295).
- Found under full-suite load: replacing a thumbnail failed with "Access is denied" while the file was held open (Defender); `ThumbCache::write` now retries the rename (test held the file open: red, then green).
- Code review fixes: the cache's size count lost writes made during eviction or the startup walk and counted replaced files twice; every write over the cap started another eviction walk; a render thread waited for the cache measurement (with one thread, requests waited too); queued items of a library just found offline were still tried; a request arriving mid-render into a full cache got no files; large PNG / WebP hit the `image` crate's 512 MiB limit (now sharp's 268 Mpx); the variable `NOT IN` query evicted cached statements; the library status update is shared with the writer. Not tested: the decode limit (needs a > 512 MiB image) and the late-request window (needs a hook between two lines).
- Open: (1) thumbnails render at normal thread priority; M6 measures whether background rendering costs frame time. (2) ffmpeg must be bundled (M11); without it videos get no thumbnails (error toast). (3) Settings for cache size / usage / clearing come in M10.
- Next: M6 (Gallery): texture streaming reads through `ThumbCache::read` and requests missing thumbnails from the service; measure cold scrolling with the real cache.

### 2026-10-27 · M4c · Indexing: change detection, oracle

- Picked up an interrupted session: the watcher and schedule (committed with M4b) were half wired in. Now finished: the indexer starts a watcher after a completed scan and stops it when the library goes offline or is removed, and reports become changes or quick scans. The app runs the schedule (quick scans at startup and every interval, offline libraries every minute, full scans for outdated rows).
- Tests for REQ-003-05 (5,000-path limit when queueing), -06 (failed watch), -07, -10 (watched again after a reconnect). Spec 003 `Implemented`, 004 `Implemented` (REQ-004-03 partial: GPS zone deviation), REQ-001-05 and REQ-002-05 pass.
- Spec fix: REQ-003-06 no longer queues a scan when watching can't start (it would follow every completed scan); the schedule retries.
- `cargo xtask oracle`: fixture library (16 media items, 19 tags: offsets, sub-seconds, hierarchies, both face-region formats, orientations, both sidecar namings, GPS, Unicode and whitespace tags, a broken file, a video, housekeeping files), indexed by the reference (bundled with its vite, run with Node) and by this indexer, then diffed. 5 documented differences (face regions, GPS zone), 0 unexpected.
- Bug found while checking the app: exiftool outlived the app (two processes per pool slot). The app now puts itself into a kill-on-close job object (`media::helpers`, the first `unsafe`); the test saw the leak fail, then pass.
- Checked in the running app: a new data folder with the fixture library, where only the startup schedule indexes it (16 items, online); no exiftool left behind.
- Code review fixes: the scheduler's read-only connection opened before the index was created, so a fresh data folder couldn't start (checked in the app afterwards); a library removed while its scan completed could be watched again; watchers are no longer dropped under the state lock or started under the watcher lock; the job object allows breakaway for programs opened for the user; a REQ-003-07 test no longer sleeps; `--screenshot-after` rejects bad numbers.
- Not changed after review: watching starts after the first completed scan, so changes during a library's first scan in folders already walked wait for the next quick scan (as in the reference; REQ-003-07 is about watched libraries). `Scheduler::set_interval` has no caller yet because the setting has no UI; wire it in M10 (REQ-010-03).
- Open: (1) exiftool processes left over from this session's runs before the fix (PIDs started 08:35–09:47) couldn't be killed from the agent shell (blocked); end them in Task Manager. (2) REQ-002-11 still has no test. (3) Oracle needs Node, the reference's `node_modules` and ffmpeg; not in CI.
- Next: M5 (Thumbnails), starting with its plan and the open thumbnail cache layout decision.

### 2026-10-26 · M4b · Indexing: scanning

- `indexer::walk` + `retry`, `indexer::scan::scan_root` (full, quick and changes scans on worker threads, batched writes, removals, offline handling), `indexer::Indexer` (scan queue with merging, one scan thread, `catch_unwind` per scan and per file). The reference's scan tests are ported and pass; an end-to-end test indexes a PNG with its sidecar through the real exiftool and checks that no file changed.
- App: two connections (UI, indexer), indexer started with exiftool from `PATH` / `GOOD_GALLERY_EXIFTOOL` (an error toast if missing), library operations queue real scans, scan progress in the bottom-left corner, counts refreshed after the first written files, every 5 s, and at the end.
- M4a review fixes went into the M4a commit: exiftool-vendored's real arguments (`-fast`, `struct=1`, `keepUTCTime`, UTF-8 filter), face regions from the region structs (the reference misses them, REQ-004-09 wins), parallel sidecar reads, DST gap, JavaScript whitespace in tag names.
- check-specs: chained REQ prefixes in test names (`req_002_01_req_002_02_…`).
- Open: REQ-002-11 has no test (a folder that can't be listed is hard to make on Windows without admin rights); no startup scans yet (M4c).
- Next: M4c (watcher, changes scans, schedule, oracle).

### 2026-10-26 · M4a · Indexing: metadata

- M4 plan written (self-approved) and split into M4a (metadata), M4b (scanning), M4c (change detection, oracle).
- `domain`: media kinds, housekeeping folders, sidecar lookup, tag normalization, `MediaMetadata`. `media::exiftool`: process pool with exiftool-vendored's arguments. `media::metadata`: the reference's extraction with its test cases, dates with exiftool-vendored's zone inference (without geo-tz). `db::writer`: port of `IndexWriter`.
- Deviation recorded: no GPS-based time zones (`docs/architecture.md`).
- REQ-004-02, -04 … -09, -11 pass; REQ-004-03, -10, REQ-002-01, -02, -04, -14, REQ-011-10 partial until scanning exists; REQ-011-09 pass.
- Next: M4b (scanning).

### 2026-10-26 · M3 · Database and libraries

- `db`: the reference's migrations byte for byte, drizzle-compatible bookkeeping, pragmas, reader connections, library rows, settings storage. `reference.db` fixture made by the reference's own migrator proves compatibility both ways (REQ-001-08).
- `indexer::libraries`: add (absolute, normalized, reachable within 5 s, duplicate check, label), remove, rescan behind a `ScanRequests` trait (no-op until M4). `cargo xtask libraries --db <file> list|add|remove|rescan`.
- `app`: data folder `%APPDATA%good-gallery-rust` (`--data-dir`), Settings page with the _Libraries_ section (add, browse, list, rescan, remove with confirmation, toasts), gallery empty states, lucide icons as SVG.
- A copy of the real index lists its two libraries (CLI) and the app opens it (20,383 media items).
- Spec 001 `Implemented` (REQ-001-05 partial until the scan queue exists); REQ-010-01 / -02 partial (storage), REQ-006-10 partial (two of four states).
- Decisions in `docs/architecture.md`: migrations and compatibility, data folder, icons, UI state and jobs. Pitfalls: parallel wgpu tests crash (serialized), copying from the reference is blocked.
- Next: M4 (Indexing, change detection, metadata).

### 2026-10-26 · M2 · Spec audit

- Specs audited (plan: `docs/rewrite/milestones/M02-spec-audit.md`, self-approved). 011 rewritten: REQ-011-01…07 deprecated (no web page, IPC, protocols), new REQ-011-08 (files only from the index), -09 (helpers can't be steered by file names), -10 (hostile metadata), -11 (no network).
- New REQs: REQ-001-08 (existing index stays compatible both ways), REQ-005-15 (existing thumbnail cache is used), REQ-006-12 (frame-time budget), REQ-006-13 (thumbnail memory budget). Reworded: REQ-002-13 (indexer failure instead of process crash), REQ-007-13 (search kept while viewing, no URL), REQ-009-02 / -03 / -12 (mouse back button, no viewer URL), REQ-014-01 / -07 (Rust checks, `Cargo.toml`), REQ-015-04.
- `docs/design.md` in egui terms (tokens in `theme`, own glass pass, Windows settings for the fallbacks); glossary (_Search_, _Indexer_).
- Doc-only milestone: no code review run (nothing to review beyond wording); `cargo xtask check-specs` clean.
- Next: M3 (Database and libraries).

### 2026-10-26 · M1 · Foundation

- The user asked to work through all milestones without their input and to commit after each one (no remote, no PRs). Milestone plans are self-approved from here on; the commit is the review point.
- Cargo workspace with `crates/{domain,db,media,indexer,app}` and `xtask`; `cargo xtask check-specs` ported from the reference with its tests (spec 012 `Implemented`, 8/8 pass). REQ convention settled: `fn req_012_01_…` or `/// REQ-012-01: …`, only the test part of a file counts.
- `cargo run -p app` opens a themed window (tokens and font in `app::theme`, first-run empty state); `egui_kittest` snapshot test `first_run`; `--screenshot <file.png>` for visual checks.
- CI workflow written (`.github/workflows/ci.yml`, Windows, fmt / clippy / test / check-specs) but not run: the repo has no remote. REQ-014-01 stays `no test`.
- Decisions in `docs/architecture.md`: indexer as a thread with message API, no async runtime, thiserror / anyhow, tracing, REQ references, visual checks.
- Open: (1) CI has never run; push to GitHub to check it (snapshot tests on WARP may need a threshold); (2) custom title bar from the spike not ported yet (comes with the toolbar in M6, polish in M10).
- Next: M2 (Spec audit).

### 2026-10-26 · M0b · Spikes: video

- Fixtures generated with ffmpeg 7.1 into `%TEMP%\gg-spike\video` (`spikes/video/fixtures.sh`): MP4 H.264, MOV HEVC (1080p, 4K, rotated 90°), MKV H.264 + Opus with chapters, WebM VP9 + Opus, plus 1 s GOP variants; 30 s with timecode, frame number and a flash + beep every second.
- **Video: Go with libmpv.** `spikes/video`: own FFI over `libmpv-2.dll` (shinchiro build 20261021, loaded at runtime), software render API on a render thread at display size, written into a `wgpu::Texture`, display-synced timing. All fixtures play; 0–4 mpv drops per 600 frames, 1–40 off-cadence frames, exact seeks on target; 1080p ≈ 1 core, 4K ≈ 2 cores (2.9 / 5.7 ms render per frame). Mains-power rerun after the first matrix was skewed by the battery.
- Decisions in `docs/architecture.md` (video playback); pitfalls added (libmpv render-thread rules, `rgb0` alpha, rotation, judder with audio-clock timing, `vulkan-1.dll` import, power source, Python).
- Open: (1) manual check by the user: controls by mouse and audio by ear (`cd spikes/video && cargo run --release -- --sync display`, fixtures in the combo box); (2) real phone videos not tried, none provided; (3) the UI misses a vblank in ≈ 12 % of frames while 4K plays on the iGPU, zero-copy (GL interop) is the lever if M9 needs it; (4) `libmpv-2.dll` is 121 MB and hard-imports `vulkan-1.dll`, a trimmed build in M11; (5) still open from M0a: snap layouts check, thumbnail cache layout (M5), fast flings (M6).
- Next: M1 (Foundation). M0 is done: three go results.

### 2026-10-26 · M0a · Spikes: grid and look

- M0 plan written and approved, split into M0a (grid + look, `spikes/grid`) and M0b (video). Thumbnail cache copied to `%TEMP%\gg-spike\thumbs` (66 096 files, 3.7 GB); the original wasn't opened.
- **Grid: Go.** p95 frame interval 7.2 ms at 12 columns over 100k items (7.3 ms at 250k), vsync off, integrated AMD GPU. Keys: worker-side texture creation, 4 low-priority decode threads, libwebp, own mipmapped textures, LRU that keeps the prefetch band.
- **Look: Go, approved by the user from screenshots.** Glass (dual Kawase in a paint callback) matches Chromium within 1–2 color levels at ≈ 0.4 ms per surface; Atkinson Hyperlegible Next with a baked `tnum` copy; title bar variant 3 (own buttons, native hit-test codes) works.
- Decisions in `docs/architecture.md`; pitfalls added (Defender first-read cost, vsync and p95, egui 0.37 API notes, …).
- Open: (1) manual check that snap layouts show when hovering the maximize button (`cargo run --release` in `spikes/grid`); (2) thumbnail cache layout vs Defender, to decide in M5; (3) fast flings at 12 columns outrun decoding, handle in M6.
- Next: M0b (video spike), `docs/rewrite/milestones/M00-spikes.md` tasks 10–13.

### 2026-10-26 · Setup

- Repo created next to the reference (`../good-gallery`). AGENTS.md, master plan, progress log, architecture stub, known pitfalls, glossary and design copies, all 17 specs ported (requirements unchanged, Verification tables reset, statuses `Approved`), `.claude/settings.json` (reference readable, not editable).
- Open: MSVC C++ build tools aren't installed yet, so nothing links (see `docs/known-pitfalls.md`). The user installs them before M0.
- Next: M0, write `docs/rewrite/milestones/M00-spikes.md`.
