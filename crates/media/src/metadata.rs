//! Metadata of a media file through exiftool (spec 004); the rules that turn its tags into [`MediaMetadata`] live in
//! `meta`, which the web build shares.

use crate::exiftool::{ExifTool, ExifToolError};
use domain::media::MediaMetadata;
pub use meta::rules::{Tags, extract_metadata, extract_tags};
use std::path::Path;

/// Reads metadata of media files (exiftool, or a fake in tests).
pub trait MetadataSource: Send + Sync {
    fn read(&self, file: &Path, sidecar: Option<&Path>) -> Result<MediaMetadata, ExifToolError>;
}

impl MetadataSource for ExifTool {
    fn read(&self, file: &Path, sidecar: Option<&Path>) -> Result<MediaMetadata, ExifToolError> {
        // File and sidecar in parallel (on two pool processes).
        let (tags, sidecar_tags) = std::thread::scope(|scope| {
            let sidecar = sidecar.map(|sidecar| scope.spawn(move || ExifTool::read(self, sidecar)));
            let tags = ExifTool::read(self, file);
            let sidecar_tags = sidecar
                .map(|handle| handle.join().expect("exiftool read panicked"))
                .transpose();
            (tags, sidecar_tags)
        });
        Ok(extract_metadata(&tags?, sidecar_tags?.as_ref()))
    }
}

#[cfg(test)]
mod exiftool_tests {
    use super::*;

    const XMP: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:lr="http://ns.adobe.com/lightroom/1.0/"
      xmlns:xmp="http://ns.adobe.com/xap/1.0/">
      <xmp:Rating>4</xmp:Rating>
      <dc:subject><rdf:Bag><rdf:li>Holiday</rdf:li></rdf:Bag></dc:subject>
      <lr:hierarchicalSubject><rdf:Bag><rdf:li>People|Anna</rdf:li></rdf:Bag></lr:hierarchicalSubject>
    </rdf:Description></rdf:RDF></x:xmpmeta>"#;

    /// REQ-004-10: dimensions come from the file, tags and the rating from its sidecar.
    #[test]
    fn req_004_10_reads_dimensions_from_the_file_and_tags_from_its_sidecar() {
        let Some(exiftool) = crate::exiftool::tests::exiftool() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.png");
        let sidecar = dir.path().join("a.xmp");
        image::GrayImage::new(2, 1).save(&file).unwrap();
        std::fs::write(&sidecar, XMP).unwrap();

        let metadata = MetadataSource::read(&exiftool, &file, Some(&sidecar)).unwrap();

        assert_eq!(
            (metadata.width, metadata.height, metadata.rating),
            (Some(2), Some(1), Some(4))
        );
        assert_eq!(
            metadata.tags,
            [vec!["People".to_string(), "Anna".into()], vec!["Holiday".into()]]
        );
    }

    /// Tags written with exiftool, as `-TAG=VALUE` arguments: the parts of spec 004 the readers in `meta` cover.
    const PHOTO_TAGS: &[&str] = &[
        "-DateTimeOriginal=2024:07:14 18:30:00",
        "-OffsetTimeOriginal=+02:00",
        "-Orientation#=6",
        "-XMP-xmp:Rating=4",
        "-XMP-dc:Subject=Beach",
        "-XMP-dc:Subject=Sunset",
        "-XMP-lr:HierarchicalSubject=Places|Italy|Rome",
        "-IPTC:Keywords=Summer",
        "-XPKeywords=Windows;Tagged",
        "-XMP-iptcExt:PersonInImage=Anna",
        "-XMP-mwg-rs:RegionInfo={AppliedToDimensions={W=64,H=48,Unit=pixel},\
         RegionList=[{Area={X=0.5,Y=0.5,W=0.2,H=0.2,Unit=normalized},Name=Bob,Type=Face}]}",
    ];

    /// Runs exiftool's command line on `file`.
    fn write_tags(file: &Path, args: &[&str]) {
        let exiftool = crate::exiftool::find_exiftool().unwrap();
        let output = std::process::Command::new(exiftool)
            .args(["-q", "-overwrite_original"])
            .args(args)
            .arg(file)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }

    /// What `meta` reads from a whole file.
    fn meta_tags(file: &Path) -> Tags {
        let name = file.file_name().unwrap().to_str().unwrap();
        meta::read(name, &meta::Bytes::whole(std::fs::read(file).unwrap())).unwrap()
    }

    /// REQ-019-04: `meta`'s readers, which the browser version uses, give the same metadata as exiftool.
    #[test]
    fn req_019_04_meta_reads_the_same_metadata_as_exiftool() {
        let Some(exiftool) = crate::exiftool::tests::exiftool() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let jpeg = dir.path().join("photo.jpg");
        image::RgbImage::new(64, 48).save(&jpeg).unwrap();
        write_tags(&jpeg, PHOTO_TAGS);
        let png = dir.path().join("plain.png");
        image::RgbImage::new(30, 20).save(&png).unwrap();
        write_tags(
            &png,
            &[
                "-XMP-exif:DateTimeOriginal=2023:01:02 03:04:05",
                "-XMP-dc:Subject=Garden",
            ],
        );
        let sidecar = dir.path().join("plain.xmp");
        std::fs::write(&sidecar, XMP).unwrap();
        let movie = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/tests/fixtures/Holiday/walk.mp4");

        for (file, sidecar) in [(&jpeg, None), (&png, Some(&sidecar)), (&movie, None)] {
            let by_exiftool = MetadataSource::read(&exiftool, file, sidecar.map(|s| s.as_path())).unwrap();
            let sidecar_tags = sidecar.map(|s| meta::read_sidecar(&std::fs::read(s).unwrap()));
            let by_meta = extract_metadata(&meta_tags(file), sidecar_tags.as_ref());
            assert_eq!(by_meta, by_exiftool, "{}", file.display());
        }
        // The comparison covers what the tags say, not only empty values.
        let photo = MetadataSource::read(&exiftool, &jpeg, None).unwrap();
        assert_eq!((photo.width, photo.height, photo.rating), (Some(48), Some(64), Some(4)));
        assert!(photo.tags.len() >= 6, "{:?}", photo.tags);
    }
}
