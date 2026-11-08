//! Good Gallery's UI. `main.rs` runs it in a window; tests drive the same [`ui`] with `egui_kittest`.

// Rebuilt when another worktree built into the same target folder (`.cargo/config.toml`).
const _: Option<&str> = option_env!("GOOD_GALLERY_WORKTREE");

pub mod calendar;
pub mod edit;
pub mod empty_state;
#[cfg(not(target_arch = "wasm32"))]
pub mod fix_dates;
pub mod format;
pub mod gallery;
pub mod glass;
pub mod icons;
pub mod info;
#[cfg(not(target_arch = "wasm32"))]
pub mod instance;
#[cfg(not(target_arch = "wasm32"))]
pub mod loose;
pub mod luts;
pub mod names;
pub mod open_with;
pub mod originals;
pub mod people_page;
pub mod processing;
#[cfg(not(target_arch = "wasm32"))]
pub mod rename;
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
pub mod unreadable;
pub mod updates;
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
    // The installer waits for the window to close (REQ-031-05).
    if std::mem::take(&mut state.updates.close) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    // A later start of the app handed over a file or asked for the window (REQ-027-02).
    if std::mem::take(&mut state.bring_to_front) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
    state.show_pending_open();
    state.show_face_photo();
    state.notice_new_day();
    #[cfg(not(target_arch = "wasm32"))]
    state.refresh_content_search();
    // Light, dark or the system's, from this frame on (REQ-020-01, REQ-020-02).
    theme::set_appearance(&ctx, state.settings.appearance);
    theme::set_transparency_setting(&ctx, state.settings.reduce_transparency);
    theme::begin_frame(&ctx);
    glass::begin_frame(&ctx);
    // The viewer's keys go to it alone (REQ-009-02), not while a dialog asks over it (REQ-009-23).
    if let Some(viewer) = &mut state.viewer
        && state.confirm_many.is_none()
    {
        viewer.input(&ctx);
    }
    ctx.layer_painter(egui::LayerId::background())
        .rect_filled(ctx.content_rect(), 0.0, theme::base());
    // Started with a file: only the background until the viewer shows it (REQ-027-07).
    if state.waiting_to_open() {
        if let Some(gallery) = &mut state.gallery {
            gallery.poll();
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
        return;
    }
    if state.is_active() {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }

    if let Some(error) = &state.error {
        empty_state::error(ui, "Couldn't open the index", error);
    } else {
        state.sidebar.set_window_width(ctx.content_rect().width());
        // Over the gallery, the sidebar closes once something in it is picked (REQ-019-15).
        let before_drawer = (!state.sidebar.is_hidden() && state.sidebar.is_drawer()).then(|| {
            (
                state.page,
                state.gallery.as_ref().map(|gallery| gallery.search().clone()),
            )
        });
        let input = sidebar::SidebarInput {
            libraries: &state.libraries,
            scanning: state.indexer_status.current.map(|scan| scan.library),
            page: &mut state.page,
            db: state.db.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            people: state.faces.detector.is_some() && state.faces.recognizer.is_some(),
            #[cfg(target_arch = "wasm32")]
            people: false,
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
        if let Some(db) = &input.db {
            state.sidebar.load(db);
        }
        // Ctrl + B hides or shows the sidebar; the viewer's keys go to it alone (REQ-008-08).
        if state.viewer.is_none() && ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::B)) {
            let hidden = !state.sidebar.is_hidden();
            state.sidebar.set_hidden(hidden, input.db.as_ref());
        }
        if !state.sidebar.is_hidden() && state.sidebar.is_drawer() {
            sidebar_drawer(&ctx, &mut state.sidebar, gallery, input);
        } else if !state.sidebar.is_hidden() {
            let sidebar = egui::Panel::left("sidebar")
                .exact_size(sidebar::WIDTH)
                .resizable(false)
                .frame(egui::Frame::new().fill(theme::recess()).inner_margin(8))
                .show(ui, |ui| sidebar::show(ui, &mut state.sidebar, gallery, input));
            // Behind the viewer the sidebar shows through the scrim too (REQ-013-14).
            glass::record_solid(&ctx, sidebar.response.rect, theme::recess(), true);
        }
        if let Some((page, search)) = before_drawer
            && (state.page != page || state.gallery.as_ref().map(|gallery| gallery.search()) != search.as_ref())
        {
            state.sidebar.set_hidden(true, None);
        }
        // Not under the viewer: it has its own info panel.
        if state.page == Page::Gallery && state.info_sheet.is_some() && state.viewer.is_none() {
            egui::Panel::right("info sheet")
                // Never wider than the window (REQ-019-15).
                .exact_size(info::WIDTH.min(ctx.content_rect().width()))
                .resizable(false)
                .frame(egui::Frame::new().fill(theme::recess()).inner_margin(16))
                .show(ui, |ui| info_sheet(ui, state));
        }
        egui::CentralPanel::no_frame().show(ui, |ui| match state.page {
            Page::Gallery => gallery_page(ui, state),
            Page::Settings => settings_page::ui(ui, state),
            Page::People => people_page::ui(ui, state),
        });
        show_viewer(&ctx, state);
        // A copied photo goes on the clipboard once decoded (REQ-009-25).
        if let Some(image) = state.take_copied() {
            ctx.copy_image(image);
        }
    }
    confirm_revert(&ctx, state);
    confirm_clear_history(&ctx, state);
    confirm_many(&ctx, state);
    #[cfg(not(target_arch = "wasm32"))]
    rename_dialog(&ctx, state);
    #[cfg(not(target_arch = "wasm32"))]
    date_dialog(&ctx, state);
    if let Some(gallery) = &mut state.gallery {
        gallery.album_dialog(&ctx);
    }
    match state.toasts.show(&ctx) {
        #[cfg(not(target_arch = "wasm32"))]
        Some(toasts::ToastAction::UndoRename) => state.undo_rename(),
        #[cfg(not(target_arch = "wasm32"))]
        Some(toasts::ToastAction::UndoDates) => state.undo_fix_dates(),
        _ => {}
    }
    // Over the viewer they go with its dark look (REQ-020-05).
    if state.viewer.is_some() {
        theme::with_palette(&theme::DARK, || titlebar::caption_buttons(&ctx));
    } else {
        titlebar::caption_buttons(&ctx);
    }
    theme::draw_focus_ring(&ctx);
}

/// The viewer over the window while it is open; closing it leaves the gallery where it was (REQ-009-03).
fn show_viewer(ctx: &egui::Context, state: &mut AppState) {
    let Some(viewer) = &mut state.viewer else { return };
    // A file outside every library shows from its scratch index (spec 033).
    #[cfg(not(target_arch = "wasm32"))]
    let loose = state
        .loose
        .as_mut()
        .filter(|loose| loose.shown)
        .map(|loose| (&mut loose.gallery, &loose.libraries[..]));
    #[cfg(target_arch = "wasm32")]
    let loose: Option<(&mut gallery::Gallery, &[db::libraries::Library])> = None;
    let (gallery, libraries) = match loose {
        Some(source) => source,
        None => match &mut state.gallery {
            Some(gallery) => (gallery, &state.libraries[..]),
            None => return,
        },
    };
    let originals = state
        .originals
        .get_or_insert_with(|| originals::Originals::new(ctx.clone(), None, None));
    // The viewer stays dark in the light appearance (REQ-020-05).
    let action = theme::with_palette(&theme::DARK, || {
        viewer.show(ctx, gallery, originals, libraries, &mut state.videos, &state.luts)
    });
    let closed = |state: &mut AppState| {
        state.viewer = None;
        state.viewer_closed();
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
        #[cfg(not(target_arch = "wasm32"))]
        Some(viewer::Action::Copy(file, orientation)) => state.copy_image(file, orientation),
        #[cfg(target_arch = "wasm32")]
        Some(viewer::Action::Copy(..)) => {}
        Some(viewer::Action::SaveEdit(request)) => state.save_edit(*request),
        Some(viewer::Action::ImportLut) => state.import_lut(),
        Some(viewer::Action::Revert(target)) => state.confirm_revert = Some(target),
        Some(viewer::Action::ClearHistory(clear)) => state.confirm_clear_history = Some(clear),
        Some(viewer::Action::Rate(rate)) => state.rate(rate),
        #[cfg(not(target_arch = "wasm32"))]
        Some(viewer::Action::Rename(id)) => state.open_rename(vec![id]),
        #[cfg(target_arch = "wasm32")]
        Some(viewer::Action::Rename(_)) => {}
        Some(viewer::Action::Turn(id, turn)) => state.ask_change_many(state::ChangeMany {
            ids: vec![id],
            what: state::Many::Turn(turn),
        }),
        Some(viewer::Action::Delete(id)) => state.ask_delete(vec![id]),
        #[cfg(not(target_arch = "wasm32"))]
        Some(viewer::Action::FixDates(id)) => state.open_fix_dates(vec![id]),
        #[cfg(target_arch = "wasm32")]
        Some(viewer::Action::FixDates(_)) => {}
        Some(viewer::Action::Tag(change)) => state.tag(change),
        Some(viewer::Action::Face(face)) => state.face_action(face),
        #[cfg(not(target_arch = "wasm32"))]
        Some(viewer::Action::AddFolder) => state.add_loose_folder(),
        #[cfg(target_arch = "wasm32")]
        Some(viewer::Action::AddFolder) => {}
        None => {}
    }
    // Details are kept only while an info view or the viewer (editing starts from them, face regions are drawn from
    // them) shows them, else every reload would ask for them again.
    let info_shown = state.info_sheet.is_some() || state.viewer.is_some();
    if !info_shown && let Some(gallery) = &mut state.gallery {
        gallery.forget_detail();
    }
}

/// "Revert to the original?" with Cancel / Revert (REQ-021-12); in the viewer's dark look while it is open.
fn confirm_revert(ctx: &egui::Context, state: &mut AppState) {
    let Some(target) = state.confirm_revert.clone() else {
        return;
    };
    let text = format!(
        "{} goes back to the unedited original, and the edit is discarded.",
        target.file_name
    );
    let ask = || {
        widgets::dialog(
            ctx,
            "confirm-revert",
            "Revert to the original?",
            &text,
            &["Revert", "Cancel"],
        )
    };
    let chosen = if state.viewer.is_some() {
        theme::with_palette(&theme::DARK, ask)
    } else {
        ask()
    };
    if let Some(chosen) = chosen {
        state.confirm_revert = None;
        if chosen == 0 {
            state.revert_edit(target);
        }
    }
}

/// "Clear the version history?" with Clear history / Cancel (REQ-021-26); in the viewer's dark look while it is open.
fn confirm_clear_history(ctx: &egui::Context, state: &mut AppState) {
    let Some(clear) = state.confirm_clear_history.clone() else {
        return;
    };
    let earlier = clear.versions.len().saturating_sub(1);
    let text = format!(
        "The {earlier} earlier saved {} of {} are removed. Its current edit and its original stay.",
        if earlier == 1 { "version" } else { "versions" },
        clear.target.file_name
    );
    let ask = || {
        widgets::dialog(
            ctx,
            "confirm-clear-history",
            "Clear the version history?",
            &text,
            &["Clear history", "Cancel"],
        )
    };
    let chosen = if state.viewer.is_some() {
        theme::with_palette(&theme::DARK, ask)
    } else {
        ask()
    };
    if let Some(chosen) = chosen {
        state.confirm_clear_history = None;
        if chosen == 0 {
            state.clear_history(clear);
        }
    }
}

/// Asks before rating or tagging many items, since their files change (REQ-025-07, REQ-026-06).
/// The rename dialog, while open (spec 035).
#[cfg(not(target_arch = "wasm32"))]
fn rename_dialog(ctx: &egui::Context, state: &mut AppState) {
    let Some(dialog) = &mut state.rename_dialog else { return };
    match dialog.show(ctx) {
        Some(rename::Asked::Rename(job)) => {
            state.rename_dialog = None;
            state.start_rename(job);
        }
        Some(rename::Asked::Undo) => {
            state.rename_dialog = None;
            state.undo_rename();
        }
        Some(rename::Asked::Close) => state.rename_dialog = None,
        None => {}
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn date_dialog(ctx: &egui::Context, state: &mut AppState) {
    let Some(dialog) = &mut state.date_dialog else { return };
    match dialog.show(ctx) {
        Some(fix_dates::Asked::Fix(job)) => {
            state.date_dialog = None;
            state.start_fix_dates(job);
        }
        Some(fix_dates::Asked::Undo) => {
            state.date_dialog = None;
            state.undo_fix_dates();
        }
        Some(fix_dates::Asked::Close) => state.date_dialog = None,
        None => {}
    }
}

fn confirm_many(ctx: &egui::Context, state: &mut AppState) {
    let Some(change) = state.confirm_many.clone() else {
        return;
    };
    let (title, text, button) = change.what.question(&format::count(change.ids.len() as u64));
    let chosen = widgets::dialog(ctx, "confirm-many", &title, &text, &[button, "Cancel"]);
    if let Some(chosen) = chosen {
        state.confirm_many = None;
        if chosen == 0 {
            state.change_many(change);
        }
    }
}

/// Adds or removes a tag of the info sheet's item (REQ-026-01, REQ-026-02).
fn tag(state: &mut AppState, id: i64, change: domain::tagging::Change) {
    let detail = state.gallery.as_ref().and_then(|gallery| gallery.cached_detail(id));
    if let Some(tag) = detail.map(|detail| state::TagChange::of(detail, change)) {
        state.tag(tag);
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
/// The sidebar over the gallery on a narrow window, the rest of the window dimmed; a click there closes it
/// (REQ-019-15).
fn sidebar_drawer(
    ctx: &egui::Context,
    sidebar_state: &mut sidebar::SidebarState,
    gallery: Option<&mut gallery::Gallery>,
    input: sidebar::SidebarInput,
) {
    let screen = ctx.content_rect();
    let beside = egui::Area::new(egui::Id::unique("sidebar drawer scrim"))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            ui.painter()
                .rect_filled(screen, 0.0, egui::Color32::from_black_alpha(128));
            ui.allocate_rect(screen, egui::Sense::click())
        })
        .inner;
    let width = sidebar::WIDTH.min(screen.width());
    egui::Area::new(egui::Id::unique("sidebar drawer"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme::recess())
                .stroke(egui::Stroke::new(1.0, theme::rim()))
                .inner_margin(8)
                .show(ui, |ui| {
                    let inner = egui::vec2(width - 16.0, screen.height() - 16.0);
                    ui.set_min_size(inner);
                    ui.set_max_size(inner);
                    sidebar::show(ui, sidebar_state, gallery, input);
                });
        });
    if beside.clicked() {
        sidebar_state.set_hidden(true, None);
    }
}

fn info_sheet(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(id) = state.info_sheet else { return };
    // Below the window controls, which take the top right of the window (REQ-013-02).
    if titlebar::reserved_right(ui.ctx()) > 0.0 {
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
            .show(ui, |ui| {
                let paths = gallery.tag_paths();
                clicked = info::show_loaded(ui, gallery.detail(id), &paths, !WEB);
            });
    }
    if close {
        state.info_sheet = None;
    }
    match clicked {
        Some(info::InfoAction::FilterByTag(tag)) => filter_by_tag(state, &tag),
        Some(info::InfoAction::LookForFacesAgain) => {
            state.face_action(state::FaceAction::LookAgain { media: id });
        }
        Some(info::InfoAction::Revert) => {
            let detail = state.gallery.as_ref().and_then(|gallery| gallery.cached_detail(id));
            state.confirm_revert = detail.map(state::RevertTarget::of);
        }
        Some(info::InfoAction::ClearHistory) => {
            let detail = state.gallery.as_ref().and_then(|gallery| gallery.cached_detail(id));
            state.confirm_clear_history = detail.and_then(state::ClearHistory::of);
        }
        Some(info::InfoAction::Rate(stars)) => {
            let detail = state.gallery.as_ref().and_then(|gallery| gallery.cached_detail(id));
            if let Some(rate) = detail.map(|detail| state::Rate::toggle_detail(detail, stars)) {
                state.rate(rate);
            }
        }
        Some(info::InfoAction::AddTag(path)) => tag(state, id, domain::tagging::Change::Add(path)),
        Some(info::InfoAction::RemoveTag(name)) => tag(state, id, domain::tagging::Change::Remove(name)),
        None => {}
    }
}

/// The gallery, or its empty state without libraries.
fn gallery_page(ui: &mut egui::Ui, state: &mut AppState) {
    let mut change_many = None;
    #[cfg(not(target_arch = "wasm32"))]
    let mut rename: Option<Vec<i64>> = None;
    #[cfg(not(target_arch = "wasm32"))]
    let mut fix_dates: Option<Vec<i64>> = None;
    let mut delete: Option<Vec<i64>> = None;
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
        if state.viewer.is_none() && state.confirm_many.is_none() {
            gallery.selection_keys(ui.ctx());
        }
        gallery.show(ui);
        // _Rate_, `1`–`5` or _Tag_ with a selection (REQ-025-07, REQ-026-06).
        if let Some(what) = gallery.take_selection_change() {
            change_many = Some(state::ChangeMany {
                ids: gallery.selected_ids(),
                what,
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        if gallery.take_rename_asked() {
            rename = Some(gallery.selected_ids());
        }
        #[cfg(not(target_arch = "wasm32"))]
        if gallery.take_fix_dates_asked() {
            fix_dates = Some(gallery.selected_ids());
        }
        if gallery.take_delete_asked() {
            delete = Some(gallery.selected_ids());
        }
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
            #[cfg(not(target_arch = "wasm32"))]
            Some((index, gallery::MediaAction::Copy)) => {
                let orientation = gallery.item(index).and_then(|item| item.sidecar_orientation);
                if let Some(file) = state.file_at(index) {
                    state.copy_image(file, orientation);
                }
            }
            #[cfg(target_arch = "wasm32")]
            Some((_, gallery::MediaAction::Copy)) => {}
            Some((index, gallery::MediaAction::Information)) => state.info_sheet = gallery.id_at(index),
            Some((index, gallery::MediaAction::Select)) => gallery.toggle_selected_at(index),
            // On the selection if the item is in it, else on the item (REQ-009-21).
            #[cfg(not(target_arch = "wasm32"))]
            Some((index, gallery::MediaAction::Rename)) => rename = Some(gallery.menu_targets(index)),
            Some((index, gallery::MediaAction::Rate(rating))) => {
                change_many = Some(state::ChangeMany {
                    ids: gallery.menu_targets(index),
                    what: state::Many::Rate(rating),
                });
            }
            Some((index, gallery::MediaAction::Turn(turn))) => {
                change_many = Some(state::ChangeMany {
                    ids: gallery.menu_targets(index),
                    what: state::Many::Turn(turn),
                });
            }
            Some((index, gallery::MediaAction::Delete)) => delete = Some(gallery.menu_targets(index)),
            #[cfg(not(target_arch = "wasm32"))]
            Some((index, gallery::MediaAction::FixDates)) => fix_dates = Some(gallery.menu_targets(index)),
            #[cfg(target_arch = "wasm32")]
            Some((_, gallery::MediaAction::FixDates)) => {}
            Some((_, gallery::MediaAction::Edit | gallery::MediaAction::Revert)) | None => {}
            #[cfg(target_arch = "wasm32")]
            Some((_, gallery::MediaAction::Rename)) => {}
        }
    }
    if let Some(change) = change_many {
        state.ask_change_many(change);
    }
    if let Some(ids) = delete {
        state.ask_delete(ids);
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(ids) = rename {
        state.open_rename(ids);
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(ids) = fix_dates {
        state.open_fix_dates(ids);
    }
    // The toolbar floats over the gallery, which scrolls underneath it (REQ-013-03).
    let area = ui.max_rect();
    // Over a narrow window the sidebar opens above the toolbar, which keeps its button (REQ-019-15).
    let sidebar_hidden = state.sidebar.is_hidden() || state.sidebar.is_drawer();
    let work = processing::running(state);
    let gallery = if state.libraries.is_empty() {
        None
    } else {
        state.gallery.as_mut()
    };
    let asked = toolbar::show(
        ui,
        area,
        gallery,
        &mut state.search_bar,
        sidebar_hidden,
        toolbar::Notices {
            unreadable: &state.unreadable,
            updates: &state.updates,
            work: &work,
        },
    );
    if asked.face_settings {
        state.open_face_settings();
    }
    if let Some(update) = asked.update {
        state.answer_update(update, ui.ctx());
    }
    if asked.show_sidebar {
        state.sidebar.set_hidden(false, state.db.as_ref());
    }
    if let Some(read_again) = asked.read_again {
        state.read_again(read_again);
    }
}
