//! The photo or video itself: placing it, zoom and pan, the wheel, turning, and preloading its neighbours.

use super::{Chrome, Keys, Layout, Viewer, WHEEL_LINE, WHEEL_SENSITIVITY};
use crate::edit::EditSession;
use crate::gallery::Gallery;
use crate::originals::{OriginalRequest, OriginalState, Originals};
use crate::video::controls::Control;
use crate::{gallery, theme};
use db::gallery::GalleryItem;
use db::libraries::Library;
use db::media::file_in;
use domain::media::MediaKind;
use domain::viewer::{self as zoom, FIT, Size, ZoomView};
use eframe::egui::{self, Color32, CursorIcon, Key, Pos2, Rect, Vec2, pos2, vec2};

impl Viewer {
    /// The photo (or a video's preview), fitted into the media area, zoomed and panned (REQ-009-05, REQ-009-14 …
    /// -16). During the tile ↔ viewer transition (`transition`: the tile and how far it is open) it is drawn between
    /// its tile and its place (REQ-013-12). Returns where it was drawn.
    #[expect(clippy::too_many_arguments, reason = "one frame's inputs of the media area")]
    pub(super) fn media(
        &mut self,
        ui: &mut egui::Ui,
        backdrop: &egui::Response,
        layout: &Layout,
        item: &GalleryItem,
        gallery: &mut Gallery,
        original: &OriginalState,
        keys: Keys,
        transition: Option<(Rect, f32)>,
    ) -> Rect {
        let area = layout.media;
        let thumbnail = gallery.thumbnail(item.id);
        let loaded = match original {
            OriginalState::Loaded(loaded) => Some(loaded.clone()),
            _ => None,
        };
        let playing = self
            .playing_shown()
            .filter(|playing| playing.id == item.id)
            .map(|playing| (playing.state.video_size, playing.state.quarters, playing.frame));
        let video_size = playing.and_then(|(size, quarters, _)| Some(domain::video::shown_size(size?, quarters)));
        let natural = match (item.width, item.height, &loaded, &thumbnail) {
            // A video's own size once the player knows it (turned as it is shown), else its upright thumbnail's.
            _ if video_size.is_some() => {
                let [width, height] = video_size.unwrap_or_default();
                Size::new(width as f32, height as f32)
            }
            (_, _, _, Some(thumbnail)) if item.kind == MediaKind::Video => {
                Size::new(thumbnail.size[0] as f32, thumbnail.size[1] as f32)
            }
            (Some(width), Some(height), _, _) if width > 0 && height > 0 => Size::new(width as f32, height as f32),
            (_, _, Some(loaded), _) => Size::new(loaded.size[0] as f32, loaded.size[1] as f32),
            (_, _, _, Some(thumbnail)) => Size::new(thumbnail.size[0] as f32, thumbnail.size[1] as f32),
            _ => Size::new(area.width(), area.height()),
        };
        let editing = self.edit.as_ref().is_some_and(|session| session.media() == item.id);
        // While editing, the photo through the recipe; held `\\` or _Compare_ shows the original (REQ-021-02, REQ-021-07).
        let compare = self.comparing || ui.input(|input| input.key_down(Key::Backslash));
        // The turned, cropped photo fits as a whole, wherever it was zoomed to (REQ-029-01).
        let edited_size = self
            .edit
            .as_ref()
            .filter(|_| editing)
            .and_then(|session| session.shown_size(compare));
        let natural = edited_size.map_or(natural, |[width, height]| Size::new(width, height));
        if self.edited_size != edited_size {
            if self.edited_size.is_some() && edited_size.is_some() {
                self.view = FIT;
            }
            self.edited_size = edited_size;
        }
        let fitted = zoom::fit_size(natural, Size::new(area.width(), area.height()));
        // Zoomed, the photo spreads beyond the media area over the whole stage (REQ-009-20).
        let stage = layout.stage;
        let viewport = Size::new(stage.width(), stage.height());
        let photo = item.kind == MediaKind::Image;
        let picking = editing && self.edit.as_ref().is_some_and(EditSession::is_picking);
        let cropping = editing && !compare && self.edit.as_ref().is_some_and(EditSession::is_cropping);
        // The handles and painting need the whole photo (REQ-030-03, REQ-030-12).
        let gradient = editing && !compare && !cropping && self.edit.as_ref().is_some_and(EditSession::has_photo_tool);
        // The frame goes over the whole photo.
        if cropping || gradient {
            self.view = FIT;
        }

        // Not over the chrome: its own widgets take those clicks and wheel turns.
        let over_chrome = backdrop.hover_pos().is_some_and(|pos| layout.is_chrome(pos));
        if picking && !over_chrome {
            // The white balance picker takes the click on the photo (REQ-021-05).
            let target = photo_rect(area, fitted, self.view);
            if backdrop.clicked()
                && let Some(pos) = backdrop.interact_pointer_pos()
                && target.contains(pos)
                && let Some(session) = &mut self.edit
            {
                session.pick(pos2(
                    (pos.x - target.left()) / target.width(),
                    (pos.y - target.top()) / target.height(),
                ));
            }
        } else if cropping {
            let target = photo_rect(area, fitted, FIT);
            if let Some(session) = &mut self.edit {
                session.set_crop_area(Some(target));
                // A drag goes on over the chrome.
                if !over_chrome || session.is_dragging_crop() {
                    session.crop_input(ui, backdrop, target);
                }
            }
            self.close_at = None;
        } else if gradient {
            let target = photo_rect(area, fitted, FIT);
            if let Some(session) = &mut self.edit {
                session.set_gradient_area(target);
                // A drag goes on over the chrome.
                if !over_chrome || session.is_using_photo_tool() {
                    session.photo_tool_input(ui, backdrop, target);
                }
            }
            self.close_at = None;
        } else if photo && !over_chrome {
            self.handle_zoom(ui, backdrop, area, stage, natural, fitted, viewport, keys);
            // A click beside the photo doesn't close while editing.
            if editing {
                self.close_at = None;
            }
        } else if playing.is_some() && !over_chrome {
            // A click on the video plays or pauses it, as in a browser; beside it, it closes (REQ-009-03).
            let video = photo_rect(area, fitted, FIT);
            if !self.backdrop_clicks_beside(backdrop, video) {
                self.control_video(Control::PlayPause);
            }
        } else if !over_chrome {
            self.backdrop_clicks(backdrop);
        }
        let view = self.view;
        let target = photo_rect(area, fitted, view);
        // During the transition the photo moves between its tile and its place, over the whole window.
        // Cropped like the tile at first, uncropped once open.
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let (rect, clip, uv) = match transition {
            Some((tile, progress)) => {
                let cover = gallery::cover_uv([natural.width as u32, natural.height as u32], tile.size());
                (
                    lerp_rect(tile, target, progress),
                    ui.clip_rect(),
                    lerp_rect(cover, full, progress),
                )
            }
            None => (target, stage, full),
        };
        if photo
            && !over_chrome
            && !cropping
            && let Some(pos) = backdrop.hover_pos()
        {
            let icon = if !target.intersect(stage).contains(pos) {
                CursorIcon::Default
            } else if picking {
                CursorIcon::Crosshair
            } else if !view.is_zoomed() {
                CursorIcon::ZoomIn
            } else if backdrop.dragged() {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            };
            ui.ctx().set_cursor_icon(icon);
        }

        let painter = ui.painter().with_clip_rect(clip);
        let fade = match loaded {
            Some(_) if self.instant == Some(true) => 1.0,
            Some(_) => {
                let now = ui.input(|input| input.time);
                let from = *self.fade_from.get_or_insert(now);
                ((now - from) as f32 / theme::DURATION_BASE).min(1.0)
            }
            None => 0.0,
        };
        // A video's frame replaces its preview once the first one has come (not while it grows or shrinks).
        let frame = playing
            .and_then(|(_, quarters, frame)| Some((frame?, quarters)))
            .filter(|_| transition.is_none());
        // The thumbnail stays under the original until it has faded in; the placeholder only without one.
        if fade < 1.0 && frame.is_none() {
            match thumbnail {
                Some(thumbnail) => {
                    painter.image(thumbnail.id, rect, uv, Color32::WHITE);
                    crate::glass::record_image(ui.ctx(), rect, uv, clip, &thumbnail.view, Color32::WHITE);
                }
                None => {
                    if let Some(placeholder) = gallery.placeholder(ui.ctx(), item.id) {
                        painter.image(placeholder, rect, uv, Color32::WHITE);
                    }
                }
            }
        }
        // While _Straighten_ is held, the whole turned photo around the crop, which keeps its place (REQ-029-12).
        let straightening = (editing && !compare && !cropping && transition.is_none())
            .then(|| self.edit.as_ref()?.straightening_area(rect))
            .flatten();
        if !cropping && let Some(session) = &mut self.edit {
            session.set_crop_area(straightening);
        }
        let edited = editing
            && transition.is_none()
            && self
                .edit
                .as_mut()
                .is_some_and(|session| session.paint(ui, straightening.unwrap_or(rect), uv, clip, compare, 1.0));
        if edited {
            if let (Some(area), Some(session)) = (straightening, &self.edit) {
                session.paint_cut_off(&painter, area.intersect(clip), rect);
            }
            if cropping && let Some(session) = &self.edit {
                session.paint_crop(&painter, target);
            }
            if let Some(session) = self.edit.as_ref().filter(|session| session.is_straightening()) {
                session.paint_grid(&painter, rect.intersect(clip));
            }
            if gradient && let Some(session) = &self.edit {
                session.paint_photo_tool(&painter, target);
            }
        } else if let Some(loaded) = &loaded {
            let tint = Color32::WHITE.gamma_multiply(fade);
            painter.image(loaded.id, rect, uv, tint);
            if let Some(view) = &loaded.view {
                crate::glass::record_image(ui.ctx(), rect, uv, clip, view, tint);
            }
            if fade < 1.0 {
                ui.ctx().request_repaint();
            }
        }
        if let Some((frame, quarters)) = frame {
            painter.add(rotated_image(frame.id, rect, quarters));
        }
        let at = pos2(target.center().x, target.bottom().min(area.bottom()) - 28.0);
        if editing && self.edit.as_ref().is_some_and(|session| session.failed().is_some()) {
            message(ui, at, "Can't load the original to edit.");
        } else if let OriginalState::Failed(_) = original {
            message(ui, at, "Can't load the original. Showing a preview.");
        }
        if item.kind == MediaKind::Video && self.video_unavailable().is_some() {
            message(ui, at, "Can't play videos here. Showing a preview.");
        } else if playing.is_some() && self.video().is_some_and(|state| state.error.is_some()) {
            message(ui, at, "Can't play this video. Showing a preview.");
        }
        rect
    }

    #[expect(clippy::too_many_arguments, reason = "one frame's inputs of the media area")]
    fn handle_zoom(
        &mut self,
        ui: &egui::Ui,
        backdrop: &egui::Response,
        area: Rect,
        stage: Rect,
        natural: Size,
        fitted: Size,
        viewport: Size,
        keys: Keys,
    ) {
        let content = fitted;
        let relative = |pos: Pos2| (pos.x - area.center().x, pos.y - area.center().y);
        // Only where it is drawn: around a zoomed photo's clipped edges the scrim shows, and a click there closes.
        let on_photo = |view: ZoomView, pos: Pos2| photo_rect(area, fitted, view).intersect(stage).contains(pos);
        let toggle = |view: ZoomView, pos: Pos2| {
            let scale = zoom::toggle_scale(view, natural.width, fitted.width);
            zoom::zoom_to(view, scale, relative(pos), content, viewport)
        };

        if keys.fit {
            self.view = FIT;
        }
        if let Some(factor) = keys.zoom {
            self.view = zoom::zoom_to(self.view, self.view.scale * factor, (0.0, 0.0), content, viewport);
        }
        if backdrop.hovered() {
            let factor = zoom_factor(ui);
            if factor != 1.0
                && let Some(pos) = backdrop.hover_pos()
            {
                self.view = zoom::zoom_to(self.view, self.view.scale * factor, relative(pos), content, viewport);
            }
        }
        if backdrop.dragged() && self.view.is_zoomed() {
            let delta = backdrop.drag_delta();
            let moved = ZoomView {
                x: self.view.x + delta.x,
                y: self.view.y + delta.y,
                ..self.view
            };
            self.view = zoom::clamp_view(moved, content, viewport);
        }
        let Some(pos) = backdrop.interact_pointer_pos() else {
            return;
        };
        if backdrop.double_clicked() {
            self.close_at = None;
            // On the photo, the first click already switched (REQ-009-16).
            if !on_photo(self.view, pos) {
                self.view = toggle(self.view, pos);
            }
        } else if backdrop.clicked() {
            if on_photo(self.view, pos) {
                self.view = toggle(self.view, pos);
            } else {
                self.close_later(&backdrop.ctx);
            }
        }
    }
}

/// A wheel event's movement in points (lines count 100).
fn wheel_points(unit: egui::MouseWheelUnit, delta: Vec2) -> Vec2 {
    match unit {
        egui::MouseWheelUnit::Point => delta,
        egui::MouseWheelUnit::Line => delta * WHEEL_LINE,
        egui::MouseWheelUnit::Page => vec2(delta.x.signum(), delta.y.signum()) * WHEEL_LINE,
    }
}

/// This frame's wheel movement in points, without Ctrl.
pub(super) fn wheel(ui: &egui::Ui) -> Vec2 {
    ui.input(|input| {
        input
            .raw
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel {
                    unit, delta, modifiers, ..
                } if !modifiers.ctrl => Some(wheel_points(*unit, *delta)),
                _ => None,
            })
            .fold(Vec2::ZERO, |sum, delta| sum + delta)
    })
}

/// How much this frame's wheel, touchpad scrolling and pinching zoom a photo (REQ-009-05). Windows reports a pinch as
/// Ctrl + wheel, other systems as a zoom event. egui's delta is positive when scrolling up, a browser's deltaY then
/// negative: up zooms in.
fn zoom_factor(ui: &egui::Ui) -> f32 {
    ui.input(|input| {
        input.raw.events.iter().fold(1.0, |factor, event| match event {
            egui::Event::MouseWheel { unit, delta, .. } => {
                factor * (wheel_points(*unit, *delta).y * WHEEL_SENSITIVITY).exp()
            }
            egui::Event::Zoom(zoom) => factor * zoom,
            _ => factor,
        })
    })
}

/// Where the photo is drawn: fitted into `area`, scaled and moved by `view`; parts outside `area` are clipped.
fn photo_rect(area: Rect, fitted: Size, view: ZoomView) -> Rect {
    Rect::from_center_size(
        area.center() + vec2(view.x, view.y),
        vec2(fitted.width, fitted.height) * view.scale,
    )
}

/// `texture` drawn into `rect` turned clockwise by `quarters` quarter turns: mpv's software renderer leaves frames
/// unrotated, the GPU turns them for free.
fn rotated_image(texture: egui::TextureId, rect: Rect, quarters: u8) -> egui::Shape {
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    let uvs = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
    let mut mesh = egui::Mesh::with_texture(texture);
    for (i, pos) in corners.into_iter().enumerate() {
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: uvs[(i + 4 - usize::from(quarters % 4)) % 4],
            color: Color32::WHITE,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    egui::Shape::mesh(mesh)
}

fn lerp_rect(from: Rect, to: Rect, t: f32) -> Rect {
    Rect::from_min_max(from.min.lerp(to.min, t), from.max.lerp(to.max, t))
}

/// Asks for the originals of the shown photo and its neighbours, nearest first (REQ-009-13); first of all `outside`, the
/// shown item if it left the result (REQ-009-18).
pub(super) fn preload(
    gallery: &mut Gallery,
    originals: &mut Originals,
    libraries: &[Library],
    outside: Option<&GalleryItem>,
    index: usize,
    count: usize,
) {
    let request = |item: &GalleryItem| {
        if item.kind != MediaKind::Image {
            return None;
        }
        Some(OriginalRequest {
            media: item.id,
            path: file_in(libraries, item.root_id, &item.rel_path)?,
            sidecar_orientation: item.sidecar_orientation,
        })
    };
    let mut wanted: Vec<_> = outside.and_then(request).into_iter().collect();
    for at in std::iter::once(index).chain(zoom::neighbours(count, index)) {
        wanted.extend(gallery.item(at).and_then(request));
    }
    originals.set(wanted);
}

/// Text in a glass pill centered at `at`.
pub(super) fn message(ui: &mut egui::Ui, at: Pos2, text: &str) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.into(), egui::FontId::proportional(theme::SIZE_BODY), theme::text());
    let rect = Rect::from_center_size(at, galley.size() + vec2(32.0, 12.0));
    ui.painter()
        .rect_filled(rect, rect.height() / 2.0, Chrome::new(None).bar);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, theme::text());
    // Hover only: clicks still reach the photo and the scrim below.
    let response = ui.interact(rect, ui.make_persistent_id(("message", text)), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
}
