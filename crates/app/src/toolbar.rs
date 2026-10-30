//! The toolbar over the top of the gallery (REQ-013-08): the search bar, _Filters_ and _View_, on glass the gallery
//! scrolls under (REQ-013-03). It is also the window's title bar (REQ-013-02).

use crate::gallery::{Gallery, TOOLBAR_HEIGHT};
use crate::icons::{Icon, icon_button};
use crate::search_bar::{self, SearchBar};
use crate::{format, theme};
use chrono::{Datelike, NaiveDate};
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

/// Draws the toolbar over the top of `area`; without a gallery (no libraries yet) it stays empty.
/// Draws the toolbar; whether _Show sidebar_ was clicked (shown while `sidebar_hidden`, REQ-008-08).
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    gallery: Option<&mut Gallery>,
    bar_state: &mut SearchBar,
    sidebar_hidden: bool,
) -> bool {
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
        pos2(bar.left() + 12.0, bar.top() + 8.0),
        pos2(
            bar.right() - 12.0 - crate::titlebar::reserved_in(ui.ctx(), bar.right()),
            bar.bottom() - 8.0,
        ),
    );
    // A stable id: the widgets before the toolbar vary (an empty gallery has none), and auto ids would move the
    // menus' open state with them.
    let mut gallery = gallery;
    ui.scope_builder(egui::UiBuilder::new().id_salt("toolbar").max_rect(controls), |ui| {
        let mut show_sidebar = false;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(gallery) = gallery.as_deref_mut() {
                view_button(ui, gallery);
                filters_button(ui, gallery);
                ui.add_space(8.0);
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if sidebar_hidden {
                    show_sidebar = crate::sidebar::show_button(ui);
                }
                if let Some(gallery) = gallery {
                    search_bar::show(ui, bar_state, gallery);
                }
            });
        });
        show_sidebar
    })
    .inner
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
fn filters_button(ui: &mut egui::Ui, gallery: &mut Gallery) {
    let count = gallery.search().active_filter_count();
    let label = if count > 0 {
        format!("Filters, {count} active")
    } else {
        "Filters".into()
    };
    let text = if count > 0 {
        format!("Filters · {count}")
    } else {
        "Filters".into()
    };
    let response = ui.add(
        egui::Button::new((Icon::SlidersHorizontal.small(), text))
            .frame(count > 0)
            .min_size(vec2(0.0, theme::CONTROL_HEIGHT)),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    menu(&response, "filters menu", 320.0, |ui| filters_menu(ui, gallery));
}

/// A menu under its button. It closes on Escape or a click outside both, but not on clicks in overlays on top of it
/// (the date pickers).
/// The menu's open state lives under the fixed `name`: a button's own id changes with its look (the filter count)
/// and with the widgets before it (an empty gallery has none), and the menu would close with it.
fn menu(button: &egui::Response, name: &str, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    let popup = egui::Popup::from_toggle_button_response(button)
        .id(egui::Id::NULL.with(name))
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        .align(egui::RectAlign::BOTTOM_END)
        .width(width);
    let id = popup.get_id();
    let Some(shown) = popup.show(|ui| crate::glass::behind_menu(ui, add)) else {
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
    ui.spacing_mut().item_spacing.y = 8.0;
    let mut search = gallery.search().clone();

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
    egui::Grid::new("date range")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            let today = domain::time_zone::today();
            ui.label(egui::RichText::new("From").color(theme::muted()));
            if let Some(day) = day_field(ui, "from", search.from, search.to.unwrap_or(today)) {
                search.from = day;
                // The end can't be before the start.
                if let (Some(from), Some(to)) = (search.from, search.to)
                    && to < from
                {
                    search.to = Some(from);
                }
            }
            ui.end_row();
            ui.label(egui::RichText::new("To").color(theme::muted()));
            if let Some(day) = day_field(ui, "to", search.to, search.from.unwrap_or(today)) {
                search.to = day;
                if let (Some(from), Some(to)) = (search.from, search.to)
                    && to < from
                {
                    search.from = Some(to);
                }
            }
            ui.end_row();
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
    gallery.set_search(search);
}

/// A section title with its value and a _Clear_ link when `clearable`; true when it was clicked.
fn section_with_clear(ui: &mut egui::Ui, label: &str, value: &str, clearable: bool, clear_label: &str) -> bool {
    let mut cleared = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).family(theme::semibold()));
        ui.label(egui::RichText::new(value).color(theme::muted()));
        if clearable {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let response = ui.add(egui::Button::new("Clear").frame(false));
                let clear_label = clear_label.to_string();
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &clear_label));
                cleared = response.clicked();
            });
        }
    });
    cleared
}

/// A day of the date range: a date picker once set, or a button that sets it to `fallback`. `Some` when it
/// changed.
fn day_field(ui: &mut egui::Ui, id: &str, day: Option<NaiveDate>, fallback: NaiveDate) -> Option<Option<NaiveDate>> {
    let Some(day) = day else {
        let label = format!("Set {id} day");
        let response = ui.add(egui::Button::new("Any day"));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        return response.clicked().then_some(Some(fallback));
    };
    let mut picked = jiff::civil::date(day.year() as i16, day.month() as i8, day.day() as i8);
    let before = picked;
    ui.add(
        egui_extras::DatePickerButton::new(&mut picked)
            .id_salt(id)
            .calendar_week(false)
            .show_icon(false),
    );
    (picked != before)
        .then(|| NaiveDate::from_ymd_opt(i32::from(picked.year()), picked.month() as u32, picked.day() as u32))
}

/// The _View_ button, showing the current view's icon, and its menu (REQ-006-01, -06, REQ-016-03, REQ-017-03).
fn view_button(ui: &mut egui::Ui, gallery: &mut Gallery) {
    let button = egui::Button::new((
        view_icon(gallery.view()).small(),
        "View",
        Icon::ChevronDown.image(theme::ICON_SIZE, theme::muted()),
    ))
    .frame(false)
    .min_size(vec2(0.0, theme::CONTROL_HEIGHT));
    let response = ui.add(button);
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
        let mut zoom = MIN_COLUMNS + MAX_COLUMNS - columns;
        let slider_width = ui.available_width() - theme::CONTROL_HEIGHT - ui.spacing().item_spacing.x;
        ui.spacing_mut().slider_width = slider_width.max(40.0);
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
        ui.add_enabled_ui(zoomable && columns > MIN_COLUMNS, |ui| {
            if icon_button(ui, Icon::ZoomIn, "Zoom in", "Zoom in (Ctrl + wheel)").clicked() {
                gallery.zoom(-1);
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
