//! Renaming a selection (spec 035): the dialog with its preview, and the job that renames in the background.

use crate::{format, theme};
use db::libraries::{Library, LibraryStatus};
use db::rename::RenameItem;
use db::rusqlite::Connection;
use domain::rename::{Outcome, Pattern, Row, max_rel_len, preview};
use domain::thumbs::ThumbKey;
use eframe::egui::{self, CornerRadius, FontId, Frame, Margin, RichText, Stroke};
use media::rename::{Batch, Journal, RowResult};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

/// The selection's media items, read on a thread, with the names in their folders.
struct Loaded {
    items: Vec<RenameItem>,
    libraries: HashMap<i64, String>,
    /// `(library, lowercase relative path)` of every file in the items' folders.
    on_disk: HashSet<(i64, String)>,
    /// Items in libraries that are offline, which are left out.
    offline: usize,
}

/// What the dialog asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Asked {
    Rename(RenameJob),
    Undo,
    Close,
}

/// Renames to run, with what each file's thumbnails are named by.
#[derive(Debug, Clone, PartialEq)]
pub struct RenameJob {
    pub batches: Vec<Batch>,
    /// `(library, old path)` → `(size, date modified)`.
    pub keys: HashMap<(i64, String), (i64, i64)>,
}

/// The rename dialog (REQ-035-01 … -05).
pub struct RenameDialog {
    count: usize,
    pattern: Pattern,
    loading: Option<Receiver<Result<Loaded, String>>>,
    loaded: Option<Loaded>,
    /// The pattern the rows are for, or why it can't be used.
    shown: Option<Pattern>,
    rows: Vec<Row>,
    error: Option<String>,
    /// The last rename can be taken back (REQ-035-08).
    undo: bool,
    /// When the pattern last changed (`egui::InputState::time`), and to what.
    typed: (f64, Pattern),
}

/// Above this many items the preview waits for a pause in typing: 50,000 take about 150 ms (REQ-035-04).
const WAIT_ABOVE: usize = 5_000;
const PAUSE: f64 = 0.25;

impl RenameDialog {
    /// Opens for `ids`, read with `db` on a thread.
    pub fn open(db: Arc<Mutex<Connection>>, libraries: &[Library], ids: Vec<i64>, undo: bool) -> Self {
        let (tx, rx) = channel();
        let libraries = libraries.to_vec();
        let count = ids.len();
        std::thread::Builder::new()
            .name("rename preview".into())
            .spawn(move || {
                let _ = tx.send(load(&db, &libraries, &ids));
            })
            .expect("can't start the rename thread");
        Self {
            count,
            pattern: Pattern::default(),
            loading: Some(rx),
            loaded: None,
            shown: None,
            rows: Vec::new(),
            error: None,
            undo,
            typed: (0.0, Pattern::default()),
        }
    }

    /// The preview's rows, once loaded.
    /// How many items it renames.
    pub fn count(&self) -> usize {
        self.count
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    fn poll(&mut self) {
        let Some(rx) = &self.loading else { return };
        match rx.try_recv() {
            Ok(Ok(loaded)) => {
                self.loaded = Some(loaded);
                self.loading = None;
            }
            Ok(Err(error)) => {
                self.error = Some(error);
                self.loading = None;
            }
            Err(_) => {}
        }
    }

    /// Works the rows out again when the pattern changed (REQ-035-04).
    fn update(&mut self, ctx: &egui::Context) {
        let Some(loaded) = &self.loaded else { return };
        if self.shown.as_ref() == Some(&self.pattern) {
            return;
        }
        let now = ctx.input(|input| input.time);
        if self.typed.1 != self.pattern {
            self.typed = (now, self.pattern.clone());
        }
        if loaded.items.len() > WAIT_ABOVE && self.shown.is_some() && now - self.typed.0 < PAUSE {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(PAUSE));
            return;
        }
        self.shown = Some(self.pattern.clone());
        let compiled = match self.pattern.compile() {
            Ok(compiled) => compiled,
            Err(error) => {
                self.error = Some(format!("The regular expression can't be used: {error}"));
                self.rows.clear();
                return;
            }
        };
        self.error = None;
        let mut by_library: Vec<(i64, Vec<domain::rename::Item>)> = Vec::new();
        for item in &loaded.items {
            match by_library.iter_mut().find(|(root, _)| *root == item.root_id) {
                Some((_, items)) => items.push(item.item.clone()),
                None => by_library.push((item.root_id, vec![item.item.clone()])),
            }
        }
        self.rows = by_library
            .into_iter()
            .flat_map(|(root, items)| {
                let library = loaded.libraries.get(&root).cloned().unwrap_or_default();
                let exists = |rel: &str| loaded.on_disk.contains(&(root, rel.to_lowercase()));
                preview(&compiled, &items, max_rel_len(&library), exists)
            })
            .collect();
    }

    /// The renames the rows ask for, by library.
    fn job(&self) -> Option<RenameJob> {
        let loaded = self.loaded.as_ref()?;
        let mut batches: Vec<Batch> = Vec::new();
        let mut keys = HashMap::new();
        for (row, item) in self.rows.iter().zip(loaded.items.iter()) {
            let Outcome::Renamed(new) = &row.outcome else { continue };
            keys.insert((item.root_id, row.old.clone()), (item.size, item.item.mtime));
            let pair = (row.old.clone(), new.clone());
            match batches.iter_mut().find(|batch| batch.root_id == item.root_id) {
                Some(batch) => batch.renames.push(pair),
                None => batches.push(Batch {
                    root_id: item.root_id,
                    library: PathBuf::from(loaded.libraries.get(&item.root_id).cloned().unwrap_or_default()),
                    renames: vec![pair],
                }),
            }
        }
        (!batches.is_empty()).then_some(RenameJob { batches, keys })
    }

    /// Draws the dialog; what was asked for, if anything.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Asked> {
        self.poll();
        self.update(ctx);
        if self.loading.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        let mut asked = None;
        let counts = Counts::of(&self.rows);
        let modal = egui::Modal::new(egui::Id::unique("rename dialog"))
            .frame(
                Frame::new()
                    .fill(theme::raised())
                    .stroke(Stroke::new(1.0, theme::rim()))
                    .corner_radius(CornerRadius::same(theme::RADIUS_PANEL))
                    .inner_margin(Margin::same(20)),
            )
            .show(ctx, |ui| {
                crate::glass::behind_frame(ui, Margin::same(20), |ui| {
                    ui.set_width(640.0);
                    let title = if self.count == 1 {
                        "Rename 1 item".to_string()
                    } else {
                        format!("Rename {} items", format::count(self.count as u64))
                    };
                    ui.label(RichText::new(title).font(FontId::new(theme::SIZE_TITLE, theme::semibold())));
                    ui.add_space(theme::SPACE * 3.0);
                    egui::Grid::new("rename fields")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Find");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.pattern.find)
                                    .desired_width(f32::INFINITY)
                                    .hint_text("Empty: the whole name"),
                            );
                            ui.end_row();
                            ui.label("Replace with");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.pattern.replace)
                                    .desired_width(f32::INFINITY)
                                    .hint_text("{taken:yyyy-MM-dd} {name}"),
                            );
                            ui.end_row();
                        });
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.pattern.regex, "Regular expression");
                        ui.checkbox(&mut self.pattern.match_case, "Match case");
                    });
                    ui.label(
                        RichText::new(
                            "{name} is the old name, {taken:yyyy-MM-dd HH-mm-ss} the date taken, {modified:…} the date \
                             modified, {tags} the tags without People and Places. Extensions stay.",
                        )
                        .size(theme::SIZE_CAPTION)
                        .color(theme::muted()),
                    );
                    ui.add_space(theme::SPACE * 2.0);
                    self.summary(ui, &counts);
                    self.list(ui);
                    ui.add_space(theme::SPACE * 4.0);
                    ui.horizontal(|ui| {
                        if self.undo && ui.button("Undo last rename").clicked() {
                            asked = Some(Asked::Undo);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let label = if counts.renamed > 0 {
                                format!("Rename {}", format::count(counts.renamed as u64))
                            } else {
                                "Rename".into()
                            };
                            let rename = egui::Button::new(
                                RichText::new(label)
                                    .color(theme::base())
                                    .font(FontId::new(theme::SIZE_BODY, theme::semibold())),
                            )
                            .fill(theme::text());
                            if ui.add_enabled(counts.renamed > 0, rename).clicked()
                                && let Some(job) = self.job()
                            {
                                asked = Some(Asked::Rename(job));
                            }
                            if ui
                                .add(egui::Button::new("Cancel").stroke(Stroke::new(1.0, theme::rim())))
                                .clicked()
                            {
                                asked = Some(Asked::Close);
                            }
                        });
                    });
                });
            });
        if modal.should_close() && asked.is_none() {
            asked = Some(Asked::Close);
        }
        asked
    }

    fn summary(&self, ui: &mut egui::Ui, counts: &Counts) {
        let text = if let Some(error) = &self.error {
            RichText::new(error).color(theme::alert())
        } else if self.loaded.is_none() {
            RichText::new("Reading the names…").color(theme::muted())
        } else {
            let mut parts = vec![
                format!("{} renamed", format::count(counts.renamed as u64)),
                format!("{} unchanged", format::count(counts.unchanged as u64)),
            ];
            if counts.failed > 0 {
                parts.push(format!("{} can't be renamed", format::count(counts.failed as u64)));
            }
            let offline = self.loaded.as_ref().map_or(0, |loaded| loaded.offline);
            if offline > 0 {
                parts.push(format!(
                    "{} in offline libraries left out",
                    format::count(offline as u64)
                ));
            }
            RichText::new(parts.join(" · ")).color(theme::text())
        };
        ui.label(text);
    }

    /// Every row, old name and new name or why not; only the rows in view are laid out.
    fn list(&self, ui: &mut egui::Ui) {
        let row_height = 22.0;
        egui::ScrollArea::vertical()
            .id_salt("rename rows")
            .max_height(300.0)
            .auto_shrink([false, true])
            .show_rows(ui, row_height, self.rows.len(), |ui, range| {
                for row in &self.rows[range] {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(ui.available_width(), row_height), egui::Sense::hover());
                    {
                        let old = file_name(&row.old);
                        let half = (rect.width() - 24.0) / 2.0;
                        let left = egui::Rect::from_min_size(rect.min, egui::vec2(half, row_height));
                        let middle = egui::Rect::from_min_size(left.right_top(), egui::vec2(24.0, row_height));
                        let right = egui::Rect::from_min_size(middle.right_top(), egui::vec2(half, row_height));
                        let (new, color) = match &row.outcome {
                            Outcome::Renamed(new) => (file_name(new).to_string(), theme::text()),
                            Outcome::Unchanged => ("Unchanged".into(), theme::muted()),
                            Outcome::Failed(problem) => (problem.text(), theme::alert()),
                        };
                        let renamed = matches!(row.outcome, Outcome::Renamed(_));
                        let old_color = if renamed { theme::text() } else { theme::muted() };
                        let cell = |ui: &mut egui::Ui, rect: egui::Rect, text: RichText| {
                            ui.scope_builder(
                                egui::UiBuilder::new()
                                    .max_rect(rect)
                                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                |ui| ui.add(egui::Label::new(text).truncate()),
                            );
                        };
                        cell(ui, left, RichText::new(old).color(old_color));
                        // The bundled fonts have no arrow.
                        crate::icons::Icon::ChevronRight.image(14.0, theme::muted()).paint_at(
                            ui,
                            egui::Rect::from_center_size(middle.center(), egui::Vec2::splat(14.0)),
                        );
                        cell(ui, right, RichText::new(new).color(color));
                    }
                }
            });
    }
}

fn file_name(rel_path: &str) -> &str {
    rel_path.rsplit('/').next().unwrap_or(rel_path)
}

#[derive(Debug, Default)]
struct Counts {
    renamed: usize,
    unchanged: usize,
    failed: usize,
}

impl Counts {
    fn of(rows: &[Row]) -> Self {
        let mut counts = Self::default();
        for row in rows {
            match row.outcome {
                Outcome::Renamed(_) => counts.renamed += 1,
                Outcome::Unchanged => counts.unchanged += 1,
                Outcome::Failed(_) => counts.failed += 1,
            }
        }
        counts
    }
}

fn load(db: &Mutex<Connection>, libraries: &[Library], ids: &[i64]) -> Result<Loaded, String> {
    let items = {
        let conn = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        db::rename::items(&conn, ids).map_err(|error| error.to_string())?
    };
    let online: HashMap<i64, String> = libraries
        .iter()
        .filter(|library| library.status != LibraryStatus::Offline)
        .map(|library| (library.id, library.path.clone()))
        .collect();
    let before = items.len();
    let mut items: Vec<RenameItem> = items
        .into_iter()
        .filter(|item| online.contains_key(&item.root_id))
        .collect();
    // In the order of their paths, as a file manager lists them.
    items.sort_by(|a, b| (a.root_id, &a.item.rel_path).cmp(&(b.root_id, &b.item.rel_path)));
    let offline = before - items.len();
    // Each folder is listed once: 50,000 stats on a share would take minutes.
    let mut folders: HashSet<(i64, String)> = HashSet::new();
    for item in &items {
        let rel = &item.item.rel_path;
        folders.insert((
            item.root_id,
            rel.rfind('/').map_or(String::new(), |slash| rel[..=slash].to_string()),
        ));
    }
    let mut on_disk = HashSet::new();
    for (root, folder) in folders {
        let path = Path::new(&online[&root]).join(&folder);
        for entry in std::fs::read_dir(&path).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            on_disk.insert((root, format!("{}{name}", folder.to_lowercase())));
        }
    }
    Ok(Loaded {
        items,
        libraries: online,
        on_disk,
        offline,
    })
}

/// What a rename did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RenameDone {
    pub renamed: usize,
    /// The first file that couldn't be renamed, and why, with how many couldn't.
    pub failed: Option<(String, String, usize)>,
    /// Paths to read again, by library: every old and new one.
    pub touched: Vec<(i64, Vec<String>)>,
    /// It finished a rename the app was ended in (REQ-035-09).
    pub recovered: bool,
}

/// Renames `job`: journal, index, files, then whatever didn't move back in the index (REQ-035-06 … -09). Runs on the
/// writer thread.
pub fn run(
    db: &Mutex<Connection>,
    cache: Option<&indexer::thumb_cache::ThumbCache>,
    journal_path: &Path,
    job: &RenameJob,
) -> Result<RenameDone, String> {
    let mut journal = Journal::start(journal_path, &job.batches).map_err(|error| error.to_string())?;
    {
        let conn = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        for batch in &job.batches {
            db::rename::rename(&conn, batch.root_id, &batch.renames).map_err(|error| error.to_string())?;
        }
    }
    journal.indexed().map_err(|error| error.to_string())?;
    let mut done = RenameDone::default();
    for (index, batch) in job.batches.iter().enumerate() {
        let results = media::rename::execute(batch, &HashSet::new(), |step| {
            let _ = journal.done(index, step);
        });
        settle(db, cache, batch, &results, &job.keys, &mut done);
    }
    journal.finish().map_err(|error| error.to_string())?;
    Ok(done)
}

/// Finishes a rename the journal at `path` says was interrupted, once its libraries are there (REQ-035-09).
/// `Ok(None)` when there is nothing to finish, or a library isn't there yet.
pub fn recover(
    db: &Mutex<Connection>,
    cache: Option<&indexer::thumb_cache::ThumbCache>,
    journal_path: &Path,
) -> Result<Option<RenameDone>, String> {
    let Some(state) = Journal::read(journal_path).map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    if state.finished {
        return Ok(None);
    }
    let mut journal = Journal::reopen(journal_path).map_err(|error| error.to_string())?;
    // Ended before the index changed: no file was touched either.
    if !state.indexed {
        journal.finish().map_err(|error| error.to_string())?;
        return Ok(None);
    }
    if state.batches.iter().any(|batch| !batch.library.is_dir()) {
        return Ok(None);
    }
    let mut done = RenameDone {
        recovered: true,
        ..RenameDone::default()
    };
    for (index, batch) in state.batches.iter().enumerate() {
        let finished: HashSet<usize> = state
            .done
            .iter()
            .filter(|(b, _)| *b == index)
            .map(|(_, step)| *step)
            .collect();
        let results = media::rename::execute(batch, &finished, |step| {
            let _ = journal.done(index, step);
        });
        settle(db, cache, batch, &results, &HashMap::new(), &mut done);
    }
    journal.finish().map_err(|error| error.to_string())?;
    Ok(Some(done))
}

/// After the files moved: rows of files that didn't reach their new name go where the file is, renamed files keep
/// their thumbnails, and everything touched is counted.
fn settle(
    db: &Mutex<Connection>,
    cache: Option<&indexer::thumb_cache::ThumbCache>,
    batch: &Batch,
    results: &[RowResult],
    keys: &HashMap<(i64, String), (i64, i64)>,
    done: &mut RenameDone,
) {
    let back: Vec<(String, String)> = results
        .iter()
        .filter(|result| result.at != result.new)
        .map(|result| (result.new.clone(), result.at.clone()))
        .collect();
    if !back.is_empty() {
        let conn = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Err(error) = db::rename::rename(&conn, batch.root_id, &back) {
            tracing::error!("can't put renamed rows back: {error}");
        }
    }
    let mut touched = Vec::new();
    for result in results {
        touched.push(result.old.clone());
        touched.push(result.new.clone());
        match &result.error {
            None => {
                done.renamed += 1;
                if let (Some(cache), Some(&(size, mtime))) = (cache, keys.get(&(batch.root_id, result.old.clone()))) {
                    let key = |rel_path: &str| ThumbKey {
                        root_id: batch.root_id,
                        rel_path: rel_path.to_string(),
                        size,
                        mtime,
                    };
                    cache.rename(&key(&result.old), &key(&result.new));
                }
            }
            Some(error) => match &mut done.failed {
                Some((_, _, count)) => *count += 1,
                None => done.failed = Some((file_name(&result.old).to_string(), error.clone(), 1)),
            },
        }
    }
    done.touched.push((batch.root_id, touched));
}

/// The last rename's journal says it finished: the renames that take it back, for files still at their new name
/// in the index (REQ-035-08). `None` without one.
pub fn undo_job(db: &Mutex<Connection>, journal_path: &Path) -> Option<RenameJob> {
    let state = Journal::read(journal_path).ok()??;
    if !state.finished || !state.indexed {
        return None;
    }
    let conn = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut batches = Vec::new();
    let mut keys = HashMap::new();
    for batch in state.batches {
        let mut renames = Vec::new();
        for (old, new) in batch.renames {
            let Ok(Some(id)) = db::media::id_of(&conn, batch.root_id, &new) else {
                continue;
            };
            // The old name was taken meanwhile: that file stays as it is.
            let taken = batch.library.join(&old).exists()
                || matches!(db::media::id_of(&conn, batch.root_id, &old), Ok(Some(_)));
            if taken {
                continue;
            }
            if let Ok(items) = db::rename::items(&conn, &[id]) {
                for item in items {
                    keys.insert((batch.root_id, new.clone()), (item.size, item.item.mtime));
                }
            }
            renames.push((new, old));
        }
        if !renames.is_empty() {
            batches.push(Batch {
                root_id: batch.root_id,
                library: batch.library,
                renames,
            });
        }
    }
    (!batches.is_empty()).then_some(RenameJob { batches, keys })
}
