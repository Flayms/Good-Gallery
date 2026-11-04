//! Opening a file with the app from the file manager (spec 027).

mod common;

use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::writer::{IndexWriter, IndexedFile};
use domain::media::{MediaKind, MediaMetadata};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use indexer::{IndexerEvent, ScanResult};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

type App = Harness<'static, app::AppState>;

fn file(rel_path: &str, taken_at: i64) -> IndexedFile {
    IndexedFile {
        rel_path: rel_path.into(),
        file_name: rel_path.rsplit('/').next().unwrap().into(),
        kind: MediaKind::Image,
        size: 1,
        mtime: 1_000,
        sidecar_mtime: None,
        metadata: MediaMetadata {
            width: Some(400),
            height: Some(300),
            taken_at: Some(taken_at),
            ..Default::default()
        },
        read_error: None,
    }
}

/// Creates the files on disk.
fn touch(folder: &Path, rel_paths: &[&str]) {
    for rel_path in rel_paths {
        let path = folder.join(rel_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"not really a photo").unwrap();
    }
}

struct Opened {
    harness: App,
    /// The library _Photos_ with `a.jpg`, `trip/b.jpg` and `c.jpg` indexed, and an index file another connection
    /// can write to, as the indexer does.
    photos: PathBuf,
    root: i64,
    index: PathBuf,
    events: Sender<IndexerEvent>,
}

fn opened(fixture: &Fixture) -> Opened {
    let photos = PathBuf::from(fixture.folder("Photos"));
    touch(&photos, &["a.jpg", "trip/b.jpg", "c.jpg"]);
    let index = fixture.dir.path().join("gallery.db");
    let conn = db::open(&index).unwrap();
    db::libraries::insert(&conn, "/elsewhere", "Elsewhere", LibraryStatus::Online).unwrap();
    let root = db::libraries::insert(&conn, photos.to_str().unwrap(), "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let mut writer = IndexWriter::new(conn);
    writer
        .write(
            root,
            &[file("a.jpg", 3_000), file("trip/b.jpg", 2_000), file("c.jpg", 1_000)],
        )
        .unwrap();
    let (events, rx) = std::sync::mpsc::channel();
    let state = fixture.state(writer.into()).with_indexer(fixture.scans.clone(), rx);
    let mut harness = common::harness(state);
    settle(&mut harness);
    Opened {
        harness,
        photos,
        root,
        index,
        events,
    }
}

fn shown_file(harness: &App) -> Option<i64> {
    harness.state().viewer.as_ref().map(app::viewer::Viewer::id)
}

fn id_of(index: &Path, root: i64, rel_path: &str) -> i64 {
    db::media::id_of(&db::open(index).unwrap(), root, rel_path)
        .unwrap()
        .unwrap()
}

#[test]
fn req_027_01_shows_a_file_from_a_library_in_the_viewer() {
    let fixture = Fixture::new();
    let Opened {
        mut harness,
        photos,
        root,
        index,
        ..
    } = opened(&fixture);
    // A search that leaves the file out is replaced by the whole library.
    let mut search = harness.state().gallery.as_ref().unwrap().search().clone();
    search.name = Some("c.jpg".into());
    harness.state_mut().gallery.as_mut().unwrap().set_search(search);
    settle(&mut harness);

    harness.state_mut().open_file(photos.join("trip").join("b.jpg"));
    settle(&mut harness);

    assert_eq!(shown_file(&harness), Some(id_of(&index, root, "trip/b.jpg")));
    let search = harness.state().gallery.as_ref().unwrap().search().clone();
    assert_eq!(search.library, Some(root));
    assert_eq!(search.name, None);
    assert!(
        fixture.scans.paths.lock().unwrap().is_empty(),
        "an indexed file needs no scan"
    );
    // Stepping moves through the library's order: c.jpg is next, by date taken.
    harness.key_press(eframe::egui::Key::ArrowRight);
    settle(&mut harness);
    assert_eq!(shown_file(&harness), Some(id_of(&index, root, "c.jpg")));
}

#[test]
fn req_027_01_indexes_a_new_file_first() {
    let fixture = Fixture::new();
    let Opened {
        mut harness,
        photos,
        root,
        index,
        events,
    } = opened(&fixture);
    touch(&photos, &["new.jpg"]);

    harness.state_mut().open_file(photos.join("new.jpg"));
    settle(&mut harness);
    assert_eq!(shown_file(&harness), None);
    assert_eq!(
        *fixture.scans.first.lock().unwrap(),
        [(root, vec!["new.jpg".to_owned()])],
        "the file is indexed ahead of other scans"
    );

    // The indexer writes it and finishes.
    let mut writer = IndexWriter::new(db::open(&index).unwrap());
    writer.write(root, &[file("new.jpg", 500)]).unwrap();
    events.send(IndexerEvent::Done(root, ScanResult::Completed)).unwrap();
    settle(&mut harness);
    assert_eq!(shown_file(&harness), Some(id_of(&index, root, "new.jpg")));
}

#[test]
fn req_027_01_says_it_opens_once_indexed() {
    let fixture = Fixture::new();
    let Opened {
        mut harness, photos, ..
    } = opened(&fixture);
    touch(&photos, &["new.jpg"]);

    harness.state_mut().open_file(photos.join("new.jpg"));
    settle(&mut harness);
    harness.get_by_label("new.jpg opens once it's indexed");
}

#[test]
fn req_027_01_says_so_when_the_scan_finds_nothing() {
    let fixture = Fixture::new();
    let Opened {
        mut harness,
        photos,
        root,
        events,
        ..
    } = opened(&fixture);
    touch(&photos, &[".hidden/new.jpg"]);

    harness.state_mut().open_file(photos.join(".hidden").join("new.jpg"));
    settle(&mut harness);
    events.send(IndexerEvent::Done(root, ScanResult::Completed)).unwrap();
    settle(&mut harness);
    assert_eq!(shown_file(&harness), None);
    harness.get_by_label("Couldn't open new.jpg: it wasn't indexed");
}

#[test]
fn req_027_04_says_why_a_file_cant_be_opened() {
    let fixture = Fixture::new();
    let Opened {
        mut harness, photos, ..
    } = opened(&fixture);
    touch(&photos, &["notes.txt"]);

    for (file, message) in [
        (photos.join("gone.jpg"), "Couldn't open gone.jpg: it doesn't exist"),
        (photos.join("trip"), "Couldn't open trip: it isn't a file"),
        (
            photos.join("notes.txt"),
            "Couldn't open notes.txt: it isn't a photo or video Good Gallery shows",
        ),
    ] {
        harness.state_mut().open_file(file);
        settle(&mut harness);
        harness.get_by_label(message);
        assert_eq!(shown_file(&harness), None);
    }
}
