# Good Gallery

A desktop gallery for large photo and video libraries (50k–500k files). Media appear in a virtualized masonry, justified, grid or list view. Libraries on network shares are supported, and thumbnails are cached locally. Search uses the tags that are already embedded in your files.

Built in Rust with eframe / egui on wgpu.

![Masonry view with the sidebar showing libraries, people, places, ratings and tags](docs/screenshots/masonry.png)

## Features

- **Views:** Masonry (columns), Justified (rows of equal height), Grid (square tiles) and List (details). All are virtualized, so they stay fast at any library size. Media are grouped by day, with the date and the number of photos and videos above each day. Switch views, zoom, sort and grouping under _View_, or zoom with Ctrl + mouse wheel.
- **Network shares:** libraries can live on `\\server\share`. Thumbnails (WebP and ThumbHash placeholders) are cached locally, and the cached index stays browsable while a share is offline.
- **Change detection:** a file watcher picks up changes, plus periodic quick scans. Manual rescans are also possible.
- **Search:** combine tags (include or exclude, hierarchical), file name text and folder; media type, date range and rating are under _Filters_. Sort by date taken, date modified, name, path or size.
- **Sidebar:** libraries with a folder tree, People and Places (from keyword hierarchies and face regions), ratings and popular tags.
- **Viewer:** the photo floats over the blurred window, with controls that step aside while you look. Keyboard navigation, an optional filmstrip to jump through the gallery, zoom and pan, video playback, a metadata panel and "Show in Explorer".
- **Look:** dark, neutral chrome so only your photos bring color; the toolbar and menus are frosted glass over the gallery.
- **Read-only:** tags and ratings come from EXIF / IPTC / XMP and `.xmp` sidecars. Files are never modified.

| Justified                                                             | Grid                                                     |
| --------------------------------------------------------------------- | -------------------------------------------------------- |
| ![Justified view](docs/screenshots/justified.png)                     | ![Grid view](docs/screenshots/grid.png)                  |
| **List**                                                              | **Viewer**                                               |
| ![List view with dates, sizes and ratings](docs/screenshots/list.png) | ![Viewer with the info panel](docs/screenshots/viewer.png) |

Supported formats: JPEG, PNG, WebP, GIF and AVIF images, plus videos (MP4, MOV, MKV, WebM and more, with poster-frame thumbnails).

## Install

Download from the [releases](https://github.com/Flayms/Good-Gallery/releases) (Windows 10 or 11, 64-bit):

- **`good-gallery-<version>-setup.exe`:** installs for your user (no admin rights), into a folder you can change, with a Start menu shortcut. Uninstall it under _Settings → Apps_.
- **`good-gallery-<version>-portable.zip`:** extract anywhere and run `good-gallery.exe`.

exiftool, ffmpeg and libmpv come with both. The downloads aren't code-signed yet, so Windows SmartScreen may warn: choose _More info_ → _Run anyway_, or unblock the zip (_Properties_ → _Unblock_) before extracting.

Add a library folder under **Settings**. The index, settings, thumbnails and logs live in `%APPDATA%\good-gallery`. Updates keep using it, so nothing is indexed twice. Uninstalling keeps that folder.

## Development

Requirements: Rust (pinned in `rust-toolchain.toml`, installed through rustup) and, on Windows, the MSVC C++ build tools with the Windows SDK. Indexing needs exiftool, thumbnails of videos ffmpeg, playback libmpv: run `./packaging/fetch-helpers.ps1` once and point `GOOD_GALLERY_EXIFTOOL`, `GOOD_GALLERY_FFMPEG` and `GOOD_GALLERY_MPV` at the files in `target/helpers/`, or put the programs on `PATH`. Releases need git-cliff, the version CI uses: `cargo install --locked git-cliff@2.14.2` (or point `GIT_CLIFF` at it). Builds other than the packaged app (`cargo xtask dist`) keep their data in `%APPDATA%\good-gallery-rust`, so development never touches the real index.

| Command                             | Purpose                                                                                                        |
| ----------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `cargo run -p app`                  | Run the app (`-- --data-dir <folder>` for another data folder)                                                 |
| `cargo test --workspace`            | All tests, including the UI snapshot tests                                                                     |
| `cargo clippy --workspace --all-targets -- -D warnings` | Lint                                                                                       |
| `cargo fmt --all`                   | Format                                                                                                         |
| `cargo xtask check-specs`           | Check that specs and tests reference each other                                                                |
| `cargo xtask screenshots`           | Render the README screenshots into `docs/screenshots/` from a generated demo library (needs exiftool, ffmpeg)  |
| `cargo xtask dist`                  | Build the installer and the portable zip into `target/dist/` (needs the helpers and NSIS)                       |
| `cargo xtask release`               | Cut a release: bump the version, update `CHANGELOG.md`, tag and push (`--dry-run` previews; needs git-cliff)   |

CI ([.github/workflows/ci.yml](.github/workflows/ci.yml)) checks formatting, clippy, the specs and all tests on every push and pull request.

### Releases

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org) (`feat: …`, `fix(viewer): …`, `feat!: …` for breaking changes). On an up-to-date, clean `main`, run `cargo xtask release`. It derives the next version from the commits since the last tag (breaking → major, or minor before 1.0; `feat` → minor; else patch), shows the release notes and asks before it sets the version in `Cargo.toml`, commits it with `CHANGELOG.md`, tags `vX.Y.Z` and pushes. Pass `major`, `minor`, `patch` or a version like `1.2.3` to override, `--yes` to skip the question. The pushed tag starts the [release workflow](.github/workflows/release.yml): it runs the checks, builds the installer and the portable zip, smoke-tests the installed app and publishes the GitHub release.

- [docs/architecture.md](docs/architecture.md): architecture and decisions
- [docs/glossary.md](docs/glossary.md): domain terms
- [docs/specs/](docs/specs/): feature specs and their plans
- [docs/roadmap.md](docs/roadmap.md): upcoming features and open issues
- [AGENTS.md](AGENTS.md): working guidelines for coding agents

## License

[GPL-3.0](LICENSE). The bundled ffmpeg and libmpv builds are GPL as well; see `packaging/windows/THIRD-PARTY-NOTICES.txt`.
