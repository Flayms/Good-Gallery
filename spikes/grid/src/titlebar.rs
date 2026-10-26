//! Title bar variants on Windows:
//! 1. `native`: the OS title bar, dark.
//! 2. `custom`: no decorations; own caption buttons, drag via `StartDrag`.
//! 3. `hybrid`: decorations kept (shadow, snap, resize), the caption is removed with `WM_NCCALCSIZE`, own caption
//!    buttons report `HTMAXBUTTON` etc. from `WM_NCHITTEST`, so Windows 11 shows snap layouts on hover.
//!    This is what Electron's `titleBarOverlay` does too (its buttons are drawn by Chromium, not the OS).

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::*;
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variant {
    Native,
    Custom,
    Hybrid,
}

pub const BUTTON_WIDTH: f32 = 46.0;

/// Caption geometry in physical client pixels, written by the UI each frame, read by the window procedure.
#[derive(Default, Clone, Copy)]
struct Caption {
    buttons: [[i32; 4]; 3],
    drag: [i32; 4],
}

static CAPTION: Mutex<Caption> = Mutex::new(Caption {
    buttons: [[0; 4]; 3],
    drag: [0; 4],
});
/// Hit-test code of the caption button under the mouse (non-client), or 0.
static HOVER: AtomicU32 = AtomicU32::new(0);
static PRESSED: AtomicU32 = AtomicU32::new(0);
static HWND_: AtomicIsize = AtomicIsize::new(0);
static CTX: std::sync::OnceLock<egui::Context> = std::sync::OnceLock::new();

pub fn hwnd(cc: &eframe::CreationContext) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as HWND),
        _ => None,
    }
}

/// Dark caption in Darkroom for the native variant.
pub fn dark_caption(hwnd: HWND) {
    unsafe {
        let dark: i32 = 1;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &dark as *const _ as _,
            4,
        );
        let color: u32 = 0x001f1f1f; // COLORREF is 0x00BBGGRR
        DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR as u32, &color as *const _ as _, 4);
    }
}

pub fn install_hybrid(hwnd: HWND, ctx: egui::Context) {
    let _ = CTX.set(ctx);
    HWND_.store(hwnd as isize, Ordering::Relaxed);
    unsafe {
        SetWindowSubclass(hwnd, Some(subclass), 1, 0);
        // Recalculate the frame so WM_NCCALCSIZE runs with the subclass.
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        );
    }
}

fn is_maximized(hwnd: HWND) -> bool {
    unsafe { IsZoomed(hwnd) != 0 }
}

fn frame_thickness(hwnd: HWND) -> i32 {
    unsafe {
        let dpi = GetDpiForWindow(hwnd);
        GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
    }
}

fn inside(r: [i32; 4], x: i32, y: i32) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}

fn repaint() {
    if let Some(ctx) = CTX.get() {
        ctx.request_repaint();
    }
}

unsafe extern "system" fn subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    unsafe {
        match msg {
            WM_NCCALCSIZE if wparam != 0 => {
                // Keep the standard left, right and bottom frame; drop the caption. Maximized windows overhang the
                // screen by the frame thickness, so inset the top by it then.
                let params = lparam as *mut NCCALCSIZE_PARAMS;
                let top = (*params).rgrc[0].top;
                let r = DefSubclassProc(hwnd, msg, wparam, lparam);
                (*params).rgrc[0].top = top
                    + if is_maximized(hwnd) {
                        frame_thickness(hwnd)
                    } else {
                        0
                    };
                r
            }
            WM_NCHITTEST => {
                let r = DefSubclassProc(hwnd, msg, wparam, lparam);
                if r != HTCLIENT as isize {
                    return r;
                }
                let mut pt = POINT {
                    x: (lparam & 0xffff) as i16 as i32,
                    y: ((lparam >> 16) & 0xffff) as i16 as i32,
                };
                ScreenToClient(hwnd, &mut pt);
                if !is_maximized(hwnd) && pt.y < frame_thickness(hwnd) {
                    return HTTOP as isize;
                }
                let c = *CAPTION.lock().unwrap();
                for (i, code) in [HTMINBUTTON, HTMAXBUTTON, HTCLOSE].into_iter().enumerate() {
                    if inside(c.buttons[i], pt.x, pt.y) {
                        return code as isize;
                    }
                }
                if inside(c.drag, pt.x, pt.y) {
                    return HTCAPTION as isize;
                }
                HTCLIENT as isize
            }
            WM_NCMOUSEMOVE => {
                let code = wparam as u32;
                let hover = if [HTMINBUTTON, HTMAXBUTTON, HTCLOSE].contains(&code) {
                    code
                } else {
                    0
                };
                if HOVER.swap(hover, Ordering::Relaxed) != hover {
                    repaint();
                }
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE | TME_NONCLIENT,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut tme);
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            WM_NCMOUSELEAVE | WM_MOUSEMOVE => {
                if HOVER.swap(0, Ordering::Relaxed) != 0 {
                    repaint();
                }
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK
                if [HTMINBUTTON, HTMAXBUTTON, HTCLOSE].contains(&(wparam as u32)) =>
            {
                // Swallow, or DefWindowProc runs its classic button tracking and draws old-style buttons.
                PRESSED.store(wparam as u32, Ordering::Relaxed);
                repaint();
                0
            }
            WM_NCLBUTTONUP if [HTMINBUTTON, HTMAXBUTTON, HTCLOSE].contains(&(wparam as u32)) => {
                let code = wparam as u32;
                if PRESSED.swap(0, Ordering::Relaxed) == code {
                    let command = match code {
                        HTMINBUTTON => SC_MINIMIZE,
                        HTMAXBUTTON if is_maximized(hwnd) => SC_RESTORE,
                        HTMAXBUTTON => SC_MAXIMIZE,
                        _ => SC_CLOSE,
                    };
                    PostMessageW(hwnd, WM_SYSCOMMAND, command as usize, 0);
                }
                repaint();
                0
            }
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }
}

/// Draws the caption buttons at the top right of `rect` and handles them. `hybrid` reads hover and press state
/// from the window procedure (the mouse is in the non-client area there); `custom` uses egui's interaction.
pub fn caption_buttons(
    ui: &mut egui::Ui,
    rect: Rect,
    drag: Rect,
    variant: Variant,
    maximized: bool,
) {
    let ppp = ui.ctx().pixels_per_point();
    let buttons: Vec<Rect> = (0..3)
        .map(|i| {
            Rect::from_min_size(
                Pos2::new(rect.right() - BUTTON_WIDTH * (3 - i) as f32, rect.top()),
                Vec2::new(BUTTON_WIDTH, rect.height()),
            )
        })
        .collect();
    let codes = [HTMINBUTTON, HTMAXBUTTON, HTCLOSE];
    if variant == Variant::Hybrid {
        let px = |r: Rect| {
            [
                (r.left() * ppp) as i32,
                (r.top() * ppp) as i32,
                (r.right() * ppp) as i32,
                (r.bottom() * ppp) as i32,
            ]
        };
        *CAPTION.lock().unwrap() = Caption {
            buttons: [px(buttons[0]), px(buttons[1]), px(buttons[2])],
            drag: px(drag),
        };
    }
    for (i, r) in buttons.iter().enumerate() {
        let (hovered, pressed) = match variant {
            Variant::Hybrid => (
                HOVER.load(Ordering::Relaxed) == codes[i],
                PRESSED.load(Ordering::Relaxed) == codes[i],
            ),
            _ => {
                let resp = ui.interact(*r, egui::Id::new(("caption", i)), egui::Sense::click());
                if resp.clicked() {
                    let cmd = match i {
                        0 => egui::ViewportCommand::Minimized(true),
                        1 => egui::ViewportCommand::Maximized(!maximized),
                        _ => egui::ViewportCommand::Close,
                    };
                    ui.ctx().send_viewport_cmd(cmd);
                }
                (resp.hovered(), resp.is_pointer_button_down_on())
            }
        };
        let close = i == 2;
        let bg = match (hovered || pressed, close) {
            (true, true) => Color32::from_rgb(0xc4, 0x2b, 0x1c),
            (true, false) => Color32::from_white_alpha(if pressed { 20 } else { 15 }),
            _ => Color32::TRANSPARENT,
        };
        let p = ui.painter();
        p.rect_filled(*r, 0.0, bg);
        let c = r.center();
        let s = Stroke::new(1.0, crate::look::PAPER);
        match i {
            0 => {
                p.hline((c.x - 5.0)..=(c.x + 5.0), c.y, s);
            }
            1 if maximized => {
                p.rect_stroke(
                    Rect::from_center_size(c + Vec2::new(-1.0, 1.0), Vec2::splat(8.0)),
                    1.0,
                    s,
                    egui::StrokeKind::Inside,
                );
                p.line_segment([c + Vec2::new(-2.0, -4.0), c + Vec2::new(4.0, -4.0)], s);
                p.line_segment([c + Vec2::new(4.0, -4.0), c + Vec2::new(4.0, 2.0)], s);
            }
            1 => {
                p.rect_stroke(
                    Rect::from_center_size(c, Vec2::splat(10.0)),
                    1.0,
                    s,
                    egui::StrokeKind::Inside,
                );
            }
            _ => {
                p.line_segment([c + Vec2::splat(-5.0), c + Vec2::splat(5.0)], s);
                p.line_segment([c + Vec2::new(-5.0, 5.0), c + Vec2::new(5.0, -5.0)], s);
            }
        }
    }
}
