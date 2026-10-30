//! The video controls (REQ-009-06): a glass bar along the bottom of the video, like a browser's: play / pause, the time,
//! a seek slider, mute and volume.

use super::PlaybackState;
use crate::icons::Icon;
use crate::{format, theme};
use eframe::egui::{self, Rect, Vec2, pos2};

pub const HEIGHT: f32 = 40.0;
/// The bar never gets narrower than this, also over a narrow (portrait) video.
const MIN_WIDTH: f32 = 360.0;
/// Distance from the video's bottom edge and sides.
const MARGIN: f32 = 8.0;
const GAP: f32 = 8.0;
const VOLUME_WIDTH: f32 = 72.0;

/// What the user asked for on the bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    /// Play or pause; play starts over at the end.
    PlayPause,
    /// Seek to seconds: exact, or to a keyframe while the slider is dragged.
    Seek(f64, bool),
    Volume(f64),
    Muted(bool),
}

/// Where the bar goes: inside the bottom of `video`, at least [`MIN_WIDTH`] wide, within `area`.
pub fn bar_rect(video: Rect, area: Rect) -> Rect {
    let width = (video.width() - 2.0 * MARGIN).max(MIN_WIDTH).min(area.width());
    let bottom = video.bottom().min(area.bottom()) - MARGIN;
    let center = video
        .center()
        .x
        .clamp(area.left() + width / 2.0, area.right() - width / 2.0);
    Rect::from_min_max(
        pos2(center - width / 2.0, bottom - HEIGHT),
        pos2(center + width / 2.0, bottom),
    )
}

/// The bar in `rect` on `backing` (glass). `dragging` holds the slider's position while it is dragged.
pub fn show(
    ui: &mut egui::Ui,
    rect: Rect,
    state: &PlaybackState,
    dragging: &mut Option<f64>,
    backing: crate::glass::Backing,
) -> Option<Control> {
    backing.paint(ui.painter(), rect, rect.height() / 2.0);
    // Clicks on the bar's background don't reach the video or the scrim.
    let background = ui.interact(rect, ui.make_persistent_id("video controls"), egui::Sense::click());
    background.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Toolbar, true, "Video controls"));
    let mut control = None;
    let y = rect.center().y;
    let button = |x: f32| Rect::from_center_size(pos2(x, y), Vec2::splat(theme::CONTROL_HEIGHT));

    let playing = !state.paused && !state.ended;
    let (icon, label) = if playing {
        (Icon::Pause, "Pause")
    } else {
        (Icon::Play, "Play")
    };
    let mut x = rect.left() + 4.0 + theme::CONTROL_HEIGHT / 2.0;
    if icon_button(ui, button(x), icon, label, &format!("{label} (Space)")).clicked() {
        control = Some(Control::PlayPause);
    }
    x += theme::CONTROL_HEIGHT / 2.0 + GAP;

    let position = dragging.unwrap_or(state.position).min(state.duration.max(0.0));
    let time = ui.painter().layout_no_wrap(
        // The position counts whole seconds passed, as in a browser.
        format!(
            "{} / {}",
            format::duration(position.floor()),
            format::duration(state.duration)
        ),
        egui::FontId::new(theme::SIZE_CAPTION, theme::numbers()),
        theme::text(),
    );
    let time_rect = Rect::from_min_size(pos2(x, y - time.size().y / 2.0), time.size());
    let time_label = time.text().to_owned();
    ui.painter().galley(time_rect.min, time, theme::text());
    ui.interact(time_rect, ui.make_persistent_id("video time"), egui::Sense::hover())
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &time_label));
    x = time_rect.right() + GAP;

    // From the right: volume slider, mute.
    let volume_rect = Rect::from_min_max(
        pos2(rect.right() - 16.0 - VOLUME_WIDTH, y - 10.0),
        pos2(rect.right() - 16.0, y + 10.0),
    );
    let mute_x = volume_rect.left() - 4.0 - theme::CONTROL_HEIGHT / 2.0;
    let seek_rect = Rect::from_min_max(
        pos2(x, y - 10.0),
        pos2(mute_x - theme::CONTROL_HEIGHT / 2.0 - GAP, y + 10.0),
    );

    if seek_rect.width() > 20.0 {
        let mut value = position;
        let response = crate::widgets::slider(ui, seek_rect, &mut value, 0.0..=state.duration.max(0.0), 0.0, "Seek");
        if response.is_pointer_button_down_on() && response.changed() {
            // Keyframe seeks while dragging, an exact one on release.
            if dragging.is_none_or(|at| (at - value).abs() > 0.05) {
                control = Some(Control::Seek(value, false));
            }
            *dragging = Some(value);
        }
        if response.clicked() || response.drag_stopped() {
            control = Some(Control::Seek(value, true));
            *dragging = None;
        }
    }

    let silent = state.muted || state.volume <= 0.0;
    let icon = if silent { Icon::VolumeX } else { Icon::Volume2 };
    // The label says what a click does: it toggles mute, also when the volume is down to 0.
    let label = if state.muted { "Unmute" } else { "Mute" };
    if icon_button(ui, button(mute_x), icon, label, label).clicked() {
        control = Some(Control::Muted(!state.muted));
    }
    let mut volume = if state.muted { 0.0 } else { state.volume };
    if crate::widgets::slider(
        ui,
        volume_rect,
        &mut volume,
        0.0..=domain::video::MAX_VOLUME,
        0.0,
        "Volume",
    )
    .changed()
    {
        control = Some(Control::Volume(volume));
    }
    control
}

fn icon_button(ui: &mut egui::Ui, rect: Rect, icon: Icon, label: &str, tooltip: &str) -> egui::Response {
    let response = ui
        .put(
            rect,
            egui::Button::image(icon.small())
                .frame(false)
                .corner_radius(theme::CONTROL_HEIGHT / 2.0),
        )
        .on_hover_text(tooltip);
    let label = label.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    response
}
