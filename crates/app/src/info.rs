//! A media item's details (REQ-009-08): file, location, library, dates, dimensions, duration, size, rating and its tags
//! grouped into People, Places and Tags. Shown in the viewer's info panel and in the info sheet beside the gallery.
//! A field the item has no value for keeps its title and place, with nothing under it.

use crate::format;
use crate::gallery::{PLACES_TAG_ROOT, TagPaths};
use crate::icons;
use crate::icons::Icon;
use crate::theme;
use db::faces::FaceScan;
use db::media::MediaDetail;
use domain::media::MediaKind;
use domain::tags::{PEOPLE_TAG_ROOT, TagCategory};
use eframe::egui;

/// Width of the info panel and sheet.
pub const WIDTH: f32 = 320.0;

/// What a click in the details asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InfoAction {
    /// Filter the gallery by this tag (REQ-009-09).
    FilterByTag(String),
    /// Put the unedited original back (REQ-021-12).
    Revert,
    /// Remove the photo's earlier versions (REQ-021-26).
    ClearHistory,
    /// The user clicked the star for this many stars (REQ-024-02).
    Rate(i64),
    /// Add the tag with this path (REQ-026-01).
    AddTag(Vec<String>),
    /// Remove the tag with this name (REQ-026-02).
    RemoveTag(String),
    /// Look for faces in this photo again (REQ-032-22).
    LookForFacesAgain,
}

/// Suggestions shown at most while adding a tag (REQ-026-01).
const SUGGESTIONS: usize = 6;

/// The details as loaded: a spinner while loading, "Media not found" or the error; what was clicked as [`show`].
pub fn show_loaded(
    ui: &mut egui::Ui,
    detail: Option<&crate::gallery::DetailResult>,
    paths: &TagPaths,
    writable: bool,
) -> Option<InfoAction> {
    match detail {
        Some(Ok(Some(detail))) => return show(ui, detail, paths, writable),
        Some(Ok(None)) => {
            ui.label("Media not found");
        }
        Some(Err(error)) => {
            ui.label(format!("Couldn't load the details: {error}"));
        }
        None => {
            ui.spinner();
        }
    }
    None
}

/// Shows `detail`; returns a tag the user clicked, to filter the gallery by it (REQ-009-09), or _Revert to original_
/// (REQ-021-12).
pub fn show(ui: &mut egui::Ui, detail: &MediaDetail, paths: &TagPaths, writable: bool) -> Option<InfoAction> {
    let mut clicked = None;
    ui.spacing_mut().item_spacing.y = 12.0;
    field(ui, "File", |ui| {
        ui.add(egui::Label::new(&detail.file_name).wrap());
    });
    field(ui, "Location", |ui| {
        // Selectable, so the path can be copied.
        ui.add(
            egui::Label::new(detail.path.display().to_string())
                .wrap()
                .selectable(true),
        );
    });
    field(ui, "Library", |ui| {
        ui.label(&detail.library_label);
    });
    field(ui, "Taken", |ui| match detail.taken_at {
        Some(taken_at) => {
            ui.label(format::medium_date_time(taken_at));
        }
        None => empty(ui, "Unknown"),
    });
    field(ui, "Modified", |ui| {
        ui.label(format::medium_date_time(detail.mtime));
    });
    field(ui, "Dimensions", |ui| match (detail.width, detail.height) {
        (Some(width), Some(height)) => {
            ui.label(format!("{width} × {height}"));
        }
        _ => empty(ui, "Unknown"),
    });
    // Only videos have a duration.
    if detail.kind == MediaKind::Video {
        field(ui, "Duration", |ui| match detail.duration {
            Some(duration) => {
                ui.label(format::duration(duration));
            }
            None => empty(ui, "Unknown"),
        });
    }
    field(ui, "Size", |ui| {
        ui.label(format::bytes(detail.size));
    });
    // What it was taken with and where, only when known (REQ-040-04). _Location_ is the file's path.
    if let Some(camera) = &detail.camera {
        field(ui, "Camera", |ui| {
            ui.add(egui::Label::new(camera).wrap());
        });
    }
    if let Some(lens) = &detail.lens {
        field(ui, "Lens", |ui| {
            ui.add(egui::Label::new(lens).wrap());
        });
    }
    if let Some(location) = detail.location {
        field(ui, "Coordinates", |ui| {
            ui.add(egui::Label::new(format::coordinates(location)).selectable(true));
        });
    }
    field(ui, "Rating", |ui| {
        // The browser version doesn't write files (REQ-024-05), nor does the app outside libraries (REQ-033-03).
        if !writable {
            match detail.rating {
                Some(rating) => stars(ui, rating),
                None => empty(ui, "Not rated"),
            }
        } else if let Some(stars) = rate_stars(ui, detail.rating) {
            clicked = Some(InfoAction::Rate(stars));
        }
    });
    if let Some(edit) = &detail.edit {
        let replaced = edit.replaced_original(&detail.rel_path);
        field(ui, "Edited", |ui| {
            if replaced {
                ui.add(egui::Label::new("Saved over the original, which is kept").wrap());
                // The browser version doesn't edit (REQ-021-01).
                if writable && ui.button("Revert to original").clicked() {
                    clicked = Some(InfoAction::Revert);
                }
            } else {
                let name = edit.source.rsplit('/').next().unwrap_or(&edit.source);
                ui.add(egui::Label::new(format!("An edited copy of {name}")).wrap());
            }
            let earlier = edit.versions().len().saturating_sub(1);
            if writable
                && earlier > 0
                && ui
                    .button("Clear version history")
                    .on_hover_text(format!(
                        "Removes the {} earlier saved {} of this photo. Its current edit and its original stay.",
                        earlier,
                        if earlier == 1 { "version" } else { "versions" }
                    ))
                    .clicked()
            {
                clicked = Some(InfoAction::ClearHistory);
            }
        });
    }
    // Only while faces are looked for (REQ-032-21).
    let found = match detail.face_scan {
        FaceScan::NotLooking => None,
        FaceScan::NotYet => Some("Not looked at yet".to_owned()),
        FaceScan::Done(0) => Some("Looked at, none found".to_owned()),
        FaceScan::Done(1) => Some("Looked at, 1 found".to_owned()),
        FaceScan::Done(faces) => Some(format!("Looked at, {faces} found")),
    };
    if let Some(found) = found {
        field(ui, "Faces", |ui| {
            ui.add(egui::Label::new(found).wrap());
            if matches!(detail.face_scan, FaceScan::Done(_)) && ui.button("Look again").clicked() {
                clicked = Some(InfoAction::LookForFacesAgain);
            }
        });
    }
    // The browser version doesn't write files (REQ-026-05), nor does the app outside libraries (REQ-033-03).
    let editable = writable;
    for (label, category, root, known) in [
        (
            "People",
            Some(TagCategory::People),
            Some(PEOPLE_TAG_ROOT),
            &paths.people,
        ),
        (
            "Places",
            Some(TagCategory::Places),
            Some(PLACES_TAG_ROOT),
            &paths.places,
        ),
        ("Tags", None, None, &paths.tags),
    ] {
        let names: Vec<&str> = detail
            .tags
            .iter()
            .filter(|tag| tag.category == category)
            .map(|tag| tag.name.as_str())
            .collect();
        let adding_id = ui.scope_id().with(("adding tag", label));
        let adding = ui.data(|data| data.get_temp::<String>(adding_id));
        if names.is_empty() && !editable {
            field(ui, label, |ui| empty(ui, "None"));
            continue;
        }
        caption(ui, label);
        // While adding the group's first tag, the field takes the row's place.
        if !names.is_empty() || adding.is_none() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for name in &names {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let chip = egui::Button::new(egui::RichText::new(*name).size(theme::SIZE_CAPTION))
                        .fill(theme::selected())
                        .corner_radius(theme::RADIUS_CONTROL)
                        .min_size(egui::vec2(0.0, 24.0));
                    let response = ui.add(chip);
                    let response = response.on_hover_text(format!("Show only {name}"));
                    if response.clicked() {
                        clicked = Some(InfoAction::FilterByTag((*name).to_owned()));
                    }
                    if editable {
                        let remove = ui
                            .add(
                                egui::Button::image(Icon::X.image(12.0, theme::muted()))
                                    .frame_when_inactive(false)
                                    .min_size(egui::vec2(20.0, 24.0)),
                            )
                            .on_hover_text(format!("Remove {name}"));
                        let spoken = format!("Remove {name}");
                        remove.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &spoken));
                        if remove.clicked() {
                            clicked = Some(InfoAction::RemoveTag((*name).to_owned()));
                        }
                    }
                    ui.add_space(6.0);
                }
                if editable && adding.is_none() {
                    let add = ui.add(
                        egui::Button::image_and_text(
                            Icon::Plus.image(12.0, theme::muted()),
                            egui::RichText::new("Add")
                                .size(theme::SIZE_CAPTION)
                                .color(theme::muted()),
                        )
                        .frame_when_inactive(false)
                        .min_size(egui::vec2(0.0, 24.0)),
                    );
                    let spoken = format!("Add to {label}");
                    add.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &spoken));
                    if add.clicked() {
                        ui.data_mut(|data| data.insert_temp(adding_id, String::new()));
                        ui.data_mut(|data| data.insert_temp(adding_id.with("opened"), true));
                    }
                }
            });
        }
        if let Some(mut typed) = adding {
            let opened = ui
                .data_mut(|data| data.remove_temp::<bool>(adding_id.with("opened")))
                .is_some();
            let group = Group {
                label,
                root,
                known,
                has: &names,
            };
            match add_field(ui, adding_id.with("field"), opened, &group, &mut typed) {
                Adding::Open => ui.data_mut(|data| {
                    data.insert_temp(adding_id, typed);
                }),
                Adding::Add(path) => {
                    clicked = Some(InfoAction::AddTag(path));
                    ui.data_mut(|data| data.remove::<String>(adding_id));
                }
                Adding::Close => ui.data_mut(|data| data.remove::<String>(adding_id)),
            }
        }
    }
    clicked
}

/// A tag group of the info views, as adding a tag to it needs it (REQ-026-01).
#[derive(Clone, Copy)]
pub(crate) struct Group<'a> {
    pub label: &'a str,
    /// Where a new tag goes: `People`, `Places`, or nowhere (a tag of its own).
    pub root: Option<&'a str>,
    /// The group's existing tags, as paths.
    pub known: &'a [Vec<String>],
    /// The item's tags in the group, left out of the suggestions.
    pub has: &'a [&'a str],
}

impl<'a> Group<'a> {
    /// People, Places and Tags with their existing tags, for adding to the selection (REQ-026-06).
    pub fn all(paths: &'a TagPaths) -> [Self; 3] {
        [
            ("People", Some(PEOPLE_TAG_ROOT), &paths.people),
            ("Places", Some(PLACES_TAG_ROOT), &paths.places),
            ("Tags", None, &paths.tags),
        ]
        .map(|(label, root, known)| Self {
            label,
            root,
            known,
            has: &[],
        })
    }
}

/// What the user did in the field for a new tag (REQ-026-01).
pub(crate) enum Adding {
    /// Still typing.
    Open,
    /// Enter on a name, or a suggestion picked: the tag's path.
    Add(Vec<String>),
    /// Escape, or a click elsewhere.
    Close,
}

/// The field for a new tag of a group, focused when it has just `opened`, with suggestions below it (REQ-026-01).
pub(crate) fn add_field(
    ui: &mut egui::Ui,
    id: egui::Id,
    opened: bool,
    group: &Group<'_>,
    typed: &mut String,
) -> Adding {
    let Group {
        label,
        root,
        known,
        has,
    } = *group;
    let response = ui.add(
        egui::TextEdit::singleline(typed)
            .id(id)
            .hint_text(format!("Add to {label}"))
            .desired_width(f32::INFINITY),
    );
    let spoken = format!("New tag for {label}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, &spoken));
    if opened {
        response.request_focus();
        return Adding::Open;
    }
    let has: Vec<String> = has.iter().map(|name| (*name).to_owned()).collect();
    for path in domain::tagging::suggest(known, typed, &has, SUGGESTIONS) {
        let shown = path.join(" › ");
        let button =
            egui::Button::new(egui::RichText::new(&shown).size(theme::SIZE_CAPTION)).frame_when_inactive(false);
        if ui.add(button).clicked() {
            return Adding::Add(path.clone());
        }
    }
    let (enter, escape, pointer_down) = ui.input(|input| {
        (
            input.key_pressed(egui::Key::Enter),
            input.key_pressed(egui::Key::Escape),
            input.pointer.any_down(),
        )
    });
    if response.lost_focus() && enter && !domain::tags::clean_tag(typed).is_empty() {
        return Adding::Add(domain::tagging::path_for(known, root, typed));
    }
    // Pressing a suggestion takes the focus away first; its click comes with the release.
    if escape || !response.has_focus() && !pointer_down {
        return Adding::Close;
    }
    Adding::Open
}

fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme::SIZE_CAPTION)
            .color(theme::muted()),
    );
}

/// A label above its value.
fn field(ui: &mut egui::Ui, label: &str, value: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        caption(ui, label);
        value(ui);
    });
}

/// No value: one empty line, so the field keeps its place; screen readers hear `spoken`.
fn empty(ui: &mut egui::Ui, spoken: &str) {
    let height = ui.text_style_height(&egui::TextStyle::Body);
    let response = ui.allocate_response(egui::vec2(0.0, height), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, spoken));
}

/// Five star buttons, `rating` of them filled and the rest outlined; the hovered one and those before it fill in as
/// a preview. Returns the star clicked (REQ-024-02).
fn rate_stars(ui: &mut egui::Ui, rating: Option<i64>) -> Option<i64> {
    let mut clicked = None;
    let response = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let size = egui::Vec2::splat(theme::ICON_SIZE);
            let responses: Vec<egui::Response> = (1..=domain::rating::MAX)
                .map(|star| {
                    let response = ui.allocate_response(size, egui::Sense::click());
                    let label = if star == 1 {
                        "Rate 1 star".into()
                    } else {
                        format!("Rate {star} stars")
                    };
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                    response.on_hover_cursor(egui::CursorIcon::PointingHand)
                })
                .collect();
            let hovered = (1..).zip(&responses).find(|(_, response)| response.hovered());
            let shown = hovered.map_or(rating.unwrap_or(0), |(star, _)| star);
            for (star, response) in (1..).zip(&responses) {
                icons::rating_star(star <= shown, theme::ICON_SIZE).paint_at(ui, response.rect);
                if response.clicked() {
                    clicked = Some(star);
                }
            }
        })
        .response;
    let spoken = match rating {
        Some(rating) => format!("Rated {rating} out of 5"),
        None => "Not rated".to_owned(),
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Group, true, &spoken));
    clicked
}

/// Five stars, `rating` of them filled and the rest outlined, named for screen readers.
fn stars(ui: &mut egui::Ui, rating: i64) {
    let response = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for star in 1..=5 {
                ui.add(icons::rating_star(star <= rating, theme::ICON_SIZE));
            }
        })
        .response;
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, format!("Rated {rating} out of 5")));
}
