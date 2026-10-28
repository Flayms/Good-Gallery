# 018 · Linux

**Status:** Implemented

## Goal

People with their libraries on a Linux desktop (and developers working on Linux) can build, run and use Good Gallery there with the same features and look as on Windows: indexing, thumbnails, search, the viewer with video, and the toolbar with the search in the title bar.

## Requirements

### REQ-018-01 · Builds and passes its checks on Linux

- **Given** a Linux machine with the pinned toolchain and the system packages listed in `README.md`
- **When** a developer runs the formatting check, clippy, the spec check and the tests
- **Then** they pass, and CI runs them on Linux too (besides Windows)

### REQ-018-02 · Data folder in the XDG data home

- **Given** the app on Linux started without `--data-dir`
- **When** it opens its index, settings, thumbnails and logs
- **Then** they live in `$XDG_DATA_HOME/good-gallery` (`~/.local/share/good-gallery` when the variable is unset or not absolute) for the packaged app, and in `good-gallery-rust` beside it for every other build

### REQ-018-03 · Helpers from the system

- **Given** exiftool, ffmpeg and libmpv installed by the system's package manager
- **When** the app needs them
- **Then** it finds them (exiftool and ffmpeg on `PATH`, libmpv where the system's dynamic loader finds it), unless the environment variables or a folder next to the app name others (REQ-011-07)

### REQ-018-04 · Search in a Linux-style title bar

- **Given** the main window on Linux (X11 or Wayland)
- **When** it is shown
- **Then** the toolbar with the search takes the place of the title bar, as in REQ-013-02, and there is no second bar above it
- **And** minimize, maximize and close sit at its right as round icon buttons, as GNOME's header bars draw them
- **And** empty toolbar space moves the window when dragged and maximizes or restores it when double-clicked, and the window can be resized from its edges and corners

### REQ-018-05 · Show in the file manager

- **Given** a media item in the viewer or the gallery
- **When** the user chooses _Show in folder_ on Linux
- **Then** the desktop's file manager opens its folder with the file selected, or the folder alone where the file manager can't select files

### REQ-018-06 · Desktop preferences

- **Given** the desktop's _animations_ setting turned off (GNOME's `enable-animations`)
- **When** the app runs
- **Then** animations are instant (REQ-013-07), following the setting while the app runs

### REQ-018-07 · Helpers end with the app

- **Given** exiftool or ffmpeg started by the app on Linux
- **When** the app exits, crashes or is killed
- **Then** they end too

### REQ-018-08 · Background work yields to the UI

- **Given** thumbnails decoding on Linux
- **When** the user scrolls
- **Then** the decode threads run at a lower priority than the UI thread, as on Windows

### REQ-018-09 · Linux package

- **Given** `cargo xtask dist` on Linux
- **When** it runs
- **Then** it builds a `.tar.gz` with the app, a `.desktop` file, the app icon, the license and the third-party notices, whose app uses the system's exiftool, ffmpeg and libmpv
- **And** the release workflow attaches it beside the Windows downloads

## Out of scope

- macOS (a later spec).
- AppImage, Flatpak, `.deb` and distribution repositories; bundling the helpers on Linux.
- Reading the desktop's button layout (`org.gnome.desktop.wm.preferences button-layout`): the buttons are always minimize, maximize, close at the right.
- Window shadows and rounded corners of the undecorated window on Wayland compositors that don't add them.
- Reduced transparency on Linux: no desktop has a common setting for it, so glass stays on (REQ-013-06 applies only where the OS has one).
- Opening files with Good Gallery from the file manager.

## Assumptions

- Desktops in scope: GNOME and KDE Plasma on Wayland and X11. Other desktops get the same window, untested.
- The system packages are a Linux distribution's (tested on Ubuntu 26.04): `libimage-exiftool-perl`, `ffmpeg`, `libmpv2`, a Vulkan driver.
- Display-synced video assumes 60 Hz on Linux (the app has no portable way to ask the compositor yet); other refresh rates play with occasional repeated or dropped frames.

## Verification

| REQ        | Test | Result |
| ---------- | ---- | ------ |
| REQ-018-01 | `xtask/tests/workflows.rs` › req_018_01_req_018_09_ci_and_the_release_cover_linux (fmt, clippy, check-specs and the whole suite run on Ubuntu 26.04 with lavapipe; the Linux CI job itself runs on GitHub) | pass |
| REQ-018-02 | `crates/app/src/startup.rs` › req_018_02_the_data_folder_lies_in_the_xdg_data_home | pass |
| REQ-018-03 | `crates/media/src/helpers.rs` › req_018_03_finds_the_systems_helpers; `crates/app/tests/video.rs` (plays through the system's libmpv 0.41) | pass |
| REQ-018-04 | `crates/app/src/titlebar.rs` › req_018_04_round_caption_buttons_control_the_window, req_018_04_edges_and_corners_resize_the_window; `*_linux.png` snapshots | partial (moving, resizing and the buttons on a real compositor checked by hand only) |
| REQ-018-05 | `crates/media/src/helpers.rs` › req_018_05_asks_the_file_manager_to_select_the_file | partial (the file manager opening checked by hand only) |
| REQ-018-06 | `crates/media/src/helpers.rs` › req_018_06_reads_the_desktops_animation_setting | pass |
| REQ-018-07 | `crates/media/src/helpers.rs` › exiftool_ends_with_an_app_that_exits_without_cleaning_up | pass |
| REQ-018-08 | `crates/media/src/helpers.rs` › req_018_08_lowers_the_calling_threads_priority_only | pass |
| REQ-018-09 | `xtask/src/dist.rs` › req_018_09_stages_the_linux_package; `xtask/tests/workflows.rs` › req_018_01_req_018_09_ci_and_the_release_cover_linux | pass |
