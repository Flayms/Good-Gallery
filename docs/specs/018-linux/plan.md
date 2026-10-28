# 018 · Linux · Plan

## Approach

The code already keeps most Windows calls behind `cfg(windows)` with stubs elsewhere; this spec fills the stubs for Linux (`target_os = "linux"`, not `unix`, so macOS stays a stub until its own spec) and makes the tooling (CI, `xtask dist`, `xtask screenshots`) run there.

- **Platform module.** `media::helpers` keeps one function per platform service (data home, show in folder, reduced motion, thread priority, helpers end with the app). The Linux bodies are small and need no new crates except `libc` (already in the tree), behind `cfg(target_os = "linux")`.
- **Data folder** (REQ-018-02): `app::startup::default_data_dir` takes the platform's base folder; on Linux `$XDG_DATA_HOME` if absolute, else `$HOME/.local/share` (XDG Base Directory spec). Pure function, tested with both cases.
- **Helpers** (REQ-018-03): `EXIFTOOL_BUNDLED` uses `exiftool` without `.exe` on Linux (the Perl script from the distribution). libmpv isn't looked up on `PATH` on Linux: `find_mpv` returns the bare name `libmpv.so.2` and `libloading` lets the dynamic loader search (`ld.so` paths), unless `GOOD_GALLERY_MPV` or the bundled folder names a file.
- **Helpers end with the app** (REQ-018-07): helper `Command`s get `PR_SET_PDEATHSIG = SIGKILL` in `pre_exec` (Linux's equivalent of the kill-on-close job). One small `unsafe` block in `media::helpers`, used by `exiftool` and `ffmpeg`. Caveat: the signal fires when the *thread* that spawned the child ends; exiftool processes are spawned from long-lived pool threads, ffmpeg runs to completion on its caller's thread, so this holds. A test starts a child from a thread, kills the parent process (a test helper process) and checks the child is gone.
- **Thread priority** (REQ-018-08): `setpriority(PRIO_PROCESS, gettid(), 10)` (Linux applies nice per thread).
- **Show in folder** (REQ-018-05): D-Bus `org.freedesktop.FileManager1.ShowItems` through `dbus-send` / `gdbus` (both on every GNOME / KDE system), falling back to `xdg-open <folder>`. A process, not a D-Bus crate: zbus pulls an async runtime that `no_network.rs` forbids. The command lines are pure functions and tested; opening a file manager is checked by hand.
- **Reduced motion** (REQ-018-06): `gsettings get org.gnome.desktop.interface enable-animations` (KDE: `kreadconfig6 --group KDE --key AnimationDurationFactor`, `0` means off). Spawning a process every 2 s on the UI thread is too slow, so `OsPreferences` reads the setting on a background thread and polls an atomic. Parsing is pure and tested.
- **Title bar** (REQ-018-04): the window is undecorated on Linux (`with_decorations(false)`): winit on Wayland would otherwise draw its own Adwaita-style frame above the toolbar, and X11 window managers their caption. `app::titlebar` draws three round caption buttons (GNOME's look: 24 px circles on hover/press, 16 px icons, 6 px apart, right of the toolbar's content) that send `ViewportCommand::Minimized / Maximized / Close`; the existing `drag_area` already moves and maximizes. Resizing: a margin of 6 px (0 when maximized) at the window's edges and corners shows the resize cursor and sends `ViewportCommand::BeginResize(direction)`, which winit hands to the compositor on both X11 and Wayland. The hit-testing of edges and the button rects are pure functions with tests; the snapshot tests get a Linux variant of the toolbar row. `reserved_width()` becomes the Linux buttons' width there.
- **Network check.** `notify`'s inotify backend uses `mio` for its event loop, which `no_network.rs` lists. mio isn't a network crate there (no sockets are opened), so the test allows `mio` when only `notify` brings it in (`cargo tree -i mio` shows no other path), keeping the guard for everything else.
- **Refresh rate.** Linux keeps the 60 Hz default (spec assumption); detection is #40.
- **Snapshots.** Snapshots are rendered by wgpu on WARP (Windows) and lavapipe (Linux, Mesa's software Vulkan). If they differ by more than kittest's threshold, platform-specific snapshot files get a `.linux` suffix; first try to keep one set.
- **CI** (REQ-018-01): a `linux` matrix entry (`ubuntu-latest`) beside Windows for lint and test: apt installs `libimage-exiftool-perl ffmpeg libmpv2 mesa-vulkan-drivers` and git-cliff; `GG_REQUIRE_HELPERS=1` so helper tests don't skip silently.
- **Package** (REQ-018-09): `cargo xtask dist` on Linux builds `app` (`--features packaged`) and writes `target/dist/good-gallery-<version>-linux-x86_64.tar.gz` with `good-gallery` (the binary), `good-gallery.desktop`, `good-gallery.png` (the 256 px window icon), `LICENSE.txt`, `THIRD-PARTY-NOTICES.txt` and a short `README` naming the apt packages. Uses the `tar` on `PATH`. The release workflow adds a Linux job that runs it and attaches the archive. `crates/app/build.rs` already only embeds Windows resources on Windows (checked).
- **Screenshots** (`xtask screenshots`): the demo library lives at a fixed short path (`/tmp/Photos` via a symlink) instead of a `subst` drive, and the app binary has no `.exe`.

### Rejected

- **winit's client-side decorations** (sctk-adwaita on Wayland): a second bar above the toolbar, so the search isn't in the title bar.
- **A D-Bus crate** (zbus) for _Show in folder_ and settings: its async runtime trips REQ-011-11's dependency guard and adds a lot of code for two calls.
- **Matching `cfg(unix)`**: macOS needs different answers (Finder, `~/Library/Application Support`), so Linux-only code stays `target_os = "linux"`.

## Tasks

- [ ] 1. Toolchain and system packages on the dev machine; first `cargo build`, `cargo test` on Linux; list what fails (REQ-018-01)
- [ ] 2. `no_network.rs`: allow `mio` only via `notify` (REQ-011-11, REQ-018-01)
- [ ] 3. Data folder in the XDG data home (REQ-018-02)
- [ ] 4. Helper lookup: exiftool name, libmpv through the dynamic loader (REQ-018-03)
- [ ] 5. `PR_SET_PDEATHSIG` for helper processes (REQ-018-07)
- [ ] 6. Lower thread priority (REQ-018-08)
- [ ] 7. Show in folder via FileManager1, `xdg-open` fallback (REQ-018-05)
- [ ] 8. Reduced motion from GNOME / KDE on a background thread (REQ-018-06)
- [ ] 9. Undecorated window, round caption buttons, drag, edge and corner resize (REQ-018-04)
- [ ] 10. Fix remaining test failures from task 1 (Windows paths in tests, snapshots on lavapipe) (REQ-018-01)
- [ ] 11. `xtask screenshots` and `xtask dist` on Linux; `.desktop` file (REQ-018-09)
- [ ] 12. CI and release workflows: Linux jobs; `workflows.rs` checks them (REQ-018-01, REQ-018-09)
- [ ] 13. Run the app on Xvfb (X11) and a headless Wayland compositor if available; screenshots; README (Linux section, system packages), architecture, issues, pitfalls
