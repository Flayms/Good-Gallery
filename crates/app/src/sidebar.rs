//! The sidebar (spec 008): libraries with the selected library's folders, People and Places, ratings and the most
//! used tags, each one click away from filtering the gallery; Settings at the bottom.

use crate::gallery::{Gallery, SidebarData};
use crate::icons::Icon;
use crate::state::Page;
use crate::{format, theme};
use db::libraries::{Library, LibraryStatus};
use db::rusqlite::Connection;
use domain::folders::{FolderNode, TagNode, folder_ancestors};
use domain::search::{GallerySearch, TagMode};
use eframe::egui::{self, Color32, Rect, Vec2, pos2, vec2};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

pub const WIDTH: f32 = 256.0;
const ROW: f32 = 32.0;
const INDENT: f32 = 16.0;
/// Top-level tags of a category shown before _Show all_ (REQ-008-03).
const PREVIEW: usize = 10;
/// Collapsible groups, stored under the settings keys `sidebar.group.<id>`.
const COLLAPSIBLE: [&str; 3] = ["people", "places", "ratings"];

/// What the sidebar remembers between frames.
#[derive(Debug, Default)]
pub struct SidebarState {
    expanded_folders: HashSet<String>,
    expanded_tags: HashSet<i64>,
    show_all: HashSet<&'static str>,
    collapsed: HashSet<String>,
    /// The library whose folder tree was expanded for the active folder.
    expanded_for: Option<(i64, Option<String>)>,
    /// Hidden with _Hide sidebar_ or Ctrl + B, stored under `sidebar.hidden` (REQ-008-08, REQ-008-09).
    hidden: bool,
    loaded: bool,
}

impl SidebarState {
    /// Reads the collapsed groups and whether the sidebar is hidden, once.
    pub fn load(&mut self, db: &Arc<Mutex<Connection>>) {
        if self.loaded {
            return;
        }
        // Not while a background job holds the connection (e.g. removing a large library): next frame.
        let Ok(conn) = db.try_lock() else { return };
        self.loaded = true;
        for id in COLLAPSIBLE {
            let stored = db::settings::value(&conn, &format!("sidebar.group.{id}"))
                .ok()
                .flatten();
            if stored.as_ref().and_then(|value| value.as_str()) == Some("closed") {
                self.collapsed.insert(id.to_string());
            }
        }
        self.hidden = db::settings::value(&conn, "sidebar.hidden").ok().flatten() == Some(true.into());
    }

    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    /// Hides or shows the sidebar and remembers it (REQ-008-08, REQ-008-09).
    pub fn set_hidden(&mut self, hidden: bool, db: Option<&Arc<Mutex<Connection>>>) {
        self.hidden = hidden;
        // As for groups: if a background job holds the connection, the choice isn't stored this time.
        if let Some(Ok(conn)) = db.map(|db| db.try_lock())
            && let Err(error) = db::settings::set_value(&conn, "sidebar.hidden", &hidden.into())
        {
            tracing::warn!("can't store whether the sidebar is hidden: {error}");
        }
    }

    /// Collapses or expands a group and remembers it (REQ-008-07). Collapsing starts over with the preview.
    fn toggle_group(&mut self, id: &'static str, db: Option<&Arc<Mutex<Connection>>>) {
        let open = self.collapsed.remove(id);
        if !open {
            self.collapsed.insert(id.to_string());
            self.show_all.remove(id);
        }
        // Only a convenience: if a background job holds the connection, the choice isn't stored this time.
        if let Some(Ok(conn)) = db.map(|db| db.try_lock()) {
            let value = serde_json::Value::from(if open { "open" } else { "closed" });
            if let Err(error) = db::settings::set_value(&conn, &format!("sidebar.group.{id}"), &value) {
                tracing::warn!("can't store the sidebar group: {error}");
            }
        }
    }

    pub fn is_collapsed(&self, id: &str) -> bool {
        self.collapsed.contains(id)
    }
}

/// What the sidebar shows besides the gallery's data.
pub struct SidebarInput<'a> {
    pub libraries: &'a [Library],
    /// The library being scanned.
    pub scanning: Option<i64>,
    pub page: &'a mut Page,
    pub db: Option<Arc<Mutex<Connection>>>,
}

/// Draws the sidebar into `ui` (a left panel).
pub fn show(ui: &mut egui::Ui, state: &mut SidebarState, gallery: Option<&mut Gallery>, input: SidebarInput) {
    if let Some(db) = &input.db {
        state.load(db);
    }
    let on_settings = *input.page == Page::Settings;
    ui.spacing_mut().item_spacing.y = 2.0;
    // The header is part of the title bar: it moves the window (REQ-013-02).
    let header = egui::Rect::from_min_size(ui.ctx().content_rect().min, vec2(WIDTH, crate::titlebar::HEIGHT));
    let drag = ui.interact(
        header,
        ui.make_persistent_id("sidebar header"),
        egui::Sense::click_and_drag(),
    );
    crate::titlebar::drag_area(ui.ctx(), &drag);
    ui.horizontal(|ui| {
        ui.set_height(48.0);
        ui.add_space(8.0);
        ui.add(Icon::Images.image(20.0, theme::text()));
        ui.label(egui::RichText::new("Good Gallery").family(theme::semibold()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let hide = crate::icons::icon_button(ui, Icon::PanelLeft, "Hide sidebar", "Hide sidebar (Ctrl + B)");
            if hide.clicked() {
                state.set_hidden(true, input.db.as_ref());
            }
        });
    });
    let Some(gallery) = gallery else {
        settings_row(ui, input.page);
        return;
    };
    let mut search = gallery.search().clone();
    let empty = SidebarData::default();
    let data = gallery.sidebar().unwrap_or(&empty);
    // Folders leading to the active folder start expanded (REQ-008-02).
    let active = search.library.map(|library| (library, search.folder.clone()));
    if active != state.expanded_for {
        if let Some(folder) = search.folder.as_deref() {
            state.expanded_folders.extend(folder_ancestors(folder));
        }
        state.expanded_for = active;
    }

    let footer = ROW + 12.0;
    egui::ScrollArea::vertical()
        .id_salt("sidebar")
        .max_height(ui.available_height() - footer)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            group_label(ui, "Libraries");
            if libraries(ui, state, &mut search, data, &input, on_settings) {
                *input.page = Page::Gallery;
            }
            for (id, label, nodes) in [("people", "People", &data.people), ("places", "Places", &data.places)] {
                category(ui, state, &mut search, id, label, nodes, input.db.as_ref());
            }
            ratings(ui, state, &mut search, data, input.db.as_ref());
            popular(ui, &mut search, data);
        });
    settings_row(ui, input.page);
    if search != *gallery.search() {
        // Filtering from the sidebar shows the gallery.
        *input.page = Page::Gallery;
        gallery.set_search(search);
    }
}

/// The _Show sidebar_ button at the left of the toolbar and the Settings header while the sidebar is hidden
/// (REQ-008-08); whether it was clicked.
pub fn show_button(ui: &mut egui::Ui) -> bool {
    crate::icons::icon_button(ui, Icon::PanelLeft, "Show sidebar", "Show sidebar (Ctrl + B)").clicked()
}

fn group_label(ui: &mut egui::Ui, label: &str) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(label)
                .size(theme::SIZE_CAPTION)
                .family(theme::semibold())
                .color(theme::muted()),
        );
    });
}

/// A collapsible group's header; false while it is collapsed.
fn collapsible(
    ui: &mut egui::Ui,
    state: &mut SidebarState,
    id: &'static str,
    label: &str,
    db: Option<&Arc<Mutex<Connection>>>,
) -> bool {
    ui.add_space(8.0);
    let open = !state.is_collapsed(id);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, theme::RADIUS_CONTROL, theme::hover());
    }
    ui.painter().text(
        pos2(rect.left() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::new(theme::SIZE_CAPTION, theme::semibold()),
        theme::muted(),
    );
    let chevron = if open { Icon::ChevronDown } else { Icon::ChevronRight };
    chevron.image(14.0, theme::muted()).paint_at(
        ui,
        Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), Vec2::splat(14.0)),
    );
    let name = label.to_string();
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, open, &name));
    if response.clicked() {
        state.toggle_group(id, db);
    }
    !state.is_collapsed(id)
}

/// A row: optional chevron, icon, label, count or badge on the right. Returns (row, chevron) responses.
struct Row<'a> {
    depth: usize,
    chevron: Option<bool>,
    icon: Option<(Icon, Color32)>,
    label: &'a str,
    active: bool,
    trailing: Option<String>,
    tooltip: Option<String>,
}

fn row(ui: &mut egui::Ui, row: Row) -> (egui::Response, Option<egui::Response>) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), egui::Sense::click());
    let response = match &row.tooltip {
        Some(tooltip) => response.on_hover_text(tooltip),
        None => response,
    };
    let label = row.label.to_string();
    let active = row.active;
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active, &label));
    if row.active {
        ui.painter().rect_filled(rect, theme::RADIUS_CONTROL, theme::selected());
    } else if response.hovered() {
        ui.painter().rect_filled(rect, theme::RADIUS_CONTROL, theme::hover());
    }
    let mut x = rect.left() + 8.0 + row.depth as f32 * INDENT;
    let mut chevron = None;
    if let Some(open) = row.chevron {
        let area = Rect::from_center_size(pos2(x + 10.0, rect.center().y), Vec2::splat(20.0));
        let toggle = ui.interact(area, response.id.with("chevron"), egui::Sense::click());
        let name = format!("{} {}", if open { "Collapse" } else { "Expand" }, row.label);
        toggle.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
        let icon = if open { Icon::ChevronDown } else { Icon::ChevronRight };
        icon.image(14.0, theme::muted())
            .paint_at(ui, Rect::from_center_size(area.center(), Vec2::splat(14.0)));
        chevron = Some(toggle);
    }
    if row.depth > 0 || row.chevron.is_some() {
        x += 20.0;
    }
    if let Some((icon, color)) = row.icon {
        icon.image(theme::ICON_SIZE, color).paint_at(
            ui,
            Rect::from_min_size(pos2(x, rect.center().y - 8.0), Vec2::splat(theme::ICON_SIZE)),
        );
        x += theme::ICON_SIZE + 8.0;
    }
    let trailing_width = row.trailing.as_ref().map_or(0.0, |_| 48.0);
    let font = if row.active {
        egui::FontId::new(theme::SIZE_BODY, theme::semibold())
    } else {
        egui::FontId::new(theme::SIZE_BODY, egui::FontFamily::Proportional)
    };
    let mut job = egui::text::LayoutJob::simple_singleline(row.label.to_string(), font, theme::text());
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - 8.0 - trailing_width - x).max(0.0));
    let galley = ui.painter().layout_job(job);
    ui.painter()
        .galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, theme::text());
    if let Some(trailing) = row.trailing {
        ui.painter().text(
            pos2(rect.right() - 8.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            trailing,
            egui::FontId::new(theme::SIZE_CAPTION, theme::numbers()),
            theme::muted(),
        );
    }
    (response, chevron)
}

/// _All libraries_ and each library: offline ones with an unplugged icon and a badge, the scanned one with a
/// spinner, the selected one with its folders (REQ-008-01, -02). True when one was clicked.
fn libraries(
    ui: &mut egui::Ui,
    state: &mut SidebarState,
    search: &mut GallerySearch,
    data: &SidebarData,
    input: &SidebarInput,
    on_settings: bool,
) -> bool {
    let all = Row {
        depth: 0,
        chevron: None,
        icon: Some((Icon::Layers, theme::text())),
        label: "All libraries",
        active: !on_settings && search.library.is_none(),
        trailing: None,
        tooltip: None,
    };
    let mut clicked = false;
    if row(ui, all).0.clicked() {
        (search.library, search.folder) = (None, None);
        clicked = true;
    }
    for library in input.libraries {
        let offline = library.status == LibraryStatus::Offline;
        let selected = search.library == Some(library.id);
        // A spinner while scanned (drawn below), else the offline badge.
        let trailing =
            (input.scanning != Some(library.id) && offline).then(|| crate::status_label(library.status).to_string());
        let (response, _) = row(
            ui,
            Row {
                depth: 0,
                chevron: None,
                icon: Some(if offline {
                    (Icon::Unplug, theme::alert())
                } else {
                    (Icon::Folder, theme::text())
                }),
                label: &library.label,
                active: !on_settings && selected && search.folder.is_none(),
                trailing,
                tooltip: Some(library.path.clone()),
            },
        );
        let state_label = if input.scanning == Some(library.id) {
            format!("{}, indexing", library.label)
        } else if offline {
            format!("{}, offline", library.label)
        } else {
            library.label.clone()
        };
        let active = !on_settings && selected && search.folder.is_none();
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active, &state_label));
        if input.scanning == Some(library.id) {
            let rect = response.rect;
            Icon::LoaderCircle.image(14.0, theme::muted()).paint_at(
                ui,
                Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), Vec2::splat(14.0)),
            );
        }
        if response.clicked() {
            // A library replaces any folder, the other filters stay.
            (search.library, search.folder) = (Some(library.id), None);
            clicked = true;
        }
        if selected
            && !on_settings
            && let Some((_, folders)) = data.folders.as_ref().filter(|(id, _)| *id == library.id)
        {
            folder_items(ui, state, search, library.id, folders, 1);
        }
    }
    clicked
}

fn folder_items(
    ui: &mut egui::Ui,
    state: &mut SidebarState,
    search: &mut GallerySearch,
    library: i64,
    nodes: &[FolderNode],
    depth: usize,
) {
    for node in nodes {
        let open = state.expanded_folders.contains(&node.path);
        let has_children = !node.children.is_empty();
        let (response, chevron) = row(
            ui,
            Row {
                depth,
                chevron: has_children.then_some(open),
                icon: Some((if open { Icon::FolderOpen } else { Icon::Folder }, theme::text())),
                label: &node.name,
                active: search.library == Some(library) && search.folder.as_deref() == Some(node.path.as_str()),
                trailing: None,
                tooltip: Some(format!("{} · {} items", node.path, format::count(node.count))),
            },
        );
        if chevron.as_ref().is_some_and(egui::Response::clicked) {
            if !state.expanded_folders.remove(&node.path) {
                state.expanded_folders.insert(node.path.clone());
            }
        } else if response.clicked() {
            // Choosing a folder also expands it.
            (search.library, search.folder) = (Some(library), Some(node.path.clone()));
            state.expanded_folders.insert(node.path.clone());
        }
        if open && has_children {
            folder_items(ui, state, search, library, &node.children, depth + 1);
        }
    }
}

/// Clicking a tag includes it, or removes it if it is in the search (REQ-008-06).
fn toggle_tag(search: &mut GallerySearch, name: &str) {
    if search.tag_mode(name).is_some() {
        search.without_tag(name);
    } else {
        search.with_tag(name, TagMode::Include);
    }
}

fn tag_row(ui: &mut egui::Ui, state: &mut SidebarState, search: &mut GallerySearch, node: &TagNode, depth: usize) {
    let mode = search.tag_mode(&node.name);
    let open = state.expanded_tags.contains(&node.id);
    let has_children = !node.children.is_empty();
    let tooltip = if mode.is_some() {
        format!("Remove {} from the filter", node.name)
    } else {
        format!("Show only {}", node.name)
    };
    let (response, chevron) = row(
        ui,
        Row {
            depth,
            chevron: has_children.then_some(open),
            // Only exclusions get an icon; a tag icon on every row would just repeat itself.
            icon: (mode == Some(TagMode::Exclude)).then_some((Icon::Minus, theme::alert())),
            label: &node.name,
            active: mode == Some(TagMode::Include),
            trailing: Some(format::count(node.count)),
            tooltip: Some(tooltip),
        },
    );
    if chevron.as_ref().is_some_and(egui::Response::clicked) {
        if !state.expanded_tags.remove(&node.id) {
            state.expanded_tags.insert(node.id);
        }
    } else if response.clicked() {
        toggle_tag(search, &node.name);
    }
    if open {
        for child in &node.children {
            tag_row(ui, state, search, child, depth + 1);
        }
    }
}

/// People or Places as a tree, 10 top-level tags until _Show all_; hidden without tags (REQ-008-03).
fn category(
    ui: &mut egui::Ui,
    state: &mut SidebarState,
    search: &mut GallerySearch,
    id: &'static str,
    label: &str,
    nodes: &[TagNode],
    db: Option<&Arc<Mutex<Connection>>>,
) {
    if nodes.is_empty() || !collapsible(ui, state, id, label, db) {
        return;
    }
    let shown = if state.show_all.contains(id) {
        nodes.len()
    } else {
        nodes.len().min(PREVIEW)
    };
    for node in &nodes[..shown] {
        tag_row(ui, state, search, node, 0);
    }
    if nodes.len() > PREVIEW {
        let text = if state.show_all.contains(id) {
            "Show less".to_string()
        } else {
            format!("Show all ({})", nodes.len())
        };
        let toggle = Row {
            depth: 0,
            chevron: None,
            icon: None,
            label: &text,
            active: false,
            trailing: None,
            tooltip: None,
        };
        if row(ui, toggle).0.clicked() && !state.show_all.remove(id) {
            state.show_all.insert(id);
        }
    }
}

/// Media items per rating; clicking one toggles it in the rating filter. Hidden while nothing is rated (REQ-008-04).
fn ratings(
    ui: &mut egui::Ui,
    state: &mut SidebarState,
    search: &mut GallerySearch,
    data: &SidebarData,
    db: Option<&Arc<Mutex<Connection>>>,
) {
    if data.ratings.iter().all(|&(rating, _)| rating == 0) || !collapsible(ui, state, "ratings", "Ratings", db) {
        return;
    }
    for &(rating, count) in &data.ratings {
        let label = format::rating(rating);
        let selected = search.ratings.contains(&rating);
        let (response, _) = row(
            ui,
            Row {
                depth: 0,
                chevron: None,
                icon: (rating == 0).then_some((Icon::StarOff, theme::text())),
                // Stars are drawn over the label's place.
                label: if rating == 0 { &label } else { "" },
                active: selected,
                trailing: Some(format::count(count)),
                tooltip: Some(if selected {
                    format!("Remove {label} from the filter")
                } else {
                    format!("Show {label}")
                }),
            },
        );
        let name = label.clone();
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &name));
        if rating > 0 {
            for star in 0..rating {
                let at = Rect::from_min_size(
                    pos2(
                        response.rect.left() + 8.0 + f32::from(star) * 16.0,
                        response.rect.center().y - 7.0,
                    ),
                    Vec2::splat(14.0),
                );
                Icon::Star.image(14.0, theme::star()).paint_at(ui, at);
            }
        }
        if response.clicked() {
            search.toggle_rating(rating);
        }
    }
}

/// The 15 most used tags outside People and Places; hidden without any (REQ-008-05).
fn popular(ui: &mut egui::Ui, search: &mut GallerySearch, data: &SidebarData) {
    if data.popular.is_empty() {
        return;
    }
    group_label(ui, "Tags");
    for tag in &data.popular {
        let mode = search.tag_mode(&tag.name);
        let tooltip = if mode.is_some() {
            format!("Remove {} from the filter", tag.name)
        } else {
            format!("Show only {}", tag.name)
        };
        let (response, _) = row(
            ui,
            Row {
                depth: 0,
                chevron: None,
                icon: (mode == Some(TagMode::Exclude)).then_some((Icon::Minus, theme::alert())),
                label: &tag.name,
                active: mode == Some(TagMode::Include),
                trailing: Some(format::count(tag.count)),
                tooltip: Some(tooltip),
            },
        );
        if response.clicked() {
            toggle_tag(search, &tag.name);
        }
    }
}

fn settings_row(ui: &mut egui::Ui, page: &mut Page) {
    ui.add_space(8.0);
    let settings = Row {
        depth: 0,
        chevron: None,
        icon: Some((Icon::Settings, theme::text())),
        label: "Settings",
        active: *page == Page::Settings,
        trailing: None,
        tooltip: None,
    };
    if row(ui, settings).0.clicked() {
        *page = Page::Settings;
    }
}
