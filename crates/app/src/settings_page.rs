//! The Settings page: libraries (spec 001), the sidebar's groups (spec 008), indexing and the thumbnail cache
//! (spec 010). Changes apply right away.

use crate::icons::{Icon, icon_button};
use crate::state::{AppState, Page};
use crate::{format, theme};
use db::libraries::{Library, LibraryStatus};
use domain::appearance::Appearance;
use domain::edit::MtimeOnSave;
use domain::sidebar::SidebarLayout;
use eframe::egui::{self, Color32, CornerRadius, FontId, Frame, Margin, Rect, RichText, Stroke, Vec2};

/// Content width; descriptions stay below 70 characters (design guide).
const MAX_WIDTH: f32 = 720.0;
const HEADER_HEIGHT: f32 = crate::gallery::TOOLBAR_HEIGHT;

/// The glass header over the page, which is also the window's title bar (REQ-013-02, REQ-013-03): back to the
/// gallery and the page's title; its empty space moves the window.
pub(crate) fn page_header(ui: &mut egui::Ui, rect: Rect, state: &mut AppState, title: &str) {
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
        egui::pos2(
            rect.left() + 12.0 + crate::titlebar::reserved_left_in(ui.ctx(), rect.left()),
            rect.top() + 8.0,
        ),
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
                ui.label(RichText::new(title).font(FontId::new(theme::SIZE_TITLE, theme::semibold())));
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
                    #[cfg(not(target_arch = "wasm32"))]
                    if state.faces.enabled() {
                        faces(ui, state);
                        ui.add_space(theme::SPACE * 10.0);
                    }
                    if !crate::WEB {
                        indexing(ui, state);
                        ui.add_space(theme::SPACE * 10.0);
                        editing(ui, state);
                        ui.add_space(theme::SPACE * 10.0);
                    }
                    // Only the installed Windows app has entries to switch (REQ-027-06).
                    if let Some(on) = state.open_with() {
                        windows(ui, state, on);
                        ui.add_space(theme::SPACE * 10.0);
                    }
                    thumbnail_cache(ui, state);
                    ui.add_space(theme::SPACE * 10.0);
                    about(ui, state);
                    ui.add_space(24.0);
                });
            });
        });
    page_header(ui, header, state, "Settings");
    #[cfg(not(target_arch = "wasm32"))]
    confirm_delete_faces(ui.ctx(), state);
    #[cfg(not(target_arch = "wasm32"))]
    confirm_look_again(ui.ctx(), state);
    confirm_remove(ui.ctx(), state);
    confirm_clear(ui.ctx(), state);
}

/// Quick check intervals offered, in minutes (REQ-010-03).
const QUICK_CHECK_INTERVALS: [u32; 7] = [0, 15, 30, 60, 180, 360, 1440];
/// Thumbnail cache sizes offered, in GB (REQ-010-05).
const CACHE_SIZES_GIB: [f64; 7] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0];
const MAX_CONCURRENCY: u32 = 16;
const LATEST_RELEASE: &str = "https://github.com/Flayms/Good-Gallery/releases/latest";
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
    group_with_border(ui, Stroke::new(1.0, theme::rim()), add);
}

fn group_with_border(ui: &mut egui::Ui, border: Stroke, add: impl FnOnce(&mut egui::Ui)) {
    Frame::new()
        .stroke(border)
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

/// _Appearance_: light, dark or the system's (REQ-020-01) and reduced transparency (REQ-013-17), applied from the next
/// frame on.
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
        divider(ui);
        setting_row(
            ui,
            "Reduce transparency",
            "Menus and the toolbar are solid, without photos showing through. Always on if your computer or browser asks for it.",
            24.0,
            |ui| {
                let on = settings.reduce_transparency;
                ui.checkbox(&mut settings.reduce_transparency, "")
                    .widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, "Reduce transparency"));
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

/// The choices of _When saving edits_ (REQ-021-15).
fn mtime_label(choice: MtimeOnSave) -> &'static str {
    match choice {
        MtimeOnSave::Keep => "Keep date modified",
        MtimeOnSave::Update => "Update date modified",
    }
}

/// _Editing_: what saving an edit does to the file's date modified (REQ-021-15).
fn editing(ui: &mut egui::Ui, state: &mut AppState) {
    section_title(ui, "Editing");
    let mut settings = state.settings;
    group(ui, |ui| {
        setting_row(
            ui,
            "When saving edits",
            "Keeping it leaves photos without a date taken in their place. Some sync tools only notice updated files.",
            224.0,
            |ui| {
                let current = settings.mtime_on_save;
                let combo = egui::ComboBox::from_id_salt("mtime on save")
                    .width(224.0)
                    .selected_text(mtime_label(current))
                    .show_ui(ui, |ui| {
                        crate::glass::behind_menu(ui, |ui| {
                            for option in [MtimeOnSave::Keep, MtimeOnSave::Update] {
                                ui.selectable_value(&mut settings.mtime_on_save, option, mtime_label(option));
                            }
                        });
                    });
                name_combo(&combo.response, "When saving edits", mtime_label(current).to_owned());
            },
        );
    });
    if settings != state.settings {
        state.update_settings(settings);
    }
}

/// _Windows_: whether Explorer offers the app under _Open with_ (REQ-027-06).
fn windows(ui: &mut egui::Ui, state: &mut AppState, on: bool) {
    section_title(ui, "Windows");
    let mut checked = on;
    group(ui, |ui| {
        setting_row(
            ui,
            "Show under \"Open with\"",
            "Right-click a photo or video in Explorer and open it with Good Gallery. It doesn't become your default app.",
            24.0,
            |ui| {
                ui.checkbox(&mut checked, "").widget_info(|| {
                    egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, "Show under \"Open with\"")
                });
            },
        );
    });
    if checked != on {
        state.set_open_with(checked);
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

/// _About_: the app's version and a way to the latest release, through the browser (REQ-010-07).
fn about(ui: &mut egui::Ui, state: &mut AppState) {
    section_title(ui, "About");
    let mut open = false;
    let mut update = None;
    let mut settings = state.settings;
    group(ui, |ui| {
        setting_row(
            ui,
            "Good Gallery",
            concat!("Version ", env!("CARGO_PKG_VERSION")),
            180.0,
            |ui| {
                open = ui
                    .add(egui::Button::new("See the latest release").stroke(Stroke::new(1.0, theme::rim())))
                    .clicked();
            },
        );
        // The browser version is always the latest (REQ-031-01).
        if !crate::WEB {
            divider(ui);
            setting_row(
                ui,
                "Check for updates",
                "Asks GitHub once a day whether there is a newer version. Nothing is downloaded until you choose to update.",
                24.0,
                |ui| {
                    let on = settings.check_for_updates;
                    ui.checkbox(&mut settings.check_for_updates, "").widget_info(|| {
                        egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, "Check for updates")
                    });
                },
            );
            if state.updates.phase != crate::updates::Phase::Unknown {
                divider(ui);
                Frame::new().inner_margin(Margin::same(12)).show(ui, |ui| {
                    update = crate::updates::offer(ui, &state.updates);
                });
            }
        }
    });
    if settings != state.settings {
        state.update_settings(settings);
    }
    if let Some(update) = update {
        state.answer_update(update, ui.ctx());
    }
    if open {
        if crate::WEB {
            ui.ctx().open_url(egui::OpenUrl::new_tab(LATEST_RELEASE));
        } else {
            state.open_in_browser(LATEST_RELEASE);
        }
    }
}

/// The page comparing the models (REQ-032-01).
#[cfg(not(target_arch = "wasm32"))]
const FACE_MODELS_PAGE: &str = "https://github.com/Flayms/Good-Gallery/blob/main/docs/face-models.md";

#[cfg(not(target_arch = "wasm32"))]
fn megabytes(bytes: u64) -> String {
    let megabytes = bytes as f64 / 1_000_000.0;
    if megabytes < 1.0 {
        format!("{:.0} KB", megabytes * 1000.0)
    } else if megabytes < 10.0 {
        format!("{megabytes:.1} MB")
    } else {
        format!("{megabytes:.0} MB")
    }
}

/// One model's row: name, what's known about it, and what can be done with it.
#[cfg(not(target_arch = "wasm32"))]
fn model_row(
    ui: &mut egui::Ui,
    state: &mut AppState,
    detect: bool,
    name: &str,
    description: &str,
    sha256: &str,
    catalog: Option<&'static domain::face_models::CatalogModel>,
) {
    let present = state.faces.models.iter().any(|model| model.sha256 == sha256);
    let arriving = state.faces.arriving().remove(sha256);
    let chosen = if detect {
        state.faces.detector.as_deref() == Some(sha256)
    } else {
        state.faces.recognizer.as_deref() == Some(sha256)
    };
    let mut command = None;
    // Short on screen, with the model's name for screen readers.
    let button = |ui: &mut egui::Ui, text: &str, spoken: String, selected: bool| {
        let response = ui.add(egui::Button::new(text).selected(selected));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &spoken));
        response.clicked()
    };
    setting_row(ui, name, description, 180.0, |ui| {
        if let Some(arriving) = &arriving {
            if button(ui, "Cancel", format!("Cancel {name}"), false) {
                command = Some("cancel");
            }
            let share = arriving
                .total
                .map_or(0.0, |total| arriving.bytes as f32 / total.max(1) as f32);
            ui.add(egui::ProgressBar::new(share).desired_width(90.0).show_percentage());
        } else if present {
            if button(ui, "Delete", format!("Delete {name}"), false) {
                command = Some("delete");
            }
            let (text, spoken) = if chosen {
                ("In use", format!("Using {name}"))
            } else {
                ("Use", format!("Use {name}"))
            };
            // In use is a state: stopping is Pause (REQ-032-19).
            if button(ui, text, spoken, chosen) && !chosen {
                command = Some("use");
            }
        } else if catalog.is_some() && button(ui, "Download", format!("Download {name}"), false) {
            command = Some("download");
        }
    });
    match command {
        Some("cancel") => state.cancel_face_model(sha256),
        Some("delete") => state.remove_face_model(sha256),
        Some("use") => state.choose_face_model(detect, Some(sha256.to_owned())),
        Some("download") => {
            if let Some(model) = catalog {
                state.download_face_model(model);
            }
        }
        _ => {}
    }
}

/// _Faces_: what the search is doing, the models, and the user's data (REQ-032-01 … REQ-032-05, REQ-032-17 …
/// REQ-032-20).
#[cfg(not(target_arch = "wasm32"))]
fn faces(ui: &mut egui::Ui, state: &mut AppState) {
    use crate::state::faces::{AddedStyle, style_label};
    use domain::face_models::{CATALOG, InputStyle, ModelKind};
    section_title(ui, "Faces");
    let summary = state.faces.summary();
    group(ui, |ui| {
        let can_pause = state.faces.detector.is_some();
        let paused = state.faces.paused;
        setting_row(ui, summary.title, &summary.detail, 90.0, |ui| {
            if can_pause && ui.button(if paused { "Resume" } else { "Pause" }).clicked() {
                state.set_face_search(!paused);
            }
        });
        if let Some(share) = summary.share {
            Frame::new().inner_margin(Margin::symmetric(12, 0)).show(ui, |ui| {
                ui.add(egui::ProgressBar::new(share).desired_height(4.0));
                ui.add_space(12.0);
            });
        }
        if state.faces.model_seems_wrong {
            divider(ui);
            Frame::new().inner_margin(Margin::same(12)).show(ui, |ui| {
                ui.colored_label(
                    theme::alert(),
                    "This model doesn't tell your named people apart. It may expect its pixels differently: remove it and add it again with the other choice.",
                );
            });
        }
    });
    ui.add_space(theme::SPACE * 4.0);
    ui.add(
        egui::Label::new(
            RichText::new("Models run on this computer. Photos and faces stay here.").color(theme::muted()),
        )
        .wrap(),
    );
    if ui.link("Compare the models").clicked() {
        state.open_in_browser(FACE_MODELS_PAGE);
    }
    ui.add_space(theme::SPACE * 2.0);
    let sections = [
        (
            "Detection",
            "Finds where the faces are. Choosing one starts looking through your photos in the background.",
            true,
        ),
        (
            "Recognition",
            "Works out who a face is, for suggested names and groups. Without one, you still see the faces.",
            false,
        ),
    ];
    for (title, what, detect) in sections {
        ui.label(RichText::new(title).font(FontId::new(theme::SIZE_BODY, theme::semibold())));
        ui.add(egui::Label::new(RichText::new(what).color(theme::muted())).wrap());
        ui.add_space(theme::SPACE);
        let is_kind = |kind: &ModelKind| matches!(kind, ModelKind::Detect(_)) == detect;
        group(ui, |ui| {
            let mut first = true;
            for model in CATALOG.iter().filter(|model| is_kind(&model.kind)) {
                if !std::mem::take(&mut first) {
                    divider(ui);
                }
                let description = format!("{}\n{}. {}", model.why, megabytes(model.bytes), model.license);
                model_row(ui, state, detect, model.name, &description, model.sha256, Some(model));
            }
            let others: Vec<_> = state
                .faces
                .models
                .iter()
                .filter(|model| is_kind(&model.kind) && media::face_store::catalog_entry(&model.sha256).is_none())
                .cloned()
                .collect();
            for model in others {
                divider(ui);
                model_row(
                    ui,
                    state,
                    detect,
                    &model.name,
                    "On this computer, not in the list",
                    &model.sha256,
                    None,
                );
            }
        });
        ui.add_space(theme::SPACE * 4.0);
    }
    ui.add(
        egui::Label::new(
            RichText::new("Changing a model looks through your photos again. Names you confirmed stay.")
                .color(theme::muted()),
        )
        .wrap(),
    );
    ui.add_space(theme::SPACE * 4.0);
    ui.label(RichText::new("Add another model").font(FontId::new(theme::SIZE_BODY, theme::semibold())));
    ui.add_space(theme::SPACE);
    let dropped: Vec<std::path::PathBuf> = ui.input(|input| {
        let dropped = input.raw.dropped_files.iter();
        dropped.map(|file| file.path().to_path_buf()).collect()
    });
    state.add_dropped_face_files(&dropped);
    let hovering = ui.input(|input| !input.raw.hovered_files.is_empty());
    let border = if hovering {
        Stroke::new(2.0, theme::ring())
    } else {
        Stroke::new(1.0, theme::rim())
    };
    group_with_border(ui, border, |ui| {
        Frame::new().inner_margin(Margin::same(12)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = theme::SPACE * 2.0;
            let hint = if hovering {
                RichText::new("Drop the model here").color(theme::text())
            } else {
                RichText::new(
                    "An ONNX file you trust. The app works out what it does. Its license is yours to check. \
                     Choose one, or drop it here.",
                )
                .color(theme::muted())
            };
            ui.add(egui::Label::new(hint).wrap());
            ui.horizontal(|ui| {
                let busy = state.faces.arriving().contains_key("added");
                if ui.add_enabled(!busy, egui::Button::new("Choose file…")).clicked() {
                    state.add_face_file();
                }
                if busy && ui.button("Cancel adding").clicked() {
                    state.cancel_face_model("added");
                }
            });
            ui.add_space(theme::SPACE);
            ui.label("Pixel values, for a recognition model");
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "How the model expects its input. A file doesn't say. If you don't know, keep the first: a warning shows above if it's wrong.",
                    )
                    .color(theme::muted()),
                )
                .wrap(),
            );
            let current = state.faces.added_style.0;
            let combo = egui::ComboBox::from_id_salt("added face model")
                .width(320.0)
                .selected_text(style_label(current))
                .show_ui(ui, |ui| {
                    crate::glass::behind_menu(ui, |ui| {
                        for option in [InputStyle::ArcFace, InputStyle::SFace] {
                            ui.selectable_value(&mut state.faces.added_style, AddedStyle(option), style_label(option));
                        }
                    });
                });
            name_combo(&combo.response, "Pixel values", style_label(current).to_owned());
        });
    });
    ui.add_space(theme::SPACE * 4.0);
    state.check_face_model();
    group(ui, |ui| {
        setting_row(
            ui,
            "Look through all photos again",
            "For when something was missed. What you decided about a face stays.",
            210.0,
            |ui| {
                if ui.button("Look through all photos again").clicked() {
                    state.faces.confirm_look_again = true;
                }
            },
        );
        divider(ui);
        setting_row(
            ui,
            "Ignored faces",
            "Faces you chose to ignore stay out of the way.",
            170.0,
            |ui| {
                if ui.button("Show ignored faces again").clicked() {
                    state.restore_ignored_faces();
                }
            },
        );
        divider(ui);
        setting_row(
            ui,
            "Face data",
            "Removes found faces and what you decided about them. Your files stay as they are.",
            140.0,
            |ui| {
                if ui.button("Delete face data").clicked() {
                    state.faces.confirm_delete = true;
                }
            },
        );
    });
}

/// "Look through all photos again?" (REQ-032-22).
#[cfg(not(target_arch = "wasm32"))]
fn confirm_look_again(ctx: &egui::Context, state: &mut AppState) {
    if !state.faces.confirm_look_again {
        return;
    }
    if let Some(again) = confirm(
        ctx,
        "confirm-look-again",
        "Look through all photos again",
        "This can take many hours on a large library. Names and what you decided about a face stay.",
        "Look again",
    ) {
        state.faces.confirm_look_again = false;
        if again {
            state.look_through_all_again();
        }
    }
}

/// "Delete what the app stored about faces?" (REQ-032-04).
#[cfg(not(target_arch = "wasm32"))]
fn confirm_delete_faces(ctx: &egui::Context, state: &mut AppState) {
    if !state.faces.confirm_delete {
        return;
    }
    if let Some(delete) = confirm(
        ctx,
        "confirm-delete-faces",
        "Delete face data",
        "Delete everything stored about faces? Names in your files stay. The search stops until you choose models again.",
        "Delete",
    ) {
        state.faces.confirm_delete = false;
        if delete {
            state.delete_face_data();
        }
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
    crate::widgets::dialog(ctx, id, title, text, &[verb, "Cancel"]).map(|chosen| chosen == 0)
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
