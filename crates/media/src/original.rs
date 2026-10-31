//! Originals for the viewer (REQ-009-04): the whole file decoded upright from the library, not the thumbnail cache.

use crate::ffmpeg;
use crate::thumbnail::{RenderError, decode_any, decode_error, fit_inside, resize};
use image::{DynamicImage, RgbaImage};
use std::path::Path;

/// The photo in `file` (absolute) as RGBA, upright (turned by `sidecar_orientation` if it has no orientation of its
/// own, REQ-004-12), shrunk only if a side is longer than `max_side` (the GPU's texture limit). Without `ffmpeg`, AVIF
/// images fail.
pub fn load_original(
    file: &Path,
    ffmpeg: Option<&Path>,
    max_side: u32,
    sidecar_orientation: Option<u8>,
) -> Result<RgbaImage, RenderError> {
    let extension = file
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let image = if extension == "avif" {
        // The `image` crate decodes AVIF only through dav1d (a C library); ffmpeg has it.
        let ffmpeg = ffmpeg.ok_or(RenderError::NoFfmpeg)?;
        let png = ffmpeg::extract_full_frame(ffmpeg, file)?;
        image::load_from_memory_with_format(&png, image::ImageFormat::Png).map_err(decode_error)?
    } else {
        // Read at once: over a network share one large read beats many small ones.
        let data = std::fs::read(file)?;
        decode_any(std::io::Cursor::new(data), sidecar_orientation)?
    };
    fit_to(image, max_side)
}

fn fit_to(image: DynamicImage, max_side: u32) -> Result<RgbaImage, RenderError> {
    if image.width() == 0 || image.height() == 0 {
        return Err(decode_error("empty image"));
    }
    let image = DynamicImage::ImageRgba8(image.into_rgba8());
    let (width, height) = fit_inside(image.width(), image.height(), max_side, max_side, false);
    Ok(resize(&image, width, height)?.into_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thumbnail::tests::{jpeg, solid, with_orientation, with_xmp_orientation};

    #[test]
    fn req_009_04_decodes_the_whole_photo_upright() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.jpg");
        std::fs::write(&file, with_orientation(&jpeg(&solid(3000, 1000, [255, 0, 0])), 6)).unwrap();

        let image = load_original(&file, None, 8192, None).unwrap();

        // Rotated by 90°, at full size: no shrinking as for thumbnails.
        assert_eq!(image.dimensions(), (1000, 3000));
        assert!(image.get_pixel(500, 1500)[0] > 200);
    }

    /// REQ-004-12, REQ-009-04: an orientation only in the embedded XMP or the sidecar turns the photo too.
    #[test]
    fn req_004_12_req_009_04_turns_photos_oriented_only_in_xmp_upright() {
        let dir = tempfile::tempdir().unwrap();
        let plain = jpeg(&solid(3000, 1000, [255, 0, 0]));
        let embedded = dir.path().join("embedded.jpg");
        std::fs::write(&embedded, with_xmp_orientation(&plain, 6)).unwrap();
        assert_eq!(
            load_original(&embedded, None, 8192, None).unwrap().dimensions(),
            (1000, 3000)
        );

        let bare = dir.path().join("bare.jpg");
        std::fs::write(&bare, &plain).unwrap();
        assert_eq!(
            load_original(&bare, None, 8192, Some(8)).unwrap().dimensions(),
            (1000, 3000)
        );
        assert_eq!(
            load_original(&bare, None, 8192, None).unwrap().dimensions(),
            (3000, 1000)
        );
    }

    #[test]
    fn req_009_04_shrinks_only_what_exceeds_the_texture_limit() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("wide.png");
        solid(1000, 250, [0, 0, 255]).save(&file).unwrap();

        assert_eq!(load_original(&file, None, 400, None).unwrap().dimensions(), (400, 100));
        assert_eq!(
            load_original(&file, None, 8192, None).unwrap().dimensions(),
            (1000, 250)
        );
    }

    #[test]
    fn req_009_15_fails_for_missing_and_broken_files_and_avif_without_ffmpeg() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            load_original(&dir.path().join("gone.jpg"), None, 8192, None),
            Err(RenderError::Io(_))
        ));
        let broken = dir.path().join("broken.jpg");
        std::fs::write(&broken, b"not an image").unwrap();
        assert!(load_original(&broken, None, 8192, None).is_err());
        let avif = dir.path().join("a.avif");
        std::fs::write(&avif, b"").unwrap();
        assert!(matches!(
            load_original(&avif, None, 8192, None),
            Err(RenderError::NoFfmpeg)
        ));
    }
}
