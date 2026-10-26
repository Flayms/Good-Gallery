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

## Visual checks

* **Screenshot of the real app:** `cargo run -p app -- --screenshot <file.png>` renders 3 frames, saves the window and exits. Save into the scratchpad, then look at the PNG. Snapshot tests: `cargo test -p app`; `UPDATE_SNAPSHOTS=1` rewrites `crates/app/tests/snapshots/*.png` (check the new PNG before committing it; `*.new.png` / `*.diff.png` are ignored).

## libmpv

* **`libmpv2` (crate) only wraps the OpenGL render API.** The software renderer (`"sw"`) needs raw `mpv_render_param`s (`SW_SIZE`, `SW_FORMAT`, `SW_STRIDE`, `SW_POINTER`); use an own FFI. Its import library is MinGW (`libmpv.dll.a`), so loading the DLL at runtime with `libloading` also avoids link trouble with MSVC.
* **The render thread must not call client functions** (`mpv_get_property` etc.): render.h forbids it, it can stall the core (timeouts, "mpv_render_context_render() not being called or stuck"). Pass the video size from the UI thread.
* **`rgb0` leaves the fourth byte undefined**, and egui blends with alpha: force it to 255 (≈ 0.7 ms per 1080p frame) or sample with alpha 1 in a shader.
* **The software renderer ignores rotation.** `video-params/rotate` is set, but frames and `dwidth` / `dheight` stay unrotated: rotate the quad.
* **Default timing judders against our vsync.** With frames timed to the audio clock, a 30 fps video on 60 Hz shows frames for 1 or 3 vblanks in up to 44 % of frames, varying run to run. Use `video-sync=display-resample` + `override-display-fps`, render once per UI frame with `report_swap`, and skip frames whose next-frame info has `REPEAT`.
* **`libmpv-2.dll` hard-imports `vulkan-1.dll`** (shinchiro builds): without the Vulkan loader it doesn't load. Load at runtime and degrade.
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
* **`python` may not run at all** (a mise shim points to a missing install). Use `node` for small scripts (`spikes/video/summarize.js`).
* **Running a spike that loads a downloaded binary** (e.g. `libmpv-2.dll`) is blocked by auto mode until the user approves it. Ask before the first run.
* **Spike crates are standalone**: run `cargo` inside `spikes/<name>/`; their `results/` and `vendor/` folders are git-ignored.

## Carried over from the reference

These still apply to the Rust implementation. The reference's own `docs/known-pitfalls.md` has the Electron-specific rest.

* **Folder mtimes** (quick scans) only change when direct children are added, removed or renamed, not on in-place edits or changes deeper down. Don't use them to skip whole subtrees or file stats outside `quick` scans.
* **Watching on Windows reports last-access changes** too, so the indexer's own reads cause events; `changes` scans must keep stating only reported files. Expect the same from `notify` (ReadDirectoryChangesW).
* **SQLite query plans:** the tag filters in the reference `src/main/db/tag-search.ts` rely on unary `+` (`+media.id`, `+tag_id`) to pin plans; removing them looks harmless but makes common-tag searches 10–100× slower. Keep the SQL as is.
* **`media.meta_version`** (vs `METADATA_VERSION`) is how the indexer tells "unchanged file" apart from "unchanged file, old extractor". Bump the constant whenever metadata extraction changes; a root with rows behind is upgraded to a full scan.
* **Migrations:** the schema source of truth is the reference's `drizzle/*.sql` (statements separated by `--> statement-breakpoint`, each chunk prepared as one statement) plus its `__drizzle_migrations` bookkeeping. Keep both compatible so databases move between the two apps. Expression indexes were avoided in favor of generated columns.
* **EXIF orientation:** the reference pins its mapping with a test against `sharp.autoOrient()` (`src/indexer/thumbnail.test.ts`). Port the expected results, not the sharp call order.
* **The user's real data** (`%APPDATA%\good-gallery\gallery.db`, `thumbs\`) must never be opened in place: an open with WAL writes `-shm` / `-wal` files and migrations would change the schema. Copy first.
