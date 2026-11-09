//! The toolbar over the top of the gallery (REQ-013-08): the search bar, _Filters_ and _View_, on glass the gallery
//! scrolls under (REQ-013-03). It is also the window's title bar (REQ-013-02).

use crate::gallery::{Gallery, TOOLBAR_HEIGHT};
use crate::icons::{Icon, icon_button};
use crate::search_bar::{self, SearchBar};
use crate::unreadable::{ReadAgain, Unreadable};
use crate::{format, theme};
use chrono::NaiveDate;
use domain::media::MediaKind;
use domain::search::SortBy;
use domain::view::{GalleryView, TagIndicatorKind};
use domain::zoom::{MAX_COLUMNS, MIN_COLUMNS};
use eframe::egui::{self, Rect, pos2, vec2};

fn view_icon(view: GalleryView) -> Icon {
    match view {
        GalleryView::Masonry => Icon::LayoutDashboard,
        GalleryView::Justified => Icon::Rows3,
        GalleryView::Grid => Icon::LayoutGrid,
        GalleryView::List => Icon::List,
    }
}

/// What was asked for in the toolbar.
#[derive(Debug, Default)]
pub struct Asked {
    /// _Show sidebar_ (shown while the sidebar is hidden, REQ-008-08).
    pub show_sidebar: bool,
    /// _Read again_ in the list of unreadable files (REQ-002-17).
    pub read_again: Option<ReadAgain>,
    /// _Update and restart_ or _See what's new_ in the update's menu (REQ-031-04).
    pub update: Option<crate::updates::Asked>,
    /// _Faces settings_ in the menu of what's running (REQ-032-20).
    pub face_settings: bool,
}

/// The notices the toolbar may show beside its controls.
pub struct Notices<'a> {
    pub unreadable: &'a Unreadable,
    pub updates: &'a crate::updates::Updates,
    pub work: &'a [crate::processing::Work],
}

/// Draws the toolbar over the top of `area`; without a gallery (no libraries yet) it stays empty.
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    gallery: Option<&mut Gallery>,
    bar_state: &mut SearchBar,
    sidebar_hidden: bool,
    notices: Notices<'_>,
) -> Asked {
    let bar = Rect::from_min_size(area.min, vec2(area.width(), TOOLBAR_HEIGHT));
    // Tiles scrolled under the bar don't react to the pointer; its empty space moves the window.
    let background = ui.interact(bar, ui.make_persistent_id("toolbar"), egui::Sense::click_and_drag());
    crate::titlebar::drag_area(ui.ctx(), &background);
    crate::glass::paint(ui.painter(), crate::glass::Surface::new(bar, 0.0, theme::glass_bar()));
    crate::glass::record_solid(ui.ctx(), bar, theme::glass_bar().fill(None), true);
    ui.painter()
        .hline(bar.x_range(), bar.bottom() - 0.5, egui::Stroke::new(1.0, theme::rim()));
    // The window controls' place stays free (REQ-013-02).
    let controls = Rect::from_min_max(
        pos2(
            bar.left() + 12.0 + crate::titlebar::reserved_left_in(ui.ctx(), bar.left()),
            bar.top() + 8.0,
        ),
        pos2(
            bar.right() - 12.0 - crate::titlebar::reserved_in(ui.ctx(), bar.right()),
            bar.bottom() - 8.0,
        ),
    );
    // A stable id: the widgets before the toolbar vary (an empty gallery has none), and auto ids would move the
    // menus' open state with them.
    let mut gallery = gallery;
    ui.scope_builder(egui::UiBuilder::new().id_salt("toolbar").max_rect(controls), |ui| {
        let mut asked = Asked::default();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(gallery) = gallery.as_deref_mut() {
                view_button(ui, gallery);
                // Next to the search bar, which stays (REQ-025-06).
                if gallery.selected_count() > 0 {
                    ui.add_space(8.0);
                    selection_bar(ui, gallery);
                } else if gallery.count().unwrap_or(0) > 0
                    && icon_button(ui, Icon::CheckCheck, "Select all", "Select all (Ctrl + A)").clicked()
                {
                    gallery.select_all();
                }
            }
            // Also without a gallery: the first thing a new install could show.
            asked.update = crate::updates::notice(ui, notices.updates);
            asked.face_settings = crate::processing::button(ui, notices.work);
            if gallery.is_some() {
                asked.read_again = crate::unreadable::notice(ui, notices.unreadable);
                ui.add_space(8.0);
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if sidebar_hidden {
                    asked.show_sidebar = crate::sidebar::show_button(ui);
                }
                if let Some(gallery) = gallery {
                    // _Filters_ narrow the gallery like the search, so they sit right beside it (REQ-013-08). Its
                    // width changes with the filter count; the search bar leaves room for it as last drawn.
                    let width_id = egui::Id::NULL.with("filters button width");
                    let reserved = ui.ctx().data(|d| d.get_temp(width_id)).unwrap_or(theme::CONTROL_HEIGHT);
                    search_bar::show(ui, bar_state, gallery, reserved + ui.spacing().item_spacing.x);
                    let width = filters_button(ui, gallery).rect.width();
                    if width != reserved {
                        ui.ctx().data_mut(|d| d.insert_temp(width_id, width));
                        ui.ctx().request_discard("the Filters button changed its width");
                    }
                }
            });
        });
        asked
    })
    .inner
}

/// _Clear_, how many items are selected, then _Rate_ and _Tag_ (the desktop app only); drawn into the toolbar's
/// right-to-left part, so from the right (REQ-025-06, REQ-025-07, REQ-026-06).
fn selection_bar(ui: &mut egui::Ui, gallery: &mut Gallery) {
    if !crate::WEB {
        // REQ-035-01: the icon alone, so the search bar keeps its room.
        let rename = egui::Button::new(Icon::Pencil.small())
            .frame_when_inactive(false)
            .min_size(vec2(theme::CONTROL_HEIGHT, theme::CONTROL_HEIGHT));
        let response = ui.add(rename).on_hover_text("Rename the selected files");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Rename…"));
        if response.clicked() {
            gallery.ask_rename();
        }
        // REQ-038-01: beside _Rename…_, the icon alone too.
        let fix = egui::Button::new(Icon::Calendar.small())
            .frame_when_inactive(false)
            .min_size(vec2(theme::CONTROL_HEIGHT, theme::CONTROL_HEIGHT));
        let response = ui.add(fix).on_hover_text("Fix the dates taken of the selected photos");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Fix dates…"));
        if response.clicked() {
            gallery.ask_fix_dates();
        }
        {
            let button = egui::Button::new((
                Icon::Plus.small(),
                "Tag",
                Icon::ChevronDown.image(theme::ICON_SIZE, theme::muted()),
            ))
            .frame_when_inactive(false)
            .min_size(vec2(0.0, theme::CONTROL_HEIGHT));
            let response = ui.add(button);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Tag the selection"));
            let paths = gallery.tag_paths();
            let mut picked = None;
            menu(&response, "tag menu", 300.0, |ui| picked = tag_menu(ui, &paths));
            if let Some(path) = picked {
                egui::Popup::close_id(ui.ctx(), egui::Id::NULL.with("tag menu"));
                gallery.tag_selection(path);
            }
        }
        {
            let button = egui::Button::new((
                Icon::StarFilled.small(),
                "Rate",
                Icon::ChevronDown.image(theme::ICON_SIZE, theme::muted()),
            ))
            .frame_when_inactive(false)
            .min_size(vec2(0.0, theme::CONTROL_HEIGHT));
            let response = ui.add(button);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Rate the selection"));
            let mut chosen = None;
            menu(&response, "rate menu", 200.0, |ui| {
                for stars in (1..=domain::rating::MAX).rev() {
                    let label = if stars == 1 {
                        "1 star".to_owned()
                    } else {
                        format!("{stars} stars")
                    };
                    let row = egui::Button::new(egui::RichText::new(&label)).frame_when_inactive(false);
                    let clicked = ui
                        .horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            for star in 1..=domain::rating::MAX {
                                ui.add(crate::icons::rating_star(star <= stars, theme::ICON_SIZE));
                            }
                            ui.add_space(6.0);
                            ui.add(row)
                        })
                        .inner;
                    clicked.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                    if clicked.clicked() {
                        chosen = Some(Some(stars));
                    }
                }
                ui.separator();
                if ui.button("Remove rating").clicked() {
                    chosen = Some(None);
                }
            });
            if let Some(rating) = chosen {
                egui::Popup::close_id(ui.ctx(), egui::Id::NULL.with("rate menu"));
                gallery.rate_selection(rating);
            }
        }
        ui.add_space(8.0);
    }
    let count = format::count(gallery.selected_count() as u64);
    ui.label(egui::RichText::new(format!("{count} selected")).family(theme::semibold()));
    if icon_button(ui, Icon::X, "Clear selection", "Clear (Esc)").clicked() {
        gallery.clear_selection();
    }
}

/// _Tag_: People, Places or Tags, and the tag to add to every selected item, with suggestions (REQ-026-06).
fn tag_menu(ui: &mut egui::Ui, paths: &crate::gallery::TagPaths) -> Option<Vec<String>> {
    use crate::info::{Adding, Group, add_field};
    let id = egui::Id::NULL.with("tag menu field");
    let groups = Group::all(paths);
    let mut chosen = ui.data(|data| data.get_temp::<usize>(id.with("group"))).unwrap_or(2);
    ui.horizontal(|ui| {
        for (index, group) in groups.iter().enumerate() {
            if ui.selectable_label(chosen == index, group.label).clicked() {
                chosen = index;
            }
        }
    });
    ui.data_mut(|data| data.insert_temp(id.with("group"), chosen));
    let typed = ui.data(|data| data.get_temp::<String>(id));
    let opened = typed.is_none();
    let mut typed = typed.unwrap_or_default();
    match add_field(ui, id.with("text"), opened, &groups[chosen], &mut typed) {
        Adding::Add(path) => {
            ui.data_mut(|data| data.remove::<String>(id));
            Some(path)
        }
        // A click on a group takes the focus, but the menu stays; it closes itself.
        Adding::Open | Adding::Close => {
            ui.data_mut(|data| data.insert_temp(id, typed));
            None
        }
    }
}

/// "Any date", one day, a span, or an open start or end (REQ-007-08).
pub fn date_range_label(from: Option<NaiveDate>, to: Option<NaiveDate>) -> String {
    let day = |day: NaiveDate| day.format("%-d %b %Y").to_string();
    match (from, to) {
        (Some(from), Some(to)) if from == to => day(from),
        (Some(from), Some(to)) => format!("{} – {}", day(from), day(to)),
        (Some(from), None) => format!("From {}", day(from)),
        (None, Some(to)) => format!("Until {}", day(to)),
        (None, None) => "Any date".into(),
    }
}

/// _Filters_: media kind, date range and rating; the button shows how many are set (REQ-007-07 … -09,
/// REQ-013-08).
fn filters_button(ui: &mut egui::Ui, gallery: &mut Gallery) -> egui::Response {
    let count = gallery.search().active_filter_count();
    let label = if count > 0 {
        format!("Filters, {count} active")
    } else {
        "Filters".into()
    };
    // The icon, and how many filters are set; its name is the tip (REQ-013-19).
    let button = if count > 0 {
        egui::Button::new((Icon::Funnel.small(), count.to_string()))
    } else {
        egui::Button::new(Icon::Funnel.small())
    };
    let response = ui
        .add(
            button
                .frame_when_inactive(count > 0)
                .min_size(vec2(theme::CONTROL_HEIGHT, theme::CONTROL_HEIGHT)),
        )
        .on_hover_text("Filters");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    menu(
        &response,
        "filters menu",
        CALENDAR_WIDTH + 16.0 + OTHER_FILTERS_WIDTH,
        |ui| filters_menu(ui, gallery),
    );
    response
}

/// A menu under its button. It closes on Escape or a click outside both, but not on clicks in overlays on top of it
/// (a combo box's list).
/// The menu's open state lives under the fixed `name`: a button's own id changes with its look (the filter count)
/// and with the widgets before it (an empty gallery has none), and the menu would close with it.
pub(crate) fn menu(button: &egui::Response, name: &str, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    let popup = egui::Popup::from_toggle_button_response(button)
        .id(egui::Id::NULL.with(name))
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        .align(egui::RectAlign::BOTTOM_END)
        .width(width);
    let id = popup.get_id();
    let Some(shown) = popup.show(|ui| {
        // The content gets the menu's width, not the width it had the frame before: rows that share out the
        // available width would otherwise widen the menu a little every frame (#285).
        let margin = ui.style().spacing.menu_margin.sum().x;
        ui.set_width(width - margin);
        crate::glass::behind_menu(ui, add)
    }) else {
        return;
    };
    let ctx = &button.ctx;
    let (click, escape) = ctx.input(|input| {
        let click = input
            .pointer
            .any_click()
            .then(|| input.pointer.interact_pos())
            .flatten();
        (click, input.key_pressed(egui::Key::Escape))
    });
    let outside = click.is_some_and(|at| {
        let on_overlay = ctx.layer_id_at(at).is_some_and(|layer| {
            layer.order == egui::Order::Foreground
                && layer != shown.response.layer_id
                && layer.id != search_bar::suggestions_id()
        });
        !shown.response.rect.contains(at) && !button.rect.contains(at) && !on_overlay
    });
    if outside || escape {
        egui::Popup::close_id(ctx, id);
    }
}

fn filters_menu(ui: &mut egui::Ui, gallery: &mut Gallery) {
    let mut search = gallery.search().clone();
    // The date and its calendar at the left, the rest at the right, so the menu fits a window of the minimum size
    // (REQ-013-08, REQ-041-01).
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        ui.allocate_ui_with_layout(
            vec2(CALENDAR_WIDTH, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(CALENDAR_WIDTH);
                ui.spacing_mut().item_spacing.y = 8.0;
                let has_date = search.from.is_some() || search.to.is_some();
                if section_with_clear(
                    ui,
                    "Date",
                    &date_range_label(search.from, search.to),
                    has_date,
                    "Clear date range",
                ) {
                    (search.from, search.to) = (None, None);
                }
                // A calendar with the photos on its days (REQ-041-01).
                crate::calendar::show(ui, gallery, &mut search);
            },
        );
        ui.allocate_ui_with_layout(
            vec2(OTHER_FILTERS_WIDTH, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(OTHER_FILTERS_WIDTH);
                ui.spacing_mut().item_spacing.y = 8.0;
                section(ui, "Media type", None);
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0;
                    let kinds = [
                        (None, "All", None),
                        (Some(MediaKind::Image), "Photos", Some(Icon::Image)),
                        (Some(MediaKind::Video), "Videos", Some(Icon::Video)),
                    ];
                    for (kind, label, icon) in kinds {
                        let button = match icon {
                            Some(icon) => egui::Button::new((icon.small(), label)),
                            None => egui::Button::new(label),
                        }
                        .min_size(vec2(width, theme::CONTROL_HEIGHT - 4.0));
                        if toggle(ui, search.kind == kind, true, button, label) {
                            search.kind = kind;
                        }
                    }
                });

                let has_rating = !search.ratings.is_empty();
                if section_with_clear(
                    ui,
                    "Rating",
                    &format::ratings(&search.ratings),
                    has_rating,
                    "Clear rating",
                ) {
                    search.ratings.clear();
                }
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 5.0 * ui.spacing().item_spacing.x) / 6.0;
                    for rating in 0..=5u8 {
                        let (button, label) = if rating == 0 {
                            (egui::Button::image(Icon::StarOff.small()), "Unrated".to_string())
                        } else {
                            (egui::Button::new(rating.to_string()), format::rating(rating))
                        };
                        let selected = search.ratings.contains(&rating);
                        if toggle(
                            ui,
                            selected,
                            true,
                            button.min_size(vec2(width, theme::CONTROL_HEIGHT - 4.0)),
                            &label,
                        ) {
                            search.toggle_rating(rating);
                        }
                    }
                });

                ui.separator();
                if ui
                    .add_enabled(search.active_filter_count() > 0, egui::Button::new("Clear filters"))
                    .clicked()
                {
                    search.clear_filters();
                }
                // Below the filters, as it keeps them all with the tags and the file name (REQ-034-01).
                if ui
                    .add_enabled(gallery.can_save_album(), egui::Button::new("Save as smart album…"))
                    .clicked()
                {
                    gallery.set_search(search.clone());
                    gallery.ask_save_album();
                    ui.close();
                }
            },
        );
    });
    gallery.set_search(search);
}

/// The filters menu's columns: the date calendar, and the media type, rating and actions.
const CALENDAR_WIDTH: f32 = 300.0;
const OTHER_FILTERS_WIDTH: f32 = 236.0;

/// A section title with its value and a _Clear_ link when `clearable`; true when it was clicked.
fn section_with_clear(ui: &mut egui::Ui, label: &str, value: &str, clearable: bool, clear_label: &str) -> bool {
    let mut cleared = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).family(theme::semibold()));
        ui.label(egui::RichText::new(value).color(theme::muted()));
        if clearable {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let response = ui.add(egui::Button::new("Clear").frame_when_inactive(false));
                let clear_label = clear_label.to_string();
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &clear_label));
                cleared = response.clicked();
            });
        }
    });
    cleared
}

/// The _View_ button, showing the current view's icon, and its menu (REQ-006-01, -06, REQ-016-03, REQ-017-03).
fn view_button(ui: &mut egui::Ui, gallery: &mut Gallery) {
    // The icon of the current view; its name is the tip (REQ-013-19).
    let chevron = Icon::ChevronDown.image(theme::ICON_SIZE, theme::muted());
    let button = egui::Button::new((view_icon(gallery.view()).small(), chevron))
        .frame_when_inactive(false)
        .min_size(vec2(0.0, theme::CONTROL_HEIGHT));
    let response = ui.add(button).on_hover_text("View");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "View options"));
    menu(&response, "view menu", 288.0, |ui| view_menu(ui, gallery));
}

/// A section title with its current value on the right.
fn section(ui: &mut egui::Ui, label: &str, value: Option<&str>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).family(theme::semibold()));
        if let Some(value) = value {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(value).color(theme::muted()));
            });
        }
    });
}

/// A toggle showing `content`, named `label` for screen readers and tests.
fn toggle(ui: &mut egui::Ui, selected: bool, enabled: bool, content: egui::Button<'_>, label: &str) -> bool {
    let response = ui.add_enabled(enabled, content.selected(selected)).on_hover_text(label);
    let label = label.to_string();
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, selected, &label));
    response.clicked()
}

fn view_menu(ui: &mut egui::Ui, gallery: &mut Gallery) {
    ui.spacing_mut().item_spacing.y = 8.0;
    let view = gallery.view();
    let zoomable = view != GalleryView::List;

    // Sort order (REQ-007-12).
    section(ui, "Sort by", None);
    let mut search = gallery.search().clone();
    // Toggles rather than a drop-down: a popup inside this popup would close it.
    ui.horizontal_wrapped(|ui| {
        for field in SortBy::ALL {
            if toggle(
                ui,
                search.sort_by == field,
                true,
                egui::Button::new(field.label()),
                field.label(),
            ) {
                search.sort_by = field;
            }
        }
    });
    ui.horizontal(|ui| {
        let (icon, label) = if search.descending {
            (Icon::ArrowDownWideNarrow, "Descending")
        } else {
            (Icon::ArrowUpNarrowWide, "Ascending")
        };
        let response = ui.add(egui::Button::new((icon.small(), label)));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        if response.clicked() {
            search.descending = !search.descending;
        }
    });
    gallery.set_search(search);

    section(ui, "Layout", Some(view.label()));
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 3.0 * ui.spacing().item_spacing.x) / 4.0;
        for option in GalleryView::ALL {
            let button = egui::Button::image(view_icon(option).small()).min_size(vec2(width, theme::CONTROL_HEIGHT));
            // Clicking the active view keeps it.
            if toggle(ui, option == view, true, button, option.label()) {
                gallery.set_view(option);
            }
        }
    });

    let columns = gallery.columns;
    let zoom_value = if zoomable {
        format!("{columns} columns")
    } else {
        "Not in the List view".into()
    };
    section(ui, "Zoom", Some(&zoom_value));
    ui.horizontal(|ui| {
        // Zooming in means fewer, wider columns.
        ui.add_enabled_ui(zoomable && columns < MAX_COLUMNS, |ui| {
            if icon_button(ui, Icon::ZoomOut, "Zoom out", "Zoom out (Ctrl + wheel)").clicked() {
                gallery.zoom(1);
            }
        });
        // _Zoom in_ first, from the right, so the slider takes exactly the room between the buttons.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled_ui(zoomable && columns > MIN_COLUMNS, |ui| {
                if icon_button(ui, Icon::ZoomIn, "Zoom in", "Zoom in (Ctrl + wheel)").clicked() {
                    gallery.zoom(-1);
                }
            });
            let mut zoom = MIN_COLUMNS + MAX_COLUMNS - columns;
            ui.spacing_mut().slider_width = ui.available_width().max(40.0);
            let response = ui.add_enabled(
                zoomable,
                egui::Slider::new(&mut zoom, MIN_COLUMNS..=MAX_COLUMNS)
                    .show_value(false)
                    .trailing_fill(true),
            );
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, zoomable, "Zoom"));
            if response.changed() {
                gallery.set_columns(MIN_COLUMNS + MAX_COLUMNS - zoom);
            }
        });
    });

    ui.separator();
    // Days only make sense in date order; the choice is kept for when the user sorts by a date again.
    let date_sort = gallery.is_date_sort();
    let mut grouped = gallery.group_by_day();
    if ui
        .add_enabled(date_sort, egui::Checkbox::new(&mut grouped, "Group by day"))
        .changed()
    {
        gallery.set_group_by_day(grouped);
    }
    if !date_sort {
        ui.label(egui::RichText::new("Only when sorted by date").color(theme::muted()));
    }

    // The List view has no tiles to show indicators on (it has a rating column).
    let mut choice = gallery.tag_indicators().clone();
    if ui
        .add_enabled(zoomable, egui::Checkbox::new(&mut choice.shown, "Tag indicators"))
        .changed()
    {
        gallery.set_tag_indicators(choice.clone());
    }
    if !zoomable {
        ui.label(egui::RichText::new("Not in the List view").color(theme::muted()));
    }
    // The picks are kept while indicators are off, so turning them on again restores them.
    ui.horizontal(|ui| {
        // Four names in a row only fit with less padding around each.
        ui.spacing_mut().button_padding.x = 2.0;
        let width = (ui.available_width() - 3.0 * ui.spacing().item_spacing.x) / 4.0;
        for kind in TagIndicatorKind::ALL {
            let button = egui::Button::new(kind.label()).min_size(vec2(width, theme::CONTROL_HEIGHT - 4.0));
            if toggle(
                ui,
                choice.picked.contains(&kind),
                choice.shown && zoomable,
                button,
                kind.label(),
            ) {
                choice.toggle(kind);
                gallery.set_tag_indicators(choice.clone());
            }
        }
    });
}
