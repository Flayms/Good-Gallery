//! Good Gallery's UI. `main.rs` runs it in a window; tests drive the same [`ui`] with `egui_kittest`.

pub mod empty_state;
pub mod format;
pub mod gallery;
pub mod glass;
pub mod icons;
pub mod indexer_progress;
pub mod info;
pub mod originals;
pub mod search_bar;
pub mod settings_page;
pub mod sidebar;
pub mod startup;
pub mod state;
pub mod textures;
pub mod theme;
pub mod titlebar;
pub mod toasts;
pub mod toolbar;
pub mod video;
pub mod viewer;
#[cfg(target_arch = "wasm32")]
pub mod web;
pub mod widgets;

#[cfg(not(target_arch = "wasm32"))]
pub use state::NativeFolderPicker;
pub use state::{AppState, CacheUsage, FolderPicker, Page, SettingsTargets};

/// The browser version (spec 019): no files by path, no file manager, no helper programs, no scheduled checks.
pub const WEB: bool = cfg!(target_arch = "wasm32");

/// A library's status as the sidebar and the settings show it: in the browser, an unreadable folder needs the user
/// to give access again rather than a drive to come back (REQ-019-09).
pub fn status_label(status: db::libraries::LibraryStatus) -> &'static str {
    match status {
        db::libraries::LibraryStatus::Offline if WEB => "needs access",
        status => status.as_str(),
    }
}

use eframe::egui;

/// Once per context: theme, fonts and image loaders (icons are SVG).
pub fn setup(ctx: &egui::Context) {
    theme::install(ctx);
    egui_extras::install_image_loaders(ctx);
}

/// Builds one frame of the whole window.
pub fn ui(ui: &mut egui::Ui, state: &mut AppState) {
    state.poll();
    let ctx = ui.ctx().clone();
    glass::begin_frame(&ctx);
    // The viewer's keys go to it alone (REQ-009-02).
    if let Some(viewer) = &mut state.viewer {
        viewer.input(&ctx);
    }
    ctx.layer_painter(egui::LayerId::background())
        .rect_filled(ctx.content_rect(), 0.0, theme::DARKROOM);
    if state.is_active() {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }

    if let Some(error) = &state.error {
        empty_state::error(ui, "Couldn't open the index", error);
    } else {
        let input = sidebar::SidebarInput {
            libraries: &state.libraries,
            scanning: state.indexer_status.current.map(|scan| scan.library),
            page: &mut state.page,
            db: state.db.clone(),
        };
        // The sidebar's data comes with the gallery, also while the Settings page shows.
        if let Some(gallery) = &mut state.gallery {
            gallery.poll();
        }
        let gallery = if state.libraries.is_empty() {
            None
        } else {
            state.gallery.as_mut()
        };
        let sidebar = egui::Panel::left("sidebar")
            .exact_size(sidebar::WIDTH)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::RECESS).inner_margin(8))
            .show(ui, |ui| sidebar::show(ui, &mut state.sidebar, gallery, input));
        // Behind the viewer the sidebar shows through the scrim too (REQ-013-14).
        glass::record_solid(&ctx, sidebar.response.rect, theme::RECESS, true);
        // Not under the viewer: it has its own info panel.
        if state.page == Page::Gallery && state.info_sheet.is_some() && state.viewer.is_none() {
            egui::Panel::right("info sheet")
                .exact_size(info::WIDTH)
                .resizable(false)
                .frame(egui::Frame::new().fill(theme::RECESS).inner_margin(16))
                .show(ui, |ui| info_sheet(ui, state));
        }
        egui::CentralPanel::no_frame().show(ui, |ui| match state.page {
            Page::Gallery => gallery_page(ui, state),
            Page::Settings => settings_page::ui(ui, state),
        });
        show_viewer(&ctx, state);
    }
    indexer_progress::show(&ctx, state);
    state.toasts.show(&ctx);
    titlebar::caption_buttons(&ctx);
    theme::draw_focus_ring(&ctx);
}

/// The viewer over the window while it is open; closing it leaves the gallery where it was (REQ-009-03).
fn show_viewer(ctx: &egui::Context, state: &mut AppState) {
    let (Some(viewer), Some(gallery)) = (&mut state.viewer, &mut state.gallery) else {
        return;
    };
    let originals = state
        .originals
        .get_or_insert_with(|| originals::Originals::new(ctx.clone(), None, None));
    let action = viewer.show(ctx, gallery, originals, &state.libraries, &mut state.videos);
    let closed = |state: &mut AppState| {
        state.viewer = None;
        if let Some(gallery) = &mut state.gallery {
            gallery.set_viewed(None);
            gallery.set_strip(0..0);
        }
        if let Some(originals) = &mut state.originals {
            originals.set(Vec::new());
        }
        state.videos.stop();
    };
    match action {
        Some(viewer::Action::Close) => closed(state),
        Some(viewer::Action::FilterByTag(tag)) => {
            closed(state);
            filter_by_tag(state, &tag);
        }
        Some(viewer::Action::ShowInExplorer(file)) => state.show_in_explorer(file),
        None => {}
    }
    // Details are kept only while an info view shows them, else every reload would ask for them again.
    let info_shown = state.info_sheet.is_some() || state.viewer.as_ref().is_some_and(viewer::Viewer::info_open);
    if !info_shown && let Some(gallery) = &mut state.gallery {
        gallery.forget_detail();
    }
}

/// Adds `tag` to the search as included, keeping the other filters (REQ-009-09).
fn filter_by_tag(state: &mut AppState, tag: &str) {
    let Some(gallery) = &mut state.gallery else { return };
    let mut search = gallery.search().clone();
    search.with_tag(tag, domain::search::TagMode::Include);
    gallery.set_search(search);
}

/// The info sheet beside the gallery (REQ-009-11): the same details as the viewer's info panel.
fn info_sheet(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(id) = state.info_sheet else { return };
    // Below the window controls, which take the top right of the window (REQ-013-02).
    if titlebar::reserved_width() > 0.0 {
        ui.add_space(titlebar::HEIGHT - 16.0);
    }
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Information")
                .size(theme::SIZE_TITLE)
                .family(theme::semibold()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = icons::icon_button(ui, icons::Icon::X, "Close information", "Close").clicked();
        });
    });
    ui.add_space(8.0);
    let mut clicked = None;
    if let Some(gallery) = &mut state.gallery {
        egui::ScrollArea::vertical()
            .id_salt("info sheet")
            .auto_shrink(false)
            .show(ui, |ui| clicked = info::show_loaded(ui, gallery.detail(id)));
    }
    if close {
        state.info_sheet = None;
    }
    if let Some(tag) = clicked {
        filter_by_tag(state, &tag);
    }
}

/// The gallery, or its empty state without libraries.
fn gallery_page(ui: &mut egui::Ui, state: &mut AppState) {
    if state.libraries.is_empty() {
        if empty_state::no_libraries(ui).clicked() {
            // The browser's folder picker needs the click (REQ-019-01, REQ-019-02).
            if WEB {
                state.browse_for_library();
            } else {
                state.page = Page::Settings;
            }
        }
    } else if let Some(gallery) = &mut state.gallery {
        gallery.show(ui);
        // A click or Enter on a tile or row opens the viewer (REQ-009-01).
        if let Some(index) = gallery.take_opened()
            && state.viewer.is_none()
            && let Some(viewer) = viewer::Viewer::open(gallery, index)
        {
            state.viewer = Some(viewer);
            // Typing and Enter go to the viewer now, not to the search bar or the tile.
            ui.memory_mut(|memory| {
                if let Some(focused) = memory.focused() {
                    memory.surrender_focus(focused);
                }
            });
        }
        // A tile's or row's context menu (REQ-009-10, REQ-009-11).
        match gallery.take_menu_action() {
            Some((index, gallery::MediaAction::ShowInExplorer)) => {
                if let Some(file) = state.file_at(index) {
                    state.show_in_explorer(file);
                }
            }
            Some((index, gallery::MediaAction::Information)) => state.info_sheet = gallery.id_at(index),
            None => {}
        }
    }
    // The toolbar floats over the gallery, which scrolls underneath it (REQ-013-03).
    let area = ui.max_rect();
    let gallery = if state.libraries.is_empty() {
        None
    } else {
        state.gallery.as_mut()
    };
    toolbar::show(ui, area, gallery, &mut state.search_bar);
}
