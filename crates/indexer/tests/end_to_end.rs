//! The indexer with the real exiftool on generated files (skipped when exiftool isn't installed, unless
//! `GG_REQUIRE_HELPERS=1`).

use db::libraries::{self, LibraryStatus};
use db::writer::IndexWriter;
use indexer::thumb_cache::{ThumbCache, thumb_path};
use indexer::thumbnails::{ThumbnailDeps, ThumbnailService};
use indexer::{Indexer, IndexerDeps, IndexerEvent, ScanResult, ScanScope};
use media::exiftool::{ExifTool, find_exiftool};
use media::thumbnail::Renderer;
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
        reconnect_grace: indexer::reachability::RECONNECT_GRACE,
        concurrency: 2,
        notify: Box::new(|| {}),
        on_written: Box::new(|| {}),
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

/// REQ-005-08, REQ-005-01: media items a scan writes get their thumbnails rendered in the background, and the
/// library's files stay as they were.
#[test]
fn req_005_08_renders_thumbnails_of_scanned_media() {
    let Some(exiftool) = exiftool() else { return };
    let exiftool = Arc::new(exiftool);
    let library = tempfile::tempdir().unwrap();
    image::RgbImage::from_pixel(1000, 500, image::Rgb([255, 0, 0]))
        .save(library.path().join("IMG_1.png"))
        .unwrap();
    let before = snapshot(library.path());
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("gallery.db");
    let conn = db::open(&path).unwrap();
    let id = libraries::insert(&conn, library.path().to_str().unwrap(), "L", LibraryStatus::Unknown)
        .unwrap()
        .id;
    let cache_dir = data.path().join("thumbs");
    let thumbnails = Arc::new(ThumbnailService::start(ThumbnailDeps {
        conn: db::open(&path).unwrap(),
        cache: ThumbCache::open(cache_dir.clone(), 1 << 30),
        render: Arc::new(Renderer::new(exiftool.clone(), media::ffmpeg::find_ffmpeg())),
        is_reachable: Arc::new(|_: &Path| true),
        workers: 2,
        on_ready: Box::new(|_| {}),
    }));
    let kick = thumbnails.clone();
    let (indexer, events) = Indexer::start(IndexerDeps {
        writer: IndexWriter::new(conn),
        metadata: exiftool,
        is_reachable: Arc::new(|_: &Path| true),
        reconnect_grace: indexer::reachability::RECONNECT_GRACE,
        concurrency: 2,
        notify: Box::new(|| {}),
        on_written: Box::new(move || kick.kick()),
    });

    indexer.request(id, ScanScope::Full);
    loop {
        if let IndexerEvent::Done(..) = events.recv_timeout(Duration::from_secs(60)).unwrap() {
            break;
        }
    }
    thumbnails.wait_idle();

    let reader = db::open_reader(&path).unwrap();
    let (status, thumbhash): (String, Option<String>) = reader
        .query_row("SELECT thumb_status, thumbhash FROM media", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(status, "ready");
    assert!(thumbhash.is_some_and(|hash| !hash.is_empty()));
    let source = db::thumbs::thumb_source(&reader, 1).unwrap().unwrap();
    for (width, height) in [(400, 200), (800, 400)] {
        let file = thumb_path(&cache_dir, &source.key, width);
        let image = image::open(&file).unwrap();
        assert_eq!((image.width(), image.height()), (width, height));
    }
    assert_eq!(snapshot(library.path()), before);
}
