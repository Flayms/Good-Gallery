//! A small binding of libmpv, loaded at runtime from `libmpv-2.dll` (M0b): the client API for commands and properties,
//! and the software render API (`"sw"`), which the `libmpv2` crate doesn't wrap. All of the app's `unsafe` for video
//! lives here.

#![allow(unsafe_code, reason = "FFI to libmpv")]

use libloading::Library;
use std::ffi::{CStr, CString, c_char, c_double, c_int, c_void};
use std::ptr;

#[repr(C)]
struct Handle {
    _p: [u8; 0],
}

#[repr(C)]
struct RenderContext {
    _p: [u8; 0],
}

#[repr(C)]
struct RenderParam {
    kind: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct RawEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct RawEndFile {
    reason: c_int,
    error: c_int,
}

const FORMAT_FLAG: c_int = 3;
const FORMAT_INT64: c_int = 4;
const FORMAT_DOUBLE: c_int = 5;

const EVENT_NONE: c_int = 0;
const EVENT_SHUTDOWN: c_int = 1;
const EVENT_START_FILE: c_int = 6;
const EVENT_END_FILE: c_int = 7;
const EVENT_FILE_LOADED: c_int = 8;
const END_FILE_REASON_ERROR: c_int = 4;

const RENDER_PARAM_INVALID: c_int = 0;
const RENDER_PARAM_API_TYPE: c_int = 1;
const RENDER_PARAM_NEXT_FRAME_INFO: c_int = 11;
const RENDER_PARAM_BLOCK_FOR_TARGET_TIME: c_int = 12;
const RENDER_PARAM_SKIP_RENDERING: c_int = 13;
const RENDER_PARAM_SW_SIZE: c_int = 17;
const RENDER_PARAM_SW_FORMAT: c_int = 18;
const RENDER_PARAM_SW_STRIDE: c_int = 19;
const RENDER_PARAM_SW_POINTER: c_int = 20;
const RENDER_UPDATE_FRAME: u64 = 1;
const FRAME_INFO_REPEAT: u64 = 1 << 2;

type UpdateCallback = unsafe extern "C" fn(*mut c_void);

/// Called on mpv's threads when a new frame may be due. It must not call into mpv.
pub type OnUpdate = Box<dyn Fn() + Send + Sync>;

unsafe extern "C" fn on_update(data: *mut c_void) {
    // SAFETY: `data` is the `OnUpdate` boxed in the `SwRender`, which outlives its render context.
    let on_update = unsafe { &*data.cast::<OnUpdate>() };
    on_update();
}

/// What the event thread cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Timed out or woken up.
    None,
    Shutdown,
    /// A `loadfile` started.
    StartFile,
    FileLoaded,
    /// The file ended; with mpv's reason when it failed.
    EndFile {
        error: Option<String>,
    },
    Other,
}

struct Api {
    _lib: Library,
    create: unsafe extern "C" fn() -> *mut Handle,
    initialize: unsafe extern "C" fn(*mut Handle) -> c_int,
    terminate_destroy: unsafe extern "C" fn(*mut Handle),
    set_option_string: unsafe extern "C" fn(*mut Handle, *const c_char, *const c_char) -> c_int,
    command: unsafe extern "C" fn(*mut Handle, *const *const c_char) -> c_int,
    set_property: unsafe extern "C" fn(*mut Handle, *const c_char, c_int, *mut c_void) -> c_int,
    get_property: unsafe extern "C" fn(*mut Handle, *const c_char, c_int, *mut c_void) -> c_int,
    error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    wait_event: unsafe extern "C" fn(*mut Handle, c_double) -> *mut RawEvent,
    wakeup: unsafe extern "C" fn(*mut Handle),
    render_create: unsafe extern "C" fn(*mut *mut RenderContext, *mut Handle, *mut RenderParam) -> c_int,
    render_set_update_callback: unsafe extern "C" fn(*mut RenderContext, Option<UpdateCallback>, *mut c_void),
    render_update: unsafe extern "C" fn(*mut RenderContext) -> u64,
    render_render: unsafe extern "C" fn(*mut RenderContext, *mut RenderParam) -> c_int,
    render_free: unsafe extern "C" fn(*mut RenderContext),
    render_report_swap: unsafe extern "C" fn(*mut RenderContext),
    render_get_info: unsafe extern "C" fn(*mut RenderContext, RenderParam) -> c_int,
}

macro_rules! sym {
    ($lib:expr, $name:literal) => {
        // SAFETY: the type the symbol is read as matches its declaration in mpv's client.h / render.h.
        *unsafe { $lib.get(concat!($name, "\0").as_bytes()) }.map_err(|e| format!("{}: {e}", $name))?
    };
}

impl Api {
    fn load(path: &std::path::Path) -> Result<Self, String> {
        // SAFETY: loading libmpv runs its initializers, which have no preconditions.
        let lib = unsafe { Library::new(path) }.map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self {
            create: sym!(lib, "mpv_create"),
            initialize: sym!(lib, "mpv_initialize"),
            terminate_destroy: sym!(lib, "mpv_terminate_destroy"),
            set_option_string: sym!(lib, "mpv_set_option_string"),
            command: sym!(lib, "mpv_command"),
            set_property: sym!(lib, "mpv_set_property"),
            get_property: sym!(lib, "mpv_get_property"),
            error_string: sym!(lib, "mpv_error_string"),
            wait_event: sym!(lib, "mpv_wait_event"),
            wakeup: sym!(lib, "mpv_wakeup"),
            render_create: sym!(lib, "mpv_render_context_create"),
            render_set_update_callback: sym!(lib, "mpv_render_context_set_update_callback"),
            render_update: sym!(lib, "mpv_render_context_update"),
            render_render: sym!(lib, "mpv_render_context_render"),
            render_free: sym!(lib, "mpv_render_context_free"),
            render_report_swap: sym!(lib, "mpv_render_context_report_swap"),
            render_get_info: sym!(lib, "mpv_render_context_get_info"),
            _lib: lib,
        })
    }
}

/// A C string; names and values with a NUL byte become empty (mpv then rejects them).
fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

/// An mpv core. libmpv's client functions are thread-safe, so it is shared between the UI, the event and the render
/// thread; only one thread may wait for events.
pub struct Mpv {
    api: Api,
    handle: *mut Handle,
}

// SAFETY: client.h: "all functions are thread-safe" for one handle, except mpv_wait_event (one thread, see there).
unsafe impl Send for Mpv {}
// SAFETY: as above.
unsafe impl Sync for Mpv {}

impl Mpv {
    /// Loads libmpv from `dll` and starts a core with `options`.
    pub fn new(dll: &std::path::Path, options: &[(&str, &str)]) -> Result<Self, String> {
        let api = Api::load(dll)?;
        // SAFETY: no preconditions; null on failure.
        let handle = unsafe { (api.create)() };
        if handle.is_null() {
            return Err("mpv_create failed".into());
        }
        let mpv = Self { api, handle };
        for (name, value) in options {
            // SAFETY: valid handle, NUL-terminated strings that live for the call.
            let rc = unsafe { (mpv.api.set_option_string)(handle, cstr(name).as_ptr(), cstr(value).as_ptr()) };
            mpv.check(rc).map_err(|e| format!("option {name}={value}: {e}"))?;
        }
        // SAFETY: valid, not yet initialized handle.
        mpv.check(unsafe { (mpv.api.initialize)(handle) })?;
        Ok(mpv)
    }

    fn check(&self, rc: c_int) -> Result<(), String> {
        if rc >= 0 { Ok(()) } else { Err(self.error(rc)) }
    }

    /// mpv's text for an error code.
    pub fn error(&self, rc: c_int) -> String {
        // SAFETY: mpv_error_string returns a static string for any code.
        unsafe { CStr::from_ptr((self.api.error_string)(rc)) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn command(&self, args: &[&str]) -> Result<(), String> {
        let owned: Vec<CString> = args.iter().map(|arg| cstr(arg)).collect();
        let mut pointers: Vec<*const c_char> = owned.iter().map(|arg| arg.as_ptr()).collect();
        pointers.push(ptr::null());
        // SAFETY: a NULL-terminated array of strings that live for the call.
        self.check(unsafe { (self.api.command)(self.handle, pointers.as_ptr()) })
    }

    fn set(&self, name: &str, format: c_int, data: *mut c_void) {
        // SAFETY: `data` points to a value of `format` that lives for the call.
        let rc = unsafe { (self.api.set_property)(self.handle, cstr(name).as_ptr(), format, data) };
        if rc < 0 {
            tracing::debug!("mpv: can't set {name}: {}", self.error(rc));
        }
    }

    fn get<T: Default>(&self, name: &str, format: c_int) -> Option<T> {
        let mut value = T::default();
        // SAFETY: `value` has the size and layout of `format` (flag: c_int, int64: i64, double: f64).
        let rc = unsafe { (self.api.get_property)(self.handle, cstr(name).as_ptr(), format, (&raw mut value).cast()) };
        (rc >= 0).then_some(value)
    }

    pub fn set_flag(&self, name: &str, value: bool) {
        let mut value = c_int::from(value);
        self.set(name, FORMAT_FLAG, (&raw mut value).cast());
    }

    pub fn set_double(&self, name: &str, mut value: f64) {
        self.set(name, FORMAT_DOUBLE, (&raw mut value).cast());
    }

    pub fn double(&self, name: &str) -> Option<f64> {
        self.get::<f64>(name, FORMAT_DOUBLE)
    }

    pub fn int(&self, name: &str) -> Option<i64> {
        self.get::<i64>(name, FORMAT_INT64)
    }

    pub fn flag(&self, name: &str) -> Option<bool> {
        self.get::<c_int>(name, FORMAT_FLAG).map(|value| value != 0)
    }

    /// Waits up to `timeout` seconds for the next event. Only one thread may call this.
    pub fn wait_event(&self, timeout: f64) -> Event {
        // SAFETY: mpv_wait_event never returns null; the event stays valid until the next call on this thread.
        let event = unsafe { &*(self.api.wait_event)(self.handle, timeout) };
        match event.event_id {
            EVENT_NONE => Event::None,
            EVENT_SHUTDOWN => Event::Shutdown,
            EVENT_START_FILE => Event::StartFile,
            EVENT_FILE_LOADED => Event::FileLoaded,
            EVENT_END_FILE => {
                // SAFETY: END_FILE carries an mpv_event_end_file.
                let end = unsafe { &*event.data.cast::<RawEndFile>() };
                let error = (end.reason == END_FILE_REASON_ERROR).then(|| self.error(end.error));
                Event::EndFile { error }
            }
            _ => Event::Other,
        }
    }

    /// Ends a [`Self::wait_event`] early.
    pub fn wakeup(&self) {
        // SAFETY: valid handle.
        unsafe { (self.api.wakeup)(self.handle) };
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        // SAFETY: the handle is valid and not used afterwards; render contexts were freed first (they hold an `Arc`
        // of this core).
        unsafe { (self.api.terminate_destroy)(self.handle) };
    }
}

/// mpv's software render context. Must be dropped before the [`Mpv`] it was made from, on the thread that renders.
pub struct SwRender {
    ctx: *mut RenderContext,
    free: unsafe extern "C" fn(*mut RenderContext),
    update: unsafe extern "C" fn(*mut RenderContext) -> u64,
    render: unsafe extern "C" fn(*mut RenderContext, *mut RenderParam) -> c_int,
    report_swap: unsafe extern "C" fn(*mut RenderContext),
    get_info: unsafe extern "C" fn(*mut RenderContext, RenderParam) -> c_int,
    /// Boxed twice, so its address stays put; dropped after the context is freed.
    _on_update: Box<OnUpdate>,
}

/// 64 bytes, 64-byte aligned: the unit of a [`FrameBuffer`].
#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct Block([u8; 64]);

/// Memory for frames, 64-byte aligned as mpv wants it.
#[derive(Default)]
pub struct FrameBuffer {
    blocks: Vec<Block>,
}

impl FrameBuffer {
    /// The first `len` bytes, grown if needed.
    pub fn bytes_mut(&mut self, len: usize) -> &mut [u8] {
        if self.blocks.len() * 64 < len {
            self.blocks = vec![Block([0; 64]); len.div_ceil(64)];
        }
        // SAFETY: `Block` is 64 plain bytes without padding; the slice stays within the blocks' memory.
        unsafe { std::slice::from_raw_parts_mut(self.blocks.as_mut_ptr().cast::<u8>(), len) }
    }
}

impl SwRender {
    /// `on_update` runs on mpv's threads whenever a new frame may be due.
    pub fn new(mpv: &Mpv, on_update: OnUpdate) -> Result<Self, String> {
        let on_update = Box::new(on_update);
        let api_type = cstr("sw");
        let mut params = [
            RenderParam {
                kind: RENDER_PARAM_API_TYPE,
                data: api_type.as_ptr().cast_mut().cast(),
            },
            RenderParam {
                kind: RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];
        let mut ctx = ptr::null_mut();
        // SAFETY: valid handle; params end with INVALID and live for the call.
        mpv.check(unsafe { (mpv.api.render_create)(&raw mut ctx, mpv.handle, params.as_mut_ptr()) })?;
        let data: *const OnUpdate = &raw const *on_update;
        let callback: UpdateCallback = self::on_update;
        // SAFETY: `ctx` was just created; `data` lives in the box kept with the context.
        unsafe { (mpv.api.render_set_update_callback)(ctx, Some(callback), data.cast_mut().cast()) };
        Ok(Self {
            ctx,
            free: mpv.api.render_free,
            update: mpv.api.render_update,
            render: mpv.api.render_render,
            report_swap: mpv.api.render_report_swap,
            get_info: mpv.api.render_get_info,
            _on_update: on_update,
        })
    }

    /// Whether a new frame should be rendered.
    pub fn frame_due(&self) -> bool {
        // SAFETY: valid context, called on the render thread.
        let flags = unsafe { (self.update)(self.ctx) };
        flags & RENDER_UPDATE_FRAME != 0
    }

    /// Whether the frame the next render would draw repeats the one shown (display-synced timing).
    pub fn next_is_repeat(&self) -> bool {
        let mut info = [0u64; 2];
        let param = RenderParam {
            kind: RENDER_PARAM_NEXT_FRAME_INFO,
            data: info.as_mut_ptr().cast(),
        };
        // SAFETY: NEXT_FRAME_INFO fills an mpv_render_frame_info (u64 flags, i64 target time).
        let rc = unsafe { (self.get_info)(self.ctx, param) };
        rc >= 0 && info[0] & FRAME_INFO_REPEAT != 0
    }

    /// Tells mpv a frame was presented (display-synced timing).
    pub fn report_swap(&self) {
        // SAFETY: valid context.
        unsafe { (self.report_swap)(self.ctx) }
    }

    /// Renders the current frame as `rgb0` (R, G, B, undefined) into `pixels`: `size` pixels, `stride` bytes per row (a
    /// multiple of 64), from a [`FrameBuffer`]. With `block`, waits until the frame's display time; with `skip`, only
    /// advances.
    pub fn render(
        &self,
        size: [u32; 2],
        stride: usize,
        pixels: &mut [u8],
        block: bool,
        skip: bool,
    ) -> Result<(), c_int> {
        assert!(
            stride.is_multiple_of(64)
                && stride >= size[0] as usize * 4
                && pixels.len() >= stride * size[1] as usize
                && pixels.as_ptr().addr().is_multiple_of(64),
            "frame buffer too small or not aligned"
        );
        let mut skip = c_int::from(skip);
        let mut block = c_int::from(block);
        let mut size = [size[0] as c_int, size[1] as c_int];
        let format = cstr("rgb0");
        let mut stride = stride;
        let mut params = [
            RenderParam {
                kind: RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
                data: (&raw mut block).cast(),
            },
            RenderParam {
                kind: RENDER_PARAM_SKIP_RENDERING,
                data: (&raw mut skip).cast(),
            },
            RenderParam {
                kind: RENDER_PARAM_SW_SIZE,
                data: size.as_mut_ptr().cast(),
            },
            RenderParam {
                kind: RENDER_PARAM_SW_FORMAT,
                data: format.as_ptr().cast_mut().cast(),
            },
            RenderParam {
                kind: RENDER_PARAM_SW_STRIDE,
                data: (&raw mut stride).cast(),
            },
            RenderParam {
                kind: RENDER_PARAM_SW_POINTER,
                data: pixels.as_mut_ptr().cast(),
            },
            RenderParam {
                kind: RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];
        // SAFETY: `pixels` holds `stride * height` bytes at an aligned address (checked above); params end with
        // INVALID and live for the call.
        let rc = unsafe { (self.render)(self.ctx, params.as_mut_ptr()) };
        if rc < 0 { Err(rc) } else { Ok(()) }
    }
}

impl Drop for SwRender {
    fn drop(&mut self) {
        // SAFETY: valid context, not used afterwards.
        unsafe { (self.free)(self.ctx) };
    }
}
