//! The browser version (spec 019). One wasm instance on the page's main thread runs the same UI as the desktop app;
//! what the desktop app does with threads, paths and helper programs happens here with the browser's services:
//!
//! - `index`: the index is an in-memory SQLite database (shared by name between connections), kept as a snapshot in
//!   the page's storage (Origin Private File System).
//! - `folders`: picked folders (a directory handle, or the files of a folder upload) and the files in them.
//! - `scan`: scans as async tasks that yield to the UI; metadata through `meta`'s readers.
//! - `thumbs`: thumbnails decoded and encoded by the browser, stored in the page's storage.
//! - `video`: playback through a `<video>` element whose frames are copied into a texture.
//!
//! Work that would block (decoding, reading files) is the browser's and runs off the main thread; our own CPU work is
//! sliced so a frame is never held up for long (REQ-019-03).

pub mod folders;
pub mod index;
pub mod js;
pub mod scan;
pub mod start;
pub mod thumbs;
pub mod video;

use std::future::Future;
use wasm_bindgen::JsCast as _;

/// Runs `task` on the page's event loop.
pub fn spawn(task: impl Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(task);
}

/// Waits `ms` milliseconds; 0 yields to the browser, so it can draw a frame and handle input.
pub async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let window = web_sys::window().expect("the page has a window");
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    js_sys::Date::now() as i64
}

/// Cooperative time slicing: long loops call [`Slice::yield_if_due`] between items, which gives the browser a frame
/// whenever the loop has run for a few milliseconds (REQ-019-03).
pub struct Slice {
    started: f64,
}

/// The longest our own work runs before the browser gets the main thread back.
const SLICE_MS: f64 = 8.0;

impl Slice {
    pub fn new() -> Self {
        Self {
            started: performance_now(),
        }
    }

    pub async fn yield_if_due(&mut self) {
        if performance_now() - self.started >= SLICE_MS {
            sleep(0).await;
            self.started = performance_now();
        }
    }
}

impl Default for Slice {
    fn default() -> Self {
        Self::new()
    }
}

fn performance_now() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map_or_else(js_sys::Date::now, |performance| performance.now())
}

/// A property of a JavaScript object.
pub fn get(value: &wasm_bindgen::JsValue, key: &str) -> wasm_bindgen::JsValue {
    js_sys::Reflect::get(value, &key.into()).unwrap_or(wasm_bindgen::JsValue::UNDEFINED)
}

/// The `index`-th item of a JavaScript array.
pub fn at(value: &wasm_bindgen::JsValue, index: u32) -> wasm_bindgen::JsValue {
    value
        .dyn_ref::<js_sys::Array>()
        .map_or(wasm_bindgen::JsValue::UNDEFINED, |array| array.get(index))
}

/// Log lines go to the browser's console.
pub fn init_logging() {
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;
    let layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .without_time()
        .with_writer(|| Console(Vec::new()));
    let filter = tracing_subscriber::filter::LevelFilter::INFO;
    let _ = tracing_subscriber::registry().with(filter).with(layer).try_init();
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&format!("panic: {info}").into());
    }));
}

/// Collects one log line and prints it when dropped.
struct Console(Vec<u8>);

impl std::io::Write for Console {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        let line = String::from_utf8_lossy(&self.0);
        let line = line.trim_end();
        if !line.is_empty() {
            web_sys::console::log_1(&line.into());
        }
    }
}
