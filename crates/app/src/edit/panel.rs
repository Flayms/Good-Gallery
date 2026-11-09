//! The editing panel: sections, sliders, curves, masks, looks and LUTs, and the versions menu.

use super::{
    Base, EditSession, Leave, Outcome, ROW_HEIGHT, SCROLL_BAR_ROOM, SLIDER_HEIGHT, SaveMode, Subject, TIP_SAVE,
    TIP_WITHOUT_BACKUP,
};
use crate::icons::Icon;
use crate::{format, theme};
use domain::crop::Ratio;
use domain::edit::{
    Adjustment, BlurKind, Channel, DEFAULT_QUALITY, HISTOGRAM_BINS, LocalMask, MAX_MASKS, MIN_LONG_EDGE, MIN_QUALITY,
    MaskShape, Part, Recipe, Section, scaled_size,
};
use domain::look::{Look, LookChoice};
use domain::paint::{MAX_PAINTINGS, MAX_POINTS, Painting};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2, pos2, vec2};

impl EditSession {
    /// _Versions_ (REQ-021-24, REQ-021-25): the saved edits, newest first, then the original. The pointer over one
    /// shows it; a click loads it as a change.
    fn versions_menu(&mut self, ui: &mut egui::Ui) {
        let button = icon_button(
            ui,
            Icon::RotateCcwClock,
            "Versions",
            if self.versions.is_empty() {
                "Versions: each save of this photo shows here"
            } else {
                "Versions: go back to an earlier save or the original"
            },
            !self.versions.is_empty(),
        );
        self.previewed = None;
        let mut chosen = None;
        egui::Popup::menu(&button)
            .align(egui::RectAlign::BOTTOM_END)
            .show(|ui| {
                crate::glass::behind_menu(ui, |ui| {
                    let last = self.versions.len().saturating_sub(1);
                    let entries = self
                        .versions
                        .iter()
                        .enumerate()
                        .rev()
                        .map(|(i, version)| {
                            let when = version.saved_at.map_or_else(
                                || "Saved earlier".to_owned(),
                                |t| crate::format::medium_date_time(t.saturating_mul(1000)),
                            );
                            let label = if i == last { format!("{when} · saved") } else { when };
                            (label, version.recipe.clone())
                        })
                        .chain(std::iter::once(("Original".to_owned(), Recipe::default())));
                    for (label, recipe) in entries {
                        let response = ui.selectable_label(self.draft == recipe, label);
                        if response.hovered() {
                            self.previewed = Some(recipe.clone());
                        }
                        if response.clicked() {
                            chosen = Some(recipe);
                        }
                    }
                });
            });
        if let Some(recipe) = chosen {
            self.previewed = None;
            if recipe != self.draft {
                self.commit(recipe);
            }
        }
    }

    /// The adjustments panel (REQ-021-03 … REQ-021-07): undo, redo, compare, the sections, and Reset all, Cancel,
    /// Save as copy and Save. Returns what was asked for, and whether _Compare_ is held.
    pub fn panel(&mut self, ui: &mut egui::Ui, rect: Rect) -> (Option<Outcome>, bool) {
        let mut outcome = None;
        let inner = rect.shrink(16.0);
        let footer_height = 2.0 * theme::CONTROL_HEIGHT + 8.0;
        let header = Rect::from_min_size(inner.min, vec2(inner.width(), theme::CONTROL_HEIGHT));
        let footer = Rect::from_min_max(pos2(inner.left(), inner.bottom() - footer_height), inner.max);
        let body = Rect::from_min_max(
            pos2(inner.left(), header.bottom() + 12.0),
            pos2(inner.right(), footer.top() - 12.0),
        );

        // Header: title, undo, redo, compare.
        let mut compare = false;
        let mut header_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(header)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        header_ui.label(egui::RichText::new("Edit").font(egui::FontId::new(theme::SIZE_TITLE, theme::semibold())));
        header_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let held = icon_button(
                ui,
                Icon::SquareSplitHorizontal,
                "Compare",
                "Hold to compare with the photo before these changes (\\)",
                true,
            );
            compare = held.is_pointer_button_down_on();
            self.versions_menu(ui);
            if icon_button(ui, Icon::Redo2, "Redo", "Redo (Ctrl+Shift+Z)", self.history.can_redo()).clicked() {
                self.redo();
            }
            if icon_button(ui, Icon::Undo2, "Undo", "Undo (Ctrl+Z)", self.history.can_undo()).clicked() {
                self.undo();
            }
        });

        let mut body_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(body)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        body_ui.set_clip_rect(body.intersect(ui.clip_rect()));
        // The sliders show once the recipe is known, while the original may still load (REQ-021-19).
        match &self.base {
            Base::Failed(error) => {
                body_ui.add(egui::Label::new(format!("Can't load the original: {error}")).wrap());
            }
            _ if !self.is_ready() => {
                body_ui.spinner();
            }
            _ => {
                egui::ScrollArea::vertical()
                    .id_salt("edit panel")
                    .auto_shrink(false)
                    .show(&mut body_ui, |ui| self.sections(ui));
            }
        }

        // Footer: Reset all, Cancel / Save as copy / Save.
        let mut footer_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(footer)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ready = self.is_loaded() && !self.saving;
        footer_ui.horizontal(|ui| {
            let reset = ui.add_enabled(ready && !self.draft.is_neutral(), egui::Button::new("Reset all"));
            if reset.clicked() {
                self.commit(Recipe::default());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(!self.saving, egui::Button::new("Cancel")).clicked() {
                    outcome = self.leave(Leave::Exit);
                }
            });
        });
        footer_ui.add_space(8.0);
        footer_ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.saving { "Saving…" } else { "Save" };
                let save = egui::Button::new(
                    egui::RichText::new(label)
                        .color(theme::base())
                        .font(egui::FontId::new(theme::SIZE_BODY, theme::semibold())),
                )
                .fill(theme::text());
                let changed = self.has_changes();
                // Also for a photo saved with an edit before, whose backup it deletes (REQ-021-22).
                let without = ready && (changed || !self.draft.is_neutral());
                let more = icon_button(ui, Icon::ChevronUp, "More ways to save", "More ways to save", without);
                egui::Popup::menu(&more).align(egui::RectAlign::TOP_END).show(|ui| {
                    crate::glass::behind_menu(ui, |ui| {
                        if ui
                            .button("Save without backup")
                            .on_hover_text(TIP_WITHOUT_BACKUP)
                            .clicked()
                        {
                            outcome = self.save(SaveMode::WithoutBackup, None);
                        }
                    });
                });
                if ui.add_enabled(ready && changed, save).on_hover_text(TIP_SAVE).clicked() {
                    outcome = self.save(SaveMode::Replace, None);
                }
                let copy = egui::Button::new("Save as copy");
                let tip = format!(
                    "Keeps the photo as it is and saves the edited one next to it, as “{}”.",
                    domain::edit::copy_name(&self.target.file_name, |_| false)
                );
                if ui
                    .add_enabled(ready && !self.draft.is_neutral(), copy)
                    .on_hover_text(tip)
                    .clicked()
                {
                    outcome = self.save(SaveMode::Copy, None);
                }
            });
        });
        // Clicks and wheel turns on the panel stay in it.
        (outcome, compare)
    }

    /// The long edge, with shortcuts and the size it gives, and the quality for formats that have one.
    fn size(&mut self, ui: &mut egui::Ui) {
        let [width, height] = self.target.full_size;
        if width > 0 && height > 0 {
            let (cropped_width, cropped_height) = self.draft.geometry().output_size(width, height);
            let full = cropped_width.max(cropped_height);
            let edge = self.draft.long_edge.unwrap_or(full).min(full);
            let mut current = f64::from(edge);
            let low = MIN_LONG_EDGE.min(full) as f32;
            let text = format!("{} px", format::count(u64::from(edge)));
            let response = slider_row(ui, "Long edge", &text, &mut current, (low, full as f32), 1.0);
            if response.changed() {
                self.draft.long_edge = Some(current.round() as u32).filter(|&edge| edge < full);
            }
            if response.drag_stopped() || response.clicked() || (response.changed() && !response.dragged()) {
                self.commit(self.draft.clone());
            }
            ui.horizontal_wrapped(|ui| {
                for percent in [100, 75, 50, 25] {
                    let target = (full * percent / 100).max(1);
                    let label = format!("{percent} %");
                    if ui.add(egui::Button::selectable(edge == target, label)).clicked() {
                        let mut recipe = self.draft.clone();
                        recipe.long_edge = (percent < 100).then_some(target);
                        self.commit(recipe);
                    }
                }
            });
            let (saved_width, saved_height) = scaled_size(cropped_width, cropped_height, self.draft.long_edge);
            ui.label(
                egui::RichText::new(format!(
                    "Saved at {} × {}",
                    format::count(u64::from(saved_width)),
                    format::count(u64::from(saved_height))
                ))
                .size(theme::SIZE_CAPTION)
                .color(theme::muted()),
            );
            ui.add_space(4.0);
        }
        if self.has_quality() {
            let quality = self.draft.quality();
            let mut current = f64::from(quality);
            let response = slider_row(
                ui,
                "Quality",
                &quality.to_string(),
                &mut current,
                (f32::from(MIN_QUALITY), 100.0),
                1.0,
            );
            if response.changed() {
                self.draft.quality = Some(current.round() as u8).filter(|&quality| quality != DEFAULT_QUALITY);
            }
            if response.drag_stopped() || response.clicked() || (response.changed() && !response.dragged()) {
                self.commit(self.draft.clone());
            }
        }
    }

    /// The photo is saved in a format with a quality: JPEG or WebP (REQ-021-21).
    fn has_quality(&self) -> bool {
        let name = self.target.rel_path.to_lowercase();
        [".jpg", ".jpeg", ".webp"].iter().any(|ext| name.ends_with(ext))
    }

    fn sections(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        self.adjusting = false;
        if !self.parts(ui) {
            return;
        }
        // Crop and rotate belong to the whole photo (spec 029).
        if self.part == Part::Photo {
            section_header(ui, "Crop and rotate", &mut self.geometry_open);
            if self.geometry_open {
                ui.horizontal(|ui| {
                    if icon_button(ui, Icon::RotateCcw, "Rotate left", "Rotate left", true).clicked() {
                        self.rotate(3);
                    }
                    if icon_button(ui, Icon::RotateCw, "Rotate right", "Rotate right", true).clicked() {
                        self.rotate(1);
                    }
                    if icon_button(ui, Icon::FlipHorizontal2, "Flip horizontal", "Flip horizontal", true).clicked() {
                        self.flip(true);
                    }
                    if icon_button(ui, Icon::FlipVertical2, "Flip vertical", "Flip vertical", true).clicked() {
                        self.flip(false);
                    }
                    let cropping = self.cropping;
                    let button = egui::Button::image_and_text(Icon::Crop.small(), "Crop").selected(cropping);
                    let response = ui.add(button);
                    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, cropping, "Crop"));
                    if response.clicked() {
                        self.set_cropping(!cropping);
                    }
                });
                self.straighten_slider(ui);
                if self.cropping {
                    self.crop_shapes(ui);
                }
                ui.add_space(8.0);
            }
        }
        // The size and quality of the saved photo (REQ-029-09, REQ-021-21).
        if self.part == Part::Photo {
            section_header(ui, "Size", &mut self.size_open);
            if self.size_open {
                self.size(ui);
                ui.add_space(8.0);
            }
        }
        // A film look, under all other adjustments; the subject and background have none (REQ-028-01).
        if self.part == Part::Photo {
            section_header(ui, "Look", &mut self.look_open);
            if self.look_open {
                self.looks(ui);
                ui.add_space(8.0);
            }
        }
        for (i, section) in Section::ALL.into_iter().enumerate() {
            // The subject and the background have light and color only (REQ-022-01).
            if self.part != Part::Photo && matches!(section, Section::Curves | Section::Effects) {
                continue;
            }
            section_header(ui, section.label(), &mut self.open[i]);
            if !self.open[i] {
                continue;
            }
            if section == Section::Curves {
                self.curves(ui);
            } else {
                if section == Section::Color && self.part == Part::Photo {
                    let picking = self.picking;
                    let button =
                        egui::Button::image_and_text(Icon::Pipette.small(), "Pick a neutral spot").selected(picking);
                    // It picks from the small copy.
                    let response = ui
                        .add_enabled(self.small().is_some(), button)
                        .on_hover_text("Click a spot in the photo that should be grey");
                    if response.clicked() {
                        self.picking = !picking;
                    }
                }
                for adjustment in Adjustment::ALL.into_iter().filter(|a| a.section() == section) {
                    if adjustment == Adjustment::Blur {
                        self.blur_kind(ui);
                    }
                    let kind = self.draft.blur_kind;
                    let shown = match adjustment {
                        Adjustment::Blur => kind != BlurKind::None,
                        Adjustment::BlurSize | Adjustment::BlurFeather => kind.has_focus(),
                        _ => true,
                    };
                    if shown {
                        self.slider(ui, adjustment);
                    }
                }
            }
            ui.add_space(8.0);
        }
    }

    /// What to edit comes first: the photo, its subject, its background and its masks, the buttons to add a mask, and the
    /// chosen one's controls with the state of finding the subject (REQ-022-01, REQ-022-02, REQ-022-04, REQ-030-15). False
    /// while the chosen part's sliders can't be shown yet.
    fn parts(&mut self, ui: &mut egui::Ui) -> bool {
        // Undo may have taken the chosen mask away.
        if matches!(self.part, Part::Mask(i) if i >= self.draft.masks.len()) {
            self.choose_part(Part::Photo);
        }
        ui.horizontal_wrapped(|ui| {
            for part in Part::ALL {
                let response = ui.add(egui::Button::selectable(self.part == part, part.label()));
                if response.clicked() {
                    self.choose_part(part);
                }
            }
            for i in 0..self.draft.masks.len() {
                let label = format!("{} {}", self.draft.masks[i].shape.label(), i + 1);
                if ui
                    .add(egui::Button::selectable(self.part == Part::Mask(i), label))
                    .clicked()
                {
                    self.choose_part(Part::Mask(i));
                }
            }
        });
        ui.add_space(4.0);
        self.add_mask(ui);
        ui.add_space(8.0);
        if self.part == Part::Photo {
            return true;
        }
        if let Part::Mask(i) = self.part {
            self.mask_controls(ui, i);
            ui.add_space(8.0);
            return true;
        }
        let ready = match &self.subject {
            Subject::Unknown | Subject::Finding(_) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Finding the subject…");
                });
                false
            }
            Subject::Failed(_) => {
                ui.add(egui::Label::new("No subject found in this photo. You can still edit the whole photo.").wrap());
                false
            }
            Subject::Found(..) => {
                self.show_selection_button(ui);
                true
            }
        };
        ui.add_space(8.0);
        ready
    }

    /// Turns the tint of what is chosen off and on (REQ-030-07, REQ-022-04).
    fn show_selection_button(&mut self, ui: &mut egui::Ui) {
        let shown = self.show_selection;
        let response = ui
            .add(egui::Button::new("Show selection").selected(shown))
            .on_hover_text("Tint the part these adjustments change");
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, shown, "Show selection"));
        if response.clicked() {
            self.set_show_selection(!shown);
        }
    }

    /// One button per kind of mask (REQ-030-01, REQ-030-12, REQ-030-13).
    fn add_mask(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let room = self.draft.masks.len() < MAX_MASKS;
            let paint_room = room && self.draft.paintings.len() < MAX_PAINTINGS;
            for (label, freeform) in [("Add brush", false), ("Add freeform", true)] {
                if ui.add_enabled(paint_room, egui::Button::new(label)).clicked() {
                    let mut recipe = self.draft.clone();
                    let shape = new_painting(&mut recipe, freeform);
                    recipe.masks.push(LocalMask::new(shape));
                    let chosen = Part::Mask(recipe.masks.len() - 1);
                    self.commit(recipe);
                    self.choose_part(chosen);
                }
            }
            for (label, shape) in [
                ("Add gradient", MaskShape::LINEAR),
                ("Add luminance", MaskShape::LUMINANCE),
                ("Add color", MaskShape::COLOR),
            ] {
                if ui.add_enabled(room, egui::Button::new(label)).clicked() {
                    let mut recipe = self.draft.clone();
                    recipe.masks.push(LocalMask::new(shape));
                    let chosen = Part::Mask(recipe.masks.len() - 1);
                    self.commit(recipe);
                    self.choose_part(chosen);
                }
            }
        });
    }

    /// The chosen painted shape's brush, and clearing it (REQ-030-12 … REQ-030-14).
    fn paint_controls(&mut self, ui: &mut egui::Ui, layer: usize, freeform: bool) {
        let hint = if freeform {
            "Draw around what the mask should select, on the photo."
        } else {
            "Paint over what the mask should select, on the photo."
        };
        ui.add(egui::Label::new(hint).wrap());
        ui.add_space(4.0);
        let (size, softness, erase) = self.brush;
        if !freeform {
            let mut current = f64::from(size);
            let response = slider_row(ui, "Size", &format!("{size:.1} %"), &mut current, (0.2, 25.0), 0.1);
            if response.changed() {
                self.brush.0 = current as f32;
            }
        }
        let mut current = f64::from(softness);
        let response = slider_row(
            ui,
            "Softness",
            &format!("{softness:.0} %"),
            &mut current,
            (0.0, 100.0),
            1.0,
        );
        if response.changed() {
            self.brush.1 = current as f32;
        }
        ui.horizontal_wrapped(|ui| {
            for (label, erases) in [("Paint", false), ("Erase", true)] {
                if ui.add(egui::Button::selectable(erase == erases, label)).clicked() {
                    self.brush.2 = erases;
                }
            }
            let has_any = self
                .draft
                .paintings
                .get(layer)
                .is_some_and(|painting| !painting.strokes.is_empty());
            if ui.add_enabled(has_any, egui::Button::new("Clear")).clicked() {
                let mut recipe = self.draft.clone();
                recipe.paintings[layer].strokes.clear();
                self.commit(recipe);
            }
        });
        if domain::paint::total_points(&self.draft.paintings) >= MAX_POINTS {
            let full = "This photo has as much painting as a saved edit can hold. Clear some to paint more.";
            ui.add(egui::Label::new(full).wrap());
        }
        ui.add_space(4.0);
    }

    /// What the chosen mask is made of and selects, and turning it around, showing or removing it
    /// (REQ-030-03 … REQ-030-07, REQ-030-10, REQ-030-11).
    fn mask_controls(&mut self, ui: &mut egui::Ui, i: usize) {
        let Some(mask) = self.draft.masks.get(i) else { return };
        let (shape, invert) = (mask.shape, mask.invert);
        if matches!(shape, MaskShape::Linear { .. }) {
            let hint = "Drag the two handles on the photo: full at the white one, none at the dark one.";
            ui.add(egui::Label::new(hint).wrap());
            ui.add_space(4.0);
        }
        if let MaskShape::Painted { layer, freeform } = shape {
            self.paint_controls(ui, usize::from(layer), freeform);
        }
        for (slider, (label, value, range, unit)) in shape.sliders().into_iter().enumerate() {
            let mut current = f64::from(value);
            let response = slider_row(ui, label, &format!("{value:.0}{unit}"), &mut current, range, 1.0);
            if response.changed() {
                self.draft.masks[i].shape.set_slider(slider, current as f32);
            }
            if response.drag_stopped() || response.clicked() || (response.changed() && !response.dragged()) {
                self.commit(self.draft.clone());
            }
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let response = ui.add(egui::Button::new("Invert").selected(invert));
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, invert, "Invert"));
            if response.clicked() {
                let mut recipe = self.draft.clone();
                recipe.masks[i].invert = !invert;
                self.commit(recipe);
            }
            self.show_selection_button(ui);
            if ui.button("Remove mask").clicked() {
                let mut recipe = self.draft.clone();
                recipe.masks.remove(i);
                self.commit(recipe);
                self.choose_part(Part::Photo);
            }
        });
    }

    /// The crop's shapes (REQ-029-03), and leaving it.
    fn crop_shapes(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for ratio in Ratio::ALL {
                let response = ui.add(egui::Button::selectable(self.crop_ratio == ratio, ratio.label()));
                if response.clicked() {
                    self.set_crop_ratio(ratio);
                }
            }
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.crop_ratio.can_swap(), egui::Button::new("Swap"))
                .clicked()
            {
                self.swap_crop_ratio();
            }
            if ui.button("Done").on_hover_text("Done cropping (Esc)").clicked() {
                self.set_cropping(false);
            }
        });
    }

    /// The kind of blur, and placing its focus (REQ-023-01, REQ-023-03). _Background_ looks for the subject.
    fn blur_kind(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Blur").color(theme::muted()));
        ui.horizontal_wrapped(|ui| {
            for kind in BlurKind::ALL {
                let response = ui.add(egui::Button::selectable(self.draft.blur_kind == kind, kind.label()));
                if response.clicked() && self.draft.blur_kind != kind {
                    let mut recipe = self.draft.clone();
                    recipe.blur_kind = kind;
                    // A kind without a strength would show nothing.
                    if kind != BlurKind::None && recipe.blur == 0.0 {
                        recipe.blur = 50.0;
                    }
                    self.commit(recipe);
                    self.placing_focus = false;
                    if kind == BlurKind::Background {
                        self.find_subject();
                    }
                }
            }
        });
        // How it blurs: softly, or in discs as a lens does (REQ-023-05).
        if self.draft.blur_kind != BlurKind::None {
            ui.horizontal(|ui| {
                for (lens, label, hover) in [
                    (false, "Soft", "An even, soft blur"),
                    (true, "Lens", "Lights spread into discs, as with a camera's lens"),
                ] {
                    let response = ui
                        .add(egui::Button::selectable(self.draft.blur_lens == lens, label))
                        .on_hover_text(hover);
                    if response.clicked() && self.draft.blur_lens != lens {
                        let mut recipe = self.draft.clone();
                        recipe.blur_lens = lens;
                        self.commit(recipe);
                    }
                }
            });
        }
        match (&self.subject, self.draft.blur_kind) {
            (Subject::Unknown | Subject::Finding(_), BlurKind::Background) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Finding the subject…");
                });
            }
            (Subject::Failed(_), BlurKind::Background) => {
                ui.add(egui::Label::new("No subject found in this photo, so nothing is blurred.").wrap());
            }
            (_, kind) if kind.has_focus() => {
                let placing = self.placing_focus;
                let button = egui::Button::image_and_text(Icon::Crosshair.small(), "Place focus").selected(placing);
                if ui
                    .add(button)
                    .on_hover_text("Click the spot in the photo that should stay sharp")
                    .clicked()
                {
                    self.placing_focus = !placing;
                    self.picking = false;
                }
            }
            _ => {}
        }
    }

    /// The looks to pick from, and the chosen one's strength and grain (REQ-028-01 … REQ-028-03).
    fn looks(&mut self, ui: &mut egui::Ui) {
        let chosen = self.draft.look;
        ui.horizontal_wrapped(|ui| {
            let none = ui.add(egui::Button::selectable(chosen.is_none(), "None"));
            if none.clicked() && chosen.is_some() {
                let mut recipe = self.draft.clone();
                recipe.look = None;
                self.commit(recipe);
            }
            for look in Look::ALL {
                let selected = chosen.is_some_and(|choice| choice.look == look);
                if ui.add(egui::Button::selectable(selected, look.label())).clicked() && !selected {
                    let mut recipe = self.draft.clone();
                    // Another look keeps the strength and grain set for the last one.
                    recipe.look =
                        Some(chosen.map_or_else(|| LookChoice::new(look), |choice| LookChoice { look, ..choice }));
                    self.commit(recipe);
                }
            }
        });
        if chosen.is_some() {
            ui.add_space(4.0);
            self.look_slider(ui, "Strength", LookChoice::STRENGTH, |choice| &mut choice.strength);
            self.look_slider(ui, "Grain", LookChoice::GRAIN, |choice| &mut choice.grain);
        }
        self.luts_ui(ui);
    }

    /// The imported LUTs to pick from, _Import LUT…_, and the chosen one's strength (REQ-043-01 … REQ-043-03).
    fn luts_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("LUT")
                .size(theme::SIZE_CAPTION)
                .color(theme::muted()),
        );
        let chosen = self.draft.lut.clone();
        let entries = self.luts.entries();
        ui.horizontal_wrapped(|ui| {
            let none = ui.add(egui::Button::selectable(chosen.is_none(), "No LUT"));
            if none.clicked() && chosen.is_some() {
                let mut recipe = self.draft.clone();
                recipe.lut = None;
                self.commit(recipe);
            }
            for entry in &entries {
                let selected = chosen.as_ref().is_some_and(|choice| choice.id == entry.id);
                if ui.add(egui::Button::selectable(selected, &entry.name)).clicked() && !selected {
                    let mut recipe = self.draft.clone();
                    // Another LUT keeps the strength set for the last one.
                    recipe.lut = Some(domain::lut::LutChoice {
                        id: entry.id.clone(),
                        name: entry.name.clone(),
                        strength: chosen.as_ref().map_or(100.0, |choice| choice.strength),
                    });
                    self.commit(recipe);
                }
            }
            if ui.add(egui::Button::new("Import LUT…")).clicked() {
                self.import_lut = true;
            }
        });
        let Some(choice) = chosen else { return };
        if self.luts.table(&choice.id).is_none() {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!(
                        "{} isn't on this computer, so the photo shows without it. Import it to see it.",
                        choice.name
                    ))
                    .size(theme::SIZE_CAPTION)
                    .color(theme::muted()),
                )
                .wrap(),
            );
        }
        ui.add_space(4.0);
        let mut current = f64::from(choice.strength);
        let text = format!("{current:.0} %");
        let response = slider_row(
            ui,
            "LUT strength",
            &text,
            &mut current,
            domain::lut::LutChoice::STRENGTH,
            1.0,
        );
        let set = |session: &mut Self, strength: f32| {
            if let Some(lut) = &mut session.draft.lut {
                lut.strength = strength;
            }
        };
        if response.double_clicked() {
            set(self, 100.0);
            let recipe = self.draft.clone();
            self.commit(recipe);
            return;
        }
        if response.changed() {
            set(self, current as f32);
        }
        if response.drag_stopped() || response.clicked() || (response.changed() && !response.dragged()) {
            let recipe = self.draft.clone();
            self.commit(recipe);
        }
    }

    /// _Strength_ or _Grain_ of the chosen look, in percent; a double-click goes back to 100 %.
    fn look_slider(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        (low, high): (f32, f32),
        value: fn(&mut LookChoice) -> &mut f32,
    ) {
        let Some(mut choice) = self.draft.look else { return };
        let mut current = f64::from(*value(&mut choice));
        let text = format!("{current:.0} %");
        let response = slider_row(ui, label, &text, &mut current, (low, high), 1.0);
        if response.double_clicked() {
            *value(&mut choice) = 100.0;
            let mut recipe = self.draft.clone();
            recipe.look = Some(choice);
            self.commit(recipe);
            return;
        }
        if response.changed() {
            *value(&mut choice) = current as f32;
            self.draft.look = Some(choice);
        }
        if response.drag_stopped() || response.clicked() || (response.changed() && !response.dragged()) {
            let recipe = self.draft.clone();
            self.commit(recipe);
        }
    }

    /// One adjustment: its name and value, and a slider. A double-click resets it (REQ-021-07).
    fn slider(&mut self, ui: &mut egui::Ui, adjustment: Adjustment) {
        let part = self.part;
        let value = self
            .draft
            .part(part)
            .map_or(adjustment.neutral(), |recipe| adjustment.get(recipe));
        let text = if adjustment == Adjustment::Exposure {
            format!("{value:+.2}")
        } else if adjustment == Adjustment::Hue {
            if value == 0.0 {
                "0°".to_owned()
            } else {
                format!("{value:+.0}°")
            }
        } else if value == 0.0 || !matches!(adjustment.range().0, x if x < 0.0) {
            format!("{value:.0}")
        } else {
            format!("{value:+.0}")
        };
        let step = if adjustment == Adjustment::Exposure { 0.05 } else { 1.0 };
        let mut current = f64::from(value);
        let response = slider_row(ui, adjustment.label(), &text, &mut current, adjustment.range(), step);
        self.adjusting |= response.is_pointer_button_down_on();
        if response.double_clicked() {
            let mut recipe = self.draft.clone();
            adjustment.set(recipe.part_mut(part), adjustment.neutral());
            self.commit(recipe);
            return;
        }
        if response.changed() {
            adjustment.set(self.draft.part_mut(part), current as f32);
        }
        // One undo step per finished drag, click or key press.
        if response.drag_stopped() || response.clicked() || (response.changed() && !response.dragged()) {
            let recipe = self.draft.clone();
            self.commit(recipe);
        }
    }

    /// The tone curve editor (REQ-021-04): channel tabs, a square with the curve; a click adds a point, a drag moves
    /// it, a double-click on a point removes it.
    fn curves(&mut self, ui: &mut egui::Ui) {
        self.update_histogram();
        ui.horizontal(|ui| {
            for channel in Channel::ALL {
                let selected = self.channel == channel;
                let response = ui.add(egui::Button::selectable(selected, channel.label()));
                if response.clicked() {
                    self.channel = channel;
                }
            }
        });
        let side = (ui.available_width() - SCROLL_BAR_ROOM).min(256.0);
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), egui::Sense::click_and_drag());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Canvas, true, "Tone curve"));
        let to_screen = |[x, y]: [f32; 2]| pos2(rect.left() + x * rect.width(), rect.bottom() - y * rect.height());
        let from_screen = |pos: Pos2| {
            [
                ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
                ((rect.bottom() - pos.y) / rect.height()).clamp(0.0, 1.0),
            ]
        };
        let channel = self.channel;
        let near = |curve: &domain::edit::Curve, pos: Pos2| {
            curve
                .points
                .iter()
                .position(|&point| to_screen(point).distance(pos) <= 8.0)
        };
        if let Some(pos) = response.interact_pointer_pos() {
            let curve = self.draft.curves.get(channel).clone();
            if response.double_clicked() {
                if let Some(i) = near(&curve, pos) {
                    let mut recipe = self.draft.clone();
                    recipe.curves.get_mut(channel).remove_point(i);
                    self.commit(recipe);
                }
                self.dragging_point = None;
            } else if response.drag_started() || (response.is_pointer_button_down_on() && self.dragging_point.is_none())
            {
                self.dragging_point = near(&curve, pos).or_else(|| {
                    let [x, y] = from_screen(pos);
                    self.draft.curves.get_mut(channel).add_point(x, y)
                });
            }
            if let Some(i) = self.dragging_point
                && response.is_pointer_button_down_on()
            {
                let [x, y] = from_screen(pos);
                self.draft.curves.get_mut(channel).move_point(i, x, y);
            }
        }
        if (response.drag_stopped() || response.clicked()) && self.dragging_point.take().is_some() {
            let recipe = self.draft.clone();
            self.commit(recipe);
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, theme::field());
        let color = match channel {
            Channel::All => theme::text(),
            Channel::Red => Color32::from_rgb(240, 90, 90),
            Channel::Green => Color32::from_rgb(110, 210, 110),
            Channel::Blue => Color32::from_rgb(110, 150, 245),
        };
        if let Some(histogram) = self.histogram() {
            paint_histogram(&painter, rect, &histogram.get(channel), color.gamma_multiply(0.22));
        }
        for i in 1..4 {
            let t = i as f32 / 4.0;
            let stroke = egui::Stroke::new(1.0, theme::rim());
            painter.vline(rect.left() + t * rect.width(), rect.y_range(), stroke);
            painter.hline(rect.x_range(), rect.top() + t * rect.height(), stroke);
        }
        let curve = self.draft.curves.get(channel);
        let line: Vec<Pos2> = (0..=64)
            .map(|i| {
                let x = i as f32 / 64.0;
                to_screen([x, curve.eval(x)])
            })
            .collect();
        painter.add(egui::Shape::line(line, egui::Stroke::new(1.5, color)));
        for &point in &curve.points {
            painter.circle_filled(to_screen(point), 4.0, color);
        }
    }
}

/// A collapsible section's header; a click opens or closes it.
fn section_header(ui: &mut egui::Ui, label: &str, open: &mut bool) {
    let icon = if *open { Icon::ChevronDown } else { Icon::ChevronRight };
    // Without padding it sits where a frameless one did, and still shows the hover state (REQ-013-19).
    let header = ui
        .scope(|ui| {
            ui.spacing_mut().button_padding = Vec2::ZERO;
            ui.add(
                egui::Button::image_and_text(
                    icon.image(14.0, theme::muted()),
                    egui::RichText::new(label).font(egui::FontId::new(theme::SIZE_BODY, theme::semibold())),
                )
                .frame_when_inactive(false),
            )
        })
        .inner;
    let expanded = *open;
    let header = header.on_hover_text(if expanded { "Collapse" } else { "Expand" });
    header.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, expanded, label));
    if header.clicked() {
        *open = !*open;
    }
}

/// A slider's row: its name and `text` above, the slider below, as wide as the panel allows.
pub(super) fn slider_row(
    ui: &mut egui::Ui,
    label: &str,
    text: &str,
    value: &mut f64,
    (low, high): (f32, f32),
    step: f64,
) -> egui::Response {
    // Clear of the scroll bar, which floats over the content.
    let width = ui.available_width() - SCROLL_BAR_ROOM;
    let (row, _) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT + SLIDER_HEIGHT), egui::Sense::hover());
    let label_rect = Rect::from_min_size(row.min, vec2(width, ROW_HEIGHT));
    let painter = ui.painter();
    painter.text(
        label_rect.left_center(),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(theme::SIZE_BODY),
        theme::text(),
    );
    painter.text(
        label_rect.right_center(),
        egui::Align2::RIGHT_CENTER,
        text,
        egui::FontId::new(theme::SIZE_BODY, theme::numbers()),
        theme::muted(),
    );
    let track = Rect::from_min_size(pos2(row.left(), label_rect.bottom()), vec2(width, SLIDER_HEIGHT));
    crate::widgets::slider(ui, track, value, f64::from(low)..=f64::from(high), step, label)
}

/// The recipe's steps before the curves, which alone shape the histogram behind the curve.
pub(super) fn before_curves(recipe: &Recipe) -> Recipe {
    let mut before = Recipe::default();
    for adjustment in Adjustment::ALL {
        if matches!(adjustment.section(), Section::Light)
            || matches!(adjustment, Adjustment::Temperature | Adjustment::Tint)
        {
            adjustment.set(&mut before, adjustment.get(recipe));
        }
    }
    before
}

/// `bins` as bars from the bottom of `rect`, scaled to the tallest bin apart from the two ends, so clipped blacks or
/// whites don't flatten the rest.
fn paint_histogram(painter: &egui::Painter, rect: Rect, bins: &[u32; HISTOGRAM_BINS], color: Color32) {
    let tallest = bins[1..HISTOGRAM_BINS - 1].iter().copied().max().unwrap_or(0).max(1) as f32;
    let width = rect.width() / HISTOGRAM_BINS as f32;
    let mut mesh = egui::Mesh::default();
    for (i, &count) in bins.iter().enumerate() {
        let height = (count as f32 / tallest).min(1.0) * rect.height();
        if height <= 0.0 {
            continue;
        }
        let left = rect.left() + i as f32 * width;
        let bar = Rect::from_min_max(pos2(left, rect.bottom() - height), pos2(left + width, rect.bottom()));
        mesh.add_colored_rect(bar, color);
    }
    painter.add(mesh);
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, label: &str, tooltip: &str, enabled: bool) -> egui::Response {
    let response = ui
        .add_enabled(
            enabled,
            egui::Button::image(icon.small())
                .frame_when_inactive(false)
                .corner_radius(theme::CONTROL_HEIGHT / 2.0)
                .min_size(Vec2::splat(theme::CONTROL_HEIGHT)),
        )
        .on_hover_text(tooltip);
    let label = label.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &label));
    response
}

/// A new painting of `recipe`, and the shape that shows it.
fn new_painting(recipe: &mut Recipe, freeform: bool) -> MaskShape {
    recipe.paintings.push(Painting::default());
    MaskShape::Painted {
        layer: (recipe.paintings.len() - 1) as u8,
        freeform,
    }
}
