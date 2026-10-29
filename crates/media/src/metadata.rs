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
}
