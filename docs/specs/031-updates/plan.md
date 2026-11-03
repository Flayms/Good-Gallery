# 031 · Updates · Plan

## Approach

- `domain::update`: `Version`, the release's URLs and file names, reading GitHub's answer, what a signature covers. Pure.
- `media::update`: runs `curl`, verifies with `ed25519-compact`, starts the installer outside the job that ends helpers with the app.
- `app::updates`: the `Updater` trait (`GitHub`, or a fake in tests), when to check, the toolbar's notice and menu. `AppState` runs the jobs. `desktop.rs` sets an updater only for packaged, non-automated builds.
- `packaging/windows/installer.nsi`: `/UPDATE` waits for the app and starts the new version. `packaging/smoke-test.ps1` runs it against a running app.
- `.github/workflows/windows-packages.yml` builds and smoke-tests the installer for every release and for pull requests that touch it, as `linux-packages.yml` does for Linux. `packaging/sign-update.sh` signs in the release's `publish` job.

## Tasks

- [x] Version, URLs, GitHub's answer (REQ-031-03, -04, -10)
- [x] `curl`, verifying, starting the installer (REQ-031-06, -07, -10)
- [x] When to check, notice, button, settings (REQ-031-01, -02, -04, -05, -07)
- [x] Installer's `/UPDATE`, smoke test (REQ-031-08)
- [x] Signing in the release workflow (REQ-031-09)
- [ ] Create the key: `packaging/new-update-key.sh` (REQ-031-09)
- [ ] On a real Windows machine: update from one release to the next, and see whether SmartScreen asks

## Decisions

### 2026-11-03 · The system's `curl`, a signature of our own

**Decided:** the system's `curl` makes the connection, by its full path on Windows. The app stays free of network crates and of a TLS stack it would have to keep patched, and `no_network.rs` keeps guarding that. A failed check is quiet, so a Linux system without `curl` loses only the notice. Windows gets no Authenticode signature (#29), so an Ed25519 signature made with a key from `packaging/new-update-key.sh` vouches for the installer. It covers the file's name as well as its contents, so an older signed installer can't pass for a newer version. URLs are built from a version that can only be digits and dots, so GitHub's answer can't point a download elsewhere.
**Rejected:** `ureq` with `rustls` (a network stack in the app, for two requests), a checksum in the same release (only catches damage), `minisign` (another format, for the same Ed25519 signature), downloading in the background before the user asks (the user decides, and a 100 MB download isn't quiet), checking only once a day across restarts (one request per start is simpler, and needs nothing stored).

### 2026-11-03 · On by default

**Decided:** _Check for updates_ is on by default (Anton's decision on #158), so the app can show that an update exists; updating itself always takes a press of the button. GitHub sees the user's IP address.
**Rejected:** off by default, or asking at first start (a notice nobody enabled reaches nobody).
