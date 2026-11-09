//! The control bar under the photo: its text, rating, buttons and toggles.

use super::{Action, BAR_HEIGHT, IDLE, INSET, Layout, NAV_SIZE, Viewer};
use crate::edit::Leave;
use crate::gallery::Gallery;
use crate::icons::Icon;
use crate::video::controls::{self as video_controls};
use crate::{format, icons, theme};
use db::gallery::GalleryItem;
use db::libraries::Library;
use domain::media::MediaKind;
use domain::viewer::{self as zoom};
use eframe::egui::{self, CursorIcon, Rect, Vec2, pos2, vec2};

impl Viewer {
    /// _Previous_ / _Next_, the control bar, the info panel and the filmstrip; the controls and the filmstrip fade out
    /// while idle (REQ-013-10, REQ-015-06). Returns what a control asked for.
    pub(super) fn controls(
        &mut self,
        ui: &mut egui::Ui,
        layout: &Layout,
        item: Option<&GalleryItem>,
        count: usize,
        gallery: &mut Gallery,
        libraries: &[Library],
    ) -> Option<Action> {
        let ctx = ui.ctx().clone();
        let screen = layout.screen;
        let looks_for_faces = gallery.looks_for_faces();
        let has_faces = self.indexed
            && item.is_some_and(|item| {
                item.kind == MediaKind::Image
                    && (looks_for_faces
                        || gallery
                            .cached_detail(item.id)
                            .is_some_and(|detail| !detail.faces.is_empty()))
            });
        let mut text = bar_text(ui, screen, item, libraries, has_faces);
        text.add_face = has_faces && self.faces && !crate::WEB;
        text.editable &= self.indexed;
        text.filmstrip = self.indexed;
        let bar = control_bar_rect(layout, bar_width(&text));
        let previous = self.step_target(-1, count).map(|_| {
            Rect::from_center_size(
                pos2(screen.left() + INSET + NAV_SIZE / 2.0, layout.media.center().y),
                Vec2::splat(NAV_SIZE),
            )
        });
        let next = self.step_target(1, count).map(|_| {
            Rect::from_center_size(
                pos2(layout.right - INSET - NAV_SIZE / 2.0, layout.media.center().y),
                Vec2::splat(NAV_SIZE),
            )
        });
        // The video's controls along its bottom, while it plays here (not for a file that fails).
        let video_bar = self
            .playing_shown()
            .is_some_and(|playing| playing.state.error.is_none())
            .then_some(self.photo)
            .flatten()
            .map(|video| video_controls::bar_rect(video, layout.media));
        let paused = self
            .playing_shown()
            .is_some_and(|playing| playing.state.paused || playing.state.ended);
        let pointer = ctx.pointer_latest_pos();
        let hovering = pointer.is_some_and(|pos| {
            bar.contains(pos)
                || previous.is_some_and(|r| r.contains(pos))
                || next.is_some_and(|r| r.contains(pos))
                || layout.strip.is_some_and(|r| r.contains(pos))
                || video_bar.is_some_and(|r| r.contains(pos))
        });
        let idle = self.last_activity.elapsed() >= IDLE;
        // A paused video keeps them (REQ-013-10).
        let hidden = zoom::controls_hidden(idle, hovering, self.info_open || self.edit.is_some(), paused);
        if !idle {
            ctx.request_repaint_after(IDLE.saturating_sub(self.last_activity.elapsed()));
        }
        let shown = ctx.animate_bool_with_time(ui.make_persistent_id("controls"), !hidden, theme::DURATION_BASE);
        if hidden && pointer.is_some_and(|pos| screen.contains(pos)) {
            ctx.set_cursor_icon(CursorIcon::None);
        }

        let mut action = None;
        if self.info_open
            && self.edit.is_none()
            && let Some(panel) = layout.info
        {
            action = self.info_panel(ui, panel, gallery);
        }
        if let Some(playing) = &mut self.playing {
            playing.bar = video_bar.filter(|_| shown > 0.0);
        }
        if shown == 0.0 {
            return action;
        }
        let mut step = None;
        ui.scope(|ui| {
            ui.multiply_opacity(shown);
            let bar_backing = self.chrome.backing(theme::glass_bar(), self.chrome_blur);
            if let Some(rect) = video_bar
                && let Some(playing) = &mut self.playing
                && let Some(control) =
                    video_controls::show(ui, rect, &playing.state, &mut playing.dragging, bar_backing)
            {
                playing.control = Some(control);
            }
            for (rect, icon, label, by) in [
                (previous, Icon::ChevronLeft, "Previous", -1),
                (next, Icon::ChevronRight, "Next", 1),
            ] {
                let Some(rect) = rect else { continue };
                let tooltip = if by < 0 { "Previous (←)" } else { "Next (→)" };
                if round_button(ui, rect, icon, label, tooltip, bar_backing).clicked() {
                    step = self.step_target(by, count);
                }
            }
            if let Some(strip) = layout.strip
                && let Some(index) = self.filmstrip(ui, strip, count, gallery)
            {
                step = Some(index);
            }
            let editing = self.edit.is_some();
            let toggles = Toggles {
                info: self.info_open,
                filmstrip: self.filmstrip,
                faces: self.faces,
                editing,
            };
            match control_bar(ui, bar, text, bar_backing, toggles) {
                Some(BarButton::Edit) => self.toggle_edit = true,
                Some(BarButton::ShowInExplorer) => {
                    if let Some(item) = item {
                        action = Self::explorer_action(item, libraries);
                    }
                }
                Some(BarButton::Info) => self.info_open = !self.info_open,
                Some(BarButton::Filmstrip) => self.toggle_filmstrip(gallery),
                Some(BarButton::Faces) => self.toggle_faces(gallery),
                Some(BarButton::AddFace) => {
                    self.face_popup = None;
                    self.drawing_face = Some(None);
                }
                Some(BarButton::Close) => action = Some(Action::Close),
                None => {}
            }
        });
        if !self.filmstrip {
            gallery.set_strip(0..0);
        }
        if let Some(index) = step
            && index < count
            && (index != self.index || self.left_result)
        {
            if self.edit.is_some() {
                self.leave_edit(Leave::Step(index), gallery);
            } else {
                self.go_to(index, gallery);
            }
            ctx.request_repaint();
        }
        action
    }
}

/// An icon button that fills `rect` exactly. With the theme's padding it paints as wide as its icon and padding want,
/// past `rect` and over its neighbours, though its response says `rect` (REQ-013-18).
pub(crate) fn put_button(ui: &mut egui::Ui, rect: Rect, button: egui::Button<'_>) -> egui::Response {
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = Vec2::ZERO;
        ui.put(rect, button.min_size(rect.size()))
    })
    .inner
}

fn round_button(
    ui: &mut egui::Ui,
    rect: Rect,
    icon: Icon,
    label: &str,
    tooltip: &str,
    backing: crate::glass::Backing,
) -> egui::Response {
    backing.paint(ui.painter(), rect, NAV_SIZE / 2.0);
    let response = put_button(
        ui,
        rect,
        egui::Button::image(icon.image(20.0, theme::text())).corner_radius(NAV_SIZE / 2.0),
    )
    .on_hover_text(tooltip);
    let label = label.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    response
}

/// What the control bar shows: file name, date and rating of the item, and whether it can be edited.
struct BarText {
    name: std::sync::Arc<egui::Galley>,
    date: Option<std::sync::Arc<egui::Galley>>,
    stars: usize,
    editable: bool,
    /// The photo may have faces (REQ-009-17, REQ-032-09).
    faces: bool,
    /// Faces are on, so one can be drawn (REQ-032-12).
    add_face: bool,
    /// The filmstrip can be shown: not for a file outside every library (REQ-033-03).
    filmstrip: bool,
}

/// Whether the item is a photo that can be edited here: JPEG, PNG or WebP in an online library, not in the browser
/// (REQ-021-01).
pub(super) fn editable(item: &GalleryItem, libraries: &[Library]) -> bool {
    !crate::WEB
        && item.kind == MediaKind::Image
        && domain::edit::is_editable(&item.file_name)
        && libraries
            .iter()
            .any(|library| library.id == item.root_id && library.status != db::libraries::LibraryStatus::Offline)
}

fn bar_text(ui: &egui::Ui, screen: Rect, item: Option<&GalleryItem>, libraries: &[Library], faces: bool) -> BarText {
    let name = item.map_or("", |item| item.file_name.as_str());
    let mut job = egui::text::LayoutJob::simple_singleline(
        name.to_owned(),
        egui::FontId::new(theme::SIZE_TITLE, theme::semibold()),
        theme::text(),
    );
    job.wrap = egui::text::TextWrapping {
        max_width: (screen.width() * 0.3).max(80.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let date = item.map(|item| {
        ui.painter().layout_no_wrap(
            format::medium_date_time(item.taken_at.unwrap_or(item.mtime)),
            egui::FontId::new(theme::SIZE_BODY, theme::numbers()),
            theme::text(),
        )
    });
    BarText {
        name: ui.painter().layout_job(job),
        date,
        stars: item.and_then(|item| item.rating).unwrap_or(0).clamp(0, 5) as usize,
        editable: item.is_some_and(|item| editable(item, libraries)),
        faces,
        add_face: false,
        filmstrip: true,
    }
}

const BAR_GAP: f32 = 12.0;
/// Between the control bar's buttons, so that two pressed ones stay two circles (REQ-013-18).
const BAR_BUTTON_GAP: f32 = theme::SPACE;

/// How wide `count` buttons of the control bar are.
fn bar_buttons_width(count: usize) -> f32 {
    count as f32 * (theme::CONTROL_HEIGHT + BAR_BUTTON_GAP) - BAR_BUTTON_GAP
}
const STAR_SIZE: f32 = 14.0;

/// The control bar's buttons, right of the separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarButton {
    Edit,
    ShowInExplorer,
    Info,
    Filmstrip,
    Faces,
    AddFace,
    Close,
}

const BAR_BUTTONS: [(BarButton, Icon, &str, &str); 7] = [
    (BarButton::Edit, Icon::SlidersHorizontal, "Edit", "Edit (e)"),
    (
        BarButton::ShowInExplorer,
        Icon::FolderOpen,
        crate::state::SHOW_IN_FOLDER,
        crate::state::SHOW_IN_FOLDER,
    ),
    (BarButton::Info, Icon::Info, "Info", "Info (i)"),
    (
        BarButton::Filmstrip,
        Icon::GalleryHorizontal,
        "Filmstrip",
        "Filmstrip (f)",
    ),
    (BarButton::Faces, Icon::ScanFace, "Faces", "Faces (p)"),
    (BarButton::AddFace, Icon::Plus, "Add face", "Draw a face on the photo"),
    (BarButton::Close, Icon::X, "Close", "Close (Esc)"),
];

/// The bar's buttons on this platform and for this item: the browser can't open a file manager (REQ-019-13), only
/// photos that can be edited have _Edit_ (REQ-021-01), only photos with face regions _Faces_ (REQ-009-17).
fn bar_buttons(
    editable: bool,
    faces: bool,
    add_face: bool,
    filmstrip: bool,
) -> Vec<(BarButton, Icon, &'static str, &'static str)> {
    BAR_BUTTONS
        .into_iter()
        .filter(|(button, ..)| match button {
            BarButton::Edit => editable,
            BarButton::Filmstrip => filmstrip,
            BarButton::Faces => faces,
            BarButton::AddFace => add_face,
            BarButton::ShowInExplorer => !crate::WEB,
            _ => true,
        })
        .collect()
}

/// The control bar's width for `text` and its buttons.
fn bar_width(text: &BarText) -> f32 {
    let mut width = 20.0 + text.name.size().x + BAR_GAP;
    if let Some(date) = &text.date {
        width += date.size().x + BAR_GAP;
    }
    if text.stars > 0 {
        width += text.stars as f32 * (STAR_SIZE + 2.0) + BAR_GAP;
    }
    // Separator, the buttons and the right padding.
    width += 1.0
        + BAR_GAP
        + bar_buttons_width(bar_buttons(text.editable, text.faces, text.add_face, text.filmstrip).len())
        + 6.0;
    width
}

/// Where the control bar of `width` goes: above the media, in the title bar's strip, over the media's middle where the
/// window controls leave room for it (REQ-009-19).
pub(super) fn control_bar_rect(layout: &Layout, width: f32) -> Rect {
    let left = layout.screen.left() + layout.reserved[0] + INSET;
    let right = layout.right - layout.reserved[1] - INSET;
    let width = width.min(right - left).max(0.0);
    let middle = (layout.screen.left() + layout.right) / 2.0;
    let x = middle.clamp(left + width / 2.0, (right - width / 2.0).max(left + width / 2.0));
    Rect::from_center_size(
        pos2(x, layout.screen.top() + INSET + BAR_HEIGHT / 2.0),
        vec2(width, BAR_HEIGHT),
    )
}

/// Which of the bar's toggles are on.
#[derive(Clone, Copy)]
struct Toggles {
    info: bool,
    filmstrip: bool,
    faces: bool,
    editing: bool,
}

/// File name, date, rating and the buttons in a pill at the bottom (REQ-009-01); the button clicked, if any.
fn control_bar(
    ui: &mut egui::Ui,
    rect: Rect,
    text: BarText,
    backing: crate::glass::Backing,
    toggles: Toggles,
) -> Option<BarButton> {
    let Toggles {
        info: info_open,
        filmstrip,
        faces,
        editing,
    } = toggles;
    let buttons = bar_buttons(
        text.editable || editing,
        text.faces && !editing,
        text.add_face && !editing,
        text.filmstrip,
    );
    backing.paint(ui.painter(), rect, rect.height() / 2.0);
    // Clicks on the bar's background don't reach the scrim behind it.
    ui.interact(rect, ui.make_persistent_id("control bar"), egui::Sense::click());
    let y = rect.center().y;
    let buttons_width = bar_buttons_width(buttons.len()) + 6.0;
    let text_clip = Rect::from_min_max(rect.min, pos2(rect.right() - buttons_width - BAR_GAP, rect.bottom()));
    let painter = ui.painter().with_clip_rect(text_clip);
    let mut x = rect.left() + 20.0;
    let name_width = text.name.size().x;
    painter.galley(pos2(x, y - text.name.size().y / 2.0), text.name, theme::text());
    x += name_width + BAR_GAP;
    if let Some(date) = text.date {
        let width = date.size().x;
        painter.galley(pos2(x, y - date.size().y / 2.0), date, theme::text());
        x += width + BAR_GAP;
    }
    if text.stars > 0 {
        for _ in 0..text.stars {
            let star = Rect::from_min_size(pos2(x, y - STAR_SIZE / 2.0), Vec2::splat(STAR_SIZE));
            if text_clip.contains_rect(star) {
                icons::rating_star(true, STAR_SIZE).paint_at(ui, star);
            }
            x += STAR_SIZE + 2.0;
        }
    }
    let separator = rect.right() - buttons_width - BAR_GAP / 2.0;
    ui.painter()
        .vline(separator, (y - 10.0)..=(y + 10.0), egui::Stroke::new(1.0, theme::rim()));
    let mut clicked = None;
    for (i, &(button, icon, label, tooltip)) in buttons.iter().enumerate() {
        let at = Rect::from_min_size(
            pos2(
                rect.right() - 6.0 - bar_buttons_width(buttons.len() - i),
                y - theme::CONTROL_HEIGHT / 2.0,
            ),
            Vec2::splat(theme::CONTROL_HEIGHT),
        );
        let pressed = match button {
            BarButton::Edit => Some(editing),
            BarButton::Info => Some(info_open && !editing),
            BarButton::Filmstrip => Some(filmstrip),
            BarButton::Faces => Some(faces),
            _ => None,
        };
        let mut widget = egui::Button::image(icon.small())
            .frame_when_inactive(false)
            .corner_radius(theme::CONTROL_HEIGHT / 2.0);
        // A pressed toggle is filled; a fill turns the frame on.
        if pressed == Some(true) {
            widget = widget.fill(theme::selected()).stroke(egui::Stroke::NONE);
        }
        let response = put_button(ui, at, widget).on_hover_text(tooltip);
        response.widget_info(|| match pressed {
            Some(pressed) => egui::WidgetInfo::selected(egui::Role::Button, true, pressed, label),
            None => egui::WidgetInfo::labeled(egui::Role::Button, true, label),
        });
        if response.clicked() {
            clicked = Some(button);
        }
    }
    clicked
}
