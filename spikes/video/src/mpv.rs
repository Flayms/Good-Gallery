//! Minimal libmpv binding, loaded at runtime from `libmpv-2.dll`.
//!
//! `libmpv2` (the crate) only wraps the OpenGL render API; the software renderer (`"sw"`) needs the raw
//! `mpv_render_param`s, so this declares the few functions the spike uses.

use libloading::Library;
use std::ffi::{CStr, CString, c_char, c_double, c_int, c_void};
use std::ptr;

#[repr(C)]
pub struct Handle {
    _p: [u8; 0],
}
#[repr(C)]
pub struct RenderContext {
    _p: [u8; 0],
}

#[repr(C)]
pub struct RenderParam {
    pub kind: c_int,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct Event {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct EventEndFile {
    pub reason: c_int,
    pub error: c_int,
}

pub const FORMAT_FLAG: c_int = 3;
pub const FORMAT_INT64: c_int = 4;
pub const FORMAT_DOUBLE: c_int = 5;

pub const EVENT_NONE: c_int = 0;
pub const EVENT_SHUTDOWN: c_int = 1;
pub const EVENT_END_FILE: c_int = 7;
pub const EVENT_FILE_LOADED: c_int = 8;
pub const EVENT_PLAYBACK_RESTART: c_int = 21;

const RENDER_PARAM_INVALID: c_int = 0;
const RENDER_PARAM_API_TYPE: c_int = 1;
const RENDER_PARAM_NEXT_FRAME_INFO: c_int = 11;
const RENDER_PARAM_BLOCK_FOR_TARGET_TIME: c_int = 12;
const RENDER_PARAM_SKIP_RENDERING: c_int = 13;
const RENDER_PARAM_SW_SIZE: c_int = 17;
const RENDER_PARAM_SW_FORMAT: c_int = 18;
const RENDER_PARAM_SW_STRIDE: c_int = 19;
const RENDER_PARAM_SW_POINTER: c_int = 20;
pub const RENDER_UPDATE_FRAME: u64 = 1;
pub const FRAME_INFO_REPEAT: u64 = 1 << 2;

type UpdateCb = unsafe extern "C" fn(*mut c_void);

pub struct Api {
    _lib: Library,
    create: unsafe extern "C" fn() -> *mut Handle,
    initialize: unsafe extern "C" fn(*mut Handle) -> c_int,
    terminate_destroy: unsafe extern "C" fn(*mut Handle),
    set_option_string: unsafe extern "C" fn(*mut Handle, *const c_char, *const c_char) -> c_int,
    command: unsafe extern "C" fn(*mut Handle, *const *const c_char) -> c_int,
    set_property: unsafe extern "C" fn(*mut Handle, *const c_char, c_int, *mut c_void) -> c_int,
    get_property: unsafe extern "C" fn(*mut Handle, *const c_char, c_int, *mut c_void) -> c_int,
    get_property_string: unsafe extern "C" fn(*mut Handle, *const c_char) -> *mut c_char,
    free: unsafe extern "C" fn(*mut c_void),
    error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    wait_event: unsafe extern "C" fn(*mut Handle, c_double) -> *mut Event,
    wakeup: unsafe extern "C" fn(*mut Handle),
    render_create:
        unsafe extern "C" fn(*mut *mut RenderContext, *mut Handle, *mut RenderParam) -> c_int,
    render_set_update_callback:
        unsafe extern "C" fn(*mut RenderContext, Option<UpdateCb>, *mut c_void),
    render_update: unsafe extern "C" fn(*mut RenderContext) -> u64,
    render_render: unsafe extern "C" fn(*mut RenderContext, *mut RenderParam) -> c_int,
    render_free: unsafe extern "C" fn(*mut RenderContext),
    render_report_swap: unsafe extern "C" fn(*mut RenderContext),
    render_get_info: unsafe extern "C" fn(*mut RenderContext, RenderParam) -> c_int,
}

macro_rules! sym {
    ($lib:expr, $name:literal) => {
        *$lib
            .get(concat!($name, "\0").as_bytes())
            .map_err(|e| format!("{}: {e}", $name))?
    };
}

impl Api {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        unsafe {
            let lib = Library::new(path).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(Self {
                create: sym!(lib, "mpv_create"),
                initialize: sym!(lib, "mpv_initialize"),
                terminate_destroy: sym!(lib, "mpv_terminate_destroy"),
                set_option_string: sym!(lib, "mpv_set_option_string"),
                command: sym!(lib, "mpv_command"),
                set_property: sym!(lib, "mpv_set_property"),
                get_property: sym!(lib, "mpv_get_property"),
                get_property_string: sym!(lib, "mpv_get_property_string"),
                free: sym!(lib, "mpv_free"),
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
}

/// An mpv core. libmpv's client functions are thread-safe, so the handle is shared between the UI thread,
/// the event thread and the render thread.
pub struct Mpv {
    api: Api,
    handle: *mut Handle,
}

unsafe impl Send for Mpv {}
unsafe impl Sync for Mpv {}

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap()
}

impl Mpv {
    pub fn new(api: Api, options: &[(String, String)]) -> Result<Self, String> {
        let handle = unsafe { (api.create)() };
        if handle.is_null() {
            return Err("mpv_create failed".into());
        }
        let mpv = Self { api, handle };
        for (k, v) in options {
            let rc =
                unsafe { (mpv.api.set_option_string)(handle, cstr(k).as_ptr(), cstr(v).as_ptr()) };
            mpv.check(rc).map_err(|e| format!("option {k}={v}: {e}"))?;
        }
        mpv.check(unsafe { (mpv.api.initialize)(handle) })?;
        Ok(mpv)
    }

    pub fn check(&self, rc: c_int) -> Result<(), String> {
        if rc >= 0 { Ok(()) } else { Err(self.error(rc)) }
    }

    pub fn error(&self, rc: c_int) -> String {
        unsafe { CStr::from_ptr((self.api.error_string)(rc)) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn command(&self, args: &[&str]) -> Result<(), String> {
        let owned: Vec<CString> = args.iter().map(|a| cstr(a)).collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(ptr::null());
        self.check(unsafe { (self.api.command)(self.handle, ptrs.as_ptr()) })
    }

    pub fn set_flag(&self, name: &str, v: bool) {
        let mut v = v as c_int;
        unsafe {
            (self.api.set_property)(
                self.handle,
                cstr(name).as_ptr(),
                FORMAT_FLAG,
                &mut v as *mut _ as _,
            )
        };
    }

    pub fn set_double(&self, name: &str, mut v: f64) {
        unsafe {
            (self.api.set_property)(
                self.handle,
                cstr(name).as_ptr(),
                FORMAT_DOUBLE,
                &mut v as *mut _ as _,
            )
        };
    }

    pub fn double(&self, name: &str) -> Option<f64> {
        let mut v = 0f64;
        let rc = unsafe {
            (self.api.get_property)(
                self.handle,
                cstr(name).as_ptr(),
                FORMAT_DOUBLE,
                &mut v as *mut _ as _,
            )
        };
        (rc >= 0).then_some(v)
    }

    pub fn int(&self, name: &str) -> Option<i64> {
        let mut v = 0i64;
        let rc = unsafe {
            (self.api.get_property)(
                self.handle,
                cstr(name).as_ptr(),
                FORMAT_INT64,
                &mut v as *mut _ as _,
            )
        };
        (rc >= 0).then_some(v)
    }

    pub fn flag(&self, name: &str) -> Option<bool> {
        let mut v: c_int = 0;
        let rc = unsafe {
            (self.api.get_property)(
                self.handle,
                cstr(name).as_ptr(),
                FORMAT_FLAG,
                &mut v as *mut _ as _,
            )
        };
        (rc >= 0).then_some(v != 0)
    }

    pub fn string(&self, name: &str) -> Option<String> {
        let p = unsafe { (self.api.get_property_string)(self.handle, cstr(name).as_ptr()) };
        if p.is_null() {
            return None;
        }
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        unsafe { (self.api.free)(p as _) };
        Some(s)
    }

    /// Waits for the next event. Only one thread may call this.
    pub fn wait_event(&self, timeout: f64) -> (c_int, c_int, *mut c_void) {
        let e = unsafe { &*(self.api.wait_event)(self.handle, timeout) };
        (e.event_id, e.error, e.data)
    }

    pub fn wakeup(&self) {
        unsafe { (self.api.wakeup)(self.handle) };
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        unsafe { (self.api.terminate_destroy)(self.handle) };
    }
}

/// The software render context. Must be dropped before the `Mpv` it was created from.
pub struct SwRender {
    ctx: *mut RenderContext,
    free: unsafe extern "C" fn(*mut RenderContext),
    update: unsafe extern "C" fn(*mut RenderContext) -> u64,
    render: unsafe extern "C" fn(*mut RenderContext, *mut RenderParam) -> c_int,
    report_swap: unsafe extern "C" fn(*mut RenderContext),
    get_info: unsafe extern "C" fn(*mut RenderContext, RenderParam) -> c_int,
}

unsafe impl Send for SwRender {}

impl SwRender {
    /// `cb(data)` is called from mpv's threads whenever a new frame is due; it must not call into mpv.
    pub fn new(mpv: &Mpv, cb: UpdateCb, data: *mut c_void) -> Result<Self, String> {
        let api_type = cstr("sw");
        let mut params = [
            RenderParam {
                kind: RENDER_PARAM_API_TYPE,
                data: api_type.as_ptr() as _,
            },
            RenderParam {
                kind: RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];
        let mut ctx = ptr::null_mut();
        mpv.check(unsafe { (mpv.api.render_create)(&mut ctx, mpv.handle, params.as_mut_ptr()) })?;
        unsafe { (mpv.api.render_set_update_callback)(ctx, Some(cb), data) };
        Ok(Self {
            ctx,
            free: mpv.api.render_free,
            update: mpv.api.render_update,
            render: mpv.api.render_render,
            report_swap: mpv.api.render_report_swap,
            get_info: mpv.api.render_get_info,
        })
    }

    /// Flags of the frame the next render call would draw (`FRAME_INFO_*`), if any.
    pub fn next_frame_flags(&self) -> u64 {
        let mut info = [0u64; 2];
        let rc = unsafe {
            (self.get_info)(
                self.ctx,
                RenderParam {
                    kind: RENDER_PARAM_NEXT_FRAME_INFO,
                    data: info.as_mut_ptr() as _,
                },
            )
        };
        if rc < 0 { 0 } else { info[0] }
    }

    /// Tells mpv a frame was presented (display-synced timing).
    pub fn report_swap(&self) {
        unsafe { (self.report_swap)(self.ctx) }
    }

    pub fn update(&self) -> u64 {
        unsafe { (self.update)(self.ctx) }
    }

    /// Renders the current frame as `rgb0` (R, G, B, undefined byte) into `buf`, `stride` bytes per row.
    /// With `block`, waits until the frame's display time (mpv's default A/V timing).
    pub fn render_with(
        &self,
        w: u32,
        h: u32,
        stride: usize,
        buf: *mut u8,
        block: bool,
        skip: bool,
    ) -> c_int {
        let mut skip = skip as c_int;
        let mut size = [w as c_int, h as c_int];
        let format = cstr("rgb0");
        let mut stride = stride;
        let mut block = block as c_int;
        let mut params = [
            RenderParam {
                kind: RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
                data: &mut block as *mut c_int as _,
            },
            RenderParam {
                kind: RENDER_PARAM_SKIP_RENDERING,
                data: &mut skip as *mut c_int as _,
            },
            RenderParam {
                kind: RENDER_PARAM_SW_SIZE,
                data: size.as_mut_ptr() as _,
            },
            RenderParam {
                kind: RENDER_PARAM_SW_FORMAT,
                data: format.as_ptr() as _,
            },
            RenderParam {
                kind: RENDER_PARAM_SW_STRIDE,
                data: &mut stride as *mut usize as _,
            },
            RenderParam {
                kind: RENDER_PARAM_SW_POINTER,
                data: buf as _,
            },
            RenderParam {
                kind: RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];
        unsafe { (self.render)(self.ctx, params.as_mut_ptr()) }
    }
}

impl Drop for SwRender {
    fn drop(&mut self) {
        unsafe { (self.free)(self.ctx) };
    }
}
