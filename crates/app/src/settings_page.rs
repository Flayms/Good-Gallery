//! The Settings page: libraries (spec 001), the sidebar's groups (spec 008), indexing and the thumbnail cache
//! (spec 010). Changes apply right away.

use crate::icons::{Icon, icon_button};
use crate::state::{AppState, Page};
use crate::{format, theme};
use db::libraries::{Library, LibraryStatus};
use domain::appearance::Appearance;
use domain::sidebar::SidebarLayout;
use eframe::egui::{self, Color32, CornerRadius, FontId, Frame, Margin, Rect, RichText, Stroke, Vec2};

/// Content width; descriptions stay below 70 characters (design guide).
const MAX_WIDTH: f32 = 720.0;
const HEADER_HEIGHT: f32 = crate::gallery::TOOLBAR_HEIGHT;

/// The glass header over the page, which is also the window's title bar (REQ-013-02, REQ-013-03): back to the
/// gallery and the page's title; its empty space moves the window.
fn page_header(ui: &mut egui::Ui, rect: Rect, state: &mut AppState) {
    let background = ui.interact(
        rect,
        ui.make_persistent_id("settings header"),
        egui::Sense::click_and_drag(),
    );
    crate::titlebar::drag_area(ui.ctx(), &background);
    crate::glass::paint(ui.painter(), crate::glass::Surface::new(rect, 0.0, theme::glass_bar()));
    ui.painter()
        .hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, theme::rim()));
    let controls = Rect::from_min_max(
        egui::pos2(rect.left() + 12.0, rect.top() + 8.0),
        egui::pos2(
            rect.right() - 12.0 - crate::titlebar::reserved_in(ui.ctx(), rect.right()),
            rect.bottom() - 8.0,
        ),
    );
    ui.scope_builder(
        egui::UiBuilder::new().id_salt("settings header").max_rect(controls),
        |ui| {
            ui.horizontal_centered(|ui| {
                if state.sidebar.is_hidden() && crate::sidebar::show_button(ui) {
                    state.sidebar.set_hidden(false, state.db.as_ref());
                }
                let back = icon_button(ui, Icon::ChevronLeft, "Back to gallery", "Back to gallery");
                if back.clicked() {
                    state.page = Page::Gallery;
                }
                ui.label(RichText::new("Settings").font(FontId::new(theme::SIZE_TITLE, theme::semibold())));
            });
        },
    );
}

pub fn ui(ui: &mut egui::Ui, state: &mut AppState) {
    let area = ui.max_rect();
    let header = Rect::from_min_size(area.min, Vec2::new(area.width(), HEADER_HEIGHT));
    // The page scrolls under the header; its scrollbar starts below it (REQ-013-15).
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .scroll_bar_rect(Rect::from_min_max(egui::pos2(area.left(), header.bottom()), area.max))
        .show(ui, |ui| {
            // Centered column, at least 24 px from the window edges.
            let width = (ui.available_width() - 48.0).min(MAX_WIDTH);
            let margin = ((ui.available_width() - width) / 2.0).max(0.0);
            ui.add_space(HEADER_HEIGHT + 24.0);
            ui.horizontal_top(|ui| {
                ui.add_space(margin);
                ui.vertical(|ui| {
                    ui.set_width(width);
                    libraries(ui, state);
                    ui.add_space(theme::SPACE * 10.0);
                    appearance(ui, state);
                    ui.add_space(theme::SPACE * 10.0);
                    sidebar(ui, state);
                    ui.add_space(theme::SPACE * 10.0);
                    // The browser has no watchers, scheduled checks or parallel reads to tune (REQ-019-13).
                    if !crate::WEB {
                        indexing(ui, state);
                        ui.add_space(theme::SPACE * 10.0);
                    }
                    thumbnail_cache(ui, state);
                    ui.add_space(24.0);
                });
            });
        });
    page_header(ui, header, state);
    confirm_remove(ui.ctx(), state);
    confirm_clear(ui.ctx(), state);
}

/// Quick check intervals offered, in minutes (REQ-010-03).
const QUICK_CHECK_INTERVALS: [u32; 7] = [0, 15, 30, 60, 180, 360, 1440];
/// Thumbnail cache sizes offered, in GB (REQ-010-05).
const CACHE_SIZES_GIB: [f64; 7] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0];
const MAX_CONCURRENCY: u32 = 16;
const SLIDER_WIDTH: f32 = 160.0;

/// "Only at startup", "Every 15 minutes", "Every hour", "Every 3 hours", "Daily".
pub fn interval_label(minutes: u32) -> String {
    match minutes {
        0 => "Only at startup".into(),
        1 => "Every minute".into(),
        2..60 => format!("Every {minutes} minutes"),
        60 => "Every hour".into(),
        1440 => "Daily".into(),
        _ => {
            // One decimal at most, without a trailing ".0": "Every 1.5 hours".
            let hours = (f64::from(minutes) / 60.0 * 10.0).round() / 10.0;
            format!("Every {hours} hours")
        }
    }
}

/// "5 GB"; a stored non-preset value keeps its decimals ("0.5 GB").
pub fn cache_size_label(gib: f64) -> String {
    format!("{gib} GB")
}

/// The preset options plus the current value, should it be something else (REQ-010-03, REQ-010-05).
fn with_current<T: Copy + PartialOrd>(options: &[T], current: T) -> Vec<T> {
    let mut all = options.to_vec();
    if !all.contains(&current) {
        all.push(current);
        all.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }
    all
}

fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).font(FontId::new(theme::SIZE_SECTION, theme::semibold())));
    ui.add_space(theme::SPACE * 2.0);
}

/// Rows in a bordered group, divided by hairlines.
fn group(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    Frame::new()
        .stroke(Stroke::new(1.0, theme::rim()))
        .corner_radius(CornerRadius::same(theme::RADIUS_PANEL))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui);
        });
}

/// One setting: label and description on the left, its control on the right (`control_width` wide).
fn setting_row(
    ui: &mut egui::Ui,
    label: &str,
    description: &str,
    control_width: f32,
    control: impl FnOnce(&mut egui::Ui),
) {
    Frame::new().inner_margin(Margin::same(12)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let text_width = (ui.available_width() - control_width - theme::SPACE * 6.0).max(120.0);
            ui.allocate_ui_with_layout(
                Vec2::new(text_width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.spacing_mut().item_spacing.y = theme::SPACE;
                    ui.label(RichText::new(label).font(FontId::new(theme::SIZE_BODY, theme::semibold())));
                    ui.add(egui::Label::new(RichText::new(description).color(theme::muted())).wrap());
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
        });
    });
}

/// Gives a combo box an accessible name besides its value (egui names it only by a label drawn beside it).
fn name_combo(response: &egui::Response, name: &str, value: String) {
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, response.enabled(), name);
        info.current_text_value = Some(value.clone());
        info
    });
}

fn divider(ui: &mut egui::Ui) {
    let rect = ui.available_rect_before_wrap();
    ui.painter()
        .hline(rect.x_range(), rect.top(), Stroke::new(1.0, theme::rim()));
}

/// _Appearance_: light, dark or the system's, applied from the next frame on (REQ-020-01).
fn appearance(ui: &mut egui::Ui, state: &mut AppState) {
    section_title(ui, "Appearance");
    let mut settings = state.settings;
    group(ui, |ui| {
        setting_row(
            ui,
            "Light or dark",
            "System follows your computer's or browser's setting. The viewer stays dark.",
            160.0,
            |ui| {
                let current = settings.appearance;
                let combo = egui::ComboBox::from_id_salt("appearance")
                    .width(160.0)
                    .selected_text(current.label())
                    .show_ui(ui, |ui| {
                        crate::glass::behind_menu(ui, |ui| {
                            for option in Appearance::ALL {
                                ui.selectable_value(&mut settings.appearance, option, option.label());
                            }
                        });
                    });
                name_combo(&combo.response, "Appearance", current.label().to_owned());
            },
        );
    });
    if settings != state.settings {
        state.update_settings(settings);
    }
}

/// _Sidebar_: the groups below the libraries, each with a switch and _Move up_ / _Move down_, and _Reset to default_
/// (REQ-008-10, REQ-008-11).
fn sidebar(ui: &mut egui::Ui, state: &mut AppState) {
    ui.label(RichText::new("Sidebar").font(FontId::new(theme::SIZE_SECTION, theme::semibold())));
    ui.label(RichText::new("Groups shown below the libraries, from top to bottom.").color(theme::muted()));
    ui.add_space(theme::SPACE * 2.0);
    let mut layout = state.sidebar.layout().clone();
    let groups = layout.groups().to_vec();
    group(ui, |ui| {
        for (i, &(sidebar_group, on)) in groups.iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            let label = sidebar_group.label();
            Frame::new().inner_margin(Margin::symmetric(12, 6)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut checked = on;
                    if ui.checkbox(&mut checked, label).changed() {
                        layout.set_shown(sidebar_group, checked);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let last = i + 1 == groups.len();
                        let down = ui.add_enabled_ui(!last, |ui| {
                            icon_button(ui, Icon::ChevronDown, &format!("Move {label} down"), "Move down")
                        });
                        if down.inner.clicked() {
                            layout.move_down(sidebar_group);
                        }
                        let up = ui.add_enabled_ui(i > 0, |ui| {
                            icon_button(ui, Icon::ChevronUp, &format!("Move {label} up"), "Move up")
                        });
                        if up.inner.clicked() {
                            layout.move_up(sidebar_group);
                        }
                    });
                });
            });
        }
    });
    ui.add_space(theme::SPACE * 3.0);
    let reset = egui::Button::new("Reset to default").stroke(Stroke::new(1.0, theme::rim()));
    if ui.add_enabled(!layout.is_default(), reset).clicked() {
        layout = SidebarLayout::default();
    }
    if layout != *state.sidebar.layout() {
        state.sidebar.set_layout(layout, state.db.as_ref());
    }
}

/// _Indexing_: how often to check for missed changes and how many files to read at once (REQ-010-03, -04).
fn indexing(ui: &mut egui::Ui, state: &mut AppState) {
    section_title(ui, "Indexing");
    let mut settings = state.settings;
    group(ui, |ui| {
        setting_row(
            ui,
            "Check for missed changes",
            "Changes are picked up as they happen; this re-checks changed folders in case a notification got lost.",
            224.0,
            |ui| {
                let current = settings.quick_check_interval_minutes;
                let combo = egui::ComboBox::from_id_salt("check interval")
                    .width(224.0)
                    .selected_text(interval_label(current))
                    .show_ui(ui, |ui| {
                        crate::glass::behind_menu(ui, |ui| {
                            for minutes in with_current(&QUICK_CHECK_INTERVALS, current) {
                                ui.selectable_value(
                                    &mut settings.quick_check_interval_minutes,
                                    minutes,
                                    interval_label(minutes),
                                );
                            }
                        });
                    });
                name_combo(&combo.response, "Check interval", interval_label(current));
            },
        );
        divider(ui);
        setting_row(
            ui,
            "Parallel file reads",
            "Higher is faster on fast disks and NAS; lower it if the share becomes sluggish.",
            SLIDER_WIDTH + 32.0,
            |ui| {
                let page = &mut state.settings_page;
                let mut shown = f64::from(page.concurrency_drag.unwrap_or(settings.io_concurrency));
                ui.label(RichText::new(format!("{shown}")).font(FontId::new(theme::SIZE_BODY, theme::numbers())));
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(SLIDER_WIDTH, theme::CONTROL_HEIGHT), egui::Sense::hover());
                let response = crate::widgets::slider(
                    ui,
                    rect,
                    &mut shown,
                    1.0..=f64::from(MAX_CONCURRENCY),
                    1.0,
                    "Parallel file reads",
                );
                let value = shown.round() as u32;
                // Saved when released, not while dragged (REQ-010-04); keys save at once.
                if response.is_pointer_button_down_on() {
                    page.concurrency_drag = Some(value);
                } else {
                    page.concurrency_drag = None;
                    settings.io_concurrency = value;
                }
            },
        );
    });
    if settings != state.settings {
        state.update_settings(settings);
    }
}

/// _Thumbnail cache_: its limit, how much it uses, and clearing it (REQ-010-05, -06).
fn thumbnail_cache(ui: &mut egui::Ui, state: &mut AppState) {
    section_title(ui, "Thumbnail cache");
    let next = state.refresh_cache_usage();
    ui.ctx().request_repaint_after(next);
    let mut settings = state.settings;
    let mut clear = false;
    group(ui, |ui| {
        setting_row(
            ui,
            "Maximum size",
            "Least recently viewed thumbnails are removed first.",
            128.0,
            |ui| {
                let current = settings.thumb_cache_gib;
                let combo = egui::ComboBox::from_id_salt("cache size")
                    .width(128.0)
                    .selected_text(cache_size_label(current))
                    .show_ui(ui, |ui| {
                        crate::glass::behind_menu(ui, |ui| {
                            for gib in with_current(&CACHE_SIZES_GIB, current) {
                                ui.selectable_value(&mut settings.thumb_cache_gib, gib, cache_size_label(gib));
                            }
                        });
                    });
                name_combo(&combo.response, "Maximum cache size", cache_size_label(current));
            },
        );
        divider(ui);
        setting_row(
            ui,
            "Used",
            "Clearing frees the space; thumbnails are rendered again when they're next shown.",
            180.0,
            |ui| {
                let button =
                    egui::Button::image_and_text(Icon::Trash2.small(), "Clear").stroke(Stroke::new(1.0, theme::rim()));
                clear = ui.add_enabled(!state.settings_page.clearing, button).clicked();
                ui.label(
                    RichText::new(state.cache_usage.label()).font(FontId::new(theme::SIZE_BODY, theme::numbers())),
                );
            },
        );
    });
    if clear {
        state.settings_page.confirm_clear = true;
    }
    if settings != state.settings {
        state.update_settings(settings);
    }
}

/// "Delete all cached thumbnails?" with Cancel / Clear (REQ-010-06).
fn confirm_clear(ctx: &egui::Context, state: &mut AppState) {
    if !state.settings_page.confirm_clear {
        return;
    }
    if let Some(clear) = confirm(
        ctx,
        "confirm-clear",
        "Clear thumbnail cache",
        "Delete all cached thumbnails?",
        "Clear",
    ) {
        state.settings_page.confirm_clear = false;
        if clear {
            state.clear_thumbnail_cache();
        }
    }
}

fn libraries(ui: &mut egui::Ui, state: &mut AppState) {
    ui.label(RichText::new("Libraries").font(FontId::new(theme::SIZE_SECTION, theme::semibold())));
    let description = if crate::WEB {
        "Folders on this computer, indexed recursively in this browser."
    } else {
        "Folders on local disks or network shares, indexed recursively."
    };
    ui.label(RichText::new(description).color(theme::muted()));
    ui.add_space(theme::SPACE * 2.0);

    let page = &mut state.settings_page;
    let busy = page.adding || page.picking;
    let mut add = false;
    let mut browse = false;
    ui.horizontal(|ui| {
        // The browser has no paths to type: folders come from its picker (REQ-019-13).
        if crate::WEB {
            let button = egui::Button::image_and_text(
                Icon::FolderPlus.image(theme::ICON_SIZE, theme::base()),
                RichText::new("Add folder…")
                    .color(theme::base())
                    .font(FontId::new(theme::SIZE_BODY, theme::semibold())),
            )
            .fill(theme::text());
            browse = ui.add_enabled(!busy, button).clicked();
            return;
        }
        let buttons_width = 190.0;
        let input = ui.add(
            egui::TextEdit::singleline(&mut page.path_input)
                .hint_text("D:\\Photos or \\\\server\\share\\photos")
                .desired_width(ui.available_width() - buttons_width)
                .min_size(Vec2::new(0.0, theme::CONTROL_HEIGHT))
                .align(egui::Align2::LEFT_CENTER),
        );
        let enter = input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let can_add = !page.path_input.trim().is_empty() && !busy;
        let add_button = egui::Button::image_and_text(
            Icon::FolderPlus.image(theme::ICON_SIZE, theme::base()),
            RichText::new("Add")
                .color(theme::base())
                .font(FontId::new(theme::SIZE_BODY, theme::semibold())),
        )
        .fill(theme::text());
        add = ui.add_enabled(can_add, add_button).clicked() || (enter && can_add);
        let browse_button =
            egui::Button::image_and_text(Icon::FolderOpen.small(), "Browse…").stroke(Stroke::new(1.0, theme::rim()));
        browse = ui.add_enabled(!busy, browse_button).clicked();
    });
    if add {
        let path = page.path_input.clone();
        state.add_library(path);
    }
    if browse {
        state.browse_for_library();
    }

    ui.add_space(theme::SPACE * 3.0);
    let mut rescan = None;
    let libraries = &state.libraries;
    Frame::new()
        .stroke(Stroke::new(1.0, theme::rim()))
        .corner_radius(CornerRadius::same(theme::RADIUS_PANEL))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if libraries.is_empty() {
                ui.label(RichText::new("No libraries yet.").color(theme::muted()));
            }
            for (i, library) in libraries.iter().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                match library_row(ui, library) {
                    Some(RowAction::Rescan) => rescan = Some(library.id),
                    Some(RowAction::Remove) => state.settings_page.confirm_remove = Some(library.clone()),
                    None => {}
                }
            }
        });
    if let Some(id) = rescan {
        state.rescan_library(id);
    }
}

enum RowAction {
    Rescan,
    Remove,
}

fn library_row(ui: &mut egui::Ui, library: &Library) -> Option<RowAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        let buttons = 2.0 * theme::CONTROL_HEIGHT + theme::SPACE * 2.0;
        ui.allocate_ui(Vec2::new(ui.available_width() - buttons, 40.0), |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme::SPACE;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&library.label).font(FontId::new(theme::SIZE_BODY, theme::semibold())));
                    status_badge(ui, library.status);
                });
                let mut details = library.path.clone();
                if let Some(at) = library.last_scan_at {
                    details.push_str(&format!(" · last indexed {}", format::medium_date_time(at)));
                }
                ui.add(
                    egui::Label::new(RichText::new(details).size(theme::SIZE_CAPTION).color(theme::muted())).truncate(),
                )
                .on_hover_text(&library.path);
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = &library.label;
            if icon_button(
                ui,
                Icon::Trash2,
                &format!("Remove {label}"),
                "Remove from gallery (files stay untouched)",
            )
            .clicked()
            {
                action = Some(RowAction::Remove);
            }
            if icon_button(ui, Icon::RefreshCw, &format!("Rescan {label}"), "Rescan").clicked() {
                action = Some(RowAction::Rescan);
            }
        });
    });
    action
}

fn status_badge(ui: &mut egui::Ui, status: LibraryStatus) {
    let (fill, stroke, text) = match status {
        LibraryStatus::Online => (theme::raised(), Color32::TRANSPARENT, theme::text()),
        LibraryStatus::Offline => (theme::alert(), Color32::TRANSPARENT, theme::base()),
        LibraryStatus::Unknown => (Color32::TRANSPARENT, theme::rim(), theme::muted()),
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, stroke))
        .corner_radius(CornerRadius::same(u8::MAX))
        .inner_margin(Margin::symmetric(8, 1))
        .show(ui, |ui| {
            ui.label(
                RichText::new(crate::status_label(status))
                    .size(theme::SIZE_CAPTION)
                    .color(text),
            );
        });
}

/// "Remove "<label>" from the gallery? Files on disk are not touched." with Cancel / Remove.
fn confirm_remove(ctx: &egui::Context, state: &mut AppState) {
    let Some(library) = state.settings_page.confirm_remove.clone() else {
        return;
    };
    let text = format!(
        "Remove \"{}\" from the gallery? Files on disk are not touched.",
        library.label
    );
    if let Some(remove) = confirm(ctx, "confirm-remove", "Remove library", &text, "Remove") {
        state.settings_page.confirm_remove = None;
        if remove {
            state.remove_library(library.id);
        }
    }
}

/// A dialog with `title`, `text`, Cancel and `verb`: `Some(true)` once confirmed, `Some(false)` once cancelled.
fn confirm(ctx: &egui::Context, id: &str, title: &str, text: &str, verb: &str) -> Option<bool> {
    let mut decided = None;
    let modal = egui::Modal::new(egui::Id::unique(id))
        .frame(
            Frame::new()
                .fill(theme::raised())
                .stroke(Stroke::new(1.0, theme::rim()))
                .corner_radius(CornerRadius::same(theme::RADIUS_PANEL))
                .inner_margin(Margin::same(20)),
        )
        .show(ctx, |ui| {
            crate::glass::behind_frame(ui, Margin::same(20), |ui| {
                ui.set_max_width(400.0);
                ui.label(RichText::new(title).font(FontId::new(theme::SIZE_TITLE, theme::semibold())));
                ui.add_space(theme::SPACE * 2.0);
                ui.label(text);
                ui.add_space(theme::SPACE * 4.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let remove = egui::Button::new(
                        RichText::new(verb)
                            .color(theme::base())
                            .font(FontId::new(theme::SIZE_BODY, theme::semibold())),
                    )
                    .fill(theme::text());
                    if ui.add(remove).clicked() {
                        decided = Some(true);
                    }
                    if ui
                        .add(egui::Button::new("Cancel").stroke(Stroke::new(1.0, theme::rim())))
                        .clicked()
                    {
                        decided = Some(false);
                    }
                });
            });
        });
    if modal.should_close() && decided.is_none() {
        decided = Some(false);
    }
    decided
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_010_03_names_the_intervals_in_words() {
        let labels: Vec<String> = [0, 1, 15, 60, 90, 180, 1440].map(interval_label).into();
        assert_eq!(
            labels,
            [
                "Only at startup",
                "Every minute",
                "Every 15 minutes",
                "Every hour",
                "Every 1.5 hours",
                "Every 3 hours",
                "Daily"
            ]
        );
        assert_eq!(cache_size_label(0.5), "0.5 GB");
        assert_eq!(with_current(&[1, 5], 3), [1, 3, 5]);
    }
}
