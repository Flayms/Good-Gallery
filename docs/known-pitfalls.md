# Known Pitfalls

Add every pitfall you hit, either to avoid it in the future or for us to fix later on.

## Toolchain

* **MSVC C++ build tools are required** on Windows (`link.exe`); without them even a hello world fails with "`link.exe` returned an unexpected error". Install the "Desktop development with C++" / VC Tools workload, e.g. `winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"`. C dependencies (libwebp, dav1d, a video backend) need them too.
* **`cargo` may be missing from `PATH` in the Bash tool**: rustup lives in `~/.cargo/bin`, which is on the Windows user `PATH` but not always in Git Bash. Use the PowerShell tool or `export PATH="$HOME/.cargo/bin:$PATH"`.
* **egui / eframe / wgpu APIs change between minor versions.** Pin exact versions and check docs.rs for the pinned one; examples from memory are often for another version.

## Carried over from the reference

These still apply to the Rust implementation. The reference's own `docs/known-pitfalls.md` has the Electron-specific rest.

* **Folder mtimes** (quick scans) only change when direct children are added, removed or renamed, not on in-place edits or changes deeper down. Don't use them to skip whole subtrees or file stats outside `quick` scans.
* **Watching on Windows reports last-access changes** too, so the indexer's own reads cause events; `changes` scans must keep stating only reported files. Expect the same from `notify` (ReadDirectoryChangesW).
* **SQLite query plans:** the tag filters in the reference `src/main/db/tag-search.ts` rely on unary `+` (`+media.id`, `+tag_id`) to pin plans; removing them looks harmless but makes common-tag searches 10–100× slower. Keep the SQL as is.
* **`media.meta_version`** (vs `METADATA_VERSION`) is how the indexer tells "unchanged file" apart from "unchanged file, old extractor". Bump the constant whenever metadata extraction changes; a root with rows behind is upgraded to a full scan.
* **Migrations:** the schema source of truth is the reference's `drizzle/*.sql` (statements separated by `--> statement-breakpoint`, each chunk prepared as one statement) plus its `__drizzle_migrations` bookkeeping. Keep both compatible so databases move between the two apps. Expression indexes were avoided in favor of generated columns.
* **EXIF orientation:** the reference pins its mapping with a test against `sharp.autoOrient()` (`src/indexer/thumbnail.test.ts`). Port the expected results, not the sharp call order.
* **The user's real data** (`%APPDATA%\good-gallery\gallery.db`, `thumbs\`) must never be opened in place: an open with WAL writes `-shm` / `-wal` files and migrations would change the schema. Copy first.
