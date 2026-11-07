//! Fixing the dates of a selection (spec 038): the dialog with its preview, the photos' dates read on a thread, and
//! the jobs that write them and take them back in the background.

use crate::{format, theme};
use db::libraries::{Library, LibraryStatus};
use db::rusqlite::Connection;
use domain::date_fix::{Field, Item, Outcome, Problem, Row, Source, Stamp, new_date};
use domain::edit::MtimeOnSave;
use domain::media::MediaKind;
use eframe::egui::{self, CornerRadius, FontId, Frame, Margin, RichText, Stroke};
use media::dates::{Change, Dates, Journal, Previous};
use media::exiftool::ExifTool;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

/// Photos read in one exiftool run.
const READ_CHUNK: usize = 100;

/// What the reading thread sends.
enum Message {
    Items {
        items: Vec<Item>,
        places: Vec<Place>,
        offline: usize,
    },
    Read {
        start: usize,
        dates: Vec<Dates>,
    },
    Failed(String),
}

/// Where an item's file is.
#[derive(Debug, Clone)]
struct Place {
    root_id: i64,
    library: PathBuf,
}

/// The kinds of source the dialog offers (REQ-038-02 … -04).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Field,
    FileName,
    Shift,
}

/// What the dialog asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Asked {
    Fix(FixJob),
    Undo,
    Close,
}

/// Dates to write: each change with its media item.
#[derive(Debug, Clone, PartialEq)]
pub struct FixJob {
    pub changes: Vec<(i64, Change)>,
}

/// The dialog (REQ-038-01 … -06).
pub struct FixDatesDialog {
    count: usize,
    pub kind: Kind,
    pub field: Field,
    /// A pattern of the user's own; empty for the built-in ones.
    pub pattern: String,
    /// The shift: days, hours and minutes, each forwards or back.
    pub shift: [i64; 3],
    messages: Option<Receiver<Message>>,
    items: Vec<Item>,
    places: Vec<Place>,
    previous: Vec<Option<Previous>>,
    offline: usize,
    /// The source the rows are for.
    shown: Option<Source>,
    rows: Vec<Row>,
    error: Option<String>,
    undo: bool,
}

impl FixDatesDialog {
    /// Opens for `ids`: read from the index with `db`, then their dates from their files with `exiftool`.
    pub fn open(
        db: Arc<Mutex<Connection>>,
        exiftool: Arc<ExifTool>,
        libraries: &[Library],
        ids: Vec<i64>,
        undo: bool,
    ) -> Self {
        let (tx, rx) = channel();
        let libraries = libraries.to_vec();
        let count = ids.len();
        std::thread::Builder::new()
            .name("date fix preview".into())
            .spawn(move || {
                let (items, places, offline) = match load(&db, &libraries, &ids) {
                    Ok(loaded) => loaded,
                    Err(error) => {
                        let _ = tx.send(Message::Failed(error));
                        return;
                    }
                };
                // Videos are left out; their files aren't read.
                let photos: Vec<usize> = (0..items.len())
                    .filter(|&i| items[i].kind != MediaKind::Video)
                    .collect();
                let files: Vec<(usize, PathBuf)> = photos
                    .iter()
                    .map(|&i| (i, places[i].library.join(&items[i].rel_path)))
                    .collect();
                if tx.send(Message::Items { items, places, offline }).is_err() {
                    return;
                }
                for chunk in files.chunks(READ_CHUNK) {
                    let paths: Vec<PathBuf> = chunk.iter().map(|(_, path)| path.clone()).collect();
                    let dates = match media::dates::read(&exiftool, &paths) {
                        Ok(dates) => dates,
                        Err(error) => {
                            let _ = tx.send(Message::Failed(format!("Couldn't read the dates: {error}")));
                            return;
                        }
                    };
                    // One message per photo position: the chunk's photos are in order, but not consecutive.
                    for ((index, _), dates) in chunk.iter().zip(dates) {
                        if tx
                            .send(Message::Read {
                                start: *index,
                                dates: vec![dates],
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            })
            .expect("can't start the date fix thread");
        Self {
            count,
            kind: Kind::FileName,
            field: Field::Created,
            pattern: String::new(),
            shift: [0; 3],
            messages: Some(rx),
            items: Vec::new(),
            places: Vec::new(),
            previous: Vec::new(),
            offline: 0,
            shown: None,
            rows: Vec::new(),
            error: None,
            undo,
        }
    }

    /// How many items it was opened for.
    pub fn count(&self) -> usize {
        self.count
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Every photo was read.
    pub fn is_read(&self) -> bool {
        self.messages.is_none() && !self.items.is_empty()
    }

    /// The source the fields ask for.
    pub fn source(&self) -> Source {
        match self.kind {
            Kind::Field => Source::Field(self.field),
            Kind::FileName => Source::FileName(self.pattern.clone()),
            Kind::Shift => Source::Shift(self.shift[0] * 24 * 60 + self.shift[1] * 60 + self.shift[2]),
        }
    }

    fn now() -> chrono::NaiveDateTime {
        domain::time_zone::wall_clock(chrono::Utc::now().timestamp_millis())
            .unwrap_or_else(|| chrono::Local::now().naive_local())
    }

    fn row(&self, source: &Source, index: usize, now: chrono::NaiveDateTime) -> Row {
        let item = &self.items[index];
        Row {
            id: item.id,
            old: item.old(),
            outcome: new_date(source, item, now, domain::time_zone::offset_at),
        }
    }

    /// Takes what the thread read; rows of newly read photos are worked out again.
    fn poll(&mut self) {
        let Some(rx) = &self.messages else { return };
        let now = Self::now();
        loop {
            match rx.try_recv() {
                Ok(Message::Items { items, places, offline }) => {
                    self.previous = vec![None; items.len()];
                    self.items = items;
                    self.places = places;
                    self.offline = offline;
                    for item in &mut self.items {
                        if item.kind == MediaKind::Video {
                            item.read = Some(domain::date_fix::Read::default());
                        }
                    }
                    self.shown = None;
                }
                Ok(Message::Read { start, dates }) => {
                    for (offset, dates) in dates.into_iter().enumerate() {
                        let index = start + offset;
                        self.items[index].read = Some(dates.read);
                        self.previous[index] = Some(dates.previous);
                        if let Some(source) = self.shown.clone() {
                            let row = self.row(&source, index, now);
                            self.rows[index] = row;
                        }
                    }
                }
                Ok(Message::Failed(error)) => {
                    self.error = Some(error);
                    self.messages = None;
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.messages = None;
                    return;
                }
            }
        }
    }

    /// Works every row out again when the source changed (REQ-038-05).
    fn update(&mut self) {
        let source = self.source();
        if self.shown.as_ref() == Some(&source) {
            return;
        }
        let now = Self::now();
        self.rows = (0..self.items.len())
            .map(|index| self.row(&source, index, now))
            .collect();
        self.shown = Some(source);
    }

    /// The dates the rows ask for.
    fn job(&self) -> Option<FixJob> {
        let changes: Vec<(i64, Change)> = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let Outcome::Changed(new) = row.outcome else {
                    return None;
                };
                let item = &self.items[index];
                Some((
                    item.id,
                    Change {
                        root_id: self.places[index].root_id,
                        library: self.places[index].library.clone(),
                        rel_path: item.rel_path.clone(),
                        new,
                        previous: self.previous[index].clone()?,
                        mtime: item.mtime,
                    },
                ))
            })
            .collect();
        (!changes.is_empty()).then_some(FixJob { changes })
    }

    /// Draws the dialog; what was asked for, if anything.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Asked> {
        self.poll();
        self.update();
        if self.messages.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        let mut asked = None;
        let counts = Counts::of(&self.rows);
        let modal = egui::Modal::new(egui::Id::unique("date fix dialog"))
            .frame(
                Frame::new()
                    .fill(theme::raised())
                    .stroke(Stroke::new(1.0, theme::rim()))
                    .corner_radius(CornerRadius::same(theme::RADIUS_PANEL))
                    .inner_margin(Margin::same(20)),
            )
            .show(ctx, |ui| {
                crate::glass::behind_frame(ui, Margin::same(20), |ui| {
                    ui.set_width(680.0);
                    let title = if self.count == 1 {
                        "Fix the date of 1 item".to_string()
                    } else {
                        format!("Fix the dates of {} items", format::count(self.count as u64))
                    };
                    ui.label(RichText::new(title).font(FontId::new(theme::SIZE_TITLE, theme::semibold())));
                    ui.add_space(theme::SPACE * 3.0);
                    self.fields(ui);
                    ui.add_space(theme::SPACE * 2.0);
                    self.summary(ui, &counts);
                    self.list(ui);
                    ui.add_space(theme::SPACE * 4.0);
                    ui.horizontal(|ui| {
                        if self.undo && ui.button("Undo last fix").clicked() {
                            asked = Some(Asked::Undo);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let label = if counts.changed > 0 {
                                format!("Fix {}", format::count(counts.changed as u64))
                            } else {
                                "Fix".into()
                            };
                            let fix = egui::Button::new(
                                RichText::new(label)
                                    .color(theme::base())
                                    .font(FontId::new(theme::SIZE_BODY, theme::semibold())),
                            )
                            .fill(theme::text());
                            if ui.add_enabled(counts.changed > 0, fix).clicked()
                                && let Some(job) = self.job()
                            {
                                asked = Some(Asked::Fix(job));
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

    /// The source: another date, the file name or a shift, each with its own fields.
    fn fields(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("date fix fields")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.radio_value(&mut self.kind, Kind::FileName, "From the file name");
                let pattern = ui.add(
                    egui::TextEdit::singleline(&mut self.pattern)
                        .desired_width(f32::INFINITY)
                        .hint_text("Empty: 20230512_101500, 2023-05-12 10.15.00 and the like"),
                );
                if pattern.gained_focus() {
                    self.kind = Kind::FileName;
                }
                ui.end_row();
                let radio = ui.radio_value(&mut self.kind, Kind::Field, "From another date");
                let mut field = self.field;
                egui::ComboBox::from_id_salt("date fix field")
                    .selected_text(field.label())
                    .show_ui(ui, |ui| {
                        for option in Field::ALL {
                            ui.selectable_value(&mut field, option, option.label());
                        }
                    })
                    .response
                    .labelled_by(radio.id);
                if field != self.field {
                    (self.field, self.kind) = (field, Kind::Field);
                }
                ui.end_row();
                ui.radio_value(&mut self.kind, Kind::Shift, "Shift");
                ui.horizontal(|ui| {
                    let mut changed = false;
                    for (value, unit) in self.shift.iter_mut().zip(["days", "hours", "minutes"]) {
                        let drag = ui.add(egui::DragValue::new(value).speed(0.2));
                        changed |= drag.changed();
                        drag.labelled_by(ui.label(unit).id);
                    }
                    if changed {
                        self.kind = Kind::Shift;
                    }
                });
                ui.end_row();
            });
        ui.label(
            RichText::new(
                "A pattern of your own uses yyyy, MM, dd, HH, mm and ss; the rest of the name is ignored. A date without \
                 a time is set to 12:00. A shift corrects a camera clock that was wrong; values below 0 go back.",
            )
            .size(theme::SIZE_CAPTION)
            .color(theme::muted()),
        );
    }

    fn summary(&self, ui: &mut egui::Ui, counts: &Counts) {
        let text = if let Some(error) = &self.error {
            RichText::new(error).color(theme::alert())
        } else if self.items.is_empty() && self.messages.is_some() {
            RichText::new("Reading the dates…").color(theme::muted())
        } else {
            let mut parts = vec![
                format!("{} changed", format::count(counts.changed as u64)),
                format!("{} unchanged", format::count(counts.unchanged as u64)),
            ];
            if counts.failed > 0 {
                parts.push(format!("{} can't be fixed", format::count(counts.failed as u64)));
            }
            if counts.reading > 0 {
                parts.push(format!("{} still being read", format::count(counts.reading as u64)));
            }
            if self.offline > 0 {
                parts.push(format!(
                    "{} in offline libraries left out",
                    format::count(self.offline as u64)
                ));
            }
            RichText::new(parts.join(" · ")).color(theme::text())
        };
        ui.label(text);
    }

    /// Every row: name, old date and new date or why not; only the rows in view are laid out.
    fn list(&self, ui: &mut egui::Ui) {
        let row_height = 22.0;
        egui::ScrollArea::vertical()
            .id_salt("date fix rows")
            .max_height(300.0)
            .auto_shrink([false, true])
            .show_rows(ui, row_height, self.rows.len(), |ui, range| {
                for (row, item) in self.rows[range.clone()].iter().zip(&self.items[range]) {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(ui.available_width(), row_height), egui::Sense::hover());
                    let name_width = rect.width() * 0.4;
                    let date_width = (rect.width() - name_width - 24.0) / 2.0;
                    let name = egui::Rect::from_min_size(rect.min, egui::vec2(name_width, row_height));
                    let old = egui::Rect::from_min_size(name.right_top(), egui::vec2(date_width, row_height));
                    let arrow = egui::Rect::from_min_size(old.right_top(), egui::vec2(24.0, row_height));
                    let new = egui::Rect::from_min_size(arrow.right_top(), egui::vec2(date_width, row_height));
                    let changed = matches!(row.outcome, Outcome::Changed(_));
                    let strong = if changed { theme::text() } else { theme::muted() };
                    let (new_text, new_color) = match &row.outcome {
                        Outcome::Changed(stamp) => (date_text(Some(*stamp)), theme::text()),
                        Outcome::Unchanged => ("Unchanged".into(), theme::muted()),
                        Outcome::Failed(Problem::Reading) => (Problem::Reading.text(), theme::muted()),
                        Outcome::Failed(problem) => (problem.text(), theme::alert()),
                    };
                    let cell = |ui: &mut egui::Ui, rect: egui::Rect, text: RichText| {
                        ui.scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(rect)
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                            |ui| ui.add(egui::Label::new(text).truncate()),
                        );
                    };
                    let file = item.rel_path.rsplit('/').next().unwrap_or(&item.rel_path);
                    cell(ui, name, RichText::new(file).color(strong));
                    cell(ui, old, RichText::new(date_text(row.old)).color(strong));
                    crate::icons::Icon::ChevronRight.image(14.0, theme::muted()).paint_at(
                        ui,
                        egui::Rect::from_center_size(arrow.center(), egui::Vec2::splat(14.0)),
                    );
                    cell(ui, new, RichText::new(new_text).color(new_color));
                }
            });
    }
}

/// A date as the list shows it, with its offset.
fn date_text(stamp: Option<Stamp>) -> String {
    match stamp {
        None => "No date taken".into(),
        Some(stamp) => {
            let at = stamp.at.format("%Y-%m-%d %H:%M:%S").to_string();
            match stamp.offset_text() {
                Some(offset) => format!("{at} {offset}"),
                None => at,
            }
        }
    }
}

#[derive(Debug, Default)]
struct Counts {
    changed: usize,
    unchanged: usize,
    failed: usize,
    reading: usize,
}

impl Counts {
    fn of(rows: &[Row]) -> Self {
        let mut counts = Self::default();
        for row in rows {
            match row.outcome {
                Outcome::Changed(_) => counts.changed += 1,
                Outcome::Unchanged => counts.unchanged += 1,
                Outcome::Failed(Problem::Reading) => counts.reading += 1,
                Outcome::Failed(_) => counts.failed += 1,
            }
        }
        counts
    }
}

fn load(db: &Mutex<Connection>, libraries: &[Library], ids: &[i64]) -> Result<(Vec<Item>, Vec<Place>, usize), String> {
    let dated = {
        let conn = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        db::media::dated(&conn, ids).map_err(|error| error.to_string())?
    };
    let online: HashMap<i64, &Library> = libraries
        .iter()
        .filter(|library| library.status != LibraryStatus::Offline)
        .map(|library| (library.id, library))
        .collect();
    let before = dated.len();
    let mut items = Vec::new();
    let mut places = Vec::new();
    for item in dated {
        let Some(library) = online.get(&item.root_id) else {
            continue;
        };
        places.push(Place {
            root_id: item.root_id,
            library: PathBuf::from(&library.path),
        });
        items.push(Item {
            id: item.id,
            rel_path: item.rel_path,
            kind: item.kind,
            mtime: item.mtime,
            indexed: item.taken_at.and_then(domain::time_zone::wall_clock),
            read: None,
        });
    }
    let offline = before - items.len();
    Ok((items, places, offline))
}

/// What fixing or taking back dates did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DatesDone {
    pub fixed: usize,
    /// The first file that couldn't be written, and why, with how many couldn't.
    pub failed: Option<(String, String, usize)>,
    /// Files changed since the fix, which _Undo_ left alone (REQ-038-09).
    pub skipped: usize,
    /// Paths to read again, by library, and the media items written.
    pub touched: Vec<(i64, Vec<String>)>,
    pub undone: bool,
}

impl DatesDone {
    fn touch(&mut self, root_id: i64, rel_path: &str) {
        match self.touched.iter_mut().find(|(root, _)| *root == root_id) {
            Some((_, paths)) => paths.push(rel_path.to_owned()),
            None => self.touched.push((root_id, vec![rel_path.to_owned()])),
        }
    }

    fn fail(&mut self, rel_path: &str, error: String) {
        let name = rel_path.rsplit('/').next().unwrap_or(rel_path).to_owned();
        match &mut self.failed {
            Some((_, _, count)) => *count += 1,
            None => self.failed = Some((name, error, 1)),
        }
    }
}

fn lock(db: &Mutex<Connection>) -> std::sync::MutexGuard<'_, Connection> {
    db.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Writes `job`'s dates one file after another, journaled for _Undo_ (REQ-038-08, REQ-038-09).
pub fn run(
    db: &Mutex<Connection>,
    exiftool: &Path,
    journal: &Path,
    job: &FixJob,
    mtime: MtimeOnSave,
) -> Result<DatesDone, String> {
    let changes: Vec<Change> = job.changes.iter().map(|(_, change)| change.clone()).collect();
    let mut log = Journal::start(journal, &changes).map_err(|error| format!("can't write the journal: {error}"))?;
    let mut done = DatesDone::default();
    for (index, (media, change)) in job.changes.iter().enumerate() {
        let written = media::dates::write(exiftool, &change.library, &change.rel_path, &change.new, mtime)
            .map_err(|error| error.to_string())
            .and_then(|()| {
                let file = change.library.join(&change.rel_path);
                media::dates::size_and_mtime(&file).map_err(|error| error.to_string())
            });
        match written {
            Ok((size, after)) => {
                if let Err(error) = log.done(index, size, after) {
                    tracing::error!("can't record a date fix in the journal: {error}");
                }
                if let Err(error) = db::media::mark_for_reading(&lock(db), *media) {
                    tracing::error!("can't mark {} for reading: {error}", change.rel_path);
                }
                done.fixed += 1;
                done.touch(change.root_id, &change.rel_path);
            }
            Err(error) => {
                tracing::error!("can't fix the date of {}: {error}", change.rel_path);
                done.fail(&change.rel_path, error);
            }
        }
    }
    log.finish()
        .map_err(|error| format!("can't write the journal: {error}"))?;
    Ok(done)
}

/// Whether the journal has a finished fix to take back.
pub fn can_undo(journal: &Path) -> bool {
    matches!(Journal::read(journal), Ok(Some(rows)) if !rows.is_empty())
}

/// Puts back the dates of the last fix, skipping files changed since; the journal goes afterwards (REQ-038-09).
pub fn undo(db: &Mutex<Connection>, exiftool: &Path, journal: &Path) -> Result<DatesDone, String> {
    let rows = Journal::read(journal)
        .map_err(|error| format!("can't read the journal: {error}"))?
        .unwrap_or_default();
    let mut done = DatesDone {
        undone: true,
        ..DatesDone::default()
    };
    for row in rows {
        let file = row.library.join(&row.rel_path);
        match media::dates::size_and_mtime(&file) {
            Ok((size, mtime)) if size == row.size && mtime == row.mtime_after => {}
            _ => {
                done.skipped += 1;
                continue;
            }
        }
        match media::dates::restore(exiftool, &row.library, &row.rel_path, &row.previous, row.mtime_before) {
            Ok(()) => {
                let conn = lock(db);
                if let Ok(Some(id)) = db::media::id_of(&conn, row.root_id, &row.rel_path) {
                    let _ = db::media::mark_for_reading(&conn, id);
                }
                done.fixed += 1;
                done.touch(row.root_id, &row.rel_path);
            }
            Err(error) => done.fail(&row.rel_path, error.to_string()),
        }
    }
    if let Err(error) = std::fs::remove_file(journal) {
        tracing::error!("can't remove the date journal: {error}");
    }
    Ok(done)
}
