//! Content in the title bar (REQ-013-02): the toolbar, the Settings header and the sidebar header take the place of the
//! caption, and the native window controls stay. On Windows the native frame is kept (shadow, resizing, Aero snap) and
//! a window subclass removes the caption (`WM_NCCALCSIZE`), answers the top resize edge and the caption buttons with
//! their native hit-test codes (`WM_NCHITTEST`; `HTMAXBUTTON` makes Windows 11 show snap layouts) and runs them on
//! release. The app draws the buttons itself and keeps their place free. Empty caption space takes drags in egui and
//! hands them to Windows as a caption drag.
//!
//! On Linux the window is undecorated (winit's Wayland frame or the X11 window manager's caption would sit above the
//! toolbar, REQ-018-04): the app draws GNOME-style round caption buttons, and thin handles along the window's edges
//! hand resizing to the compositor.
//!
//! All of the app's window `unsafe` is in this module. Other platforms keep their native caption.

#[cfg(any(windows, target_os = "linux"))]
use crate::theme;
#[cfg(any(windows, target_os = "linux"))]
use eframe::egui::Stroke;
use eframe::egui::{self, Pos2, Rect, Vec2};

/// Width of one caption button: Windows' 46 px buttons, or on Linux a slot for a GNOME header bar's 24 px circle with
/// 6 px between circles.
pub const BUTTON_WIDTH: f32 = if cfg!(target_os = "linux") { 30.0 } else { 46.0 };
/// Room between the last caption button and the window's right edge (GNOME keeps one; Windows' buttons touch it).
const BUTTON_MARGIN: f32 = if cfg!(target_os = "linux") { 6.0 } else { 0.0 };
/// Height of the caption: the toolbar's.
pub const HEIGHT: f32 = crate::gallery::TOOLBAR_HEIGHT;
/// The close button's hover color, as Windows'.
#[cfg(windows)]
const CLOSE_HOVER: egui::Color32 = egui::Color32::from_rgb(0xc4, 0x2b, 0x1c);

/// Room kept free at the right of the caption for the window controls (minimize, maximize, close).
pub fn reserved_width() -> f32 {
    if cfg!(any(windows, target_os = "linux")) {
        3.0 * BUTTON_WIDTH + BUTTON_MARGIN
    } else {
        0.0
    }
}

/// Room a caption area ending at `right` keeps free: the window controls' width if it reaches the window's right edge
/// (an info sheet beside it takes them otherwise). "Reaches" allows for a margin around the content, as the test
/// harness has.
pub fn reserved_in(ctx: &egui::Context, right: f32) -> f32 {
    if right >= ctx.content_rect().right() - BUTTON_WIDTH {
        reserved_width()
    } else {
        0.0
    }
}

/// Where the window controls are, at the top right of `screen`: minimize, maximize, close.
pub fn button_rects(screen: Rect) -> [Rect; 3] {
    std::array::from_fn(|i| {
        Rect::from_min_size(
            Pos2::new(
                screen.right() - BUTTON_MARGIN - BUTTON_WIDTH * (3 - i) as f32,
                screen.top(),
            ),
            Vec2::new(BUTTON_WIDTH, HEIGHT),
        )
    })
}

/// Makes `response` (the background of an empty caption area) move the window when dragged and maximize or restore
/// it when double-clicked.
pub fn drag_area(ctx: &egui::Context, response: &egui::Response) {
    if response.drag_started_by(egui::PointerButton::Primary) {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if response.double_clicked() {
        let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
}

/// Installs the window subclass. Call once, when the window exists.
pub fn install(cc: &eframe::CreationContext) {
    #[cfg(windows)]
    windows::install(cc);
    #[cfg(not(windows))]
    let _ = cc;
}

/// Draws the window controls over everything, and tells the window procedure where they are. Call once per frame.
pub fn caption_buttons(ctx: &egui::Context) {
    #[cfg(windows)]
    {
        if !windows::installed() {
            return;
        }
        let screen = ctx.content_rect();
        let rects = button_rects(screen);
        windows::set_buttons(rects, ctx.pixels_per_point());
        let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        let (hovered, pressed) = windows::button_state();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::unique("caption buttons"),
        ));
        for (i, rect) in rects.into_iter().enumerate() {
            paint_button(&painter, rect, i, hovered == Some(i), pressed == Some(i), maximized);
        }
    }
    #[cfg(target_os = "linux")]
    {
        round_buttons(ctx);
        resize_handles(ctx);
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = ctx;
}

/// Width of the resize handles along the window's edges, and the length of the corner handles along each edge.
#[cfg(target_os = "linux")]
const EDGE: f32 = 4.0;
#[cfg(target_os = "linux")]
const CORNER: f32 = 12.0;

/// Where the window can be resized from: the corners, then the edges between them (REQ-018-04).
#[cfg(target_os = "linux")]
pub fn resize_handles_in(screen: Rect) -> [(egui::ResizeDirection, Rect); 8] {
    use egui::ResizeDirection as D;
    let (l, r, t, b) = (screen.left(), screen.right(), screen.top(), screen.bottom());
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1));
    [
        (D::NorthWest, rect(l, t, l + CORNER, t + CORNER)),
        (D::NorthEast, rect(r - CORNER, t, r, t + CORNER)),
        (D::SouthWest, rect(l, b - CORNER, l + CORNER, b)),
        (D::SouthEast, rect(r - CORNER, b - CORNER, r, b)),
        (D::North, rect(l + CORNER, t, r - CORNER, t + EDGE)),
        (D::South, rect(l + CORNER, b - EDGE, r - CORNER, b)),
        (D::West, rect(l, t + CORNER, l + EDGE, b - CORNER)),
        (D::East, rect(r - EDGE, t + CORNER, r, b - CORNER)),
    ]
}

/// A widget covering `rect` above the window's content at `order`, so presses on it don't reach the controls below.
#[cfg(target_os = "linux")]
fn on_top(ctx: &egui::Context, id: egui::Id, rect: Rect, order: egui::Order, sense: egui::Sense) -> egui::Response {
    let response = egui::Area::new(id)
        .order(order)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(ctx, |ui| ui.allocate_exact_size(rect.size(), sense).1)
        .inner;
    // Above the viewer, which shares the order.
    ctx.move_to_top(response.layer_id);
    response
}

/// Handles along the window's edges that start a resize by the compositor; none while the window is maximized.
#[cfg(target_os = "linux")]
fn resize_handles(ctx: &egui::Context) {
    use egui::{CursorIcon as C, ResizeDirection as D};
    let (maximized, fullscreen) = ctx.input(|input| {
        let viewport = input.viewport();
        (
            viewport.maximized.unwrap_or(false),
            viewport.fullscreen.unwrap_or(false),
        )
    });
    if maximized || fullscreen {
        return;
    }
    for (direction, rect) in resize_handles_in(ctx.content_rect()) {
        let id = egui::Id::unique(("resize handle", direction as u8));
        // Below menus and popups (`Foreground`), so items near the window's edge stay clickable; not focusable, so Tab
        // skips them.
        let response = on_top(ctx, id, rect, egui::Order::Middle, egui::Sense::DRAG);
        let cursor = match direction {
            D::North => C::ResizeNorth,
            D::South => C::ResizeSouth,
            D::East => C::ResizeEast,
            D::West => C::ResizeWest,
            D::NorthEast => C::ResizeNorthEast,
            D::SouthEast => C::ResizeSouthEast,
            D::NorthWest => C::ResizeNorthWest,
            D::SouthWest => C::ResizeSouthWest,
        };
        let response = response.on_hover_cursor(cursor);
        // egui gives a drag to the topmost widget that senses drags, even below a menu item that took the click, so
        // only a press that began on the handle's own layer resizes.
        let pressed_on_handle = ctx
            .input(|input| input.pointer.press_origin())
            .is_some_and(|origin| ctx.layer_id_at(origin) == Some(response.layer_id));
        if response.drag_started_by(egui::PointerButton::Primary) && pressed_on_handle {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    }
}

/// GNOME-style caption buttons: minimize, maximize or restore, close, each a faint circle that brightens on hover and
/// press (REQ-018-04).
#[cfg(target_os = "linux")]
fn round_buttons(ctx: &egui::Context) {
    let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
    for (i, rect) in button_rects(ctx.content_rect()).into_iter().enumerate() {
        let response = on_top(
            ctx,
            egui::Id::unique(("caption button", i)),
            rect,
            egui::Order::Foreground,
            egui::Sense::click(),
        );
        let label = ["Minimize window", "Maximize window", "Close window"][i];
        let label = if i == 1 && maximized { "Restore window" } else { label };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        let alpha = if response.is_pointer_button_down_on() {
            0.3
        } else if response.hovered() {
            0.15
        } else {
            0.1
        };
        let painter = ctx.layer_painter(response.layer_id);
        let c = rect.center();
        painter.circle_filled(c, 12.0, theme::text().gamma_multiply(alpha));
        let stroke = Stroke::new(1.5, theme::text());
        match i {
            0 => {
                painter.hline((c.x - 4.0)..=(c.x + 4.0), c.y + 3.5, stroke);
            }
            1 if maximized => {
                painter.rect_stroke(
                    Rect::from_center_size(c + Vec2::new(-1.0, 1.0), Vec2::splat(6.0)),
                    1.0,
                    stroke,
                    egui::StrokeKind::Middle,
                );
                painter.line_segment([c + Vec2::new(-1.0, -4.0), c + Vec2::new(4.0, -4.0)], stroke);
                painter.line_segment([c + Vec2::new(4.0, -4.0), c + Vec2::new(4.0, 1.0)], stroke);
            }
            1 => {
                painter.rect_stroke(
                    Rect::from_center_size(c, Vec2::splat(8.0)),
                    1.0,
                    stroke,
                    egui::StrokeKind::Middle,
                );
            }
            _ => {
                painter.line_segment([c + Vec2::splat(-4.0), c + Vec2::splat(4.0)], stroke);
                painter.line_segment([c + Vec2::new(-4.0, 4.0), c + Vec2::new(4.0, -4.0)], stroke);
            }
        }
        if response.clicked() {
            ctx.send_viewport_cmd(match i {
                0 => egui::ViewportCommand::Minimized(true),
                1 => egui::ViewportCommand::Maximized(!maximized),
                _ => egui::ViewportCommand::Close,
            });
        }
    }
}

#[cfg(windows)]
fn paint_button(painter: &egui::Painter, rect: Rect, index: usize, hovered: bool, pressed: bool, maximized: bool) {
    let close = index == 2;
    let fill = match (hovered || pressed, close) {
        (true, true) => CLOSE_HOVER,
        (true, false) if pressed => theme::selected(),
        (true, false) => theme::hover(),
        _ => egui::Color32::TRANSPARENT,
    };
    painter.rect_filled(rect, 0.0, fill);
    let c = rect.center();
    let stroke = Stroke::new(1.0, theme::text());
    match index {
        0 => {
            painter.hline((c.x - 5.0)..=(c.x + 5.0), c.y, stroke);
        }
        1 if maximized => {
            painter.rect_stroke(
                Rect::from_center_size(c + Vec2::new(-1.0, 1.0), Vec2::splat(8.0)),
                1.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment([c + Vec2::new(-2.0, -4.0), c + Vec2::new(4.0, -4.0)], stroke);
            painter.line_segment([c + Vec2::new(4.0, -4.0), c + Vec2::new(4.0, 2.0)], stroke);
        }
        1 => {
            painter.rect_stroke(
                Rect::from_center_size(c, Vec2::splat(10.0)),
                1.0,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        _ => {
            painter.line_segment([c + Vec2::splat(-5.0), c + Vec2::splat(5.0)], stroke);
            painter.line_segment([c + Vec2::new(-5.0, 5.0), c + Vec2::new(5.0, -5.0)], stroke);
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows {
    use eframe::egui::{self, Rect};
    use eframe::wgpu::rwh::{HasWindowHandle, RawWindowHandle};
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Mutex, OnceLock};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HTCLIENT, HTCLOSE, HTMAXBUTTON, HTMINBUTTON, HTTOP, IsZoomed, NCCALCSIZE_PARAMS, PostMessageW, SC_CLOSE,
        SC_MAXIMIZE, SC_MINIMIZE, SC_RESTORE, SM_CXPADDEDBORDER, SM_CYSIZEFRAME, SWP_FRAMECHANGED, SWP_NOMOVE,
        SWP_NOSIZE, SWP_NOZORDER, SetWindowPos, WM_MOUSEMOVE, WM_NCCALCSIZE, WM_NCHITTEST, WM_NCLBUTTONDBLCLK,
        WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_SYSCOMMAND,
    };

    const CODES: [u32; 3] = [HTMINBUTTON, HTMAXBUTTON, HTCLOSE];

    /// The caption buttons in physical client pixels (left, top, right, bottom), written by the UI every frame.
    static BUTTONS: Mutex<[[i32; 4]; 3]> = Mutex::new([[0; 4]; 3]);
    /// Hit-test code of the caption button under the mouse, or pressed on it, or 0.
    static HOVER: AtomicU32 = AtomicU32::new(0);
    static PRESSED: AtomicU32 = AtomicU32::new(0);
    static INSTALLED: AtomicBool = AtomicBool::new(false);
    static CTX: OnceLock<egui::Context> = OnceLock::new();

    pub fn installed() -> bool {
        INSTALLED.load(Ordering::Relaxed)
    }

    pub fn install(cc: &eframe::CreationContext) {
        let Ok(handle) = cc.window_handle() else { return };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        let hwnd = handle.hwnd.get() as HWND;
        let _ = CTX.set(cc.egui_ctx.clone());
        // SAFETY: `hwnd` is the app's live window, owned by this thread (eframe creates it on the UI thread before
        // the app); the subclass procedure lives for the program.
        let ok = unsafe { SetWindowSubclass(hwnd, Some(subclass), 1, 0) };
        if ok == 0 {
            tracing::warn!("can't subclass the window: the native caption stays");
            return;
        }
        INSTALLED.store(true, Ordering::Relaxed);
        // SAFETY: as above; recalculates the frame so `WM_NCCALCSIZE` runs through the subclass.
        unsafe {
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

    pub fn set_buttons(rects: [Rect; 3], pixels_per_point: f32) {
        let px = |r: Rect| {
            [
                (r.left() * pixels_per_point).round() as i32,
                (r.top() * pixels_per_point).round() as i32,
                (r.right() * pixels_per_point).round() as i32,
                (r.bottom() * pixels_per_point).round() as i32,
            ]
        };
        *BUTTONS.lock().unwrap_or_else(|p| p.into_inner()) = rects.map(px);
    }

    /// The button under the mouse and the one pressed, as indices (minimize, maximize, close).
    pub fn button_state() -> (Option<usize>, Option<usize>) {
        let index = |code: u32| CODES.iter().position(|&c| c == code);
        (
            index(HOVER.load(Ordering::Relaxed)),
            index(PRESSED.load(Ordering::Relaxed)),
        )
    }

    fn is_maximized(hwnd: HWND) -> bool {
        // SAFETY: a plain query on the app's window.
        unsafe { IsZoomed(hwnd) != 0 }
    }

    fn frame_thickness(hwnd: HWND) -> i32 {
        // SAFETY: plain queries on the app's window and the system metrics.
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

    fn is_button(code: u32) -> bool {
        CODES.contains(&code)
    }

    /// SAFETY (for the whole procedure): Windows calls it on the window's thread with valid messages; `lparam` of
    /// `WM_NCCALCSIZE` with `wparam != 0` points to an `NCCALCSIZE_PARAMS` for the duration of the call.
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
                    // Keep the left, right and bottom frame; drop the caption. A maximized window overhangs the
                    // screen by the frame thickness, so its top is inset by it.
                    let params = lparam as *mut NCCALCSIZE_PARAMS;
                    let top = (*params).rgrc[0].top;
                    let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                    (*params).rgrc[0].top = top + if is_maximized(hwnd) { frame_thickness(hwnd) } else { 0 };
                    result
                }
                WM_NCHITTEST => {
                    let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                    if result != HTCLIENT as isize {
                        return result;
                    }
                    let mut point = POINT {
                        x: (lparam & 0xffff) as i16 as i32,
                        y: ((lparam >> 16) & 0xffff) as i16 as i32,
                    };
                    ScreenToClient(hwnd, &mut point);
                    if !is_maximized(hwnd) && point.y < frame_thickness(hwnd) {
                        return HTTOP as isize;
                    }
                    let buttons = *BUTTONS.lock().unwrap_or_else(|p| p.into_inner());
                    for (rect, code) in buttons.into_iter().zip(CODES) {
                        if inside(rect, point.x, point.y) {
                            return code as isize;
                        }
                    }
                    HTCLIENT as isize
                }
                WM_NCMOUSEMOVE => {
                    let code = wparam as u32;
                    let hover = if is_button(code) { code } else { 0 };
                    if HOVER.swap(hover, Ordering::Relaxed) != hover {
                        repaint();
                    }
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE | TME_NONCLIENT,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut track);
                    DefSubclassProc(hwnd, msg, wparam, lparam)
                }
                WM_NCMOUSELEAVE | WM_MOUSEMOVE => {
                    if HOVER.swap(0, Ordering::Relaxed) != 0 {
                        repaint();
                    }
                    if msg == WM_NCMOUSELEAVE {
                        PRESSED.store(0, Ordering::Relaxed);
                    }
                    DefSubclassProc(hwnd, msg, wparam, lparam)
                }
                // Swallowed: DefWindowProc would track the press itself and draw classic buttons.
                WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if is_button(wparam as u32) => {
                    PRESSED.store(wparam as u32, Ordering::Relaxed);
                    repaint();
                    0
                }
                WM_NCLBUTTONUP if is_button(wparam as u32) => {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2};

    /// Runs one frame with `events` over a caption area covering the window and returns the commands it sent.
    fn frame(ctx: &egui::Context, time: f64, events: Vec<egui::Event>) -> Vec<egui::ViewportCommand> {
        run(ctx, time, events, false, |ui| {
            let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, HEIGHT));
            let response = ui.interact(rect, egui::Id::unique("caption"), egui::Sense::click_and_drag());
            drag_area(ui.ctx(), &response);
        })
    }

    /// Runs one frame of an 800 × 600 window and returns the commands it sent.
    fn run(
        ctx: &egui::Context,
        time: f64,
        events: Vec<egui::Event>,
        maximized: bool,
        ui: impl FnMut(&mut egui::Ui),
    ) -> Vec<egui::ViewportCommand> {
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            time: Some(time),
            events,
            ..Default::default()
        };
        input.viewports.entry(egui::ViewportId::ROOT).or_default().maximized = Some(maximized);
        let mut output = ctx.run_ui(input, ui);
        output.textures_delta.clear();
        output
            .viewport_output
            .remove(&egui::ViewportId::ROOT)
            .map(|viewport| viewport.commands)
            .unwrap_or_default()
    }

    /// A press and release at `at` over a window whose content is one big button, with the Linux window chrome on
    /// top; the commands sent and whether the content got the click.
    #[cfg(target_os = "linux")]
    fn click_linux_chrome(at: Pos2, maximized: bool) -> (Vec<egui::ViewportCommand>, bool) {
        let ctx = egui::Context::default();
        let mut content_clicked = false;
        let mut commands = Vec::new();
        // A first frame lays the window chrome out (a new area is invisible for a frame).
        let events = [
            vec![],
            vec![egui::Event::PointerMoved(at)],
            vec![press(at, true)],
            vec![press(at, false)],
        ];
        for (time, events) in events.into_iter().enumerate() {
            commands.extend(run(&ctx, time as f64 * 0.05, events, maximized, |ui| {
                let all = ui.interact(
                    ui.max_rect(),
                    egui::Id::unique("content"),
                    egui::Sense::click_and_drag(),
                );
                content_clicked |= all.clicked() || all.drag_started();
                round_buttons(ui.ctx());
                resize_handles(ui.ctx());
            }));
        }
        // egui's own, not the window chrome's.
        commands.retain(|command| !matches!(command, egui::ViewportCommand::SetTheme(_)));
        (commands, content_clicked)
    }

    fn press(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }

    #[test]
    fn req_013_02_empty_caption_space_moves_and_maximizes_the_window() {
        let ctx = egui::Context::default();
        let at = pos2(400.0, 20.0);
        frame(&ctx, 0.0, vec![egui::Event::PointerMoved(at)]);
        frame(&ctx, 0.1, vec![press(at, true)]);
        let dragged = frame(&ctx, 0.2, vec![egui::Event::PointerMoved(at + vec2(30.0, 0.0))]);
        assert!(dragged.contains(&egui::ViewportCommand::StartDrag), "{dragged:?}");
        frame(&ctx, 0.3, vec![press(at + vec2(30.0, 0.0), false)]);

        let ctx = egui::Context::default();
        frame(&ctx, 0.0, vec![egui::Event::PointerMoved(at)]);
        frame(&ctx, 0.1, vec![press(at, true)]);
        frame(&ctx, 0.15, vec![press(at, false)]);
        frame(&ctx, 0.2, vec![press(at, true)]);
        let double = frame(&ctx, 0.25, vec![press(at, false)]);
        assert!(double.contains(&egui::ViewportCommand::Maximized(true)), "{double:?}");
    }

    #[test]
    fn req_013_02_keeps_the_window_controls_place_free() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let [minimize, maximize, close] = button_rects(screen);
        assert_eq!(close.right(), 800.0 - BUTTON_MARGIN);
        assert_eq!(minimize.left(), 800.0 - reserved_width());
        assert!(minimize.right() <= maximize.left() && maximize.right() <= close.left());
        assert_eq!(close.height(), HEIGHT);
    }

    /// REQ-018-04: the round caption buttons minimize, maximize or restore and close the window, and the content below
    /// doesn't get the click.
    #[cfg(target_os = "linux")]
    #[test]
    fn req_018_04_round_caption_buttons_control_the_window() {
        use egui::ViewportCommand as C;
        let [minimize, maximize, close] = button_rects(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)));
        for (rect, maximized, command) in [
            (minimize, false, C::Minimized(true)),
            (maximize, false, C::Maximized(true)),
            (maximize, true, C::Maximized(false)),
            (close, false, C::Close),
        ] {
            let (commands, content) = click_linux_chrome(rect.center(), maximized);
            assert_eq!(commands, [command], "{rect:?}");
            assert!(!content, "the content under {rect:?} got the click");
        }
        // Between the circles is toolbar space again.
        assert!(minimize.right() <= maximize.left());
    }

    /// REQ-018-04: pressing the window's edges and corners hands the resize to the compositor, except while the window
    /// is maximized; inside the window the content gets the press.
    #[cfg(target_os = "linux")]
    #[test]
    fn req_018_04_edges_and_corners_resize_the_window() {
        use egui::ResizeDirection as D;
        for (at, direction) in [
            (pos2(400.0, 1.0), D::North),
            (pos2(400.0, 599.0), D::South),
            (pos2(1.0, 300.0), D::West),
            (pos2(799.0, 300.0), D::East),
            (pos2(2.0, 2.0), D::NorthWest),
            (pos2(798.0, 2.0), D::NorthEast),
            (pos2(2.0, 598.0), D::SouthWest),
            (pos2(798.0, 598.0), D::SouthEast),
        ] {
            let (commands, content) = click_linux_chrome(at, false);
            assert_eq!(commands, [egui::ViewportCommand::BeginResize(direction)], "{at:?}");
            assert!(!content, "the content at {at:?} got the press");
            let (commands, content) = click_linux_chrome(at, true);
            assert!(commands.is_empty(), "maximized: {commands:?}");
            assert!(content);
        }
        let (commands, content) = click_linux_chrome(pos2(400.0, 300.0), false);
        assert!(commands.is_empty() && content);
    }

    /// REQ-018-04: a menu reaching the window's edge keeps its clicks; the resize handles lie below it.
    #[cfg(target_os = "linux")]
    #[test]
    fn req_018_04_menus_at_the_edge_cover_the_resize_handles() {
        let ctx = egui::Context::default();
        let at = pos2(798.0, 300.0);
        let mut menu_clicked = false;
        let mut commands = Vec::new();
        for (time, events) in [
            vec![],
            vec![egui::Event::PointerMoved(at)],
            vec![press(at, true)],
            vec![press(at, false)],
        ]
        .into_iter()
        .enumerate()
        {
            commands.extend(run(&ctx, time as f64 * 0.05, events, false, |ui| {
                let menu = Rect::from_min_max(pos2(600.0, 200.0), pos2(800.0, 400.0));
                menu_clicked |= on_top(
                    ui.ctx(),
                    egui::Id::unique("menu"),
                    menu,
                    egui::Order::Foreground,
                    egui::Sense::click(),
                )
                .clicked();
                resize_handles(ui.ctx());
            }));
        }
        assert!(menu_clicked);
        assert!(
            !commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::BeginResize(_))),
            "{commands:?}"
        );
    }
}
