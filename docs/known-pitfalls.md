# Known Pitfalls

Add every pitfall you hit, either to avoid it in the future or for us to fix later on.

## Toolchain

* **MSVC C++ build tools are required** on Windows (`link.exe`); without them even a hello world fails with "`link.exe` returned an unexpected error". Install the "Desktop development with C++" / VC Tools workload, e.g. `winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"`. C dependencies (libwebp, dav1d, a video backend) need them too.
* **`cargo` may be missing from `PATH` in the Bash tool**: rustup lives in `~/.cargo/bin`, which is on the Windows user `PATH` but not always in Git Bash. Use the PowerShell tool or `export PATH="$HOME/.cargo/bin:$PATH"`.
* **egui / eframe / wgpu APIs change between minor versions.** Pin exact versions and check docs.rs for the pinned one; examples from memory are often for another version.

## egui / eframe 0.37

* **`App::ui(&mut self, ui, frame)`** replaced `update`; screenshots go through `ctx.request_screenshot(callback)`, not an event; `Ui::id` and `Id::new` are deprecated; `Color32::from_white_alpha` isn't `const` (use `from_rgba_premultiplied`).
* **No OpenType feature API** (`tnum`, `ss01`, …): bake features into a font copy with `pyftfeatfreeze` (fonttools). Variable weights work through `FontTweak::coords`.
* **egui-managed textures have no mipmaps**, so large images drawn small alias. Register own `wgpu::Texture`s (`Rgba8Unorm`) with mips via `register_native_texture_with_sampler_options`.
* **`eframe` default features can't include `winit/default`** in a dependency spec (no slashes); use the defaults.

* **Dropping a `FullOutput` with texture deltas panics in debug builds** (`TexturesDelta` asserts on drop). Tests that call `ctx.run_ui` themselves must `.textures_delta.clear()` the result.
* **The root `Ui` of an `egui_kittest` harness has a margin**, so painting `ui.max_rect()` leaves a black frame. Paint the window background on `LayerId::background()` over `ctx.content_rect()`.

* **wgpu harnesses on parallel test threads crash the test process** on Windows (`STATUS_HEAP_CORRUPTION`). UI tests take a global lock through `tests/common::Fixture`; keep every `.wgpu()` harness behind it.
* **Labels default to the non-interactive `fg_stroke` color.** `theme` sets it to Paper; secondary text sets Mist itself.
* **Icon-only buttons need an accessible name** for tests and screen readers: `icons::icon_button` sets one through `Response::widget_info`.

* **egui's `ComboBox` has no accessible name** without a label drawn beside it; its value is `current_text_value`. Set the name with `widget_info` after `show_ui` (`settings_page::name_combo`) and query its value with `accesskit_node().value()`.
* **`ctx.egui_wants_keyboard_input()` is true for any focused widget**, not only text inputs. Tell a focused text input by its state: `TextEditState::load(ctx, id).is_some()`.
* **A paint callback ignores the painter's opacity** (`ui.multiply_opacity` / `set_opacity` reach only ordinary shapes): pass the opacity into the callback (`glass::place`).
* **wgpu's `TextureView` is `Clone`** (cheap), so textures made on worker threads can hand their view to glass.
* **Don't `destroy()` a texture a paint callback may still use.** Callbacks queued this frame run `prepare` after the UI code, so ending the edit session mid-frame (the _Discard_ button) and destroying its texture made `create_bind_group` fail fatally (#69). Drop the texture instead, wgpu frees it with its last view and bind group, and keep per-frame bind groups in the callback, not in `CallbackResources`, so they don't keep it alive.
* **ICU4X formatters aren't `Send`** (yoke'd `Rc` data): cache them in a `thread_local!`, not a `static Mutex`.

## Visual checks

* **Screenshot of the real app:** `cargo run -p app -- --screenshot <file.png>` renders 3 frames, saves the window and exits. Save into the scratchpad, then look at the PNG. Snapshot tests: `cargo test -p app`; `UPDATE_SNAPSHOTS=1` rewrites `crates/app/tests/snapshots/*.png` (check the new PNG before committing it; `*.new.png` / `*.diff.png` are ignored).
* **Snapshots must not depend on the machine.** `tests/common` pins the locale (`format::set_locale`) and the time zone (`domain::time_zone::set_fixed`, UTC) for every harness. Go through `domain::time_zone` instead of `chrono::Local` for anything shown, grouped or searched by day, or snapshots taken in Europe fail on the UTC CI runner.

## libmpv

* **`libmpv2` (crate) only wraps the OpenGL render API.** The software renderer (`"sw"`) needs raw `mpv_render_param`s (`SW_SIZE`, `SW_FORMAT`, `SW_STRIDE`, `SW_POINTER`); use an own FFI. Its import library is MinGW (`libmpv.dll.a`), so loading the DLL at runtime with `libloading` also avoids link trouble with MSVC.
* **The render thread must not call client functions** (`mpv_get_property` etc.): render.h forbids it, it can stall the core (timeouts, "mpv_render_context_render() not being called or stuck"). Pass the video size from the UI thread.
* **`rgb0` leaves the fourth byte undefined**, and egui blends with alpha: force it to 255 (≈ 0.7 ms per 1080p frame) or sample with alpha 1 in a shader.
* **The software renderer ignores rotation.** `video-params/rotate` is set, but frames and `dwidth` / `dheight` stay unrotated: rotate the quad.
* **Default timing judders against our vsync.** With frames timed to the audio clock, a 30 fps video on 60 Hz shows frames for 1 or 3 vblanks in up to 44 % of frames, varying run to run. Use `video-sync=display-resample` + `override-display-fps`, render once per UI frame with `report_swap`, and skip frames whose next-frame info has `REPEAT`.
* **`libmpv-2.dll` hard-imports `vulkan-1.dll`** (shinchiro builds): without the Vulkan loader it doesn't load. Load at runtime and degrade.
* **Tests and `cargo run` find libmpv only through `GOOD_GALLERY_MPV`** (or `target\debug\mpv\libmpv-2.dll`, or `PATH`). Point it at `target/helpers/mpv/libmpv-2.dll` (`packaging/fetch-helpers.ps1`); without it `crates/app/tests/video.rs` skips and videos show their preview.
* **A late `FILE_LOADED` belongs to the previous `loadfile`** when the user steps quickly between videos. Count `START_FILE` events against the `loadfile`s sent and accept frames only when they match; a `loadfile` mpv refuses sends no `START_FILE`.
* **mpv's properties (`dwidth`, `duration`, `eof-reached` …) are the previous file's until the new one's `FILE_LOADED`**; `loadfile` only queues the switch. Read them only after it.
* **With `keep-open=yes` mpv pauses on the last frame at the end** (`eof-reached`); unpausing does nothing. Seek to 0, then unpause.
* **wgpu needs `bytes_per_row` aligned to 256**, mpv wants stride and pointer aligned to 64: allocate the frame buffer with a 256-byte-aligned stride and a 64-byte-aligned pointer.

## Performance measurement

* **Check the power source before benchmarking.** On battery (`Get-CimInstance Win32_Battery` → `BatteryStatus` 1 = discharging, 2 = AC) an idle, continuously repainting window measured 30 ms per frame in one run and 16.7 ms in the next. Record an idle baseline at the start and end of a matrix.
* **At 60 Hz with vsync, every frame interval is ≈ 16.7 ms**, so p95 targets below that can only be checked with `PresentMode::AutoNoVsync`. Count intervals > 20 ms with vsync on separately for real stutter.
* **Windows Defender scans every first read of a file** (≈ 13 ms per thumbnail, verdicts expire within minutes). Warm files before measuring rendering, and measure cold IO separately. A 30-byte header read doesn't warm a file, a full read does.
* **Too many decode threads hurt frame time** on an integrated GPU: 14 threads p95 67 ms, 4 threads 7 ms. Run workers at below-normal priority.
* **An LRU that only counts drawn textures as used evicts its own prefetch** and decodes in a loop. Touch prefetched entries when requesting them.
* **libwebp scaled decoding (`use_scaling`) doesn't decode faster**; VP8 decodes at full size and rescales.

## Shell and tools

* **robocopy exit code 1 means "files copied"**, not failure (≥ 8 is an error).
* **PowerShell passes `$null` to a P/Invoke `string` parameter as `""`**: use `[NullString]::Value` (e.g. `FindWindowW`). `Process.MainWindowHandle` may return a hidden 16 × 16 helper window right after start; find the window by title.
* **Windows Python doesn't see Git Bash's `/tmp`**; use the scratchpad or relative paths.
* **`python` may not run at all** (a mise shim points to a missing install). Use `node` for small scripts.
* **Running something that loads a downloaded binary** (e.g. `libmpv-2.dll`) can be blocked by auto mode until the user approves it. Ask before the first run.
* **"The paging file is too small" (os error 1455) while compiling** shows up as "can't find crate" / "invalid metadata" errors: too many parallel rustc processes for the free memory (C: was nearly full). Build with `cargo … -j 4`.
* **`cargo` may not be on the agent shell's `PATH`**: prepend `$env:USERPROFILE\.cargo\bin`.
* **Restoring a file from a backup with `Copy-Item` or `cp` keeps the older mtime**, so cargo doesn't rebuild and keeps testing the mutated code. `touch` the file after restoring (mutation checks for red tests).
* **Worktrees sharing one `CARGO_TARGET_DIR` test each other's code**: the workspace crates get the same artifact names in every worktree, and cargo's mtime check doesn't rebuild when the other worktree's build is newer than this one's sources (a run showed another branch's sidebar button in its snapshots). Give each worktree its own target, or `cargo clean -p app -p db -p domain -p indexer -p media -p meta -p xtask` before switching (the dependencies stay built).
* **Long Git Bash heredocs can fail with "unexpected EOF while looking for matching `'`"** (the content is cut). Write scripts with the Write tool into the scratchpad and run them from there.
* **Git Bash heredocs here turn `\\` into `\`**, even with a quoted `'EOF'`, which corrupts escapes in scripts (`\0` became a NUL byte in a `.mjs`). Use the Edit tool, or `chr(92)` in Python.
* **Node's output through `| tail` was lost** when the process was killed by `timeout`; redirect to a file to see what a hanging Node script printed.
* **exiftool on Windows mangles non-ASCII command-line arguments**; pass tag values through an argument file (`-@ file`, UTF-8). In exiftool's struct syntax, a comma inside a value is escaped with `|`.

## Indexer

* **Changing `media::exiftool::read_args` changes what existing index rows mean**: they were read with exiftool-vendored v38's default options (`-fast`, `struct=1`, `keepUTCTime`, `-ignoreMinorErrors`, a UTF-8 repair filter). Bump `METADATA_VERSION` if the extracted values change.
* **Killing the Windows `exiftool.exe` leaves its `perl.exe` child running** with the pipes open; `Process::drop` ends the tree with `taskkill /T /F`. That isn't enough when the app exits: each drop takes ≈ 1.1 s (the indexer gets 2 s), and the PAR-packed exiftool 12.83 survived even a finished taskkill. The app's job object (`media::helpers`) is what ends them. Leftover exiftool processes also keep the stdout pipe of whoever started them open, so `cargo run … | Select-Object` seems to hang after the app has exited.
* **`db::open_reader` can't create or migrate the index**: open it only after `db::open`. App checks on a data folder made by `cargo xtask libraries` don't catch this; also start once on an empty one.
* **A watcher that can't start must not queue a scan**: it would follow every completed scan, so scans would run forever. Leave the retry to the schedule.
* **Watcher tests that wait for a quick scan race the folder mtime**: a file written within the folder's stored mtime tick isn't seen by the quick scan. Keep the indexer busy with a gated scan of another library instead of relying on the new file.
* **Folder mtimes need `FILE_FLAG_BACKUP_SEMANTICS`** to open a folder for `set_modified` on Windows (see the scan tests' `set_mtime`).

* **A stat on a hung SMB share blocks its thread for the OS timeout** (often 30–60 s), past our 5 s reachability timeout. `reachability::with_timeout` leaves that thread behind; fine for single checks, but periodic checks of many offline libraries must not start a new check while the previous one for the same library still runs. (M4c: scans run one at a time and queued scans merge per library, so the minute-by-minute re-checks leave at most one stuck thread behind per hung library.)

## Thumbnails

* **Replacing a file on Windows fails while someone has it open without delete sharing** (Defender scanning a file just written, often): `std::fs::rename` returns "Access is denied". `ThumbCache::write` retries for ≈ 300 ms; a test under full-suite load hit it.
* **exiftool can't write `PreviewImage` into a plain JPEG**, so embedded previews can't be generated as fixtures; the unit tests fake the preview source. `ThumbnailImage` (IFD1) is writable and tests the binary extraction.
* **Compare transparent thumbnails premultiplied.** libwebp drops the color of fully transparent pixels, sharp's output doesn't, so raw RGBA differences look large (mean 5.5) where nothing visible differs (1.4 premultiplied).
* **The `image` crate decodes AVIF only with `avif-native`** (dav1d, a C library with a meson build); AVIF goes through ffmpeg instead.
* **`fit: 'inside'` in sharp enlarges** unless `withoutEnlargement` is set: the 100 px ThumbHash input is enlarged from small images, the thumbnails never are.

## Gallery

* **egui turns Ctrl + wheel into a zoom delta**, not a scroll: read `Event::MouseWheel` with `modifiers.ctrl` from `input.raw.events` for zoom steps. winit reports one *line* per notch and may coalesce a fast spin into several lines in one event, unlike the DOM's one event per notch.
* **Don't clone the tile layout per frame**: at 100k items it is ≈ 3 MB of `Vec<f64>`. Take it out of the struct for the frame and put it back.
* **Hard links need the same volume**: `cargo xtask gallery-bench` links the cache copy's files, so its data folder must be on that drive.

## Viewer

* **A video tile's accessible label is "name, video, m:ss"** (e.g. `1.mp4, video, 1:00`), not the file name alone; tests that click a video tile must use it.
* **egui's `Slider` rail is invisible with our widget visuals** (transparent inactive fill); the video controls draw their own slider.

* **kittest's default step is 0.25 s**, so two clicks a couple of frames apart are no double-click for egui (0.3 s window). Tests of double-clicks build their harness with `with_step_dt(1.0 / 60.0)` (`common::harness_with_step`).
* **Fades timed with `Instant` never finish in tests** (frames run in microseconds): a snapshot catches a half-transparent photo. Time UI animations with egui's `input.time`, which kittest advances per step.
* **A new `egui::Area` is invisible in its first frame** (a sizing pass) unless its state exists from an earlier frame. The viewer's area keeps one fixed id, so only the very first opening loses a frame.
* **Screenshots look brighter than they are** when read downscaled; measure pixel values before chasing a "missing" scrim.

* **kittest's `click()` sends real pointer events at the node's center**, so a node outside the window (a long panel) never gets the click. Make the harness tall enough (`set_size`) or scroll first.
* **`WidgetInfo::selected` reaches AccessKit as `toggled`**, not `selected`: query `accesskit_node().toggled()`.
* **An `egui::Button` with a `fill` draws its frame** even with `.frame(false)`; set the fill only when the frame should show.
* **A widget's `hovered()` is false while another widget on top of it is hovered** (the filmstrip under its thumbnails): use `ui.rect_contains_pointer(rect)` for wheel handling over an area.

## Search

* **egui popups lose their open state when their button's id changes.** A button's automatic id moves with its own look (a frame or text that appears) and with the number of widgets before it (an empty gallery draws none), so the menu closes. Give popups fixed ids (`Popup::id`), as `toolbar::menu` does.
* **A single-line `TextEdit` gives up focus on Enter and Escape in that same frame**: check `lost_focus()` too before handling the keys. A suggestions `Area` lies outside the toolbar's clip rect, so `ui.rect_contains_pointer` is false over it; compare with `pointer_latest_pos()`.
* **egui 0.37's default fonts have no emoji or `★`**, and `egui_kittest` panics on missing glyphs. Write ratings in words or draw icons; turn off `DatePickerButton::show_icon` (📆).
* **Labels repeat across the window** (a library in the sidebar and on the Settings page, "5 stars" in the sidebar and in Filters): query tests by role and label, or take the last match for a menu that opens on top.

* **A sort-index walk with SQLite's default page cache is a read call per row.** On a 200 MB index, listing 500k items took 5 s; memory-mapped reads plus a 64 MB cache bring it to 0.7 s (`db::open_reader`). Benchmarks must also run in release: the debug build doubled the times.

## Index and metadata

* **Folder mtimes** (quick scans) only change when direct children are added, removed or renamed, not on in-place edits or changes deeper down. Don't use them to skip whole subtrees or file stats outside `quick` scans.
* **Watching on Windows reports last-access changes** too, so the indexer's own reads cause events; `changes` scans must keep stating only reported files. Expect the same from `notify` (ReadDirectoryChangesW).
* **SQLite query plans:** the tag filters in `db::gallery` rely on unary `+` (`+media.id`, `+tag_id`) to pin plans; removing them looks harmless but makes common-tag searches 10–100× slower. Keep the SQL as is.
* **`media.meta_version`** (vs `METADATA_VERSION`) is how the indexer tells "unchanged file" apart from "unchanged file, old extractor". Bump the constant whenever metadata extraction changes; a root with rows behind is upgraded to a full scan.
* **Migrations:** `crates/db/migrations/*.sql` (statements separated by `--> statement-breakpoint`, each chunk prepared as one statement) plus the `__drizzle_migrations` bookkeeping that existing indexes carry. Never edit an applied migration (its hash is recorded); add a new file and journal entry. Expression indexes were avoided in favor of generated columns.
* **EXIF orientation:** `req_005_03_turns_every_orientation_upright` pins the expected pixels for all 8 orientations. Change the mapping only against that test.
* **The user's real data** (`%APPDATA%\good-gallery\gallery.db`, `thumbs\`) must never be opened in place: an open with WAL writes `-shm` / `-wal` files and migrations would change the schema. Copy first.

## Packaging and releases

* **Git Bash's `tar` is GNU tar**, which can't write zip or read 7z. Call Windows' own `%SystemRoot%\System32\tar.exe` (bsdtar) by its full path; it writes zip with `-a` and reads 7z, but has no `-s` (rename while extracting): use `--strip-components` and copy afterwards.
* **Freshly unpacked executables can't be moved or renamed right away** ("access denied"): Defender holds them open while it scans. Copy them instead, and retry the delete of the original.
* **`git add` right after appending to a file through a still-open handle stages nothing** on Windows (the release tests): close the file before calling git.
* **The release tests need git-cliff**: set `GIT_CLIFF` or install it (2.14.2, as CI does); without it they skip, with `CI` or `GG_REQUIRE_HELPERS` set they fail.
* **NSIS's uninstaller copies itself to a temp folder and returns at once.** Run it with `/S _?=<install folder>` to make it wait (the smoke test does); it then can't delete itself, so remove the folder afterwards.
* **NSIS: `/D=<folder>` must be the last argument and unquoted**, even with spaces.
* **makensis locally:** install NSIS (`Program Files (x86)\NSIS` is found) or point `MAKENSIS` at a `makensis.exe`.
* **GitHub's windows-2025 images don't come with NSIS** (older ones did). The release workflow installs it with `choco install nsis`.
* **The packaged app (`--features packaged`, as `cargo xtask dist` builds it) has no console**: its output only shows when the caller redirects stdout / stderr, so look at `logs\app.log` in the data folder instead. Never start it without `--data-dir` during development: it opens the real `%APPDATA%\good-gallery`.
* **A zip in `target/dist` can stay locked for minutes** after it was written (Defender or an open Explorer window), so `cargo xtask dist` fails with "used by another process": pass another `--out`.

## Linux

* **Setup on Ubuntu:** `build-essential pkg-config` (rusqlite's bundled SQLite and libwebp compile C), `libimage-exiftool-perl ffmpeg libmpv2`, `mesa-vulkan-drivers` (lavapipe: wgpu and the snapshot tests on a machine without a GPU, e.g. a VM) and `xvfb` (`xvfb-run -a cargo run -p app -- --screenshot …` without a display). The first build of the workspace on 2 cores takes well over 10 minutes; build with `-j2` on a 4 GB machine.
* **`PR_SET_PDEATHSIG` fires when the *thread* that started the child ends**, not the process. Helpers are started from short-lived scan threads and pooled, so `media::helpers::spawn_helper` starts them all from one thread that lives as long as the app. Start every helper program through it. Without it, exiftool outlives the app on Linux (it keeps waiting on stdin).
* **Threads inherit the nice value of the thread that creates them**, and an unprivileged process can't lower it again: the helper spawner starts in `end_helpers_with_app` on the main thread, before any decode thread (nice 10) could start it.
* **`pre_exec` makes std `fork()` instead of `posix_spawn`**, so starting a helper copies the app's page tables (a few ms for a large app). Fine for exiftool's pool and one ffmpeg per video thumbnail; don't start helpers in a tight loop.
* **The ruleset on `main` requires checks named exactly `Lint` and `Test`**: renaming or splitting CI jobs leaves those checks "Expected — Waiting for status" forever and blocks merging. The `Lint` and `Test` jobs in `ci.yml` sum up the per-platform jobs; add a new job to their `needs`.
* **CI tests Linux in an `ubuntu:26.04` container**: the `*_linux.png` snapshots come from its lavapipe and the video tests from its mpv 0.41. Rewrite the snapshots when the container's release changes. The release archive is built on `ubuntu-latest` (an older glibc runs on more distributions).
* **ext4 lists folder entries in hash order** (NTFS sorted); the walk sorts each listing so scans behave the same everywhere. Don't depend on `read_dir` order.
* **File mtimes on Linux come from a coarse clock**: files written milliseconds apart often get the same mtime, to the nanosecond. Thumbnail eviction orders by mtime, so tests that depend on which file goes first set the mtimes they compare (`File::set_modified`) instead of writing files one after another.
* **Linux paths are case-sensitive and `\` is an ordinary file name character**: path rules in `indexer::libraries` branch on `cfg!(windows)`, and tests build expected paths with `Path::join`, not `"D:\\…"` strings. `std::path::absolute` keeps `..` on Unix, so library paths resolve `..` lexically, as on Windows (a symlink before a `..` isn't followed). Sidecars that differ only in letter case each go to the media file named the same way (`domain::media::find_sidecar`).
* **Folders can't be opened for writing on Linux** (EISDIR); set their mtime through a read-only handle.
* **Git runs only executable hooks outside Windows**: tests that write hooks set mode 755.
* **mpv 0.41's software renderer aborts the process on rotated videos** (`mp_image_crop` assertion). The player sets `video-rotate=no` and reads the rotation from `video-dec-params/rotate`, turning the frame itself as before.
* **egui hands a press's click and its drag out separately**: the topmost widget that senses clicks gets the click, the topmost one that senses drags the drag, even if it lies below. The resize handles check that the press began on their own layer (`Context::layer_id_at`), or a click on a menu item at the window's edge would also start a resize.
* **A new egui `Area` is invisible for its first frame** (sizing pass): tests that click the Linux caption buttons or resize handles run one empty frame first.
* **Snapshots on Linux** are their own files (`<name>_linux.png`, `common::snapshot_name`): the caption buttons and the narrower room kept for them change the toolbar. lavapipe renders everything else pixel-identical to WARP, so a difference elsewhere is a real change.
* **The kittest harness insets the UI by 8 px but not the caption buttons** (they align to the window's edge), so in Linux snapshots the buttons sit 8 px above the toolbar row; in the app they line up.

## Browser version (spec 019)

* **`sqlite-wasm-rs` needs clang** to compile SQLite for wasm (`failed to find tool "clang"`), and its **`wasm-bindgen` feature**: without it the page fails to load with `Failed to resolve module specifier "env"` (SQLite's clock, randomness and local time are unresolved imports).
* **rusqlite 0.41 has no `DatabaseName`:** use `rusqlite::MAIN_DB` with `serialize` / `deserialize_read_exact` (features `serialize`, `backup`).
* **`cargo install` builds in `/tmp`**, which is a small RAM disk on some machines (`Disk quota exceeded`, os error 122). Set `CARGO_TARGET_DIR` to a folder on the disk for the install.
* **Snapshot tests embed the temp folder's path** (the info panel's location): a longer `TMPDIR` wraps it to two lines and fails `info_sheet_linux`. Run the native tests with the default `/tmp`.
* **Playwright's Chromium needs system libraries** on Linux (`libnspr4.so: cannot open shared object file`): `sudo npx playwright install-deps chromium` (CI: `npx playwright install --with-deps chromium`).
* **egui misses clicks that arrive with the pointer** (Playwright's `mouse.click` moves, presses and releases at once): the browser test moves first, waits 100 ms, then presses (`click` in `web.spec.js`).
* **A 1 % screenshot tolerance hides small changes** like a status badge: the browser test allows 50 pixels. Fix the page's clock (`page.clock.setFixedTime`), time zone and locale, since Settings show when a library was last indexed.
* **The page can't be driven through Chromium's directory picker headless:** the test deletes `showDirectoryPicker`, so the page falls back to the folder upload input, which `setInputFiles` fills.
* **`pkill -f <pattern>` in a shell command can kill the shell itself** when the pattern appears in its own command line. Stop the test server by port: `fuser -k 8765/tcp`.
