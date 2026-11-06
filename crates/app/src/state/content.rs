//! Content search's part of the app's state (spec 036): the chosen model, its downloads, the pass and the matcher the
//! gallery asks for the photos that match some words.

use super::{AppState, Event, lock};
use db::content;
use db::rusqlite::Connection;
use domain::content::ContentModel;
use media::content::{ContentStore, TextHalf};
use media::face_store::StoreError;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A model on its way.
#[derive(Clone)]
pub struct Downloading {
    pub bytes: u64,
    pub total: u64,
    cancel: Arc<AtomicBool>,
}

/// What the pass is doing, in words (REQ-036-04).
#[derive(Debug, Clone, PartialEq)]
pub struct PassSummary {
    pub title: &'static str,
    pub detail: String,
    /// How far it has come.
    pub share: Option<f32>,
    /// It is working right now, so the processing icon says so too.
    pub working: bool,
}

/// Puts what the pass reports into words.
pub fn pass_summary(chosen: bool, paused: bool, status: Option<&indexer::content::ContentStatus>) -> PassSummary {
    let summary = |title, detail: String, share, working| PassSummary {
        title,
        detail,
        share,
        working,
    };
    if !chosen {
        return summary(
            "Off",
            "Download a model below and choose it to start.".into(),
            None,
            false,
        );
    }
    let Some(status) = status.filter(|status| status.counted) else {
        let title = if paused { "Paused" } else { "Getting ready" };
        return summary(title, String::new(), None, false);
    };
    if let Some(problem) = &status.problem {
        return summary("Stopped", problem.clone(), None, false);
    }
    let progress = status.progress;
    let count = crate::format::count;
    let share = progress.done as f32 / progress.total.max(1) as f32;
    let done = format!("{} of {} done.", count(progress.done), count(progress.total));
    if paused {
        return summary(
            "Paused",
            format!("{done} What's done can be searched."),
            Some(share),
            false,
        );
    }
    if progress.done < progress.total && status.working {
        return summary("Looking at photos", done, Some(share), true);
    }
    if progress.done < progress.total {
        let left = progress.total - progress.done;
        let detail = format!(
            "{done} The other {} are waiting for their thumbnails or for their library to be reachable.",
            count(left)
        );
        return summary("Waiting", detail, Some(share), false);
    }
    summary("Finished", format!("All {} done.", count(progress.total)), None, false)
}

/// Content search's part of the app's state.
#[derive(Default)]
pub struct Content {
    store: Option<Arc<ContentStore>>,
    service: Option<Arc<indexer::content::ContentService>>,
    /// The chosen model's id.
    pub model: Option<String>,
    pub paused: bool,
    downloading: Arc<Mutex<HashMap<&'static str, Downloading>>>,
    pub confirm_delete: bool,
    /// The pass's progress when the gallery was last loaded again for it.
    refreshed: Option<(u64, Instant)>,
}

impl Content {
    pub fn enabled(&self) -> bool {
        self.store.is_some()
    }

    /// Whether every file of `model` is on this computer.
    pub fn has(&self, model: &ContentModel) -> bool {
        self.store.as_ref().is_some_and(|store| store.has(model))
    }

    pub fn downloading(&self) -> HashMap<&'static str, Downloading> {
        lock(&self.downloading).clone()
    }

    pub fn status(&self) -> Option<indexer::content::ContentStatus> {
        self.service.as_ref().map(|service| service.status())
    }

    pub fn summary(&self) -> PassSummary {
        pass_summary(self.model.is_some(), self.paused, self.status().as_ref())
    }
}

/// While a content search shows, the gallery is loaded again this often as the pass adds photos (REQ-036-06).
const REFRESH_EVERY: Duration = Duration::from_secs(10);

impl AppState {
    /// Content search with models in `data_folder` and the pass `service` (spec 036).
    pub fn with_content(mut self, data_folder: &Path, service: Option<Arc<indexer::content::ContentService>>) -> Self {
        let store = Arc::new(ContentStore::in_data_folder(data_folder));
        self.content.store = Some(store.clone());
        self.content.service = service;
        if let Some(db) = &self.db {
            let db = lock(db);
            self.content.model = content::chosen(&db, content::MODEL_KEY).ok().flatten();
            self.content.paused = content::chosen(&db, content::PASS_KEY).ok().flatten().as_deref() == Some("paused");
        }
        if let Some(gallery) = &mut self.gallery {
            gallery.set_content_matcher(Arc::new(Matcher::new(store)));
        }
        self
    }

    fn store_content_setting(&self, key: &str, value: Option<&str>) {
        let Some(db) = &self.db else { return };
        let value = value.map_or(serde_json::Value::Null, serde_json::Value::from);
        if let Err(error) = db::settings::set_value(&lock(db), key, &value) {
            tracing::error!("can't store {key}: {error}");
        }
    }

    fn kick_content(&self) {
        if let Some(service) = &self.content.service {
            service.kick();
        }
    }

    /// Chooses a model, or none (REQ-036-01, REQ-036-08).
    pub fn choose_content_model(&mut self, model: Option<&'static ContentModel>) {
        let id = model.map(|model| model.id.to_owned());
        self.store_content_setting(content::MODEL_KEY, id.as_deref());
        self.content.model = id;
        self.kick_content();
        if let Some(gallery) = &mut self.gallery {
            gallery.reload();
        }
    }

    /// Pauses or resumes the pass (REQ-036-04).
    pub fn set_content_pass(&mut self, paused: bool) {
        self.content.paused = paused;
        self.store_content_setting(content::PASS_KEY, Some(if paused { "paused" } else { "running" }));
        self.kick_content();
    }

    /// Downloads a listed model in the background (REQ-036-02).
    pub fn download_content_model(&mut self, model: &'static ContentModel) {
        let Some(store) = self.content.store.clone() else {
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let downloading = self.content.downloading.clone();
        lock(&downloading).insert(
            model.id,
            Downloading {
                bytes: 0,
                total: model.bytes(),
                cancel: cancel.clone(),
            },
        );
        let repaint = self.repaint.clone();
        let mut shown = Instant::now();
        self.spawn(move || {
            let mut progress = |bytes: u64, _total: Option<u64>| {
                if let Some(entry) = lock(&downloading).get_mut(model.id) {
                    entry.bytes = bytes;
                }
                if shown.elapsed() > Duration::from_millis(200) {
                    shown = Instant::now();
                    if let Some(repaint) = &repaint {
                        repaint.request_repaint();
                    }
                }
                !cancel.load(Ordering::SeqCst)
            };
            Event::ContentModelArrived {
                model,
                result: store.download(model, &mut progress),
            }
        });
    }

    /// Stops a download; what arrived is dropped (REQ-036-02).
    pub fn cancel_content_download(&self, model: &ContentModel) {
        if let Some(entry) = lock(&self.content.downloading).get(model.id) {
            entry.cancel.store(true, Ordering::SeqCst);
        }
    }

    pub(super) fn content_model_arrived(&mut self, model: &'static ContentModel, result: Result<(), StoreError>) {
        lock(&self.content.downloading).remove(model.id);
        match result {
            Ok(()) => {
                self.toasts.success(format!("{} is ready", model.name));
                self.kick_content();
            }
            Err(StoreError::Cancelled) => {}
            Err(error) => {
                tracing::error!("can't get {}: {error}", model.name);
                self.toasts.error(format!("Couldn't get {}: {error}", model.name));
            }
        }
    }

    /// Removes a model's files; if it was chosen, content search goes off (REQ-036-01).
    pub fn remove_content_model(&mut self, model: &'static ContentModel) {
        if self.content.model.as_deref() == Some(model.id) {
            self.choose_content_model(None);
        }
        if let Some(store) = &self.content.store
            && let Err(error) = store.remove(model)
        {
            self.toasts.error(format!("Couldn't remove {}: {error}", model.name));
        }
    }

    /// Removes everything content search stored; files stay as they are (REQ-036-03).
    pub fn delete_content_data(&mut self) {
        let Some(db) = self.db.clone() else { return };
        self.choose_content_model(None);
        match content::delete_all(&lock(&db)) {
            Ok(()) => self.toasts.success("Search data deleted"),
            Err(error) => self.toasts.error(format!("Couldn't delete the search data: {error}")),
        }
    }

    /// While a content search shows, loads the gallery again now and then as the pass adds photos (REQ-036-06).
    /// Called every frame.
    pub fn refresh_content_search(&mut self) {
        let Some(status) = self.content.status() else { return };
        let Some(gallery) = &mut self.gallery else { return };
        let progress = status.progress;
        gallery.set_content_left(progress.total.saturating_sub(progress.done));
        if gallery.search().content.is_none() {
            return;
        }
        let done = status.progress.done;
        let due = self
            .content
            .refreshed
            .is_none_or(|(at, when)| at != done && when.elapsed() >= REFRESH_EVERY);
        if due {
            if self.content.refreshed.is_some() {
                gallery.reload();
            }
            self.content.refreshed = Some((done, Instant::now()));
        }
    }
}

/// The last answer: the model, the words, the number of vectors then, and the matches.
type LastMatch = (String, String, u64, Vec<i64>);

/// What content words match: the chosen model's text half, loaded on the first search, and the photos whose vectors
/// reach the model's cutoff (REQ-036-05, REQ-036-06). The last answer is kept until the words, the model or the number
/// of vectors change.
pub struct Matcher {
    store: Arc<ContentStore>,
    text: Mutex<Option<(&'static str, TextHalf)>>,
    last: Mutex<Option<LastMatch>>,
}

impl Matcher {
    pub fn new(store: Arc<ContentStore>) -> Self {
        Self {
            store,
            text: Mutex::new(None),
            last: Mutex::new(None),
        }
    }
}

impl crate::gallery::ContentMatches for Matcher {
    fn matches(&self, conn: &Connection, words: &str) -> Result<Vec<i64>, String> {
        let chosen = content::chosen(conn, content::MODEL_KEY).map_err(|error| error.to_string())?;
        let Some(model) = chosen.as_deref().and_then(domain::content::model) else {
            return Ok(Vec::new());
        };
        if !self.store.has(model) {
            return Ok(Vec::new());
        }
        let count = content::count(conn, model.id).map_err(|error| error.to_string())?;
        if let Some((last_model, last_words, last_count, ids)) = &*lock(&self.last)
            && last_model == model.id
            && last_words == words
            && *last_count == count
        {
            return Ok(ids.clone());
        }
        let vector = {
            let mut text = lock(&self.text);
            if text.as_ref().is_none_or(|(id, _)| *id != model.id) {
                *text = None;
                *text = Some((model.id, TextHalf::load(&self.store, model)?));
            }
            let (_, half) = text.as_ref().ok_or("the model isn't loaded")?;
            domain::content::normalized(&half.vector(words)?)
        };
        let mut ids = Vec::new();
        content::each_vector(conn, model.id, |media, stored| {
            if domain::content::similarity(stored, &vector) >= model.cutoff {
                ids.push(media);
            }
        })
        .map_err(|error| error.to_string())?;
        *lock(&self.last) = Some((model.id.to_owned(), words.to_owned(), count, ids.clone()));
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexer::content::ContentStatus;

    fn status(done: u64, total: u64, working: bool) -> ContentStatus {
        ContentStatus {
            progress: db::content::ContentProgress { done, total },
            counted: true,
            working,
            problem: None,
        }
    }

    /// REQ-036-03, REQ-036-04: the section and the processing icon say what the pass is doing.
    #[test]
    fn req_036_03_req_036_04_says_what_the_pass_is_doing() {
        assert_eq!(pass_summary(false, false, None).title, "Off");
        let working = pass_summary(true, false, Some(&status(1234, 50_000, true)));
        assert_eq!(working.title, "Looking at photos");
        assert!(
            working.detail.contains("1,234 of 50,000 done") && working.working,
            "{working:?}"
        );
        let paused = pass_summary(true, true, Some(&status(5, 10, false)));
        assert_eq!((paused.title, paused.working), ("Paused", false));
        assert_eq!(
            pass_summary(true, false, Some(&status(10, 10, false))).title,
            "Finished"
        );
        assert_eq!(pass_summary(true, false, Some(&status(8, 10, false))).title, "Waiting");
        let uncounted = ContentStatus::default();
        assert_eq!(
            pass_summary(true, false, Some(&uncounted)).title,
            "Getting ready",
            "never 0 of 0"
        );
    }

    /// REQ-036-05, REQ-036-06: with a real model on this computer (`GG_CONTENT_MODELS`, as in `media`'s test), words
    /// find the photo that shows them through the stored vectors and the model's cutoff, and not the one that doesn't.
    #[test]
    fn req_036_05_req_036_06_a_real_model_finds_the_photo() {
        use crate::gallery::ContentMatches as _;
        let Some(models) = std::env::var_os("GG_CONTENT_MODELS") else {
            eprintln!("skipped: GG_CONTENT_MODELS isn't set");
            return;
        };
        let data = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(data.path()).unwrap();
        let link = data.path().join("content-models");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&models, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&models, &link).unwrap();
        let store = Arc::new(ContentStore::in_data_folder(data.path()));
        let model = &domain::content::CATALOG[0];
        if !store.has(model) {
            eprintln!("skipped: {} isn't there", model.id);
            return;
        }
        let conn = db::open_in_memory().unwrap();
        db::libraries::insert(&conn, "/photos", "Photos", db::libraries::LibraryStatus::Online).unwrap();
        for name in ["meadow.jpg", "night.jpg"] {
            conn.execute(
                "INSERT INTO media (root_id, rel_path, file_name, kind, size, mtime) VALUES (1, ?1, ?1, 'image', 1, 1)",
                [name],
            )
            .unwrap();
        }
        let image = media::content::ImageHalf::load(&store, model).unwrap();
        let meadow = image::RgbImage::from_fn(320, 240, |_, y| {
            if y < 120 {
                image::Rgb([110, 170, 235])
            } else {
                image::Rgb([60, 150, 50])
            }
        });
        let night = image::RgbImage::from_pixel(320, 240, image::Rgb([5, 5, 12]));
        for item in content::pending(&conn, model.id, 10, 0).unwrap() {
            let photo = if item.rel_path == "meadow.jpg" { &meadow } else { &night };
            let vector = image.vector(photo).unwrap();
            content::store(&conn, &item, model.id, &domain::content::quantize(&vector)).unwrap();
        }
        db::settings::set_value(&conn, content::MODEL_KEY, &serde_json::json!(model.id)).unwrap();
        let matcher = Matcher::new(store);
        let meadow_id = db::media::id_of(&conn, 1, "meadow.jpg").unwrap().unwrap();
        assert_eq!(
            matcher.matches(&conn, "a green meadow under a blue sky").unwrap(),
            [meadow_id]
        );
        assert_eq!(
            matcher.matches(&conn, "a green meadow under a blue sky").unwrap(),
            [meadow_id],
            "kept"
        );
        db::settings::set_value(&conn, content::MODEL_KEY, &serde_json::Value::Null).unwrap();
        assert!(
            matcher.matches(&conn, "a green meadow").unwrap().is_empty(),
            "off without a model"
        );
    }
}
