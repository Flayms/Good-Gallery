//! Bindings to `glue.js`, the browser services the web build uses. Promises come back as `JsValue`s; the wrappers in
//! the other `web` modules turn them into Rust types.

use js_sys::{Array, Promise, Uint8Array};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/web/glue.js")]
extern "C" {
    #[wasm_bindgen(js_name = opfsFile)]
    pub fn opfs_file(path: &str) -> Promise;
    #[wasm_bindgen(js_name = opfsRead)]
    pub fn opfs_read(path: &str) -> Promise;
    #[wasm_bindgen(js_name = opfsWrite)]
    pub fn opfs_write(path: &str, data: &JsValue) -> Promise;
    #[wasm_bindgen(js_name = opfsRemove)]
    pub fn opfs_remove(path: &str) -> Promise;
    #[wasm_bindgen(js_name = opfsList)]
    pub fn opfs_list(path: &str) -> Promise;
    #[wasm_bindgen(js_name = persistStorage)]
    pub fn persist_storage() -> Promise;

    #[wasm_bindgen(js_name = canKeepFolders)]
    pub fn can_keep_folders() -> bool;
    #[wasm_bindgen(js_name = pickDirectory)]
    pub fn pick_directory() -> Promise;
    #[wasm_bindgen(js_name = pickFiles)]
    pub fn pick_files() -> Promise;
    #[wasm_bindgen(js_name = listDirectory)]
    pub fn list_directory(dir: &JsValue) -> Promise;
    #[wasm_bindgen(js_name = fileAt)]
    pub fn file_at(dir: &JsValue, path: &str) -> Promise;
    #[wasm_bindgen(js_name = folderPermission)]
    pub fn folder_permission(dir: &JsValue) -> Promise;
    #[wasm_bindgen(js_name = requestFolderPermission)]
    pub fn request_folder_permission(dir: &JsValue) -> Promise;
    #[wasm_bindgen(js_name = saveFolder)]
    pub fn save_folder(key: &str, dir: &JsValue) -> Promise;
    #[wasm_bindgen(js_name = loadFolder)]
    pub fn load_folder(key: &str) -> Promise;
    #[wasm_bindgen(js_name = deleteFolder)]
    pub fn delete_folder(key: &str) -> Promise;

    #[wasm_bindgen(js_name = decodeImage)]
    pub fn decode_image(blob: &JsValue) -> Promise;
    #[wasm_bindgen(js_name = rgbaOf, catch)]
    pub fn rgba_of(source: &JsValue, width: u32, height: u32) -> Result<Uint8Array, JsValue>;
    #[wasm_bindgen(js_name = encodeImage)]
    pub fn encode_image(bitmap: &JsValue, width: u32, height: u32, quality: f64) -> Promise;
    #[wasm_bindgen(js_name = storedRgba)]
    pub fn stored_rgba(path: &str) -> Promise;
    #[wasm_bindgen(js_name = videoFrame)]
    pub fn video_frame(file: &JsValue, fraction: f64) -> Promise;

    pub type Player;
    #[wasm_bindgen(constructor)]
    pub fn new() -> Player;
    #[wasm_bindgen(method, getter)]
    pub fn video(this: &Player) -> web_sys::HtmlVideoElement;
    #[wasm_bindgen(method)]
    pub fn load(this: &Player, file: &JsValue);
    #[wasm_bindgen(method)]
    pub fn fail(this: &Player, message: &str);
    #[wasm_bindgen(method)]
    pub fn stop(this: &Player);
    #[wasm_bindgen(method, js_name = setPaused)]
    pub fn set_paused(this: &Player, paused: bool);
    #[wasm_bindgen(method)]
    pub fn seek(this: &Player, position: f64, exact: bool);
    #[wasm_bindgen(method, js_name = setVolume)]
    pub fn set_volume(this: &Player, volume: f64);
    #[wasm_bindgen(method, js_name = setMuted)]
    pub fn set_muted(this: &Player, muted: bool);
    #[wasm_bindgen(method)]
    pub fn state(this: &Player) -> Array;

    #[wasm_bindgen(js_name = holdTabLock)]
    pub fn hold_tab_lock() -> Promise;
    #[wasm_bindgen(js_name = prefersReducedMotion)]
    pub fn prefers_reduced_motion() -> bool;
    #[wasm_bindgen(js_name = prefersReducedTransparency)]
    pub fn prefers_reduced_transparency() -> bool;
    #[wasm_bindgen(js_name = onPageHidden)]
    pub fn on_page_hidden(save: &Closure<dyn FnMut()>);
}

/// Waits for `promise`; a rejection becomes its message.
pub async fn wait(promise: Promise) -> Result<JsValue, String> {
    wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(|error| message(&error))
}

/// The message of a thrown JavaScript value.
pub fn message(error: &JsValue) -> String {
    if let Some(error) = error.dyn_ref::<js_sys::Error>() {
        return String::from(error.message());
    }
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}

/// Bytes from a `Uint8Array`, or `None` for `null`.
pub fn bytes(value: &JsValue) -> Option<Vec<u8>> {
    value.dyn_ref::<Uint8Array>().map(Uint8Array::to_vec)
}
