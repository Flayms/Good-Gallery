//! The indexer with the real exiftool on generated files (skipped when exiftool isn't installed, unless
//! `GG_REQUIRE_HELPERS=1`).

use db::libraries::{self, LibraryStatus};
use db::writer::IndexWriter;
use indexer::{Indexer, IndexerDeps, IndexerEvent, ScanResult, ScanScope};
use media::exiftool::{ExifTool, find_exiftool};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const XMP: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:lr="http://ns.adobe.com/lightroom/1.0/"
  xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmlns:exif="http://ns.adobe.com/exif/1.0/">
 <xmp:Rating>3</xmp:Rating>
 <exif:DateTimeOriginal>2021-05-06T07:08:09+02:00</exif:DateTimeOriginal>
 <dc:subject><rdf:Bag><rdf:li>Holiday</rdf:li><rdf:li>Berlin</rdf:li></rdf:Bag></dc:subject>
 <lr:hierarchicalSubject><rdf:Bag><rdf:li>Places|Germany|Berlin</rdf:li></rdf:Bag></lr:hierarchicalSubject>
</rdf:Description></rdf:RDF></x:xmpmeta>"#;

fn exiftool() -> Option<ExifTool> {
    match find_exiftool() {
        Some(path) => Some(ExifTool::new(path, 2)),
        None if std::env::var_os("GG_REQUIRE_HELPERS").is_some() => panic!("exiftool not found"),
        None => {
            eprintln!("skipped: exiftool not found");
            None
        }
    }
}

/// Every file below `dir` with its size and modification time.
fn snapshot(dir: &Path) -> Vec<(String, u64, SystemTime)> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let meta = entry.metadata().unwrap();
        files.push((
            entry.file_name().to_string_lossy().into_owned(),
            meta.len(),
            meta.modified().unwrap(),
        ));
    }
    files.sort();
    files
}

/// REQ-004-01, REQ-004-07, REQ-004-10: a full scan reads the file and its sidecar into the index and changes no file.
#[test]
fn req_004_01_req_004_10_indexes_a_photo_with_its_sidecar_without_touching_files() {
    let Some(exiftool) = exiftool() else { return };
    let library = tempfile::tempdir().unwrap();
    image::RgbImage::new(4, 3)
        .save(library.path().join("IMG_1.png"))
        .unwrap();
    std::fs::write(library.path().join("IMG_1.png.xmp"), XMP).unwrap();
    let before = snapshot(library.path());
    let index = tempfile::tempdir().unwrap();
    let path = index.path().join("gallery.db");
    let conn = db::open(&path).unwrap();
    let id = libraries::insert(&conn, library.path().to_str().unwrap(), "L", LibraryStatus::Unknown)
        .unwrap()
        .id;
    let (indexer, events) = Indexer::start(IndexerDeps {
        writer: IndexWriter::new(conn),
        metadata: Arc::new(exiftool),
        is_reachable: Arc::new(|_: &Path| true),
        concurrency: 2,
        notify: Box::new(|| {}),
    });

    indexer.request(id, ScanScope::Full);
    let result = loop {
        if let IndexerEvent::Done(_, result) = events.recv_timeout(Duration::from_secs(60)).unwrap() {
            break result;
        }
    };

    assert_eq!(result, ScanResult::Completed);
    let reader = db::open_reader(&path).unwrap();
    let (width, height, rating, taken_at): (i64, i64, i64, i64) = reader
        .query_row("SELECT width, height, rating, taken_at FROM media", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap();
    assert_eq!((width, height, rating), (4, 3, 3));
    assert_eq!(taken_at, 1_620_277_689_000); // 2021-05-06T05:08:09Z
    let mut stmt = reader
        .prepare("SELECT tags.name FROM media_tags JOIN tags ON tags.id = media_tags.tag_id ORDER BY tags.name")
        .unwrap();
    let tags: Vec<String> = stmt.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
    assert_eq!(tags, ["Berlin", "Holiday"]);
    assert_eq!(snapshot(library.path()), before);
}
