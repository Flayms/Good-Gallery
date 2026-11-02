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
- **And** minimize, maximize and close sit at its right as round icon buttons, as GNOME's header bars draw them (or as the desktop's layout names them, REQ-018-10)
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

### REQ-018-12 · Packages for the package manager

- **Given** `cargo xtask dist` on Linux
- **When** it runs
- **Then** it also builds a `.deb` and an `.rpm` that install the app, its menu entry for the file types of REQ-027-05, its icon and the licenses, and that depend on the distribution's exiftool, ffmpeg and libmpv
- **And** the release workflow attaches them beside the `.tar.gz`, after installing each into a clean Ubuntu 24.04, Ubuntu 26.04 and Fedora system, starting the app and removing it again
- **And** removing a package keeps the index, settings and thumbnail cache

### REQ-018-10 · Caption buttons in the desktop's layout

- **Given** a desktop whose button layout names the window controls' sides and order (GNOME's `org.gnome.desktop.wm.preferences button-layout`, KDE's _Titlebar Buttons_)
- **When** the window is drawn
- **Then** the caption buttons appear on the side, in the order and with the buttons the layout names, and buttons it doesn't name are left out
- **And** the title bar keeps their place free on that side, and follows a changed layout while the app runs
- **And** on other desktops, or when the layout can't be read, they stay minimize, maximize, close at the right

### REQ-018-11 · Video in step with the display

- **Given** a display that doesn't run at 60 Hz (e.g. 75, 120 or 144 Hz)
- **When** a video plays on Linux
- **Then** its frames are timed to the rate the window's frames are presented at, measured while it plays, so it plays without regularly repeated or dropped frames
- **And** a dropped frame or a pause doesn't change that rate, and a window moved to a display with another rate follows it

## Out of scope

- macOS (a later spec).
- AppImage, Flatpak and distribution repositories, so packages get no updates through the package manager; bundling the helpers on Linux.
- Buttons of the desktop's layout the app doesn't have (the app menu, the window icon, _On all desktops_, spacers).
- Window shadows and rounded corners of the undecorated window on Wayland compositors that don't add them.
- Reduced transparency on Linux: no desktop has a common setting for it, so glass stays on (REQ-013-06 applies only where the OS has one).
- Opening files with Good Gallery from the file manager.

## Assumptions

- Desktops in scope: GNOME and KDE Plasma on Wayland and X11. Other desktops get the same window, untested.
- The system packages are a Linux distribution's (tested on Ubuntu 26.04): `libimage-exiftool-perl`, `ffmpeg`, `libmpv2`, a Vulkan driver.
- Display-synced video starts timed to 60 Hz on Linux (neither X11 and Wayland nor eframe tell the window's refresh rate in a common way) and follows the measured rate within about half a second of playing (REQ-018-11). Where the app can't keep up with the display, the measured rate is the one it presents at, which is the one to time video to.

- Packages are built on Ubuntu 24.04, whose glibc is the oldest they run on. Video playback is tested with Ubuntu 26.04's libmpv 0.41; on 24.04 the package test only starts the app.
- Fedora's own `ffmpeg-free` satisfies the package's dependency but lacks some video codecs; the full ffmpeg comes from RPM Fusion.

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
| REQ-018-12 | `xtask/src/dist.rs` › req_018_12_packages_are_named_like_the_archive, req_018_12_the_packages_hold_the_app_and_depend_on_the_helpers; `xtask/tests/workflows.rs` › req_018_12_the_release_tests_and_attaches_the_packages; `packaging/linux/smoke-test.sh` (`linux-packages.yml`) | pass |
| REQ-018-10 | `crates/domain/src/caption.rs` › req_018_10_reads_gnomes_button_layout, req_018_10_reads_kdes_button_layout, req_018_10_a_button_named_twice_is_shown_once, req_018_10_an_unreadable_gnome_value_is_none; `crates/app/src/titlebar.rs` › req_018_10_places_the_buttons_as_the_layout_names_them, req_018_10_round_caption_buttons_follow_the_layout; `crates/app/tests/gallery.rs` › req_018_10_caption_buttons_at_the_left (`caption_left*_linux.png`) | partial (reading the setting on a real GNOME and KDE desktop checked by hand only) |
| REQ-018-11 | `crates/domain/src/video.rs` › req_018_11_measures_the_rate_frames_are_presented_at, req_018_11_dropped_frames_and_pauses_dont_move_the_rate, req_018_11_follows_a_new_rate; `crates/app/src/video/mpv.rs` › req_018_11_the_display_fps_changes_while_mpv_runs | partial (smooth playback on a 120/144 Hz display checked by hand only) |
