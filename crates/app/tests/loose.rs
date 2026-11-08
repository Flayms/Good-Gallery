//! Files outside every library, shown without indexing them (spec 033).

mod common;

use common::{Fixture, settle};
use db::libraries::LibraryStatus;
use db::writer::{IndexWriter, IndexedFile};
use domain::media::{MediaKind, MediaMetadata};
use eframe::egui::Key;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use indexer::{IndexerEvent, ScanResult};
use media::exiftool::ExifToolError;
use media::metadata::MetadataSource;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Sender;

type App = Harness<'static, app::AppState>;

/// Metadata as exiftool would read it from the test photos.
struct PhotoMetadata;

impl MetadataSource for PhotoMetadata {
    /// Crashes on `crash.png`, as a decoder bug would: the scan then skips the file.
    fn read(&self, file: &Path, _: Option<&Path>) -> Result<MediaMetadata, ExifToolError> {
        assert!(!file.ends_with("crash.png"), "the reader crashed");
        Ok(MediaMetadata {
            width: Some(64),
            height: Some(48),
            ..Default::default()
        })
    }
}

/// A photo, modified on 2020-01-01 so snapshots show a fixed date.
fn photo(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_pixel(64, 48, image::Rgb([200, 120, 40]))
        .save(path)
        .unwrap();
    let modified = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_577_836_800);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
}

/// Hides the file's location, a temporary folder, from snapshots.
fn mask_location(harness: &mut App) {
    let location = harness
        .get_by_label_contains(&format!("Away{}", std::path::MAIN_SEPARATOR))
        .rect();
    let right = harness.ctx.content_rect().right();
    harness.mask(eframe::egui::Rect::from_min_max(
        location.min,
        eframe::egui::pos2(right, location.max.y),
    ));
}

fn indexed(rel_path: &str) -> IndexedFile {
    IndexedFile {
        rel_path: rel_path.into(),
        file_name: rel_path.rsplit('/').next().unwrap().into(),
        kind: MediaKind::Image,
        size: 1,
        mtime: 1_000,
        sidecar_mtime: None,
        metadata: MediaMetadata {
            width: Some(64),
            height: Some(48),
            taken_at: Some(1_000),
            ..Default::default()
        },
        read_error: None,
    }
}

struct Opened {
    harness: App,
    /// The library _Photos_ with `a.png` indexed.
    photos: PathBuf,
    root: i64,
    /// The index file, which another connection can write to as the indexer does.
    index: PathBuf,
    /// A folder in no library.
    away: PathBuf,
    scratch: PathBuf,
    events: Sender<IndexerEvent>,
}

fn opened(fixture: &Fixture) -> Opened {
    let photos = PathBuf::from(fixture.folder("Photos"));
    photo(&photos.join("a.png"));
    let away = PathBuf::from(fixture.folder("Away"));
    let index = fixture.dir.path().join("gallery.db");
    let scratch = fixture.dir.path().join("scratch");
    let conn = db::open(&index).unwrap();
    let root = db::libraries::insert(&conn, photos.to_str().unwrap(), "Photos", LibraryStatus::Online)
        .unwrap()
        .id;
    let mut writer = IndexWriter::new(conn);
    writer.write(root, &[indexed("a.png")]).unwrap();
    let (events, rx) = std::sync::mpsc::channel();
    let state = fixture
        .state(writer.into())
        .with_indexer(fixture.scans.clone(), rx)
        .with_loose_files(scratch.clone(), Arc::new(PhotoMetadata));
    let mut harness = common::harness(state);
    settle(&mut harness);
    Opened {
        harness,
        photos,
        root,
        index,
        away,
        scratch,
        events,
    }
}

/// The file the viewer shows, by its name.
fn shown(harness: &App) -> Option<String> {
    use egui_kittest::kittest::NodeT as _;
    harness.state().viewer.as_ref()?;
    harness
        .query_all_by_role(Role::Dialog)
        .find_map(|node| node.accesskit_node().label().map(|label| label.to_string()))
}

fn media_count(index: &Path) -> i64 {
    db::open(index)
        .unwrap()
        .query_row("SELECT count(*) FROM media", [], |row| row.get(0))
        .unwrap()
}

fn scratch_files(scratch: &Path) -> usize {
    std::fs::read_dir(scratch).map_or(0, |entries| entries.count())
}

/// REQ-033-01: a file in no library shows at once, and nothing is added to the index or the libraries.
#[test]
fn req_033_01_shows_a_file_outside_every_library() {
    let fixture = Fixture::new();
    let Opened {
        mut harness,
        index,
        away,
        ..
    } = opened(&fixture);
    photo(&away.join("a.png"));
    photo(&away.join("b.png"));
    let search = harness.state().gallery.as_ref().unwrap().search().clone();

    harness.state_mut().open_file(away.join("b.png"));
    settle(&mut harness);

    assert_eq!(shown(&harness).as_deref(), Some("b.png"));
    assert!(!harness.state().viewer.as_ref().unwrap().is_indexed());
    assert_eq!(media_count(&index), 1, "nothing is indexed");
    assert_eq!(harness.state().libraries.len(), 1, "no library is added");
    assert_eq!(harness.state().gallery.as_ref().unwrap().search(), &search);
    assert!(
        harness.query_by_label("b.png isn't in a library").is_none(),
        "no question first"
    );
}

/// REQ-033-02: ← and → step through the file's folder by name, without its subfolders and files the app doesn't show.
#[test]
fn req_033_02_steps_through_its_folder_by_name() {
    let fixture = Fixture::new();
    let Opened { mut harness, away, .. } = opened(&fixture);
    for name in ["c.png", "a.png", "B.png", "sub/aa.png"] {
        photo(&away.join(name));
    }
    std::fs::write(away.join("notes.txt"), "not a photo").unwrap();

    harness.state_mut().open_file(away.join("B.png"));
    settle(&mut harness);
    assert_eq!(shown(&harness).as_deref(), Some("B.png"));
    let mut seen = Vec::new();
    for key in [
        Key::ArrowRight,
        Key::ArrowRight,
        Key::ArrowLeft,
        Key::ArrowLeft,
        Key::ArrowLeft,
    ] {
        harness.key_press(key);
        settle(&mut harness);
        seen.push(shown(&harness).unwrap());
    }
    assert_eq!(seen, ["c.png", "c.png", "B.png", "a.png", "a.png"]);
}

/// REQ-033-03: the info panel says the file isn't in a library and offers to add its folder; rating, tagging,
/// editing, faces and the filmstrip aren't offered.
#[test]
fn req_033_03_offers_what_needs_no_index() {
    let fixture = Fixture::new();
    let Opened { mut harness, away, .. } = opened(&fixture);
    photo(&away.join("b.png"));
    harness.state_mut().open_file(away.join("b.png"));
    settle(&mut harness);

    for key in [Key::I, Key::E, Key::F, Key::P] {
        harness.key_press(key);
        settle(&mut harness);
    }
    let viewer = harness.state().viewer.as_ref().unwrap();
    assert!(viewer.info_open());
    assert!(viewer.editing().is_none(), "no editing");
    assert!(!viewer.filmstrip_shown(), "no filmstrip");
    assert!(!viewer.faces_shown(), "no faces");
    harness.get_by_label("Not in a library");
    harness.get_by_label("Add folder as library");
    for absent in ["Edit", "Rate 1 star", "Add"] {
        assert!(harness.query_by_label(absent).is_none(), "{absent} is offered");
    }
    // The photo itself, which loads on a thread of its own: a slow machine took the snapshot before it.
    if let Some(originals) = harness.state_mut().originals.as_mut() {
        originals.wait();
    }
    harness.run_steps(3);
    mask_location(&mut harness);
    harness.snapshot(common::snapshot_name("loose_info"));
}

/// REQ-033-04: _Add folder as library_ adds it and keeps the file shown; once the first scan has it, the viewer shows
/// the indexed item.
#[test]
fn req_033_04_adds_its_folder_and_switches_to_the_indexed_item() {
    let fixture = Fixture::new();
    let Opened {
        mut harness,
        index,
        away,
        events,
        ..
    } = opened(&fixture);
    photo(&away.join("b.png"));
    harness.state_mut().open_file(away.join("b.png"));
    settle(&mut harness);
    harness.key_press(Key::I);
    settle(&mut harness);

    harness.get_by_label("Add folder as library").click();
    settle(&mut harness);
    settle(&mut harness);
    let library = harness
        .state()
        .libraries
        .iter()
        .find(|library| library.path == away.to_str().unwrap())
        .expect("the folder is a library")
        .id;
    assert_eq!(shown(&harness).as_deref(), Some("b.png"), "the file stays shown");

    // The library's first scan.
    let mut writer = IndexWriter::new(db::open(&index).unwrap());
    writer.write(library, &[indexed("b.png")]).unwrap();
    events.send(IndexerEvent::Done(library, ScanResult::Completed)).unwrap();
    settle(&mut harness);
    assert_eq!(shown(&harness).as_deref(), Some("b.png"));
    assert!(harness.state().viewer.as_ref().unwrap().is_indexed());
    assert!(harness.state().loose.is_none());
}

/// REQ-033-05: closing returns to the gallery as it was and leaves nothing of the folder behind; leftovers of an
/// earlier run go at startup.
#[test]
fn req_033_05_closing_leaves_nothing_behind() {
    let fixture = Fixture::new();
    let Opened {
        mut harness,
        away,
        scratch,
        ..
    } = opened(&fixture);
    photo(&away.join("b.png"));
    let search = harness.state().gallery.as_ref().unwrap().search().clone();
    harness.state_mut().open_file(away.join("b.png"));
    settle(&mut harness);
    assert!(scratch_files(&scratch) > 0, "the scratch index is in the data folder");

    harness.key_press(Key::Escape);
    settle(&mut harness);
    harness.run_steps(30);
    settle(&mut harness);
    assert!(harness.state().viewer.is_none());
    assert!(harness.state().loose.is_none());
    assert_eq!(harness.state().gallery.as_ref().unwrap().search(), &search);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while scratch_files(&scratch) > 0 {
        assert!(std::time::Instant::now() < deadline, "the scratch index stays");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    std::fs::write(scratch.join("left-over.db"), "x").unwrap();
    app::loose::clear_scratch(&scratch);
    assert_eq!(scratch_files(&scratch), 0);
}

/// REQ-033-06: a file in a folder its library skips is shown as a file outside every library.
#[test]
fn req_033_06_shows_a_file_its_library_skips() {
    let fixture = Fixture::new();
    let Opened {
        mut harness,
        photos,
        root,
        events,
        ..
    } = opened(&fixture);
    photo(&photos.join(".hidden").join("new.png"));

    harness.state_mut().open_file(photos.join(".hidden").join("new.png"));
    settle(&mut harness);
    // The changes scan finds nothing to index there.
    events.send(IndexerEvent::Done(root, ScanResult::Completed)).unwrap();
    settle(&mut harness);
    assert_eq!(shown(&harness).as_deref(), Some("new.png"));
    assert!(!harness.state().viewer.as_ref().unwrap().is_indexed());
    assert!(
        harness
            .query_by_label("Couldn't open new.png: it wasn't indexed")
            .is_none()
    );
}

/// REQ-027-04: a file outside every library that can't be read says so, and the app stops waiting for it.
#[test]
fn req_027_04_says_so_when_a_file_outside_libraries_cant_be_read() {
    let fixture = Fixture::new();
    let Opened { mut harness, away, .. } = opened(&fixture);
    photo(&away.join("crash.png"));

    harness.state_mut().open_file(away.join("crash.png"));
    settle(&mut harness);
    settle(&mut harness);
    assert!(harness.state().viewer.is_none());
    assert!(harness.state().loose.is_none());
    assert!(!harness.state().is_active(), "it stops waiting");
    harness.get_by_label("Couldn't open crash.png: it couldn't be read");
}
