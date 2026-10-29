//! Good Gallery.
//!
//! Options: `--data-dir <folder>` (index, settings, thumbnails and logs; default `%APPDATA%\good-gallery` in the
//! packaged app, `%APPDATA%\good-gallery-rust` otherwise; on Linux the same names in `~/.local/share`),
//! `--screenshot <file.png>` (saves a screenshot of the window
//! after a few frames and exits), `--screenshot-after <seconds>` (waits that long before the screenshot, e.g. for scans
//! or thumbnails), `--bench-scroll <seconds>` (vsync off; scrolls the gallery once to warm the files, then again while
//! measuring frame times, prints them and exits; REQ-006-12), `--view <position>` (opens the viewer at that gallery
//! position once the gallery has loaded, e.g. for a screenshot; with `--info` its info panel too), `--scroll <points>`
//! (scrolls the gallery that far once it has loaded, e.g. to show the glass toolbar over tiles).
//!
//! In the browser (spec 019) the page starts it instead; see `app::web`.

// The packaged app (`cargo xtask dist` builds it with the `packaged` feature) opens no console window; the log goes to
// `logs\app.log` in the data folder.
#![cfg_attr(feature = "packaged", windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
mod desktop;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> std::process::ExitCode {
    desktop::main()
}

#[cfg(target_arch = "wasm32")]
fn main() {
    wasm_bindgen_futures::spawn_local(app::web::start::start());
}
