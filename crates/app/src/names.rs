//! Suggestions of known people below a field for a face's name, picked with a click or the arrow keys and Enter
//! (REQ-032-34).

use crate::theme;
use eframe::egui::{self, Key, Modifiers, RichText};

/// Suggestions shown at most.
const SHOWN: usize = 6;

/// The field's state between frames.
#[derive(Clone, Copy, Default)]
struct Shown {
    /// The suggestion the arrow keys moved to.
    at: Option<usize>,
    /// Whether the suggestions showed last frame: pressing one takes the focus away before its click.
    open: bool,
}

/// The known people's names whose name contains `typed`, those starting with it first (REQ-032-34).
pub fn matching<'a>(people: &'a [Vec<String>], typed: &str) -> Vec<&'a str> {
    if domain::tags::clean_tag(typed).is_empty() {
        return Vec::new();
    }
    let mut names: Vec<&str> = Vec::new();
    for path in domain::tagging::suggest(people, typed, &[], usize::MAX) {
        let Some(name) = path.last() else { continue };
        // People under a parent tag (`People|Family|Ben`) are named by their own name.
        if path.len() > 1 && !names.iter().any(|known| known.eq_ignore_ascii_case(name)) {
            names.push(name);
        }
        if names.len() == SHOWN {
            break;
        }
    }
    names
}

/// A field for a face's name with the id `id`. The arrow keys move through its suggestions (REQ-032-34).
pub fn field(ui: &mut egui::Ui, id: egui::Id, name: &mut String, hint: &str, width: f32) -> egui::Response {
    if ui.memory(|memory| memory.has_focus(id)) {
        let (down, up) = ui.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
            )
        });
        if down || up {
            let mut shown: Shown = ui.data(|data| data.get_temp(id).unwrap_or_default());
            shown.at = match (shown.at, down) {
                (None, true) => Some(0),
                (None, false) => None,
                (Some(at), true) => Some(at + 1),
                (Some(at), false) => at.checked_sub(1),
            };
            ui.data_mut(|data| data.insert_temp(id, shown));
        }
    }
    let before = name.clone();
    let response = ui.add(
        egui::TextEdit::singleline(name)
            .id(id)
            .hint_text(hint)
            .desired_width(width),
    );
    if *name != before {
        ui.data_mut(|data| data.insert_temp(id, Shown { at: None, open: true }));
    }
    response
}

/// The suggestions below the field `id` while it has the focus, and the name to write: a suggestion clicked, or Enter
/// on the field, with the suggestion moved to or else what was typed (REQ-032-34).
pub fn suggestions(
    ui: &mut egui::Ui,
    id: egui::Id,
    field: &egui::Response,
    name: &str,
    people: &[Vec<String>],
) -> Option<String> {
    let mut shown: Shown = ui.data(|data| data.get_temp(id).unwrap_or_default());
    let names = matching(people, name);
    let at = shown.at.map(|at| at.min(names.len().saturating_sub(1)));
    let enter = field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
    let mut chosen = None;
    let pointer = ui.input(|input| input.pointer.any_down() || input.pointer.any_released());
    shown.open = !names.is_empty() && (field.has_focus() || shown.open && pointer);
    if shown.open {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (index, known) in names.iter().enumerate() {
                let text = RichText::new(*known).size(theme::SIZE_CAPTION);
                let button = egui::Button::new(text)
                    .selected(at == Some(index))
                    .stroke(egui::Stroke::new(1.0, theme::rim()))
                    .corner_radius(theme::RADIUS_CONTROL);
                let response = ui.add(button);
                let spoken = format!("Suggested name {known}");
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &spoken));
                if response.clicked() {
                    chosen = Some((*known).to_owned());
                }
            }
        });
    }
    if enter {
        chosen = match at.and_then(|at| names.get(at)) {
            Some(known) => Some((*known).to_owned()),
            None => clean(name),
        };
    }
    if chosen.is_some() {
        shown = Shown::default();
    } else {
        shown.at = at;
    }
    ui.data_mut(|data| data.insert_temp(id, shown));
    chosen
}

/// A typed name without surrounding or repeated spaces, if anything is left.
pub fn clean(name: &str) -> Option<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn people(names: &[&str]) -> Vec<Vec<String>> {
        names
            .iter()
            .map(|path| {
                std::iter::once("People")
                    .chain(path.split('|'))
                    .map(String::from)
                    .collect()
            })
            .collect()
    }

    /// REQ-032-34: known people containing what was typed, those starting with it first, by their own name.
    #[test]
    fn req_032_34_suggests_known_people_starting_with_what_was_typed_first() {
        let known = people(&["Anna", "Johanna", "Family", "Family|Annabel", "Ben"]);
        assert_eq!(matching(&known, "ann"), ["Anna", "Annabel", "Johanna"]);
        assert_eq!(matching(&known, "  "), Vec::<&str>::new());
        assert_eq!(matching(&known, "zed"), Vec::<&str>::new());
        let many: Vec<String> = (0..10).map(|i| format!("Ann {i}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert_eq!(matching(&people(&many), "ann").len(), SHOWN);
    }
}
