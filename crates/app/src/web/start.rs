//! Starting the page (REQ-019-01): the index from the last visit, the scan queue, thumbnails, then the UI in the
//! canvas. `index.html` has checked that the browser has what the page needs.

use super::{folders, index, js, scan, thumbs};
use crate::AppState;
use eframe::egui;
use std::sync::{Arc, Mutex};

/// The page's canvas, and the element that says why the page can't run.
const CANVAS: &str = "good-gallery";
const PROBLEM: &str = "problem";

/// Starts Good Gallery in the page.
pub async fn start() {
    super::init_logging();
    tracing::info!("Good Gallery {} in the browser", env!("CARGO_PKG_VERSION"));
    if js::wait(js::hold_tab_lock()).await.ok().and_then(|held| held.as_bool()) == Some(false) {
        show_problem("Good Gallery is already open in another tab of this browser. Close it there to use it here.");
        return;
    }
    if let Err(error) = index::restore().await {
        tracing::error!("can't restore the index: {error}");
    }
    let Some(canvas) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(CANVAS))
        .and_then(|element| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlCanvasElement>(element).ok())
    else {
        tracing::error!("the page has no canvas");
        return;
    };
    let runner = eframe::WebRunner::new();
    let started = runner
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(|cc| {
                crate::setup(&cc.egui_ctx);
                if let Some(render_state) = &cc.wgpu_render_state {
                    crate::glass::install(&cc.egui_ctx, render_state);
                }
                Ok(Box::new(WebApp {
                    state: open_state(&cc.egui_ctx, cc.wgpu_render_state.clone()),
                    preferences: Default::default(),
                }))
            }),
        )
        .await;
    if let Err(error) = started {
        let error = js::message(&error);
        tracing::error!("can't start: {error}");
        show_problem(&format!(
            "Good Gallery couldn't start in this browser ({error}). It needs a current Chrome, Edge, Firefox or \
             Safari with WebGPU or WebGL 2."
        ));
    }
    // The runner must live as long as the page.
    std::mem::forget(runner);
}

fn show_problem(text: &str) {
    if let Some(element) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(PROBLEM))
    {
        element.set_text_content(Some(text));
        let _ = element.set_attribute("style", "display: flex");
    }
}

struct WebApp {
    state: AppState,
    preferences: crate::theme::OsPreferences,
}

impl eframe::App for WebApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.preferences.poll(ui.ctx());
        crate::ui(ui, &mut self.state);
    }
}

/// The index, the scan queue and thumbnails, as `main.rs`'s `open_state` builds them for the desktop.
fn open_state(ctx: &egui::Context, render_state: Option<eframe::egui_wgpu::RenderState>) -> AppState {
    let opened = index::open().and_then(|conn| Ok((conn, index::open()?)));
    let (conn, gallery_conn) = match opened {
        Ok(connections) => connections,
        Err(error) => {
            tracing::error!("can't open the index: {error}");
            return AppState::failed(error.to_string());
        }
    };
    let settings = db::settings::load(&conn).unwrap_or_default();
    let cache = thumbs::ThumbCache::open(crate::state::gib_to_bytes(settings.thumb_cache_gib));
    let (ready_tx, ready) = std::sync::mpsc::channel();
    let thumbnails = thumbs::ThumbnailService::start(cache.clone(), ready_tx, ctx.clone());
    let (events_tx, events) = std::sync::mpsc::channel();
    let rendering = thumbnails.clone();
    let indexer = scan::WebIndexer::new(events_tx, ctx.clone(), Box::new(move || rendering.kick()));
    indexer.install();
    let state = AppState::new(conn, indexer.clone(), Arc::new(crate::state::NoFolderPicker));
    let db = state.db().expect("opened above");
    let mut gallery = crate::gallery::Gallery::new(Arc::new(Mutex::new(gallery_conn)), db.clone(), Some(ctx.clone()));
    let originals = crate::originals::Originals::new(ctx.clone(), render_state.clone(), None);
    let videos = match render_state.clone() {
        Some(render_state) => crate::video::Videos::new(move || {
            Ok(Box::new(super::video::WebPlayback::new(render_state)) as Box<dyn crate::video::Playback>)
        }),
        None => crate::video::Videos::unavailable("no renderer"),
    };
    match render_state {
        Some(render_state) => {
            gallery = gallery.with_thumbnails(crate::gallery::Thumbnails {
                streamer: crate::textures::TextureStreamer::new(
                    render_state,
                    cache,
                    crate::textures::WEB_BUDGET,
                    ctx.clone(),
                ),
                cache: thumbnails.cache().clone(),
                service: Some(thumbnails.clone()),
                ready,
            });
        }
        None => tracing::error!("no wgpu renderer: the gallery shows no thumbnails"),
    }
    let settings_targets = Arc::new(thumbs::WebSettings {
        thumbnails: thumbnails.clone(),
    });
    let state = state
        .with_gallery(gallery)
        .with_originals(originals)
        .with_videos(videos)
        .with_indexer(indexer.clone(), events)
        .with_settings_targets(settings_targets);
    // Folders of an earlier visit: readable ones are scanned again (REQ-019-10), then thumbnails catch up.
    let libraries = state.libraries.clone();
    super::spawn(async move {
        folders::restore(&libraries, &db, indexer.as_ref()).await;
        thumbnails.kick();
    });
    if let Err(error) = index::keep_saved() {
        tracing::error!("the index won't be kept: {error}");
    }
    state
}
