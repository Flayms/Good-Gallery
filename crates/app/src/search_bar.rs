//! The search bar in the toolbar (REQ-007-04 … -06): chips for the tags in the search and the file name filter, an
//! input, and tag suggestions under it while it has focus. A leading `-` excludes the picked tag.

use crate::gallery::Gallery;
use crate::icons::Icon;
use crate::{format, theme};
use domain::search::TagMode;
use domain::tags::normalize_tag;
use eframe::egui::{self, Color32, FontId, Rect, Vec2, pos2, vec2};

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

/// A suggestion: a tag with its count, the file name filter, or what the photos show (REQ-036-05).
enum Choice {
    Tag(String),
    FileName(String),
    Content(String),
}

/// Draws the search bar into `ui`'s remaining width, less `reserve` for what follows it.
pub fn show(ui: &mut egui::Ui, bar: &mut SearchBar, gallery: &mut Gallery, reserve: f32) {
    let search = gallery.search().clone();
    let width = (ui.available_width() - reserve).clamp(0.0, MAX_WIDTH);
    let (field, _) = ui.allocate_exact_size(vec2(width, theme::CONTROL_HEIGHT), egui::Sense::hover());
    ui.painter().rect_filled(field, theme::RADIUS_CONTROL, theme::field());
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
            ui.add(Icon::Search.image(theme::ICON_SIZE, theme::muted()));
            let tags: Vec<(String, TagMode)> = search
                .tags
                .iter()
                .map(|name| (name.clone(), TagMode::Include))
                .chain(search.exclude.iter().map(|name| (name.clone(), TagMode::Exclude)))
                .collect();
            let mut chips: Vec<(Chip, ChipFor)> = tags
                .iter()
                .map(|(name, mode)| (Chip::tag(name, *mode), ChipFor::Tag(name.clone(), *mode)))
                .collect();
            if let Some(name) = &search.name {
                chips.push((Chip::file_name(name), ChipFor::FileName));
            }
            // Searching while the model reads the words (REQ-036-05).
            if let Some(words) = &search.content {
                chips.push((Chip::content(words, gallery.is_loading()), ChipFor::Content));
            }
            // A smart album's rules, as one chip with its name (REQ-039-05).
            if let Some(rules) = &search.rules {
                chips.push((Chip::rules(&rules.name), ChipFor::Rules));
            }
            // Photos chosen on the map (REQ-044-03).
            if !search.media.is_empty() {
                chips.push((Chip::map(search.media.len()), ChipFor::Map));
            }
            let hint = if chips.is_empty() { LABEL } else { "" };
            // As many chips as fit before the room to type; the rest are counted in a last chip (REQ-007-15).
            let room = ui.available_width() - MIN_INPUT;
            let shown = fitting(ui, &chips, room);
            for (index, (chip, target)) in chips.iter().enumerate().take(shown) {
                match chip.show(ui, index) {
                    Some(ChipAction::Toggle) => {
                        if let ChipFor::Tag(name, mode) = target {
                            let other = if *mode == TagMode::Include {
                                TagMode::Exclude
                            } else {
                                TagMode::Include
                            };
                            next.with_tag(name, other);
                        }
                    }
                    Some(ChipAction::Remove) => match target {
                        ChipFor::Tag(name, _) => next.without_tag(name),
                        ChipFor::FileName => next.name = None,
                        ChipFor::Content => next.content = None,
                        ChipFor::Rules => next.rules = None,
                        ChipFor::Map => next.media.clear(),
                    },
                    None => {}
                }
            }
            if shown < chips.len() {
                more_chip(ui, &chips[shown..]);
            }
            let edit = egui::TextEdit::singleline(&mut bar.query)
                .id_salt("search bar")
                .frame(egui::Frame::NONE)
                .hint_text(hint)
                .align(egui::Align2::LEFT_CENTER)
                .margin(egui::Margin::symmetric(0, 6))
                .desired_width(ui.available_width().max(0.0));
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
    let mut choices: Vec<Choice> = Vec::new();
    // What the photos show comes first while content search is on (REQ-036-05).
    if mode == TagMode::Include && !file_name.is_empty() && gallery.content_search_on() {
        choices.push(Choice::Content(file_name.clone()));
    }
    choices.extend(tags.iter().map(|(name, _)| Choice::Tag(name.clone())));
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
        // The file name filter first, then the content words, then the last tag.
        if next.name.is_some() {
            next.name = None;
        } else if next.content.is_some() {
            next.content = None;
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
                .fill(theme::raised())
                .corner_radius(theme::RADIUS_PANEL)
                .inner_margin(4.0)
                .show(ui, |ui| {
                    crate::glass::behind_frame(ui, egui::Margin::same(4), |ui| {
                        ui.set_width(width - 8.0);
                        if loaded && tags.is_empty() && choices.is_empty() {
                            ui.label(egui::RichText::new("No matching tags").color(theme::muted()));
                        }
                        for (index, choice) in choices.iter().enumerate() {
                            let count = match choice {
                                Choice::Tag(name) => tags.iter().find(|(n, _)| n == name).map(|(_, c)| *c),
                                Choice::FileName(_) | Choice::Content(_) => None,
                            };
                            if suggestion_row(ui, choice, count, mode, index == bar.highlighted).clicked() {
                                picked = Some(index);
                            }
                        }
                        ui.label(
                            egui::RichText::new("Start with - to exclude a tag")
                                .size(theme::SIZE_CAPTION)
                                .color(theme::muted()),
                        );
                    });
                });
        });
    bar.popup = Some(area.response.rect);

    if let Some(index) = picked.filter(|&index| index < choices.len()) {
        match &choices[index] {
            Choice::Tag(name) => next.with_tag(name, mode),
            Choice::FileName(name) => next.name = Some(name.clone()),
            // One content chip: another replaces it (REQ-036-05).
            Choice::Content(words) => next.content = Some(words.clone()),
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

/// Every chip's height, and the room kept to type beside them (REQ-007-15).
const CHIP_HEIGHT: f32 = 22.0;
const MIN_INPUT: f32 = 80.0;
/// The longest a chip's name gets before it is shortened with "…".
const MAX_CHIP_TEXT: f32 = 140.0;
const CHIP_PAD: f32 = 6.0;
const CHIP_ICON: f32 = 12.0;
/// The square of a chip's ×.
const CHIP_REMOVE: f32 = 18.0;
const CHIP_GAP: f32 = 4.0;

/// What a chip in the search stands for, to change the search when it is clicked.
enum ChipFor {
    Tag(String, TagMode),
    FileName,
    Content,
    Rules,
    Map,
}

/// A chip in the search bar: an icon (a spinner while searching), its text, and ×. Tags are filled, excluded ones
/// struck through on red, the other filters outlined. All are one height (REQ-007-15).
struct Chip {
    /// `None`: a spinner.
    icon: Option<Icon>,
    text: String,
    excluded: bool,
    filled: bool,
    /// A click on the text switches it (tags, REQ-007-05).
    toggles: bool,
    /// How screen readers and tests name its text.
    spoken: String,
    tip: &'static str,
    remove: String,
}

impl Chip {
    fn tag(name: &str, mode: TagMode) -> Self {
        let excluded = mode == TagMode::Exclude;
        Self {
            icon: Some(if excluded { Icon::Minus } else { Icon::Tag }),
            text: name.to_owned(),
            excluded,
            filled: true,
            toggles: true,
            spoken: format!("{name}, {}", if excluded { "excluded" } else { "included" }),
            tip: if excluded {
                "Excluded – click to include"
            } else {
                "Included – click to exclude"
            },
            remove: format!("Remove {name}"),
        }
    }

    fn outlined(icon: Option<Icon>, text: &str, spoken: String, tip: &'static str, remove: String) -> Self {
        Self {
            icon,
            text: text.to_owned(),
            excluded: false,
            filled: false,
            toggles: false,
            spoken,
            tip,
            remove,
        }
    }

    fn file_name(name: &str) -> Self {
        let remove = format!("Remove file name filter {name}");
        Self::outlined(Some(Icon::File), name, name.to_owned(), "File name contains", remove)
    }

    fn content(words: &str, searching: bool) -> Self {
        let icon = (!searching).then_some(Icon::Sparkles);
        let (spoken, remove) = (
            format!("Photos showing {words}"),
            format!("Remove photos showing {words}"),
        );
        Self::outlined(icon, words, spoken, "Photos showing this", remove)
    }

    fn rules(name: &str) -> Self {
        let remove = format!("Remove smart album {name}");
        Self::outlined(Some(Icon::Search), name, name.to_owned(), "Smart album", remove)
    }

    fn map(count: usize) -> Self {
        let label = if count == 1 {
            "1 photo from the map".to_owned()
        } else {
            format!("{} photos from the map", crate::format::count(count as u64))
        };
        Self::outlined(Some(Icon::Map), &label, label.clone(), "", format!("Remove {label}"))
    }

    fn galley(&self, ui: &egui::Ui) -> std::sync::Arc<egui::Galley> {
        let mut format = egui::TextFormat::simple(FontId::proportional(theme::SIZE_CAPTION), theme::text());
        if self.excluded {
            format.strikethrough = egui::Stroke::new(1.0, theme::text());
        }
        let mut job = egui::text::LayoutJob::single_section(self.text.clone(), format);
        job.wrap = egui::text::TextWrapping::truncate_at_width(MAX_CHIP_TEXT);
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    }

    fn width(&self, ui: &egui::Ui) -> f32 {
        chip_width(self.galley(ui).size().x)
    }

    fn show(&self, ui: &mut egui::Ui, index: usize) -> Option<ChipAction> {
        let galley = self.galley(ui);
        let (rect, _) = ui.allocate_exact_size(vec2(chip_width(galley.size().x), CHIP_HEIGHT), egui::Sense::hover());
        let body = Rect::from_min_max(rect.min, pos2(rect.right() - CHIP_REMOVE - 2.0, rect.bottom()));
        let cross = Rect::from_center_size(
            pos2(rect.right() - 2.0 - CHIP_REMOVE / 2.0, rect.center().y),
            Vec2::splat(CHIP_REMOVE),
        );
        let id = ui.scope_id().with(("chip", index));
        let sense = if self.toggles {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let mut text_response = ui.interact(body, id.with("text"), sense);
        let tip = if galley.elided {
            if self.tip.is_empty() {
                self.text.clone()
            } else {
                format!("{}\n{}", self.text, self.tip)
            }
        } else {
            self.tip.to_owned()
        };
        if !tip.is_empty() {
            text_response = text_response.on_hover_text(tip);
        }
        let role = if self.toggles {
            egui::Role::Button
        } else {
            egui::Role::Label
        };
        text_response.widget_info(|| egui::WidgetInfo::labeled(role, true, &self.spoken));
        let remove_response = ui.interact(cross, id.with("remove"), egui::Sense::click());
        remove_response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &self.remove));

        let painter = ui.painter();
        let radius = theme::RADIUS_CONTROL;
        if self.excluded {
            // Alert at 16 %.
            let alert = theme::alert();
            painter.rect_filled(
                rect,
                radius,
                Color32::from_rgba_unmultiplied(alert.r(), alert.g(), alert.b(), 40),
            );
        } else if self.filled {
            painter.rect_filled(rect, radius, theme::selected());
        } else {
            painter.rect_stroke(
                rect,
                radius,
                egui::Stroke::new(1.0, theme::rim()),
                egui::StrokeKind::Inside,
            );
        }
        if self.toggles && text_response.hovered() {
            painter.rect_filled(body, radius, theme::hover());
        }
        if remove_response.hovered() {
            painter.rect_filled(cross.shrink(1.0), radius, theme::hover());
        }
        let icon_rect = Rect::from_center_size(
            pos2(rect.left() + CHIP_PAD + CHIP_ICON / 2.0, rect.center().y),
            Vec2::splat(CHIP_ICON),
        );
        match self.icon {
            Some(icon) => icon.image(CHIP_ICON, theme::text()).paint_at(ui, icon_rect),
            None => egui::Spinner::new().size(CHIP_ICON).paint_at(ui, icon_rect),
        }
        let text_at = pos2(icon_rect.right() + 4.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(text_at, galley, theme::text());
        Icon::X
            .image(CHIP_ICON, theme::muted())
            .paint_at(ui, Rect::from_center_size(cross.center(), Vec2::splat(CHIP_ICON)));

        if remove_response.clicked() {
            Some(ChipAction::Remove)
        } else if self.toggles && text_response.clicked() {
            Some(ChipAction::Toggle)
        } else {
            None
        }
    }
}

/// A chip's width for a text this wide.
fn chip_width(text: f32) -> f32 {
    CHIP_PAD + CHIP_ICON + 4.0 + text + 2.0 + CHIP_REMOVE + 2.0
}

/// How many of `chips` fit in `room`, with a "+N" chip after them if not all do (REQ-007-15).
fn fitting(ui: &egui::Ui, chips: &[(Chip, ChipFor)], room: f32) -> usize {
    let widths: Vec<f32> = chips.iter().map(|(chip, _)| chip.width(ui) + CHIP_GAP).collect();
    if widths.iter().sum::<f32>() <= room {
        return chips.len();
    }
    let more = MORE_WIDTH + CHIP_GAP;
    let mut used = 0.0;
    widths
        .iter()
        .take_while(|width| {
            used += **width;
            used + more <= room
        })
        .count()
}

/// Room for the "+N" chip.
const MORE_WIDTH: f32 = 36.0;

/// The chips that didn't fit, counted; their names on hover (REQ-007-15).
fn more_chip(ui: &mut egui::Ui, hidden: &[(Chip, ChipFor)]) {
    let (rect, response) = ui.allocate_exact_size(vec2(MORE_WIDTH, CHIP_HEIGHT), egui::Sense::hover());
    ui.painter().rect_stroke(
        rect,
        theme::RADIUS_CONTROL,
        egui::Stroke::new(1.0, theme::rim()),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        format!("+{}", hidden.len()),
        FontId::proportional(theme::SIZE_CAPTION),
        theme::text(),
    );
    let names: Vec<&str> = hidden.iter().map(|(chip, _)| chip.spoken.as_str()).collect();
    let spoken = format!("{} more", hidden.len());
    let response = response.on_hover_text(names.join("\n"));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &spoken));
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
            if highlighted { theme::selected() } else { theme::hover() },
        );
    }
    let (icon, color, text) = match choice {
        Choice::Tag(name) if mode == TagMode::Exclude => (Icon::Minus, theme::alert(), name.clone()),
        Choice::Tag(name) => (Icon::Plus, theme::text(), name.clone()),
        Choice::FileName(name) => (Icon::FileSearch, theme::text(), format!("File name contains “{name}”")),
        Choice::Content(words) => (Icon::Sparkles, theme::text(), format!("Photos showing “{words}”")),
    };
    let icon_rect = Rect::from_min_size(pos2(rect.left() + 8.0, rect.center().y - 8.0), Vec2::splat(16.0));
    icon.image(16.0, color).paint_at(ui, icon_rect);
    let font = egui::FontId::new(theme::SIZE_BODY, egui::FontFamily::Proportional);
    let galley = ui.painter().layout_no_wrap(text.clone(), font, theme::text());
    ui.painter().galley(
        pos2(icon_rect.right() + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        theme::text(),
    );
    if let Some(count) = count {
        ui.painter().text(
            pos2(rect.right() - 8.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            format::count(count),
            egui::FontId::new(theme::SIZE_CAPTION, theme::numbers()),
            theme::muted(),
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ListItem, true, &text));
    response
}
