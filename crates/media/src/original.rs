//! Originals for the viewer (REQ-009-04): the whole file decoded upright from the library, not the thumbnail cache.

use crate::ffmpeg;
use crate::thumbnail::{RenderError, decode_any, decode_error, fit_inside, resize};
use image::{DynamicImage, RgbaImage};
use std::path::Path;

/// The photo in `file` (absolute) as RGBA, upright, shrunk only if a side is longer than `max_side` (the GPU's
/// texture limit). Without `ffmpeg`, AVIF images fail.
pub fn load_original(file: &Path, ffmpeg: Option<&Path>, max_side: u32) -> Result<RgbaImage, RenderError> {
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
        decode_any(std::io::Cursor::new(data))?
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
    use crate::thumbnail::tests::{jpeg, solid, with_orientation};

    #[test]
    fn req_009_04_decodes_the_whole_photo_upright() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.jpg");
        std::fs::write(&file, with_orientation(&jpeg(&solid(3000, 1000, [255, 0, 0])), 6)).unwrap();

        let image = load_original(&file, None, 8192).unwrap();

        // Rotated by 90°, at full size: no shrinking as for thumbnails.
        assert_eq!(image.dimensions(), (1000, 3000));
        assert!(image.get_pixel(500, 1500)[0] > 200);
    }

    #[test]
    fn req_009_04_shrinks_only_what_exceeds_the_texture_limit() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("wide.png");
        solid(1000, 250, [0, 0, 255]).save(&file).unwrap();

        assert_eq!(load_original(&file, None, 400).unwrap().dimensions(), (400, 100));
        assert_eq!(load_original(&file, None, 8192).unwrap().dimensions(), (1000, 250));
    }

    #[test]
    fn req_009_15_fails_for_missing_and_broken_files_and_avif_without_ffmpeg() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            load_original(&dir.path().join("gone.jpg"), None, 8192),
            Err(RenderError::Io(_))
        ));
        let broken = dir.path().join("broken.jpg");
        std::fs::write(&broken, b"not an image").unwrap();
        assert!(load_original(&broken, None, 8192).is_err());
        let avif = dir.path().join("a.avif");
        std::fs::write(&avif, b"").unwrap();
        assert!(matches!(load_original(&avif, None, 8192), Err(RenderError::NoFfmpeg)));
    }
}
