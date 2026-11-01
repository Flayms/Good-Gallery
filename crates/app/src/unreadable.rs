//! Files whose metadata couldn't be read (REQ-002-12): the toolbar's notice and its list with _Read again_
//! (REQ-002-16, REQ-002-17).

use crate::icons::Icon;
use crate::{format, theme};
use db::libraries::Library;
use db::media::UnreadableFile;
use eframe::egui::{self, RichText, vec2};
use std::collections::HashMap;

/// At most this many files are listed; the rest are counted.
pub const LISTED: usize = 200;

/// An unreadable file as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub file: UnreadableFile,
    /// Its library and folder: "Photos › trip".
    pub location: String,
}

/// The unreadable files of all libraries, as last loaded from the index.
#[derive(Debug, Default)]
pub struct Unreadable {
    pub count: i64,
    pub rows: Vec<Row>,
    /// Files asked to be read again, by id, with their library: _Reading…_ until that library's scan is done.
    reading: HashMap<i64, i64>,
}

/// What the user asked for in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadAgain {
    /// These media items.
    Files(Vec<i64>),
    /// Every unreadable file, listed or not.
    All,
}

impl Unreadable {
    pub fn load(&mut self, count: i64, files: Vec<UnreadableFile>, libraries: &[Library]) {
        self.count = count;
        self.rows = files
            .into_iter()
            .map(|file| {
                let library = libraries
                    .iter()
                    .find(|library| library.id == file.root_id)
                    .map_or("Library", |library| library.label.as_str());
                let location = match file.rel_path.rsplit_once('/') {
                    Some((folder, _)) => format!("{library} › {}", folder.replace('/', " › ")),
                    None => library.to_string(),
                };
                Row { file, location }
            })
            .collect();
    }

    pub fn is_reading(&self, id: i64) -> bool {
        self.reading.contains_key(&id)
    }

    /// Marks files as being read again.
    pub fn start_reading(&mut self, files: impl IntoIterator<Item = (i64, i64)>) {
        self.reading.extend(files);
    }

    /// The scan of `library` is done: its files show _Read again_ again, if they are still unreadable.
    pub fn done(&mut self, library: i64) {
        self.reading.retain(|_, root| *root != library);
    }
}

/// "1 file couldn't be read", "2 files couldn't be read".
pub fn summary(count: i64) -> String {
    let files = if count == 1 { "file" } else { "files" };
    format!("{} {files} couldn't be read", format::count(count.max(0) as u64))
}

/// The toolbar's notice, nothing without unreadable files (REQ-002-16); what the user asked to read again.
pub fn notice(ui: &mut egui::Ui, unreadable: &Unreadable) -> Option<ReadAgain> {
    if unreadable.count == 0 {
        return None;
    }
    // Text on the glass bar is Paper; the icon carries the alert.
    let button = egui::Button::new((
        Icon::TriangleAlert.image(theme::ICON_SIZE, theme::alert()),
        format!("{} unreadable", format::count(unreadable.count as u64)),
    ))
    .frame(false)
    .min_size(vec2(0.0, theme::CONTROL_HEIGHT));
    let response = ui.add(button);
    let label = summary(unreadable.count);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    let mut asked = None;
    crate::toolbar::menu(&response, "unreadable files", 360.0, |ui| {
        asked = list(ui, unreadable);
    });
    asked
}

fn list(ui: &mut egui::Ui, unreadable: &Unreadable) -> Option<ReadAgain> {
    let mut asked = None;
    ui.spacing_mut().item_spacing.y = 8.0;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Unreadable files").family(theme::semibold()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let reading_all = unreadable.rows.iter().all(|row| unreadable.is_reading(row.file.id));
            let button = egui::Button::new((Icon::RefreshCw.small(), "Read all again"));
            if ui.add_enabled(!reading_all, button).clicked() {
                asked = Some(ReadAgain::All);
            }
        });
    });
    ui.label(
        RichText::new("Their metadata couldn't be read. Every scan of their library tries again.")
            .color(theme::muted()),
    );
    ui.separator();
    egui::ScrollArea::vertical()
        .max_height(320.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for row in &unreadable.rows {
                if let Some(id) = file_row(ui, row, unreadable.is_reading(row.file.id)) {
                    asked = Some(ReadAgain::Files(vec![id]));
                }
            }
            let more = unreadable.count - unreadable.rows.len() as i64;
            if more > 0 {
                ui.label(RichText::new(format!("and {} more", format::count(more as u64))).color(theme::muted()));
            }
        });
    asked
}

/// One file: its name, library and folder, and why it couldn't be read, with _Read again_.
fn file_row(ui: &mut egui::Ui, row: &Row, reading: bool) -> Option<i64> {
    let mut clicked = false;
    ui.horizontal(|ui| {
        let (text, label) = if reading {
            ("Reading…", format!("Reading {}…", row.file.file_name))
        } else {
            ("Read again", format!("Read {} again", row.file.file_name))
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
            let response = ui.add_enabled(!reading, egui::Button::new(text));
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !reading, &label));
            clicked = response.clicked();
            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(egui::Label::new(&row.file.file_name).truncate());
                ui.add(egui::Label::new(RichText::new(&row.location).color(theme::muted())).truncate());
                ui.add(
                    egui::Label::new(
                        RichText::new(&row.file.reason)
                            .size(theme::SIZE_CAPTION)
                            .color(theme::muted()),
                    )
                    .wrap(),
                );
            });
        });
    });
    clicked.then_some(row.file.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use db::libraries::LibraryStatus;

    /// REQ-002-16: a file shows its library and folders, a file in the library's own folder only the library.
    #[test]
    fn req_002_16_names_the_library_and_folder() {
        let library = Library {
            id: 1,
            path: "D:\\Photos".into(),
            label: "Photos".into(),
            status: LibraryStatus::Online,
            last_scan_at: None,
        };
        let file = |rel_path: &str| UnreadableFile {
            id: 1,
            root_id: 1,
            rel_path: rel_path.into(),
            file_name: rel_path.rsplit('/').next().unwrap().into(),
            reason: "broken".into(),
        };
        let mut unreadable = Unreadable::default();
        unreadable.load(2, vec![file("2024/trip/a.jpg"), file("b.jpg")], &[library]);
        let locations: Vec<&str> = unreadable.rows.iter().map(|row| row.location.as_str()).collect();
        assert_eq!(locations, ["Photos › 2024 › trip", "Photos"]);
        assert_eq!(summary(1), "1 file couldn't be read");
        assert_eq!(summary(2), "2 files couldn't be read");
    }
}
