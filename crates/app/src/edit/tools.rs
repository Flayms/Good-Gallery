//! Tools used on the photo: crop and straighten, the gradient, the brush and the picker.

use super::gpu::PaintTexture;
use super::panel::slider_row;
use super::{EditSession, GRIP_LENGTH, GRIP_REACH, PICK_RADIUS};
use domain::crop::{self, Grip, Ratio};
use domain::edit::{Adjustment, Geometry, MaskShape, Part, Recipe};
use domain::paint::{MAX_POINTS, Painting, Stroke};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2, pos2};

impl EditSession {
    pub fn is_cropping(&self) -> bool {
        self.cropping
    }

    /// Where the whole photo was last drawn while cropping.
    pub fn crop_area(&self) -> Option<Rect> {
        self.crop_area
    }

    pub fn set_crop_area(&mut self, area: Rect) {
        self.crop_area = Some(area);
    }

    pub fn is_dragging_crop(&self) -> bool {
        self.crop_drag.is_some()
    }

    /// Shows the crop's frame over the whole photo, or leaves it (REQ-029-02).
    pub fn set_cropping(&mut self, cropping: bool) {
        self.cropping = cropping;
        self.crop_drag = None;
    }

    /// The whole canvas's size as it is turned on screen: the photo, straightened (REQ-029-10).
    fn turned_size(&self) -> [f32; 2] {
        let [width, height] = self.target.full_size;
        let (width, height) = self.draft.geometry().uncropped().output_size(width, height);
        [width as f32, height as f32]
    }

    /// The photo's width per height, upright, which straightening needs (REQ-029-10).
    fn aspect(&self) -> f32 {
        let [width, height] = self.target.full_size;
        if width == 0 || height == 0 {
            1.0
        } else {
            width as f32 / height as f32
        }
    }

    /// `proposed(1.0)` if its frame shows only the photo, else as far towards it from `proposed(0.0)` as does: a drag
    /// or a new shape stops at the edge of a straightened photo (REQ-029-10).
    fn frame_on_photo(&self, proposed: impl Fn(f32) -> [f32; 4]) -> [f32; 4] {
        let whole = self.draft.geometry().uncropped();
        if whole.frame_shows_photo(proposed(1.0)) || !whole.frame_shows_photo(proposed(0.0)) {
            return proposed(1.0);
        }
        let (mut low, mut high) = (0.0, 1.0);
        for _ in 0..16 {
            let middle = (low + high) / 2.0;
            if whole.frame_shows_photo(proposed(middle)) {
                low = middle;
            } else {
                high = middle;
            }
        }
        proposed(low)
    }

    /// `frame`, made smaller about its center until it shows only the photo (REQ-029-10).
    fn shrunk_onto_photo(&self, frame: [f32; 4]) -> [f32; 4] {
        let [x, y, w, h] = frame;
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        // From a tiny frame at its center (t = 0) to the frame itself (t = 1).
        self.frame_on_photo(|t| {
            let t = 0.01 + 0.99 * t;
            [cx - w * t / 2.0, cy - h * t / 2.0, w * t, h * t]
        })
    }

    fn crop_frame(&self) -> [f32; 4] {
        self.draft.geometry().frame()
    }

    fn with_frame(&self, frame: [f32; 4]) -> Recipe {
        let mut recipe = self.draft.clone();
        recipe.crop = Geometry::crop_of_frame(frame, recipe.quarter_turns);
        recipe
    }

    /// Picks a shape: the largest frame of it inside the current one (REQ-029-03).
    pub(super) fn set_crop_ratio(&mut self, ratio: Ratio) {
        self.crop_ratio = ratio;
        let (frame, size) = (self.crop_frame(), self.turned_size());
        self.crop_portrait = crop::is_portrait(frame, size);
        if let Some(value) = ratio.value(self.crop_portrait, size) {
            let fitted = self.shrunk_onto_photo(crop::fit_ratio(frame, value, size));
            self.commit(self.with_frame(fitted));
        }
    }

    /// Turns the 4:3 or 16:9 to its other form (REQ-029-03).
    pub(super) fn swap_crop_ratio(&mut self) {
        self.crop_portrait = !self.crop_portrait;
        let swapped = self.shrunk_onto_photo(crop::swap_sides(self.crop_frame(), self.turned_size()));
        self.commit(self.with_frame(swapped));
    }

    /// Takes drags on the crop's frame, over the whole photo at `area` (REQ-029-02).
    pub fn crop_input(&mut self, ui: &egui::Ui, backdrop: &egui::Response, area: Rect) {
        let fraction = |pos: Pos2| {
            [
                (pos.x - area.left()) / area.width(),
                (pos.y - area.top()) / area.height(),
            ]
        };
        let tolerance = [GRIP_REACH / area.width(), GRIP_REACH / area.height()];
        let frame = self.crop_frame();
        let hover = backdrop
            .hover_pos()
            .and_then(|pos| crop::grip_at(frame, fraction(pos), tolerance));
        if let Some(grip) = self.crop_drag.map(|(grip, _)| grip).or(hover) {
            ui.ctx().set_cursor_icon(match grip {
                Grip::Move => egui::CursorIcon::Grab,
                Grip::Left | Grip::Right => egui::CursorIcon::ResizeHorizontal,
                Grip::Top | Grip::Bottom => egui::CursorIcon::ResizeVertical,
                Grip::TopLeft | Grip::BottomRight => egui::CursorIcon::ResizeNwSe,
                Grip::TopRight | Grip::BottomLeft => egui::CursorIcon::ResizeNeSw,
            });
        }
        let origin = ui.input(|input| input.pointer.press_origin());
        if backdrop.drag_started()
            && let Some(origin) = origin
        {
            self.crop_drag = crop::grip_at(frame, fraction(origin), tolerance).map(|grip| (grip, frame));
        }
        if let (Some((grip, start)), Some(origin), Some(pos)) =
            (self.crop_drag, origin, backdrop.interact_pointer_pos())
        {
            let delta = [(pos.x - origin.x) / area.width(), (pos.y - origin.y) / area.height()];
            let size = self.turned_size();
            let ratio = self.crop_ratio.value(self.crop_portrait, size);
            let frame = self.frame_on_photo(|t| crop::drag(start, grip, [delta[0] * t, delta[1] * t], ratio, size));
            self.draft = self.with_frame(frame);
            ui.ctx().request_repaint();
        }
        // One undo step per drag.
        if backdrop.drag_stopped() && self.crop_drag.take().is_some() {
            self.commit(self.draft.clone());
        }
    }

    /// The chosen shape's painting, if it is painted: its place, and whether it's an outline.
    fn painted(&self) -> Option<(usize, bool)> {
        let Part::Mask(i) = self.part else { return None };
        match self.draft.masks.get(i)?.shape {
            MaskShape::Painted { layer, freeform } => Some((usize::from(layer), freeform)),
            _ => None,
        }
    }

    /// Whether the chosen shape is used on the photo itself: a gradient's handles or painting (REQ-030-03, REQ-030-12).
    pub fn has_photo_tool(&self) -> bool {
        self.is_on_gradient() || self.painted().is_some()
    }

    /// Whether a handle or a stroke is being dragged.
    pub fn is_using_photo_tool(&self) -> bool {
        self.gradient_drag.is_some() || self.stroking
    }

    /// Takes drags on the photo at `area` for the chosen shape.
    pub fn photo_tool_input(&mut self, ui: &egui::Ui, backdrop: &egui::Response, area: Rect) {
        if self.painted().is_some() {
            self.paint_input(ui, backdrop, area);
        } else {
            self.gradient_input(ui, backdrop, area);
        }
    }

    /// What the chosen shape shows over the photo at `area`.
    pub fn paint_photo_tool(&self, painter: &egui::Painter, area: Rect) {
        self.paint_gradient(painter, area);
        self.paint_brush(painter, area);
    }

    /// Draws strokes and outlines on the photo at `area` (REQ-030-12, REQ-030-13). A stroke ends thinned, as one undo step.
    fn paint_input(&mut self, ui: &egui::Ui, backdrop: &egui::Response, area: Rect) {
        let Some((layer, freeform)) = self.painted() else {
            return;
        };
        if layer >= self.draft.paintings.len() {
            return;
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        let geometry = self.shown_geometry(false);
        let point = |pos: Pos2| {
            geometry
                .to_original([
                    (pos.x - area.left()) / area.width(),
                    (pos.y - area.top()) / area.height(),
                ])
                .map(|c| c.clamp(0.0, 1.0))
        };
        let room = domain::paint::total_points(&self.draft.paintings) < MAX_POINTS;
        let (size, softness, erase) = self.brush;
        let new_stroke = |first: [f32; 2]| Stroke {
            points: vec![first],
            size,
            softness,
            erase,
            closed: freeform,
        };
        let origin = ui.input(|input| input.pointer.press_origin());
        if backdrop.drag_started()
            && room
            && let Some(origin) = origin
        {
            self.draft.paintings[layer].strokes.push(new_stroke(point(origin)));
            self.stroking = true;
        }
        if self.stroking
            && room
            && let Some(pos) = backdrop.interact_pointer_pos()
            && let Some(stroke) = self.draft.paintings[layer].strokes.last_mut()
        {
            let next = point(pos);
            let moved = stroke.points.last().is_none_or(|last| {
                let (x, y) = (next[0] - last[0], next[1] - last[1]);
                x * x + y * y > 0.002 * 0.002
            });
            if moved {
                stroke.points.push(next);
            }
            ui.ctx().request_repaint();
        }
        if backdrop.drag_stopped() && std::mem::take(&mut self.stroking) {
            let strokes = &mut self.draft.paintings[layer].strokes;
            if let Some(stroke) = strokes.last_mut() {
                stroke.points = domain::paint::thin(&stroke.points);
            }
            if freeform && strokes.last().is_some_and(|stroke| stroke.points.len() < 3) {
                strokes.pop();
            }
            self.commit(self.draft.clone());
        }
        // A click is a dot.
        if backdrop.clicked()
            && !freeform
            && room
            && let Some(pos) = backdrop.interact_pointer_pos()
        {
            let mut recipe = self.draft.clone();
            let mut stroke = new_stroke(point(pos));
            stroke.points = domain::paint::thin(&stroke.points);
            recipe.paintings[layer].strokes.push(stroke);
            self.commit(recipe);
        }
    }

    /// The brush's reach around the pointer, and the outline being drawn, over the photo at `area`.
    fn paint_brush(&self, painter: &egui::Painter, area: Rect) {
        let Some((layer, freeform)) = self.painted() else {
            return;
        };
        let painter = painter.with_clip_rect(area);
        let dark = egui::Stroke::new(3.0, Color32::from_black_alpha(120));
        let white = egui::Stroke::new(1.0, Color32::WHITE);
        if freeform && self.stroking {
            let points: Vec<Pos2> = self
                .draft
                .paintings
                .get(layer)
                .and_then(|painting| painting.strokes.last())
                .map(|stroke| stroke.points.iter().map(|&point| self.on_photo(point, area)).collect())
                .unwrap_or_default();
            for stroke in [dark, white] {
                painter.add(egui::Shape::closed_line(points.clone(), stroke));
            }
        }
        if !freeform && let Some(pos) = painter.ctx().input(|input| input.pointer.hover_pos()) {
            // The photo's longer side as shown, cropped or not.
            let (corner, other) = (self.on_photo([0.0, 0.0], area), self.on_photo([1.0, 1.0], area));
            let longer = (other.x - corner.x).abs().max((other.y - corner.y).abs());
            let radius = self.brush.0 / 100.0 * longer;
            painter.circle_stroke(pos, radius, dark);
            painter.circle_stroke(pos, radius, white);
        }
    }

    /// Keeps the preview's texture of the paintings up to date, once the recipe has any.
    pub(super) fn sync_paintings(&mut self, paintings: &[Painting]) {
        let Some(render_state) = &self.render_state else { return };
        if paintings.is_empty() && self.paint_texture.is_none() {
            return;
        }
        let size = self.target.full_size.map(|side| side.max(1) as f32);
        let texture = self
            .paint_texture
            .get_or_insert_with(|| PaintTexture::new(&render_state.device, size));
        texture.sync(&render_state.queue, paintings);
    }

    /// The chosen mask's gradient, if it is one: its handles.
    fn gradient(&self) -> Option<([f32; 2], [f32; 2])> {
        let Part::Mask(i) = self.part else { return None };
        match self.draft.masks.get(i)?.shape {
            MaskShape::Linear { from, to } => Some((from, to)),
            _ => None,
        }
    }

    /// Whether the chosen mask is a gradient, whose handles need the whole photo (REQ-030-03).
    pub fn is_on_gradient(&self) -> bool {
        self.gradient().is_some()
    }

    pub fn is_dragging_gradient(&self) -> bool {
        self.gradient_drag.is_some()
    }

    /// Where the photo was last drawn with a gradient chosen.
    pub fn gradient_area(&self) -> Option<Rect> {
        self.gradient_area
    }

    pub fn set_gradient_area(&mut self, area: Rect) {
        self.gradient_area = Some(area);
    }

    /// A point of the original on the photo at `area`.
    fn on_photo(&self, point: [f32; 2], area: Rect) -> Pos2 {
        let [u, v] = self.shown_geometry(false).to_output(point);
        pos2(area.left() + u * area.width(), area.top() + v * area.height())
    }

    /// Takes drags on the chosen gradient's handles, on the photo at `area` (REQ-030-03).
    pub fn gradient_input(&mut self, ui: &egui::Ui, backdrop: &egui::Response, area: Rect) {
        let Some((from, to)) = self.gradient() else { return };
        let (first, second) = (self.on_photo(from, area), self.on_photo(to, area));
        let grabs = |pos: Pos2| -> Option<bool> {
            let (near_first, near_second) = (pos.distance(first), pos.distance(second));
            (near_first.min(near_second) <= GRIP_REACH).then_some(near_second < near_first)
        };
        if self.gradient_drag.is_some() || backdrop.hover_pos().and_then(grabs).is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let origin = ui.input(|input| input.pointer.press_origin());
        if backdrop.drag_started()
            && let Some(origin) = origin
        {
            self.gradient_drag = grabs(origin);
        }
        if let (Some(second), Some(pos), Part::Mask(i)) =
            (self.gradient_drag, backdrop.interact_pointer_pos(), self.part)
        {
            let [u, v] = self.shown_geometry(false).to_original([
                (pos.x - area.left()) / area.width(),
                (pos.y - area.top()) / area.height(),
            ]);
            let point = [u.clamp(-1.0, 2.0), v.clamp(-1.0, 2.0)];
            if let Some(MaskShape::Linear { from, to }) = self.draft.masks.get_mut(i).map(|mask| &mut mask.shape) {
                *(if second { to } else { from }) = point;
            }
            ui.ctx().request_repaint();
        }
        // One undo step per drag.
        if backdrop.drag_stopped() && self.gradient_drag.take().is_some() {
            self.commit(self.draft.clone());
        }
    }

    /// The chosen gradient over the photo at `area`: its two handles, each with a line across the photo.
    pub fn paint_gradient(&self, painter: &egui::Painter, area: Rect) {
        let Some((from, to)) = self.gradient() else { return };
        let (first, second) = (self.on_photo(from, area), self.on_photo(to, area));
        let painter = painter.with_clip_rect(area);
        let dark = egui::Stroke::new(3.0, Color32::from_black_alpha(120));
        let white = egui::Stroke::new(1.0, Color32::WHITE);
        let across = (second - first).normalized().rot90() * (area.width() + area.height());
        for stroke in [dark, white] {
            painter.line_segment([first, second], stroke);
            for handle in [first, second] {
                painter.line_segment([handle - across, handle + across], stroke);
            }
        }
        painter.circle(first, 6.0, Color32::WHITE, dark);
        painter.circle(
            second,
            6.0,
            Color32::from_black_alpha(160),
            egui::Stroke::new(1.5, Color32::WHITE),
        );
    }

    /// The crop's frame over the whole photo at `area`: darkened outside, thirds and grips inside (REQ-029-02).
    pub fn paint_crop(&self, painter: &egui::Painter, area: Rect) {
        let [x, y, w, h] = self.crop_frame();
        let frame = Rect::from_min_size(
            pos2(area.left() + x * area.width(), area.top() + y * area.height()),
            Vec2::new(w * area.width(), h * area.height()),
        );
        let dark = Color32::from_black_alpha(150);
        for outside in [
            Rect::from_min_max(area.min, pos2(area.right(), frame.top())),
            Rect::from_min_max(pos2(area.left(), frame.bottom()), area.max),
            Rect::from_min_max(pos2(area.left(), frame.top()), pos2(frame.left(), frame.bottom())),
            Rect::from_min_max(pos2(frame.right(), frame.top()), pos2(area.right(), frame.bottom())),
        ] {
            painter.rect_filled(outside, 0.0, dark);
        }
        let thin = egui::Stroke::new(1.0, Color32::from_white_alpha(120));
        for third in [1.0, 2.0] {
            let across = frame.left() + frame.width() * third / 3.0;
            let down = frame.top() + frame.height() * third / 3.0;
            painter.line_segment([pos2(across, frame.top()), pos2(across, frame.bottom())], thin);
            painter.line_segment([pos2(frame.left(), down), pos2(frame.right(), down)], thin);
        }
        painter.rect_stroke(
            frame,
            0.0,
            egui::Stroke::new(1.0, Color32::WHITE),
            egui::StrokeKind::Outside,
        );
        let grip = egui::Stroke::new(3.0, Color32::WHITE);
        let long = GRIP_LENGTH.min(frame.width() / 2.0).min(frame.height() / 2.0);
        for (corner, sign_x, sign_y) in [
            (frame.left_top(), 1.0, 1.0),
            (frame.right_top(), -1.0, 1.0),
            (frame.left_bottom(), 1.0, -1.0),
            (frame.right_bottom(), -1.0, -1.0),
        ] {
            painter.line_segment([corner, corner + Vec2::new(sign_x * long, 0.0)], grip);
            painter.line_segment([corner, corner + Vec2::new(0.0, sign_y * long)], grip);
        }
    }

    /// Mirrors the photo as it shows, left to right or top to bottom (REQ-029-11).
    pub(super) fn flip(&mut self, horizontal: bool) {
        let mut recipe = self.draft.clone();
        recipe.flip(horizontal);
        self.commit(recipe);
    }

    /// _Straighten_ (REQ-029-10): a free angle in tenths of a degree; a double-click sets it back to 0°.
    pub(super) fn straighten_slider(&mut self, ui: &mut egui::Ui) {
        let value = self.draft.straighten;
        let text = if value == 0.0 {
            "0°".to_owned()
        } else {
            format!("{value:+.1}°")
        };
        let mut current = f64::from(value);
        let range = (-domain::edit::MAX_STRAIGHTEN, domain::edit::MAX_STRAIGHTEN);
        let response = slider_row(ui, "Straighten", &text, &mut current, range, 0.1);
        self.straightening = response.is_pointer_button_down_on() || response.dragged();
        let aspect = self.aspect();
        if response.double_clicked() {
            let mut recipe = self.draft.clone();
            recipe.set_straighten(0.0, aspect);
            self.commit(recipe);
            return;
        }
        if response.changed() {
            self.draft.set_straighten(current as f32, aspect);
        }
        // One undo step per finished drag, click or key press.
        if response.drag_stopped() || response.clicked() || (response.changed() && !response.dragged()) {
            let recipe = self.draft.clone();
            self.commit(recipe);
        }
    }

    /// Whether _Straighten_ is being dragged, which shows a fine grid over the photo (REQ-029-10).
    pub fn is_straightening(&self) -> bool {
        self.straightening
    }

    /// A fine grid over the photo shown at `rect`, to line it up while straightening (REQ-029-10).
    pub fn paint_grid(&self, painter: &egui::Painter, rect: Rect) {
        const LINES: usize = 12;
        let thin = egui::Stroke::new(1.0, Color32::from_white_alpha(90));
        for i in 1..LINES {
            let t = i as f32 / LINES as f32;
            let across = rect.left() + rect.width() * t;
            let down = rect.top() + rect.height() * t;
            painter.line_segment([pos2(across, rect.top()), pos2(across, rect.bottom())], thin);
            painter.line_segment([pos2(rect.left(), down), pos2(rect.right(), down)], thin);
        }
    }

    /// Turns the photo by `quarters` quarter turns clockwise (REQ-029-01).
    pub(super) fn rotate(&mut self, quarters: u8) {
        if quarters % 2 == 1 {
            self.crop_portrait = !self.crop_portrait;
        }
        let mut recipe = self.draft.clone();
        recipe.quarter_turns = (recipe.quarter_turns + quarters) % 4;
        self.commit(recipe);
    }

    /// Picks the white balance at `uv` of the photo (REQ-021-05), or places the blur's focus there (REQ-023-03).
    pub fn pick(&mut self, uv: Pos2) {
        // `uv` is on the output as shown; the photo's own coordinates stay on the content (REQ-029-04).
        let [x, y] = self
            .shown_geometry(false)
            .to_original([uv.x.clamp(0.0, 1.0), uv.y.clamp(0.0, 1.0)]);
        let uv = pos2(x, y);
        if std::mem::take(&mut self.placing_focus) {
            let mut recipe = self.draft.clone();
            recipe.blur_focus = [uv.x.clamp(0.0, 1.0), uv.y.clamp(0.0, 1.0)];
            self.commit(recipe);
            return;
        }
        self.picking = false;
        let Some(small) = self.small() else { return };
        let (cx, cy) = (
            (uv.x.clamp(0.0, 1.0) * (small.width() - 1) as f32).round() as i64,
            (uv.y.clamp(0.0, 1.0) * (small.height() - 1) as f32).round() as i64,
        );
        let mut sum = [0.0f32; 3];
        let mut count = 0.0;
        for y in (cy - PICK_RADIUS)..=(cy + PICK_RADIUS) {
            for x in (cx - PICK_RADIUS)..=(cx + PICK_RADIUS) {
                if x < 0 || y < 0 || x >= i64::from(small.width()) || y >= i64::from(small.height()) {
                    continue;
                }
                let pixel = small.get_pixel(x as u32, y as u32);
                for (c, total) in sum.iter_mut().enumerate() {
                    *total += f32::from(pixel[c]) / 255.0;
                }
                count += 1.0;
            }
        }
        if count == 0.0 {
            return;
        }
        let (temperature, tint) = domain::edit::neutralize(sum.map(|c| c / count));
        let mut recipe = self.draft.clone();
        Adjustment::Temperature.set(&mut recipe, temperature.round());
        Adjustment::Tint.set(&mut recipe, tint.round());
        self.commit(recipe);
    }
}
