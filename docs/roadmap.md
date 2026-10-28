# Roadmap

Upcoming features, open issues and bug reports. Unchecked tasks in the specs' `plan.md` files belong to their spec; the larger ones are listed here too.

## Open issues

- **First release:** run `cargo xtask release` (pushes `main` and the tag). The release workflow has never run on GitHub (REQ-014-08).
- **Branch protection** on `main` requiring CI (REQ-014-02, a repository setting).
- **Clean machine check:** install and run on a Windows machine without development tools, especially whether `libmpv-2.dll` finds `vulkan-1.dll` without GPU drivers. A trimmed libmpv build would also shrink the 121 MB DLL.
- **Search speed** (REQ-007-14): broad searches take 0.25–0.7 s and suggestions 14 ms on 500k items (budget 50 ms / 10 ms). Needs a covering index or rows stored in sort order; see spec 007's plan.
- **Glass cost:** the glass pass rebuilds its per-tile bind groups and buffers every frame (≈ 300 for the viewer's scrim at 12 columns) and keeps slot textures until exit. Measure with `cargo xtask gallery-bench` before optimizing.
- **Scrolling while rendering:** thumbnails render at normal thread priority; the frame-time budget was measured with every thumbnail cached.
- **Viewer:** the chrome blurs the scrim, not the photo under it; photos oriented only in XMP show sideways; Tab doesn't move focus into the viewer (REQ-015-04).
- **Look:** the date pickers' popups stay Raised; counts use English digit grouping.
- **Metadata:** no time zone from the GPS position (REQ-004-03); ICC profiles aren't applied to thumbnails.
- **Tests:** REQ-002-11 (folders that can't be listed) has no test; the thumbnail cache tests `req_005_13_writes_leave_no_temp_files` and `replaces_a_thumbnail_that_is_open_for_a_moment` fail now and then in the first full run after a rebuild (Defender holds new files open longer than the rename retries).
- **Icon tooling:** the scripts that rendered `app-icon.ico` / `app-icon.png` and wrote the lucide SVGs depended on the old app's `node_modules` and were removed. Add an `xtask icons` command (render the SVG to a multi-size ICO with PNG entries, set lucide's `stroke` / `stroke-width`) before the next icon change.
- **Linux by hand:** dragging, resizing and the caption buttons on GNOME and KDE (Wayland and X11), _Show in folder_ in Nautilus and Dolphin, video audio. Only Xvfb (X11, no window manager) was tried.
- **Manual checks never done:** video audio by ear and the video controls by hand in the real app; snap layouts when hovering the maximize button.

## Features

- Update detection and auto updates for the app.
- Code signing (no certificate yet).
- Publish in the Microsoft Store.

### Manual tagging and rating

- **Spec:** extends [004-metadata](specs/004-metadata/spec.md) and [009-media-viewer](specs/009-media-viewer/spec.md) · **Tier:** 3 (first time the app writes to user files)
- Add and remove tags, Places and People, and set the rating, from the viewer and for a selection in the gallery.
- Write both embedded metadata and XMP sidecars.
- Open questions:
  - Default target: embedded (travels with the file, but rewrites originals and is impossible for some formats, e.g. most RAW and videos) or sidecar (safe, but lost when the file is moved without it)? A likely answer: sidecar for RAW and video, embedded otherwise, configurable per library.
  - Gallery selection model (multi-select) is a prerequisite and doesn't exist yet.
  - Libraries on read-only or offline network shares.

### Cropping and rotating

- Non-destructive, stored like the edits below; rotation possibly also as the EXIF orientation flag.
- Shares the edit storage decision with [Photo editing](#photo-editing), so decide that first.

### Faces

1. Show face regions already embedded in files (MWG / Microsoft regions, already read for People tags) as boxes in the viewer, toggleable.
2. Face detection and recognition (as in digiKam): detect faces, cluster them, let the user name clusters, write the names back as face regions (depends on [Manual tagging](#manual-tagging-and-rating)).

- Open questions: model and runtime (ONNX on the indexer threads?), where embeddings live (index database), performance on 500k files.

### Photo editing

- Non-destructive: edits are stored as parameters (index database, sidecar, or both), the original stays untouched; thumbnails and the viewer show the edited result; export writes a new file.
- Light: exposure, contrast, highlights, shadows, whites, blacks
- Curves (RGB and per channel)
- Color: white balance (temperature, tint), vibrance, saturation
- Effects: grain, vignette
- Open questions: GPU rendering with wgpu vs. CPU in the indexer; compatibility with other tools' sidecars (e.g. darktable, Lightroom XMP `crs:` tags); color profiles and bit depth.

### Platforms

- macOS support (Linux: spec 018).
- Linux follow-ups: AppImage / Flatpak / `.deb`; the desktop's button layout (`org.gnome.desktop.wm.preferences button-layout`) for the caption buttons; the display's refresh rate for display-synced video (60 Hz assumed); window shadows on compositors that don't add them to undecorated windows; reduced transparency where a desktop has a setting.
- Open photos with Good Gallery from the Windows context menu (and the macOS / Linux equivalents: a `MimeType=` line in the `.desktop` file and opening a file passed on the command line).

### Other

- Light mode.
- Browser version without a backend (wasm, hosted on GitHub Pages).
- Smaller bundle: Rust-native alternatives to exiftool (metadata), ffmpeg (video frames) and libmpv (playback)?
- Autoscroll by holding the middle mouse button and moving the pointer, as in a browser.
- `docs/known-pitfalls.md` grew large: what of it can be fixed, and what belongs elsewhere?
