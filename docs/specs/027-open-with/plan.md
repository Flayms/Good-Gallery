# 027 · Open with · Plan

## Approach

- `desktop.rs` takes a file as its one positional argument (made absolute) and asks `app::instance` whether another app runs on the data folder before it opens a window.
- `AppState::open_file` checks the file, finds its library with `domain::media::rel_path_in` (the longest root that contains it), switches the gallery to that library and keeps a `PendingOpen` until the gallery lists the file; `show_pending_open` opens the viewer on it. A file the index lacks gets a changes scan (`ScanRequests::request_paths`); if the scan ends and the library has nothing queued, the app says it wasn't indexed.
- A file outside every library asks with `widgets::dialog`; _Add folder_ goes through `add_library`, and the file is opened once the library is in `AppState::libraries`.
- Packages: the NSIS installer writes a ProgID, `Applications\good-gallery.exe` with `SupportedTypes`, and `OpenWithProgids` per extension under `HKCU\Software\Classes`; the `.desktop` file gets `MimeType=` and `%f`. `xtask/tests/packaging.rs` keeps both in step with `domain::media::IMAGE_EXTENSIONS` / `VIDEO_EXTENSIONS`.

## Tasks

- [x] Spec (REQ-027-01 … REQ-027-05)
- [x] File argument and `rel_path_in` (REQ-027-01)
- [x] One app per data folder (REQ-027-02)
- [x] Open, scan first, give up (REQ-027-01, REQ-027-04)
- [x] Offer to add the folder (REQ-027-03)
- [x] Installer and `.desktop` file (REQ-027-05)

## Decisions

### 2026-11-02 · One app per data folder through files

**Decided:** the running app holds an OS file lock (`File::try_lock`) on `<data>/instance.lock`. A later start that finds it locked writes its request into `<data>/open/` (a `.partial` file renamed to `.request`, so it is never read half-written) and exits; the running app's `open requests` thread looks into that folder every 250 ms and wakes the UI. On Windows the later start calls `AllowSetForegroundWindow(ASFW_ANY)` first, since only the foreground process may bring a window to the front. Screenshot and benchmark runs skip all of it. The lock ends with the process, however it ends, so there is no stale state.
**Rejected:** a named pipe or a Unix socket (IPC code per platform, and sockets are what REQ-011-11's guard looks for), D-Bus activation (Linux only, and a D-Bus crate brings an async runtime), `notify` on the folder (another watcher for something checked four times a second at no cost), showing files outside every library without an index (the viewer is built on gallery items: their metadata, originals and stepping all come from the index).
