//! The smart album editor (spec 039): a name, all or any, a list of conditions with their values, and the sort order.

use crate::theme;
use domain::album_rules::{Bound, Condition, DateField, Days, Orientation, Rules, Unit};
use domain::media::MediaKind;
use domain::search::{GallerySearch, SortBy};
use eframe::egui::{self, CornerRadius, FontId, Frame, Margin, RichText, Stroke};

/// The kinds of condition the editor offers, in its menu's order (REQ-039-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Tag,
    NotTag,
    HasPeople,
    HasPlaces,
    NoTags,
    Name,
    MediaKind,
    RatingIn,
    RatingAtLeast,
    Library,
    Folder,
    TakenBetween,
    TakenLast,
    ModifiedBetween,
    ModifiedLast,
    SizeMb,
    DurationS,
    Megapixels,
    Orientation,
    Shows,
}

impl Kind {
    const ALL: [Self; 20] = [
        Self::Tag,
        Self::NotTag,
        Self::HasPeople,
        Self::HasPlaces,
        Self::NoTags,
        Self::Name,
        Self::MediaKind,
        Self::RatingIn,
        Self::RatingAtLeast,
        Self::Library,
        Self::Folder,
        Self::TakenBetween,
        Self::TakenLast,
        Self::ModifiedBetween,
        Self::ModifiedLast,
        Self::SizeMb,
        Self::DurationS,
        Self::Megapixels,
        Self::Orientation,
        Self::Shows,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Tag => "Tag is",
            Self::NotTag => "Tag is not",
            Self::HasPeople => "Has People",
            Self::HasPlaces => "Has Places",
            Self::NoTags => "Has no tags",
            Self::Name => "File name contains",
            Self::MediaKind => "Kind is",
            Self::RatingIn => "Rating is",
            Self::RatingAtLeast => "Rating at least",
            Self::Library => "Library is",
            Self::Folder => "Folder is",
            Self::TakenBetween => "Date taken between",
            Self::TakenLast => "Date taken in the last",
            Self::ModifiedBetween => "Date modified between",
            Self::ModifiedLast => "Date modified in the last",
            Self::SizeMb => "Size (MB)",
            Self::DurationS => "Duration (seconds)",
            Self::Megapixels => "Megapixels",
            Self::Orientation => "Orientation is",
            Self::Shows => "Shows",
        }
    }
}

/// A condition as the editor holds it while it is typed.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub kind: Kind,
    /// A tag, file name, folder or content words; the first day of a range.
    pub text: String,
    /// The last day of a range.
    pub text2: String,
    /// N of _in the last_, MB, seconds or megapixels.
    pub number: f64,
    pub at_least: bool,
    pub unit: Unit,
    pub library: Option<i64>,
    /// Ratings 0 (unrated) to 5.
    pub ratings: [bool; 6],
    pub stars: u8,
    pub media: MediaKind,
    pub orientation: Orientation,
}

impl Row {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            text: String::new(),
            text2: String::new(),
            number: 30.0,
            at_least: true,
            unit: Unit::Days,
            library: None,
            ratings: [false; 6],
            stars: 3,
            media: MediaKind::Image,
            orientation: Orientation::Landscape,
        }
    }

    fn of(condition: &Condition) -> Self {
        let day = |day: Option<chrono::NaiveDate>| day.map(|day| day.to_string()).unwrap_or_default();
        let bounded = |kind, bound: &Bound, scale: f64| Self {
            number: bound.value / scale,
            at_least: bound.at_least,
            ..Self::new(kind)
        };
        match condition {
            Condition::Tag { name, is } => Self {
                text: name.clone(),
                ..Self::new(if *is { Kind::Tag } else { Kind::NotTag })
            },
            Condition::HasPeople => Self::new(Kind::HasPeople),
            Condition::HasPlaces => Self::new(Kind::HasPlaces),
            Condition::NoTags => Self::new(Kind::NoTags),
            Condition::NameContains(name) => Self {
                text: name.clone(),
                ..Self::new(Kind::Name)
            },
            Condition::Kind(media) => Self {
                media: *media,
                ..Self::new(Kind::MediaKind)
            },
            Condition::RatingIn(ratings) => {
                let mut row = Self::new(Kind::RatingIn);
                for &rating in ratings.iter().filter(|&&rating| rating <= 5) {
                    row.ratings[usize::from(rating)] = true;
                }
                row
            }
            Condition::RatingAtLeast(stars) => Self {
                stars: *stars,
                ..Self::new(Kind::RatingAtLeast)
            },
            Condition::Library(library) => Self {
                library: Some(*library),
                ..Self::new(Kind::Library)
            },
            Condition::Folder { library, folder } => Self {
                library: Some(*library),
                text: folder.clone(),
                ..Self::new(Kind::Folder)
            },
            Condition::Date {
                field,
                days: Days::Between(from, to),
            } => Self {
                text: day(*from),
                text2: day(*to),
                ..Self::new(match field {
                    DateField::Taken => Kind::TakenBetween,
                    DateField::Modified => Kind::ModifiedBetween,
                })
            },
            Condition::Date {
                field,
                days: Days::Last(n, unit),
            } => Self {
                number: f64::from(*n),
                unit: *unit,
                ..Self::new(match field {
                    DateField::Taken => Kind::TakenLast,
                    DateField::Modified => Kind::ModifiedLast,
                })
            },
            Condition::Size(bound) => bounded(Kind::SizeMb, bound, 1_000_000.0),
            Condition::Duration(bound) => bounded(Kind::DurationS, bound, 1.0),
            Condition::Megapixels(bound) => bounded(Kind::Megapixels, bound, 1.0),
            Condition::Orientation(orientation) => Self {
                orientation: *orientation,
                ..Self::new(Kind::Orientation)
            },
            Condition::Shows(words) => Self {
                text: words.clone(),
                ..Self::new(Kind::Shows)
            },
            // Never in a stored album.
            Condition::Matches(_) => Self::new(Kind::Shows),
        }
    }

    /// The condition, or `None` while a value is missing or can't be read (REQ-039-02).
    pub fn condition(&self) -> Option<Condition> {
        let text = || Some(self.text.trim().to_owned()).filter(|text| !text.is_empty());
        let day = |text: &str| -> Result<Option<chrono::NaiveDate>, ()> {
            match text.trim() {
                "" => Ok(None),
                text => text.parse().map(Some).map_err(|_| ()),
            }
        };
        let between = |field| {
            let (Ok(from), Ok(to)) = (day(&self.text), day(&self.text2)) else {
                return None;
            };
            Some(Condition::Date {
                field,
                days: Days::Between(from, to),
            })
        };
        let last = |field| {
            let n = self.number.round();
            (n >= 1.0 && n <= f64::from(u16::MAX)).then_some(Condition::Date {
                field,
                days: Days::Last(n as u32, self.unit),
            })
        };
        let bound = |scale: f64| Bound {
            at_least: self.at_least,
            value: self.number * scale,
        };
        let condition = match self.kind {
            Kind::Tag => Condition::Tag {
                name: text()?,
                is: true,
            },
            Kind::NotTag => Condition::Tag {
                name: text()?,
                is: false,
            },
            Kind::HasPeople => Condition::HasPeople,
            Kind::HasPlaces => Condition::HasPlaces,
            Kind::NoTags => Condition::NoTags,
            Kind::Name => Condition::NameContains(text()?),
            Kind::MediaKind => Condition::Kind(self.media),
            Kind::RatingIn => Condition::RatingIn((0..=5u8).filter(|&r| self.ratings[usize::from(r)]).collect()),
            Kind::RatingAtLeast => Condition::RatingAtLeast(self.stars.clamp(1, 5)),
            Kind::Library => Condition::Library(self.library?),
            Kind::Folder => Condition::Folder {
                library: self.library?,
                folder: text()?,
            },
            Kind::TakenBetween => between(DateField::Taken)?,
            Kind::ModifiedBetween => between(DateField::Modified)?,
            Kind::TakenLast => last(DateField::Taken)?,
            Kind::ModifiedLast => last(DateField::Modified)?,
            Kind::SizeMb => Condition::Size(bound(1_000_000.0)),
            Kind::DurationS => Condition::Duration(bound(1.0)),
            Kind::Megapixels => Condition::Megapixels(bound(1.0)),
            Kind::Orientation => Condition::Orientation(self.orientation),
            Kind::Shows => Condition::Shows(text()?),
        };
        condition.is_complete().then_some(condition)
    }
}

/// What the editor asks for.
pub enum Outcome {
    Save { name: String, search: Box<GallerySearch> },
    Cancel,
}

/// The editor of a new or existing smart album (REQ-039-01).
pub struct AlbumEditor {
    /// The album edited; `None` for a new one.
    pub id: Option<i64>,
    pub name: String,
    pub any: bool,
    pub rows: Vec<Row>,
    pub sort_by: SortBy,
    pub descending: bool,
    libraries: Vec<(i64, String)>,
}

impl AlbumEditor {
    /// Opens for `search`'s rules, or its filters as conditions matching all (REQ-039-01, REQ-039-06).
    pub fn new(id: Option<i64>, name: String, search: &GallerySearch, libraries: Vec<(i64, String)>) -> Self {
        let rules = search
            .rules
            .as_deref()
            .cloned()
            .unwrap_or_else(|| Rules::from_search(search));
        Self {
            id,
            name,
            any: rules.any,
            rows: rules.conditions.iter().map(Row::of).collect(),
            sort_by: search.sort_by,
            descending: search.descending,
            libraries,
        }
    }

    /// The conditions, if each is complete and at most one is _Shows_ (REQ-039-02).
    fn conditions(&self) -> Option<Vec<Condition>> {
        let conditions: Option<Vec<Condition>> = self.rows.iter().map(Row::condition).collect();
        let conditions = conditions?;
        let shows = conditions
            .iter()
            .filter(|condition| matches!(condition, Condition::Shows(_)))
            .count();
        (shows <= 1).then_some(conditions)
    }

    /// The search to keep: a plain search where the filters bar can show the rules, so older versions read it too
    /// (REQ-039-05, REQ-039-06), else the rules.
    pub fn search(&self) -> Option<GallerySearch> {
        let name = domain::smart_albums::clean_name(&self.name)?;
        let rules = Rules {
            name,
            any: self.any,
            conditions: self.conditions()?,
        };
        let search = match rules.as_search() {
            Some(search) => search,
            None => GallerySearch {
                rules: Some(Box::new(rules)),
                ..GallerySearch::default()
            },
        };
        Some(GallerySearch {
            sort_by: self.sort_by,
            descending: self.descending,
            ..search
        })
    }

    /// Draws the editor; what was asked for, if anything.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        let mut outcome = None;
        let modal = egui::Modal::new(egui::Id::unique("smart album editor"))
            .frame(
                Frame::new()
                    .fill(theme::raised())
                    .stroke(Stroke::new(1.0, theme::rim()))
                    .corner_radius(CornerRadius::same(theme::RADIUS_PANEL))
                    .inner_margin(Margin::same(20)),
            )
            .show(ctx, |ui| {
                crate::glass::behind_frame(ui, Margin::same(20), |ui| {
                    ui.set_width(620.0);
                    let title = if self.id.is_some() {
                        "Edit smart album"
                    } else {
                        "Save as smart album"
                    };
                    ui.label(RichText::new(title).font(FontId::new(theme::SIZE_TITLE, theme::semibold())));
                    ui.add_space(theme::SPACE * 3.0);
                    ui.horizontal(|ui| {
                        let label = ui.label("Name");
                        ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(f32::INFINITY))
                            .labelled_by(label.id);
                    });
                    ui.add_space(theme::SPACE * 2.0);
                    ui.horizontal(|ui| {
                        let label = ui.label("Match");
                        egui::ComboBox::from_id_salt("album match")
                            .selected_text(if self.any { "any" } else { "all" })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.any, false, "all");
                                ui.selectable_value(&mut self.any, true, "any");
                            })
                            .response
                            .labelled_by(label.id);
                        ui.label("of these conditions");
                    });
                    ui.add_space(theme::SPACE);
                    self.rows_ui(ui);
                    if ui.button("Add condition").clicked() {
                        self.rows.push(Row::new(Kind::Tag));
                    }
                    ui.add_space(theme::SPACE * 2.0);
                    ui.horizontal(|ui| {
                        let label = ui.label("Sort by");
                        egui::ComboBox::from_id_salt("album sort")
                            .selected_text(self.sort_by.label())
                            .show_ui(ui, |ui| {
                                for sort_by in SortBy::ALL {
                                    ui.selectable_value(&mut self.sort_by, sort_by, sort_by.label());
                                }
                            })
                            .response
                            .labelled_by(label.id);
                        ui.checkbox(&mut self.descending, "Descending");
                    });
                    let search = self.search();
                    if search.is_none() {
                        ui.label(
                            RichText::new("Give it a name, and fill in every condition. Shows can be used once.")
                                .size(theme::SIZE_CAPTION)
                                .color(theme::muted()),
                        );
                    }
                    ui.add_space(theme::SPACE * 4.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let save = egui::Button::new(
                            RichText::new("Save")
                                .color(theme::base())
                                .font(FontId::new(theme::SIZE_BODY, theme::semibold())),
                        )
                        .fill(theme::text());
                        if ui.add_enabled(search.is_some(), save).clicked()
                            && let Some(search) = search
                        {
                            let name = domain::smart_albums::clean_name(&self.name).unwrap_or_default();
                            outcome = Some(Outcome::Save {
                                name,
                                search: Box::new(search),
                            });
                        }
                        if ui
                            .add(egui::Button::new("Cancel").stroke(Stroke::new(1.0, theme::rim())))
                            .clicked()
                        {
                            outcome = Some(Outcome::Cancel);
                        }
                    });
                });
            });
        if modal.should_close() && outcome.is_none() {
            outcome = Some(Outcome::Cancel);
        }
        outcome
    }

    fn rows_ui(&mut self, ui: &mut egui::Ui) {
        let mut remove = None;
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (at, row) in self.rows.iter_mut().enumerate() {
                    ui.push_id(at, |ui| {
                        ui.horizontal(|ui| {
                            let complete = row.condition().is_some();
                            egui::ComboBox::from_id_salt("kind")
                                .selected_text(row.kind.label())
                                .width(190.0)
                                .show_ui(ui, |ui| {
                                    for kind in Kind::ALL {
                                        if ui.selectable_label(row.kind == kind, kind.label()).clicked()
                                            && row.kind != kind
                                        {
                                            *row = Row { kind, ..row.clone() };
                                        }
                                    }
                                })
                                .response
                                .widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::Role::ComboBox,
                                        true,
                                        format!("Condition {}", at + 1),
                                    )
                                });
                            value_ui(ui, row, &self.libraries);
                            if !complete {
                                ui.add(crate::icons::Icon::TriangleAlert.image(theme::ICON_SIZE, theme::alert()))
                                    .on_hover_text("This condition needs a value");
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let label = format!("Remove condition {}", at + 1);
                                let button =
                                    ui.add(egui::Button::new(crate::icons::Icon::X.small()).frame_when_inactive(false));
                                button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                                if button.clicked() {
                                    remove = Some(at);
                                }
                            });
                        });
                    });
                }
            });
        if let Some(at) = remove {
            self.rows.remove(at);
        }
    }
}

/// The value fields of `row`'s kind.
fn value_ui(ui: &mut egui::Ui, row: &mut Row, libraries: &[(i64, String)]) {
    let text = |ui: &mut egui::Ui, value: &mut String, hint: &str| {
        ui.add(egui::TextEdit::singleline(value).desired_width(150.0).hint_text(hint))
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, hint));
    };
    let bound = |ui: &mut egui::Ui, row: &mut Row| {
        egui::ComboBox::from_id_salt("bound")
            .selected_text(if row.at_least { "at least" } else { "at most" })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut row.at_least, true, "at least");
                ui.selectable_value(&mut row.at_least, false, "at most");
            })
            .response
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Comparison"));
        ui.add(egui::DragValue::new(&mut row.number).range(0.0..=f64::MAX).speed(0.5))
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, "Value"));
    };
    let library = |ui: &mut egui::Ui, row: &mut Row| {
        let name = row
            .library
            .and_then(|id| libraries.iter().find(|(lib, _)| *lib == id))
            .map_or("Choose a library", |(_, name)| name.as_str());
        egui::ComboBox::from_id_salt("library")
            .selected_text(name)
            .show_ui(ui, |ui| {
                for (id, name) in libraries {
                    ui.selectable_value(&mut row.library, Some(*id), name);
                }
            })
            .response
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Library"));
    };
    match row.kind {
        Kind::Tag | Kind::NotTag => text(ui, &mut row.text, "Tag"),
        Kind::Name => text(ui, &mut row.text, "Part of the name"),
        Kind::Shows => text(ui, &mut row.text, "a dog on a beach"),
        Kind::HasPeople | Kind::HasPlaces | Kind::NoTags => {}
        Kind::MediaKind => {
            egui::ComboBox::from_id_salt("media kind")
                .selected_text(if row.media == MediaKind::Video {
                    "video"
                } else {
                    "photo"
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut row.media, MediaKind::Image, "photo");
                    ui.selectable_value(&mut row.media, MediaKind::Video, "video");
                })
                .response
                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Kind"));
        }
        Kind::RatingIn => {
            // The bundled fonts have no star.
            for (rating, on) in row.ratings.iter_mut().enumerate() {
                let label = if rating == 0 {
                    "Unrated".to_string()
                } else {
                    rating.to_string()
                };
                ui.toggle_value(on, label);
            }
            ui.label("stars");
        }
        Kind::RatingAtLeast => {
            ui.add(egui::DragValue::new(&mut row.stars).range(1..=5).suffix(" stars"))
                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, "Stars"));
        }
        Kind::Library => library(ui, row),
        Kind::Folder => {
            library(ui, row);
            text(ui, &mut row.text, "2023/Rome");
        }
        Kind::TakenBetween | Kind::ModifiedBetween => {
            text(ui, &mut row.text, "From yyyy-mm-dd");
            ui.label("and");
            text(ui, &mut row.text2, "To yyyy-mm-dd");
        }
        Kind::TakenLast | Kind::ModifiedLast => {
            ui.add(egui::DragValue::new(&mut row.number).range(1.0..=9999.0).speed(0.2))
                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, "How many"));
            egui::ComboBox::from_id_salt("unit")
                .selected_text(row.unit.key())
                .show_ui(ui, |ui| {
                    for unit in Unit::ALL {
                        ui.selectable_value(&mut row.unit, unit, unit.key());
                    }
                })
                .response
                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Unit"));
        }
        Kind::SizeMb | Kind::DurationS | Kind::Megapixels => bound(ui, row),
        Kind::Orientation => {
            egui::ComboBox::from_id_salt("orientation")
                .selected_text(row.orientation.key())
                .show_ui(ui, |ui| {
                    for orientation in Orientation::ALL {
                        ui.selectable_value(&mut row.orientation, orientation, orientation.key());
                    }
                })
                .response
                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Orientation"));
        }
    }
}
