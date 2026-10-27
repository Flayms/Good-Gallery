//! The search bar in the toolbar (REQ-007-04 … -06): chips for the tags in the search and the file name filter, an
//! input, and tag suggestions under it while it has focus. A leading `-` excludes the picked tag.

use crate::gallery::Gallery;
use crate::icons::Icon;
use crate::{format, theme};
use domain::search::TagMode;
use domain::tags::normalize_tag;
use eframe::egui::{self, Color32, Rect, Vec2, pos2, vec2};

/// Suggestions shown at most.
const SUGGESTIONS: usize = 10;
const MAX_WIDTH: f32 = 576.0;
pub const LABEL: &str = "Search tags or file names";

/// The layer of the suggestions, which menus don't count as their own overlays.
pub fn suggestions_id() -> egui::Id {
    egui::Id::NULL.with("search suggestions")
}

/// What the search bar remembers between frames.
#[derive(Debug, Default)]
pub struct SearchBar {
    pub query: String,
    /// The highlighted suggestion (arrow keys, Enter).
    highlighted: usize,
    /// The suggestions asked for, so they are asked for once per input.
    requested: Option<(String, usize)>,
    /// Where the suggestions were last drawn: a click there keeps them open.
    popup: Option<Rect>,
}

/// A suggestion: a tag with its count, or the file name filter.
enum Choice {
    Tag(String),
    FileName(String),
}

/// Draws the search bar into `ui`'s remaining width.
pub fn show(ui: &mut egui::Ui, bar: &mut SearchBar, gallery: &mut Gallery) {
    let search = gallery.search().clone();
    let width = ui.available_width().min(MAX_WIDTH);
    let (field, _) = ui.allocate_exact_size(vec2(width, theme::CONTROL_HEIGHT), egui::Sense::hover());
    ui.painter().rect_filled(field, theme::RADIUS_CONTROL, theme::FIELD);
    let mut next = search.clone();
    let mut input = None;
    let was_empty = bar.query.is_empty();
    let inner = Rect::from_min_max(field.min + vec2(8.0, 0.0), field.max - vec2(8.0, 0.0));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add(Icon::Search.image(theme::ICON_SIZE, theme::MIST));
            let chips: Vec<(String, TagMode)> = search
                .tags
                .iter()
                .map(|name| (name.clone(), TagMode::Include))
                .chain(search.exclude.iter().map(|name| (name.clone(), TagMode::Exclude)))
                .collect();
            for (name, mode) in &chips {
                match tag_chip(ui, name, *mode) {
                    Some(ChipAction::Toggle) => {
                        let other = if *mode == TagMode::Include {
                            TagMode::Exclude
                        } else {
                            TagMode::Include
                        };
                        next.with_tag(name, other);
                    }
                    Some(ChipAction::Remove) => next.without_tag(name),
                    None => {}
                }
            }
            if let Some(name) = &search.name
                && file_name_chip(ui, name)
            {
                next.name = None;
            }
            let hint = if chips.is_empty() && search.name.is_none() {
                LABEL
            } else {
                ""
            };
            let edit = egui::TextEdit::singleline(&mut bar.query)
                .id_salt("search bar")
                .frame(egui::Frame::NONE)
                .hint_text(hint)
                .align(egui::Align2::LEFT_CENTER)
                .margin(egui::Margin::symmetric(0, 6))
                .desired_width(ui.available_width().max(96.0));
            let response = ui.add(edit);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, LABEL));
            input = Some(response);
        },
    );
    let Some(response) = input else { return };

    // The suggestions live in their own layer, outside the toolbar's clip rect: test the pointer directly.
    let pointer = ui.ctx().pointer_latest_pos();
    let pointer_in_popup = bar
        .popup
        .zip(pointer)
        .is_some_and(|(popup, pointer)| popup.contains(pointer));
    // Enter and Escape make the input give up focus in the frame they are pressed; they are handled below.
    let open = response.has_focus() || response.lost_focus() || (pointer_in_popup && bar.requested.is_some());
    let mode = if bar.query.starts_with('-') {
        TagMode::Exclude
    } else {
        TagMode::Include
    };
    let prefix = if mode == TagMode::Exclude {
        &bar.query[1..]
    } else {
        bar.query.as_str()
    }
    .to_string();
    if !open {
        bar.requested = None;
        bar.popup = None;
        bar.highlighted = 0;
        gallery.set_search(next);
        return;
    }

    // Tags in the search stay in the list, so ask for enough extra to still fill it.
    let limit = SUGGESTIONS + search.tags.len() + search.exclude.len();
    if bar.requested.as_ref() != Some(&(prefix.clone(), limit)) {
        gallery.request_suggestions(&prefix, limit);
        bar.requested = Some((prefix.clone(), limit));
        bar.highlighted = 0;
    }
    let selected: Vec<String> = search
        .tags
        .iter()
        .chain(&search.exclude)
        .map(|name| normalize_tag(name))
        .collect();
    // Only answers for the current input: an older one could put the wrong tag under Enter.
    let (loaded, tags) = match gallery.suggestions() {
        Some((asked, tags)) if asked == prefix => (true, tags),
        _ => (false, &[][..]),
    };
    let tags: Vec<(String, u64)> = tags
        .iter()
        .filter(|tag| !selected.contains(&normalize_tag(&tag.name)))
        .take(SUGGESTIONS)
        .map(|tag| (tag.name.clone(), tag.count))
        .collect();
    // Exclusions only exist for tags.
    let file_name = bar.query.trim().to_string();
    let mut choices: Vec<Choice> = tags.iter().map(|(name, _)| Choice::Tag(name.clone())).collect();
    if mode == TagMode::Include && !file_name.is_empty() {
        choices.push(Choice::FileName(file_name.clone()));
    }
    bar.highlighted = bar.highlighted.min(choices.len().saturating_sub(1));

    // Keys while the input has focus (REQ-007-05).
    let (down, up, enter, escape, backspace) = ui.input(|input| {
        (
            input.key_pressed(egui::Key::ArrowDown),
            input.key_pressed(egui::Key::ArrowUp),
            input.key_pressed(egui::Key::Enter),
            input.key_pressed(egui::Key::Escape),
            input.key_pressed(egui::Key::Backspace),
        )
    });
    let mut picked = None;
    if down && !choices.is_empty() {
        bar.highlighted = (bar.highlighted + 1) % choices.len();
    }
    if up && !choices.is_empty() {
        bar.highlighted = (bar.highlighted + choices.len() - 1) % choices.len();
    }
    if enter && !choices.is_empty() {
        picked = Some(bar.highlighted);
    }
    if backspace && was_empty {
        // The file name filter first, then the last tag.
        if next.name.is_some() {
            next.name = None;
        } else if let Some(last) = next.exclude.last().or(next.tags.last()).cloned() {
            next.without_tag(&last);
        }
    }
    let mut keep_focus = false;
    if escape {
        if bar.query.is_empty() {
            response.surrender_focus();
        } else {
            bar.query.clear();
            keep_focus = true;
        }
    }

    let width = field.width().max(256.0);
    let area = egui::Area::new(suggestions_id())
        .order(egui::Order::Foreground)
        .fixed_pos(field.left_bottom() + vec2(0.0, 4.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(theme::RAISED)
                .corner_radius(theme::RADIUS_PANEL)
                .inner_margin(4.0)
                .show(ui, |ui| {
                    ui.set_width(width - 8.0);
                    if loaded && choices.is_empty() {
                        ui.label(egui::RichText::new("No matching tags").color(theme::MIST));
                    }
                    for (index, choice) in choices.iter().enumerate() {
                        let count = match choice {
                            Choice::Tag(name) => tags.iter().find(|(n, _)| n == name).map(|(_, c)| *c),
                            Choice::FileName(_) => None,
                        };
                        if suggestion_row(ui, choice, count, mode, index == bar.highlighted).clicked() {
                            picked = Some(index);
                        }
                    }
                    ui.label(
                        egui::RichText::new("Start with - to exclude a tag")
                            .size(theme::SIZE_CAPTION)
                            .color(theme::MIST),
                    );
                });
        });
    bar.popup = Some(area.response.rect);

    if let Some(index) = picked.filter(|&index| index < choices.len()) {
        match &choices[index] {
            Choice::Tag(name) => next.with_tag(name, mode),
            Choice::FileName(name) => next.name = Some(name.clone()),
        }
        bar.query.clear();
        // The suggestions stay open for the next tag.
        keep_focus = true;
    }
    if keep_focus {
        response.request_focus();
    }
    gallery.set_search(next);
}

enum ChipAction {
    Toggle,
    Remove,
}

/// A tag in the search: its name switches between included and excluded (struck through), × removes it.
fn tag_chip(ui: &mut egui::Ui, name: &str, mode: TagMode) -> Option<ChipAction> {
    let excluded = mode == TagMode::Exclude;
    let (icon, text, fill) = if excluded {
        (
            Icon::Minus,
            egui::RichText::new(name).strikethrough(),
            Color32::from_rgba_unmultiplied(242, 112, 106, 40),
        )
    } else {
        (Icon::Tag, egui::RichText::new(name), theme::SELECTED)
    };
    let mut action = None;
    egui::Frame::new()
        .fill(fill)
        .corner_radius(theme::RADIUS_CONTROL)
        .inner_margin(egui::Margin::symmetric(4, 1))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let label = if excluded {
                format!("{name}, excluded")
            } else {
                format!("{name}, included")
            };
            let toggle = ui
                .add(egui::Button::new((icon.image(12.0, theme::PAPER), text.size(theme::SIZE_CAPTION))).frame(false))
                .on_hover_text(if excluded {
                    "Excluded – click to include"
                } else {
                    "Included – click to exclude"
                });
            toggle.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
            if toggle.clicked() {
                action = Some(ChipAction::Toggle);
            }
            if remove_button(ui, &format!("Remove {name}")) {
                action = Some(ChipAction::Remove);
            }
        });
    action
}

/// The file name filter as a chip; true when its × was clicked.
fn file_name_chip(ui: &mut egui::Ui, name: &str) -> bool {
    let mut removed = false;
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, theme::RIM))
        .corner_radius(theme::RADIUS_CONTROL)
        .inner_margin(egui::Margin::symmetric(4, 1))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add(Icon::File.image(12.0, theme::PAPER));
            ui.label(egui::RichText::new(name).size(theme::SIZE_CAPTION))
                .on_hover_text("File name contains");
            removed = remove_button(ui, &format!("Remove file name filter {name}"));
        });
    removed
}

fn remove_button(ui: &mut egui::Ui, label: &str) -> bool {
    let response = ui.add(egui::Button::image(Icon::X.image(12.0, theme::MIST)).frame(false));
    let label = label.to_string();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    response.clicked()
}

/// A suggestion: + or − and the tag with its count, or the file name filter (REQ-007-04, -06).
fn suggestion_row(
    ui: &mut egui::Ui,
    choice: &Choice,
    count: Option<u64>,
    mode: TagMode,
    highlighted: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), egui::Sense::click());
    if highlighted || response.hovered() {
        ui.painter().rect_filled(
            rect,
            theme::RADIUS_CONTROL,
            if highlighted { theme::SELECTED } else { theme::HOVER },
        );
    }
    let (icon, color, text) = match choice {
        Choice::Tag(name) if mode == TagMode::Exclude => (Icon::Minus, theme::ALERT, name.clone()),
        Choice::Tag(name) => (Icon::Plus, theme::PAPER, name.clone()),
        Choice::FileName(name) => (Icon::FileSearch, theme::PAPER, format!("File name contains “{name}”")),
    };
    let icon_rect = Rect::from_min_size(pos2(rect.left() + 8.0, rect.center().y - 8.0), Vec2::splat(16.0));
    icon.image(16.0, color).paint_at(ui, icon_rect);
    let font = egui::FontId::new(theme::SIZE_BODY, egui::FontFamily::Proportional);
    let galley = ui.painter().layout_no_wrap(text.clone(), font, theme::PAPER);
    ui.painter().galley(
        pos2(icon_rect.right() + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        theme::PAPER,
    );
    if let Some(count) = count {
        ui.painter().text(
            pos2(rect.right() - 8.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            format::count(count),
            egui::FontId::new(theme::SIZE_CAPTION, theme::numbers()),
            theme::MIST,
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ListItem, true, &text));
    response
}
