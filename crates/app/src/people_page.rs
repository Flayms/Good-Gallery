//! _Unconfirmed faces_: faces the app thinks it knows, by person, to confirm (REQ-032-24), then groups of similar faces
//! nobody has named yet (REQ-032-15), largest first, the unknown faces in no group (REQ-032-28), and at the end the
//! ignored faces and non-faces, to change the decision (REQ-032-30). A face opens its photo (REQ-032-26) and has a menu
//! to decide about it (REQ-032-29).

use crate::gallery::FaceGroup;
use crate::icons::Icon;
use crate::names;
use crate::state::{AppState, FaceAction};
use crate::theme;
use db::faces::GroupFace;
use domain::thumbs::ThumbKey;
use eframe::egui::{self, FontId, Margin, Rect, RichText, Sense, Stroke, Vec2, pos2, vec2};

const MAX_WIDTH: f32 = 720.0;
const CROP: f32 = 72.0;
/// How long the pointer rests on a face before its whole photo shows, in seconds, and that photo's long side
/// (REQ-032-37).
const PREVIEW_DELAY: f32 = 0.4;
const PREVIEW: f32 = 240.0;
/// How far beyond the screen, in screens, faces are loaded ahead of scrolling.
const NEAR_SCREENS: f32 = 0.5;
/// Faces shown of a long section or group at first, and more each time the user asks (REQ-032-28, REQ-032-32).
const PART: usize = 48;

/// What each decision on a face does, said where it is offered, the same in the viewer and on the page (REQ-032-33).
pub mod tips {
    pub const NAME: &str = "Writes the name into the photo";
    pub const NO_NAME: &str = "Type a name first";
    pub const IGNORE: &str = "For a real person you won't name: not asked about again";
    pub const NOT_A_FACE: &str = "For a wrong detection: not found again";
    pub const REMOVE_BOX: &str = "Forgets this box: it may be found again when the photo is looked at again";
    pub const REMOVE_NAME: &str = "Takes the name out of the photo";
    pub const BACK_TO_UNKNOWN: &str = "Asks about this face again";
}

/// A button for one of a face's decisions, with what it does (REQ-032-33).
pub fn decision(ui: &mut egui::Ui, label: &str, tip: &str) -> bool {
    decision_spoken(ui, label, tip, label)
}

/// [`decision`], read out by screen readers as `spoken`, which says whose it is where a page has several.
fn decision_spoken(ui: &mut egui::Ui, label: &str, tip: &str, spoken: &str) -> bool {
    let response = ui.button(label).on_hover_text(tip);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, spoken));
    response.clicked()
}

/// A button that names or renames, off until there is a name to write (REQ-032-33).
pub fn naming(ui: &mut egui::Ui, label: &str, named: bool) -> bool {
    naming_spoken(ui, label, named, label)
}

fn naming_spoken(ui: &mut egui::Ui, label: &str, named: bool, spoken: &str) -> bool {
    let response = ui
        .add_enabled(named, egui::Button::new(label))
        .on_hover_text(tips::NAME)
        .on_disabled_hover_text(tips::NO_NAME);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, named, spoken));
    response.clicked()
}

/// A part of the page shown in parts: a section by its title, or a group by its first face (REQ-032-32).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Part {
    Section(&'static str),
    Group(i64),
}

/// What the user chose on the page.
enum Chosen {
    Face(FaceAction),
    /// Several, one after the other: a decided face back among the unknown ones, then named (REQ-032-30).
    Faces(Vec<FaceAction>),
    /// A group's photos in the viewer, on this one (REQ-032-26).
    Open {
        photos: Vec<i64>,
        at: i64,
    },
}

/// A name typed for a group and waiting for the user to confirm it.
pub struct NamingGroup {
    pub name: String,
    pub places: Vec<(i64, [f32; 4])>,
}

/// The state of the page's fields, by the group's first face.
#[derive(Default)]
pub struct PeoplePage {
    names: std::collections::HashMap<i64, String>,
    pub confirm: Option<NamingGroup>,
    /// Faces shown of a long section, by its title, beyond the first part (REQ-032-28).
    shown: std::collections::HashMap<Part, usize>,
}

/// What an ignored face or a non-face is, for its menu (REQ-032-30).
#[derive(Clone, Copy, PartialEq)]
enum Decided {
    Ignored,
    NotAFace,
}

/// The square of a thumbnail `size` pixels large around a face (fractions of the photo), a little wider than its box, so
/// the face isn't stretched by a box that isn't square. Moved inside the photo where it would reach past its edge.
fn crop_uv([x, y, w, h]: [f32; 4], size: [u32; 2]) -> Rect {
    const WIDER: f32 = 1.5;
    let (width, height) = (size[0].max(1) as f32, size[1].max(1) as f32);
    let side = (w * width).max(h * height) * WIDER;
    let side = side.min(width).min(height);
    let (half_u, half_v) = (side / width / 2.0, side / height / 2.0);
    let center = pos2(
        (x + w / 2.0).clamp(half_u, 1.0 - half_u),
        (y + h / 2.0).clamp(half_v, 1.0 - half_v),
    );
    Rect::from_center_size(center, vec2(half_u * 2.0, half_v * 2.0))
}

/// The menu of a face nobody decided about yet: _Ignore_, _Not a face_ and _Remove box_ (REQ-032-29).
fn open_face_menu(response: &egui::Response, face: i64) -> Option<Chosen> {
    let mut chosen = None;
    response.context_menu(|ui| {
        crate::glass::behind_menu(ui, |ui| {
            if decision(ui, "Ignore", tips::IGNORE) {
                chosen = Some(Chosen::Face(FaceAction::Ignore { faces: vec![face] }));
            }
            if decision(ui, "Not a face", tips::NOT_A_FACE) {
                chosen = Some(Chosen::Face(FaceAction::NotAFace { faces: vec![face] }));
            }
            if decision(ui, "Remove box", tips::REMOVE_BOX) {
                chosen = Some(Chosen::Face(FaceAction::Delete { face }));
            }
        });
    });
    chosen
}

/// One face, cropped out of its thumbnail, a little wider than its box; a click opens its photo (REQ-032-26).
fn crop(ui: &mut egui::Ui, state: &mut AppState, face: i64, media: i64, rect: [f32; 4]) -> egui::Response {
    crop_spoken(ui, state, media, rect, &format!("Show face {face} in its photo"))
}

fn crop_spoken(ui: &mut egui::Ui, state: &mut AppState, media: i64, rect: [f32; 4], spoken: &str) -> egui::Response {
    let (area, response) = ui.allocate_exact_size(Vec2::splat(CROP), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, spoken));
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    ui.painter().rect_filled(area, theme::RADIUS_CONTROL, theme::selected());
    // Asking marks a thumbnail as in use, so only for what is on screen.
    if !ui.is_rect_visible(area) {
        return response;
    }
    let texture = state.gallery.as_mut().and_then(|gallery| gallery.thumbnail(media));
    if let Some(texture) = &texture {
        let uv = crop_uv(rect, texture.size);
        egui::Image::new((texture.id, area.size()))
            .uv(uv)
            .corner_radius(theme::RADIUS_CONTROL)
            .paint_at(ui, area);
    }
    // Resting on the face, its whole photo beside it (REQ-032-37). Not over its own buttons, which are on it.
    if response.hovered() {
        let rested = ui.input(|input| input.pointer.time_since_last_movement());
        if rested >= PREVIEW_DELAY {
            let size = texture.as_ref().map_or([4, 3], |texture| texture.size);
            preview(ui, area, rect, size, texture.map(|texture| texture.id));
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f32(PREVIEW_DELAY - rested));
        }
    }
    response
}

/// The whole photo of a face shown at `crop` (REQ-032-37): beside it on the right, or the left where the window ends,
/// with the face's box (`rect`, fractions of the photo) outlined; the photo's thumbnail is `size` pixels large.
fn preview(ui: &egui::Ui, crop: Rect, rect: [f32; 4], size: [u32; 2], texture: Option<egui::TextureId>) {
    let (width, height) = (size[0].max(1) as f32, size[1].max(1) as f32);
    let scale = PREVIEW / width.max(height);
    let shown = vec2(width * scale, height * scale);
    let screen = ui.ctx().content_rect();
    let gap = theme::SPACE;
    let x = if crop.right() + gap + shown.x <= screen.right() {
        crop.right() + gap
    } else {
        crop.left() - gap - shown.x
    };
    let y = crop.top().min(screen.bottom() - shown.y).max(screen.top());
    let photo = Rect::from_min_size(pos2(x, y), shown);
    let layer = egui::LayerId::new(egui::Order::Tooltip, ui.scope_id().with("face preview"));
    let painter = ui.ctx().layer_painter(layer);
    painter.rect_filled(photo.expand(4.0), theme::RADIUS_PANEL, theme::raised());
    match texture {
        Some(texture) => {
            painter.image(
                texture,
                photo,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            painter.rect_filled(photo, 0.0, theme::placeholder());
        }
    }
    let [fx, fy, fw, fh] = rect;
    let face = Rect::from_min_size(
        pos2(photo.left() + fx * photo.width(), photo.top() + fy * photo.height()),
        vec2(fw * photo.width(), fh * photo.height()),
    );
    painter.rect_stroke(
        face,
        2.0,
        Stroke::new(2.0, egui::Color32::WHITE),
        egui::StrokeKind::Outside,
    );
    // For screen readers and tests: what shows, where.
    let response = ui.interact(
        photo,
        ui.scope_id().with(("face preview", rect.map(f32::to_bits))),
        Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Whole photo of the face"));
}

pub fn ui(ui: &mut egui::Ui, state: &mut AppState) {
    if let Some(gallery) = &mut state.gallery {
        gallery.begin_thumbnails();
    }
    let groups = state
        .gallery
        .as_mut()
        .and_then(|gallery| gallery.face_groups().cloned());
    let header_height = crate::gallery::TOOLBAR_HEIGHT;
    let area = ui.max_rect();
    let mut chosen = None;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let width = (ui.available_width() - 48.0).min(MAX_WIDTH);
        let margin = ((ui.available_width() - width) / 2.0).max(0.0);
        ui.add_space(header_height + 24.0);
        ui.horizontal_top(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_width(width);
                match &groups {
                    None => {
                        ui.label(RichText::new("Looking through the faces…").color(theme::muted()));
                    }
                    Some(groups) => {
                        // What's asked for can't be evicted, so groups out of reach aren't: a page of them would
                        // pass the textures' budget for good.
                        let view = ui.clip_rect();
                        let near = view.expand2(vec2(0.0, view.height() * NEAR_SCREENS));
                        let (mut on_screen, mut ahead) = (Vec::new(), Vec::new());
                        let mut want = |faces: &[(GroupFace, Option<ThumbKey>)], card: Rect| {
                            if near.intersects(card) {
                                let wanted = if view.intersects(card) {
                                    &mut on_screen
                                } else {
                                    &mut ahead
                                };
                                wanted.extend(faces.iter().filter_map(|(face, key)| Some((face.media, key.clone()?))));
                            }
                        };
                        let mut choose = |action: Option<Chosen>| {
                            if action.is_some() {
                                chosen = action;
                            }
                        };
                        if groups.suggested.is_empty() && groups.unknown.is_empty() && groups.others.is_empty() {
                            ui.label(RichText::new("No faces to confirm yet.").color(theme::muted()));
                            ui.add_space(theme::SPACE * 6.0);
                        }
                        if !groups.suggested.is_empty() {
                            section_title(ui, "To confirm");
                            for (name, group) in &groups.suggested {
                                let (action, card, shown) = group_card(ui, state, group, Some(name));
                                choose(action);
                                want(&group[..shown], card);
                                ui.add_space(theme::SPACE * 6.0);
                            }
                        }
                        if !groups.unknown.is_empty() {
                            section_title(ui, "Unknown");
                            for group in &groups.unknown {
                                let (action, card, shown) = group_card(ui, state, group, None);
                                choose(action);
                                want(&group[..shown], card);
                                ui.add_space(theme::SPACE * 6.0);
                            }
                        }
                        if !groups.others.is_empty() {
                            const TITLE: &str = "Other unknown faces";
                            section_title(ui, TITLE);
                            let (action, card, shown) = others_card(ui, state, TITLE, &groups.others);
                            choose(action);
                            want(&groups.others[..shown], card);
                            ui.add_space(theme::SPACE * 6.0);
                        }
                        for (title, faces, decided) in [
                            ("Ignored", &groups.ignored, Decided::Ignored),
                            ("Not faces", &groups.not_faces, Decided::NotAFace),
                        ] {
                            if faces.is_empty() {
                                continue;
                            }
                            let header = egui::CollapsingHeader::new(
                                RichText::new(format!("{title} · {}", faces.len()))
                                    .font(FontId::new(theme::SIZE_BODY, theme::semibold())),
                            )
                            .id_salt(("decided", title))
                            .default_open(false)
                            .show(ui, |ui| decided_card(ui, state, title, faces, decided));
                            if let Some((action, card, shown)) = header.body_returned {
                                choose(action);
                                want(&faces[..shown], card);
                            }
                            ui.add_space(theme::SPACE * 3.0);
                        }
                        on_screen.append(&mut ahead);
                        let wanted: Vec<(i64, ThumbKey)> = on_screen;
                        let pixels_per_point = ui.ctx().pixels_per_point();
                        let viewer = state.viewer.is_some();
                        if let Some(gallery) = &mut state.gallery {
                            // The viewer hides the page; its photo and filmstrip need theirs (REQ-032-26).
                            if viewer {
                                gallery.want_viewer_thumbnails(pixels_per_point);
                            } else {
                                gallery.want_thumbnails(&wanted);
                            }
                        }
                    }
                }
                ui.add_space(24.0);
            });
        });
    });
    let header = Rect::from_min_size(area.min, Vec2::new(area.width(), header_height));
    crate::settings_page::page_header(ui, header, state, "Unconfirmed faces");
    match chosen {
        Some(Chosen::Face(action)) => state.face_action(action),
        Some(Chosen::Faces(actions)) => {
            for action in actions {
                state.face_action(action);
            }
        }
        Some(Chosen::Open { photos, at }) => state.show_face_photos(&photos, at),
        None => {}
    }
    confirm_naming(ui.ctx(), state);
}

fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).font(FontId::new(theme::SIZE_TITLE, theme::semibold())));
    ui.add_space(theme::SPACE * 3.0);
}

fn faces_count(count: usize) -> String {
    if count == 1 {
        "1 face".into()
    } else {
        format!("{count} faces")
    }
}

/// One group with its faces. A person's group (REQ-032-24) is confirmed with _Yes, all_, and a face's ✕ says it isn't
/// them; an unknown group (REQ-032-15) is named in a field or ignored, and a face's ✕ takes it out (REQ-032-25). A
/// person's group can be named as someone else, ignored or marked as not faces too (REQ-032-35).
/// Without saying who, nothing is written. A large group shows in parts (REQ-032-32). What the user chose, where the
/// card is, and how many faces show.
fn group_card(
    ui: &mut egui::Ui,
    state: &mut AppState,
    group: &FaceGroup,
    person: Option<&String>,
) -> (Option<Chosen>, Rect, usize) {
    let key = group.first().map_or(0, |(face, _)| face.id);
    let part = Part::Group(key);
    let shown = shown_part(state, part, group.len());
    let photos: Vec<i64> = group.iter().map(|(face, _)| face.media).collect();
    let people = known_people(state);
    let mut chosen = None;
    let card = card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        let title = match person {
            Some(name) => format!("{name}? · {}", faces_count(group.len())),
            None => faces_count(group.len()),
        };
        ui.label(RichText::new(title).font(FontId::new(theme::SIZE_BODY, theme::semibold())));
        ui.add_space(theme::SPACE * 2.0);
        ui.horizontal_wrapped(|ui| {
            for (face, _) in &group[..shown] {
                let shown = crop(ui, state, face.id, face.media, face.rect);
                let corner = Rect::from_min_size(shown.rect.right_top() + vec2(-22.0, 2.0), Vec2::splat(20.0));
                let (spoken, hover) = match person {
                    Some(name) => (format!("Face {} isn't {name}", face.id), format!("Not {name}")),
                    None => (
                        format!("Take face {} out of the group", face.id),
                        "Not one of them".to_string(),
                    ),
                };
                let take_out = ui.interact(corner, ui.make_persistent_id(("take out", face.id)), Sense::click());
                ui.painter()
                    .rect_filled(corner, 4.0, egui::Color32::from_black_alpha(153));
                Icon::X
                    .image(12.0, egui::Color32::WHITE)
                    .paint_at(ui, Rect::from_center_size(corner.center(), Vec2::splat(12.0)));
                take_out.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &spoken));
                // A second person as likely: a click names the face so (REQ-032-36).
                let other = face
                    .alternative
                    .as_ref()
                    .filter(|_| person.is_some())
                    .and_then(|other| {
                        let font = FontId::proportional(theme::SIZE_CAPTION);
                        let text = format!("or {other}");
                        let galley = ui.painter().layout_no_wrap(text, font, egui::Color32::WHITE);
                        let size = vec2((galley.size().x + 10.0).min(shown.rect.width() - 4.0), 18.0);
                        let chip = Rect::from_min_size(shown.rect.left_bottom() + vec2(2.0, -20.0), size);
                        let response = ui.interact(chip, ui.make_persistent_id(("or", face.id)), Sense::click());
                        ui.painter()
                            .rect_filled(chip, 4.0, egui::Color32::from_black_alpha(153));
                        ui.painter().with_clip_rect(chip).galley(
                            chip.left_center() + vec2(5.0, -galley.size().y / 2.0),
                            galley,
                            egui::Color32::WHITE,
                        );
                        let spoken = format!("Face {} is {other}", face.id);
                        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &spoken));
                        response
                            .on_hover_text(format!("It's {other}"))
                            .clicked()
                            .then(|| other.clone())
                    });
                if let Some(action) = open_face_menu(&shown, face.id) {
                    chosen = Some(action);
                } else if let Some(other) = other {
                    chosen = Some(Chosen::Face(FaceAction::Name {
                        name: other,
                        places: vec![(face.media, face.rect)],
                    }));
                } else if take_out.on_hover_text(hover).clicked() {
                    chosen = Some(Chosen::Face(match person {
                        Some(name) => FaceAction::NotThisPerson {
                            face: face.id,
                            name: name.clone(),
                        },
                        None => FaceAction::Ungroup { face: face.id },
                    }));
                } else if shown.clicked() {
                    chosen = Some(Chosen::Open {
                        photos: photos.clone(),
                        at: face.media,
                    });
                }
            }
        });
        show_more(ui, state, part, shown, group.len());
        ui.add_space(theme::SPACE * 2.0);
        let places = || group.iter().map(|(face, _)| (face.media, face.rect)).collect();
        let field_id = ui.make_persistent_id(("face group name", key));
        let field = ui.horizontal(|ui| {
            if let Some(name) = person {
                let yes = ui.button("Yes, all");
                let spoken = format!("Yes, all are {name}");
                yes.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &spoken));
                if yes.clicked() {
                    state.people_page.confirm = Some(NamingGroup {
                        name: name.clone(),
                        places: places(),
                    });
                }
            }
            // Whose the buttons are, where a page has several of each.
            let (hint, spoken, whose) = match person {
                Some(name) => (
                    "Someone else?",
                    format!("Someone else than {name}? Group of {}", group.len()),
                    format!(" ({name}?)"),
                ),
                None => (
                    "Who is this?",
                    format!("Who is this? Group of {}", group.len()),
                    String::new(),
                ),
            };
            let page = &mut state.people_page;
            let name = page.names.entry(key).or_default();
            let field = names::field(ui, field_id, name, hint, 240.0);
            field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, &spoken));
            if naming_spoken(
                ui,
                "Name them",
                clean_name(name).is_some(),
                &format!("Name them{whose}"),
            ) && let Some(name) = clean_name(name)
            {
                page.confirm = Some(NamingGroup { name, places: places() });
            }
            let faces = || group.iter().map(|(face, _)| face.id).collect();
            if decision_spoken(ui, "Ignore", tips::IGNORE, &format!("Ignore{whose}")) {
                chosen = Some(Chosen::Face(FaceAction::Ignore { faces: faces() }));
            }
            if decision_spoken(ui, "Not faces", tips::NOT_A_FACE, &format!("Not faces{whose}")) {
                chosen = Some(Chosen::Face(FaceAction::NotAFace { faces: faces() }));
            }
            field
        });
        // Below the row, so the buttons stay where they are (REQ-032-34).
        let name = state.people_page.names.entry(key).or_default();
        if let Some(name) = names::suggestions(ui, field_id, &field.inner, name, &people.people) {
            state.people_page.confirm = Some(NamingGroup { name, places: places() });
        }
    });
    (chosen, card.response.rect, shown)
}

/// How many of a long section's or group's faces show, and _Show more_ below them for the rest (REQ-032-28,
/// REQ-032-32).
fn shown_part(state: &AppState, part: Part, count: usize) -> usize {
    state.people_page.shown.get(&part).copied().unwrap_or(PART).min(count)
}

fn show_more(ui: &mut egui::Ui, state: &mut AppState, part: Part, shown: usize, count: usize) {
    if shown < count {
        ui.add_space(theme::SPACE * 2.0);
        if ui.button("Show more").clicked() {
            state.people_page.shown.insert(part, shown + PART);
        }
    }
}

/// The unknown faces in no group, each opening its photo among theirs and with its menu (REQ-032-28, REQ-032-29).
/// What the user chose, where the card is, and how many faces show.
fn others_card(
    ui: &mut egui::Ui,
    state: &mut AppState,
    title: &'static str,
    faces: &FaceGroup,
) -> (Option<Chosen>, Rect, usize) {
    let part = Part::Section(title);
    let shown = shown_part(state, part, faces.len());
    let mut chosen = None;
    let card = card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            for (face, _) in &faces[..shown] {
                let response = crop(ui, state, face.id, face.media, face.rect);
                if let Some(action) = open_face_menu(&response, face.id) {
                    chosen = Some(action);
                } else if response.clicked() {
                    chosen = Some(Chosen::Open {
                        photos: faces.iter().map(|(face, _)| face.media).collect(),
                        at: face.media,
                    });
                }
            }
        });
        show_more(ui, state, part, shown, faces.len());
    });
    (chosen, card.response.rect, shown)
}

/// Ignored faces or non-faces. A click opens a face's menu: a name, _Back to unknown_, the other decision, _Remove box_
/// (REQ-032-30). The viewer doesn't show them, so a click doesn't open the photo.
fn decided_card(
    ui: &mut egui::Ui,
    state: &mut AppState,
    title: &'static str,
    faces: &FaceGroup,
    decided: Decided,
) -> (Option<Chosen>, Rect, usize) {
    let part = Part::Section(title);
    let shown = shown_part(state, part, faces.len());
    let people = known_people(state);
    let mut chosen = None;
    let card = card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            for (face, _) in &faces[..shown] {
                let spoken = format!("Change face {}", face.id);
                let response = crop_spoken(ui, state, face.media, face.rect, &spoken);
                let names = &mut state.people_page.names;
                let people = &people.people;
                egui::Popup::menu(&response)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        crate::glass::behind_menu(ui, |ui| {
                            if let Some(action) = decided_menu(ui, names, people, face, decided) {
                                chosen = Some(action);
                                ui.close();
                            }
                        });
                    });
            }
        });
        show_more(ui, state, part, shown, faces.len());
    });
    (chosen, card.response.rect, shown)
}

fn decided_menu(
    ui: &mut egui::Ui,
    names: &mut std::collections::HashMap<i64, String>,
    people: &[Vec<String>],
    face: &GroupFace,
    decided: Decided,
) -> Option<Chosen> {
    let mut chosen = None;
    let name = names.entry(face.id).or_default();
    let id = ui.make_persistent_id(("decided face name", face.id));
    let (field, save) = ui
        .horizontal(|ui| {
            let field = names::field(ui, id, name, "Who is this?", 180.0);
            let spoken = format!("Who is this? Face {}", face.id);
            field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, &spoken));
            (field, naming(ui, "Save", clean_name(name).is_some()))
        })
        .inner;
    let picked = names::suggestions(ui, id, &field, name, people);
    if let Some(name) = picked.or_else(|| save.then(|| clean_name(name)).flatten()) {
        chosen = Some(Chosen::Faces(vec![
            FaceAction::Reopen { faces: vec![face.id] },
            FaceAction::Name {
                name,
                places: vec![(face.media, face.rect)],
            },
        ]));
    }
    ui.separator();
    let faces = vec![face.id];
    if decision(ui, "Back to unknown", tips::BACK_TO_UNKNOWN) {
        chosen = Some(Chosen::Face(FaceAction::Reopen { faces: faces.clone() }));
    }
    let other = match decided {
        Decided::Ignored => decision(ui, "Not a face", tips::NOT_A_FACE).then_some(FaceAction::NotAFace { faces }),
        Decided::NotAFace => decision(ui, "Ignore", tips::IGNORE).then_some(FaceAction::Ignore { faces }),
    };
    if let Some(action) = other {
        chosen = Some(Chosen::Face(action));
    }
    if decision(ui, "Remove box", tips::REMOVE_BOX) {
        chosen = Some(Chosen::Face(FaceAction::Delete { face: face.id }));
    }
    if chosen.is_some() {
        names.remove(&face.id);
    }
    chosen
}

fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .stroke(Stroke::new(1.0, theme::rim()))
        .corner_radius(theme::RADIUS_PANEL)
        .inner_margin(Margin::same(12))
}

fn clean_name(name: &str) -> Option<String> {
    names::clean(name)
}

/// The known people, suggested while a name is typed (REQ-032-34).
fn known_people(state: &AppState) -> std::sync::Arc<crate::gallery::TagPaths> {
    state
        .gallery
        .as_ref()
        .map(|gallery| gallery.tag_paths())
        .unwrap_or_default()
}

/// Naming a group writes to many files, so it asks first (REQ-032-15).
fn confirm_naming(ctx: &egui::Context, state: &mut AppState) {
    let Some(naming) = &state.people_page.confirm else {
        return;
    };
    let files = naming.places.len();
    let text = format!(
        "Write {} into {files} {}?",
        naming.name,
        if files == 1 { "file" } else { "files" }
    );
    let Some(chosen) = crate::widgets::dialog(ctx, "name-group", "Name this group", &text, &["Name them", "Cancel"])
    else {
        return;
    };
    let naming = state.people_page.confirm.take();
    if chosen == 0
        && let Some(naming) = naming
    {
        state.face_action(FaceAction::Name {
            name: naming.name,
            places: naming.places,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ-032-15: a crop is square in pixels, whatever the box and the photo's shape, and stays on the photo.
    #[test]
    fn req_032_15_crops_faces_without_stretching_them() {
        let uv = crop_uv([0.4, 0.4, 0.1, 0.2], [800, 400]);
        let (pixels_wide, pixels_high) = (uv.width() * 800.0, uv.height() * 400.0);
        assert!((pixels_wide - pixels_high).abs() < 0.5, "{uv:?}");
        assert!(pixels_wide > 80.0, "wider than the box: {uv:?}");
        assert!(
            (uv.center().x - 0.45).abs() < 1e-4 && (uv.center().y - 0.5).abs() < 1e-4,
            "{uv:?}"
        );

        let corner = crop_uv([0.0, 0.0, 0.1, 0.1], [400, 400]);
        assert!(corner.min.x >= 0.0 && corner.min.y >= 0.0, "{corner:?}");
        let huge = crop_uv([0.0, 0.0, 1.0, 1.0], [400, 300]);
        assert!(huge.max.x <= 1.0 && huge.max.y <= 1.0, "{huge:?}");
        assert!((huge.width() * 400.0 - huge.height() * 300.0).abs() < 0.5, "{huge:?}");
    }
}
