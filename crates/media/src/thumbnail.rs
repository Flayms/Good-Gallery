//! Thumbnail rendering: one decode per file (shrunk while decoding where the format allows), upright, then every
//! thumbnail width as WebP and a ThumbHash placeholder from it.

use crate::ffmpeg::{self, FfmpegError};
use base64::Engine as _;
use domain::media::MediaKind;
pub(crate) use domain::thumbs::fit_inside;
use domain::thumbs::{MAX_THUMB_HEIGHT, MAX_THUMB_WIDTH, THUMB_WIDTHS, THUMBHASH_SIZE, VIDEO_FRAME_POSITION};
use image::metadata::Orientation;
use image::{DynamicImage, ImageReader};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const WEBP_QUALITY: f32 = 80.0;
const PREVIEW_ASPECT_TOLERANCE: f64 = 0.02;
/// What a decode may allocate: 268 402 689 pixels (16 383²) as RGBA. The `image` crate's own default (512 MiB) would
/// fail on large panoramas.
const MAX_DECODE_BYTES: u64 = 268_402_689 * 4;

pub(crate) fn limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    limits
}

/// Every thumbnail width of a media item and its placeholder.
#[derive(Debug, Clone)]
pub struct Thumbnails {
    /// Base64-encoded ThumbHash (with padding, as the index stores it).
    pub thumbhash: String,
    /// `(width, WebP file)` for each of [`THUMB_WIDTHS`].
    pub images: Vec<(u32, Vec<u8>)>,
}

/// Embedded JPEG previews (exiftool's `PreviewImage`), or a fake in tests.
pub trait PreviewSource: Send + Sync {
    fn extract_preview(&self, file: &Path) -> Option<Vec<u8>>;
}

pub struct NoPreviews;

impl PreviewSource for NoPreviews {
    fn extract_preview(&self, _: &Path) -> Option<Vec<u8>> {
        None
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("can't read the file: {0}")]
    Io(#[from] std::io::Error),
    #[error("can't decode the image: {0}")]
    Decode(String),
    #[error(transparent)]
    Ffmpeg(#[from] FfmpegError),
    #[error("ffmpeg isn't available")]
    NoFfmpeg,
    #[error("can't encode WebP: {0}")]
    Encode(String),
}

pub(crate) fn decode_error(error: impl std::fmt::Display) -> RenderError {
    RenderError::Decode(error.to_string())
}

/// Renders thumbnails of media files.
pub struct Renderer {
    previews: Arc<dyn PreviewSource>,
    ffmpeg: Option<PathBuf>,
}

impl Renderer {
    /// Without `ffmpeg`, videos and AVIF images fail to render.
    pub fn new(previews: Arc<dyn PreviewSource>, ffmpeg: Option<PathBuf>) -> Self {
        Self { previews, ffmpeg }
    }

    /// All thumbnails of `file` (absolute). `duration` in seconds, for videos; `sidecar_orientation` turns a photo
    /// without an orientation of its own (REQ-004-12).
    pub fn render(
        &self,
        file: &Path,
        kind: MediaKind,
        duration: Option<f64>,
        sidecar_orientation: Option<u8>,
    ) -> Result<Thumbnails, RenderError> {
        let image = match kind {
            MediaKind::Video => {
                let position = duration
                    .filter(|d| d.is_finite() && *d > 0.0)
                    .map_or(0.0, |d| d * VIDEO_FRAME_POSITION);
                self.frame(file, position)?
            }
            MediaKind::Image => self.load_image(file, sidecar_orientation)?,
        };
        render_thumbnails(image)
    }

    fn frame(&self, file: &Path, position: f64) -> Result<DynamicImage, RenderError> {
        let ffmpeg = self.ffmpeg.as_deref().ok_or(RenderError::NoFfmpeg)?;
        let png = ffmpeg::extract_frame(ffmpeg, file, position)?;
        image::load_from_memory_with_format(&png, image::ImageFormat::Png).map_err(decode_error)
    }

    fn load_image(&self, file: &Path, sidecar_orientation: Option<u8>) -> Result<DynamicImage, RenderError> {
        let extension = file
            .extension()
            .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        match extension.as_str() {
            "jpg" | "jpeg" => {
                if let Some(preview) = self.load_preview(file, sidecar_orientation) {
                    return Ok(preview);
                }
                let data = std::fs::read(file)?;
                // A file named .jpg may hold another format.
                decode_jpeg(&data, None, sidecar_orientation)
                    .or_else(|error| decode_any(Cursor::new(&data), sidecar_orientation).map_err(|_| error))
            }
            // The `image` crate decodes AVIF only through dav1d (a C library); ffmpeg has it.
            "avif" => self.frame(file, 0.0),
            _ => decode_any(std::io::BufReader::new(std::fs::File::open(file)?), sidecar_orientation),
        }
    }

    /// Camera JPEGs often embed a large preview; decoding it avoids reading the whole file over the network
    /// (REQ-005-04). Used only if it is as wide as the largest thumbnail (or the original, if that is smaller) and has
    /// the original's shape.
    fn load_preview(&self, file: &Path, sidecar_orientation: Option<u8>) -> Option<DynamicImage> {
        let buffer = self.previews.extract_preview(file)?;
        let original = jpeg_header(
            &mut std::io::BufReader::new(std::fs::File::open(file).ok()?),
            sidecar_orientation,
        )
        .ok()?;
        // Some cameras (e.g. DJI) point PreviewImage at data that isn't an image.
        let preview = jpeg_header(&mut Cursor::new(&buffer), None).ok()?;
        let upright = |header: &JpegHeader| {
            if swaps_sides(original.orientation) {
                header.height
            } else {
                header.width
            }
        };
        let aspect = (f64::from(preview.width) / f64::from(preview.height))
            / (f64::from(original.width) / f64::from(original.height));
        let large_enough = upright(&preview) >= MAX_THUMB_WIDTH.min(upright(&original));
        if !large_enough || (aspect - 1.0).abs() > PREVIEW_ASPECT_TOLERANCE {
            return None;
        }
        decode_jpeg(&buffer, Some(original.orientation), None).ok()
    }
}

/// Applies an EXIF orientation (1–8); other values leave the image as it is.
pub fn orient(mut image: DynamicImage, orientation: u8) -> DynamicImage {
    if let Some(orientation) = Orientation::from_exif(orientation) {
        image.apply_orientation(orientation);
    }
    image
}

fn swaps_sides(orientation: Orientation) -> bool {
    matches!(
        orientation,
        Orientation::Rotate90 | Orientation::Rotate270 | Orientation::Rotate90FlipH | Orientation::Rotate270FlipH
    )
}

struct JpegHeader {
    width: u32,
    height: u32,
    orientation: Orientation,
}

/// Size and orientation (REQ-004-12) from the JPEG's header, without decoding it.
fn jpeg_header(reader: &mut impl std::io::Read, sidecar_orientation: Option<u8>) -> Result<JpegHeader, RenderError> {
    let mut decoder = jpeg_decoder::Decoder::new(reader);
    decoder.read_info().map_err(decode_error)?;
    let info = decoder.info().ok_or_else(|| decode_error("no frame"))?;
    if info.width == 0 || info.height == 0 {
        return Err(decode_error("empty image"));
    }
    Ok(JpegHeader {
        width: u32::from(info.width),
        height: u32::from(info.height),
        orientation: shown_orientation(
            decoder.exif_data().and_then(Orientation::from_exif_chunk),
            decoder.xmp_data(),
            sidecar_orientation,
        ),
    })
}

/// The orientation a photo is shown with (REQ-004-12): its EXIF one (`exif`), else the one in its embedded XMP
/// packet, else its sidecar's.
pub(crate) fn shown_orientation(
    exif: Option<Orientation>,
    xmp: Option<&[u8]>,
    sidecar_orientation: Option<u8>,
) -> Orientation {
    exif.or_else(|| xmp.and_then(xmp_orientation))
        .or_else(|| sidecar_orientation.and_then(Orientation::from_exif))
        .unwrap_or(Orientation::NoTransforms)
}

fn xmp_orientation(packet: &[u8]) -> Option<Orientation> {
    let value = meta::read_sidecar(packet).get("Orientation")?.as_f64()?;
    (1.0..=8.0)
        .contains(&value)
        .then(|| Orientation::from_exif(value as u8))?
}

/// The orientation `decoder` reads from the file (REQ-004-12): EXIF, a TIFF's own tag, else embedded XMP, else the
/// sidecar's.
pub(crate) fn decoder_orientation(
    decoder: &mut impl image::ImageDecoder,
    sidecar_orientation: Option<u8>,
) -> Orientation {
    let exif = decoder
        .exif_metadata()
        .ok()
        .flatten()
        .and_then(|chunk| Orientation::from_exif_chunk(&chunk))
        .or_else(|| {
            decoder
                .orientation()
                .ok()
                .filter(|orientation| *orientation != Orientation::NoTransforms)
        });
    let xmp = decoder.xmp_metadata().ok().flatten();
    shown_orientation(exif, xmp.as_deref(), sidecar_orientation)
}

/// Decodes a JPEG shrunk by the DCT scale (½, ¼, ⅛) that keeps it at least as large as the thumbnail box, and turns
/// it upright: with `orientation`, or the file's own (REQ-004-12). A truncated file decodes partially (the missing
/// part stays decoder fill).
fn decode_jpeg(
    data: &[u8],
    orientation: Option<Orientation>,
    sidecar_orientation: Option<u8>,
) -> Result<DynamicImage, RenderError> {
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(data));
    decoder.read_info().map_err(decode_error)?;
    let info = decoder.info().ok_or_else(|| decode_error("no frame"))?;
    let orientation = orientation.unwrap_or_else(|| {
        shown_orientation(
            decoder.exif_data().and_then(Orientation::from_exif_chunk),
            decoder.xmp_data(),
            sidecar_orientation,
        )
    });
    let mut image = if info.pixel_format == jpeg_decoder::PixelFormat::CMYK32 {
        // The `image` crate converts CMYK (including Adobe's inverted variant) to RGB.
        let mut reader = ImageReader::with_format(Cursor::new(data), image::ImageFormat::Jpeg);
        reader.limits(limits());
        reader.decode().map_err(decode_error)?
    } else {
        let (width, height) = (u32::from(info.width), u32::from(info.height));
        let (w, h) = if swaps_sides(orientation) {
            let (h, w) = fit_inside(height, width, MAX_THUMB_WIDTH, MAX_THUMB_HEIGHT, false);
            (w, h)
        } else {
            fit_inside(width, height, MAX_THUMB_WIDTH, MAX_THUMB_HEIGHT, false)
        };
        decoder
            .scale(w.min(u32::from(u16::MAX)) as u16, h.min(u32::from(u16::MAX)) as u16)
            .map_err(decode_error)?;
        let pixels = decoder.decode().map_err(decode_error)?;
        let info = decoder.info().ok_or_else(|| decode_error("no frame"))?;
        let (width, height) = (u32::from(info.width), u32::from(info.height));
        let image = match info.pixel_format {
            jpeg_decoder::PixelFormat::L8 => image::GrayImage::from_raw(width, height, pixels).map(DynamicImage::from),
            // Big-endian 16-bit samples: the high byte is enough for a thumbnail.
            jpeg_decoder::PixelFormat::L16 => {
                image::GrayImage::from_raw(width, height, pixels.chunks_exact(2).map(|s| s[0]).collect())
                    .map(DynamicImage::from)
            }
            jpeg_decoder::PixelFormat::RGB24 => {
                image::RgbImage::from_raw(width, height, pixels).map(DynamicImage::from)
            }
            jpeg_decoder::PixelFormat::CMYK32 => None,
        };
        image.ok_or_else(|| decode_error("decoder returned too few pixels"))?
    };
    image.apply_orientation(orientation);
    Ok(image)
}

/// Any other format the `image` crate knows (PNG, GIF's first frame, WebP), by content, upright (REQ-004-12).
pub(crate) fn decode_any(
    reader: impl std::io::BufRead + std::io::Seek,
    sidecar_orientation: Option<u8>,
) -> Result<DynamicImage, RenderError> {
    let mut reader = ImageReader::new(reader).with_guessed_format()?;
    reader.limits(limits());
    let mut decoder = reader.into_decoder().map_err(decode_error)?;
    let orientation = decoder_orientation(&mut decoder, sidecar_orientation);
    let mut image = DynamicImage::from_decoder(decoder).map_err(decode_error)?;
    image.apply_orientation(orientation);
    Ok(image)
}

/// The size of `width` × `height` scaled to fit into `max_width` × `max_height`, keeping its aspect ratio.
pub(crate) fn resize(image: &DynamicImage, width: u32, height: u32) -> Result<DynamicImage, RenderError> {
    if (image.width(), image.height()) == (width, height) {
        return Ok(image.clone());
    }
    let mut resized = DynamicImage::new(width, height, image.color());
    // Lanczos3; alpha is premultiplied while resampling.
    fast_image_resize::Resizer::new()
        .resize(image, &mut resized, &fast_image_resize::ResizeOptions::new())
        .map_err(decode_error)?;
    Ok(resized)
}

fn encode_webp(image: &DynamicImage) -> Result<Vec<u8>, RenderError> {
    let (width, height) = (image.width(), image.height());
    let encoder = match image {
        DynamicImage::ImageRgba8(rgba) => webp::Encoder::from_rgba(rgba.as_raw(), width, height),
        DynamicImage::ImageRgb8(rgb) => webp::Encoder::from_rgb(rgb.as_raw(), width, height),
        _ => return Err(RenderError::Encode("unexpected pixel format".into())),
    };
    let encoded = encoder
        .encode_simple(false, WEBP_QUALITY)
        .map_err(|error| RenderError::Encode(format!("{error:?}")))?;
    Ok(encoded.to_vec())
}

/// Every thumbnail width and the ThumbHash from one decoded image (REQ-005-01): never enlarged, within
/// 800 × 2400 px.
pub fn render_thumbnails(image: DynamicImage) -> Result<Thumbnails, RenderError> {
    if image.width() == 0 || image.height() == 0 {
        return Err(decode_error("empty image"));
    }
    // sRGB, 8 bits per channel; alpha only where the image has it.
    let image = if image.color().has_alpha() {
        DynamicImage::ImageRgba8(image.into_rgba8())
    } else {
        DynamicImage::ImageRgb8(image.into_rgb8())
    };
    let (width, height) = fit_inside(image.width(), image.height(), MAX_THUMB_WIDTH, MAX_THUMB_HEIGHT, false);
    let decoded = resize(&image, width, height)?;
    drop(image);

    let (hash_width, hash_height) = fit_inside(width, height, THUMBHASH_SIZE, THUMBHASH_SIZE, true);
    let small = resize(&decoded, hash_width, hash_height)?.into_rgba8();
    let hash = thumbhash::rgba_to_thumb_hash(hash_width as usize, hash_height as usize, small.as_raw());

    let images = THUMB_WIDTHS
        .into_iter()
        .map(|thumb_width| {
            let image = if width > thumb_width {
                let thumb_height =
                    ((f64::from(height) * f64::from(thumb_width) / f64::from(width)).round() as u32).max(1);
                resize(&decoded, thumb_width, thumb_height)?
            } else {
                decoded.clone()
            };
            Ok((thumb_width, encode_webp(&image)?))
        })
        .collect::<Result<_, RenderError>>()?;
    Ok(Thumbnails {
        thumbhash: base64::engine::general_purpose::STANDARD.encode(hash),
        images,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use image::{GenericImageView as _, Rgb, RgbImage};

    pub(crate) fn solid(width: u32, height: u32, color: [u8; 3]) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, Rgb(color)))
    }

    pub(crate) fn jpeg(image: &DynamicImage) -> Vec<u8> {
        let mut data = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut data), image::ImageFormat::Jpeg)
            .unwrap();
        data
    }

    /// The JPEG with an EXIF segment that holds only `orientation`.
    pub(crate) fn with_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
        let mut tiff = b"MM\0\x2a\0\0\0\x08\0\x01".to_vec();
        tiff.extend([0x01, 0x12, 0, 3, 0, 0, 0, 1]);
        tiff.extend(orientation.to_be_bytes());
        tiff.extend([0, 0, 0, 0, 0, 0]);
        let mut segment = b"Exif\0\0".to_vec();
        segment.extend(tiff);
        let mut out = jpeg[..2].to_vec();
        out.extend([0xff, 0xe1]);
        out.extend(((segment.len() + 2) as u16).to_be_bytes());
        out.extend(segment);
        out.extend(&jpeg[2..]);
        out
    }

    /// An XMP packet with only `tiff:Orientation`.
    pub(crate) fn xmp_packet(orientation: u8) -> String {
        format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
            <rdf:Description xmlns:tiff="http://ns.adobe.com/tiff/1.0/" tiff:Orientation="{orientation}"/>
            </rdf:RDF></x:xmpmeta>"#
        )
    }

    /// The JPEG with an XMP segment that holds only `orientation`.
    pub(crate) fn with_xmp_orientation(jpeg: &[u8], orientation: u8) -> Vec<u8> {
        let mut segment = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        segment.extend(xmp_packet(orientation).into_bytes());
        let mut out = jpeg[..2].to_vec();
        out.extend([0xff, 0xe1]);
        out.extend(((segment.len() + 2) as u16).to_be_bytes());
        out.extend(segment);
        out.extend(&jpeg[2..]);
        out
    }

    struct Previews(Option<Vec<u8>>);

    impl PreviewSource for Previews {
        fn extract_preview(&self, _: &Path) -> Option<Vec<u8>> {
            self.0.clone()
        }
    }

    fn renderer(preview: Option<Vec<u8>>) -> Renderer {
        Renderer::new(Arc::new(Previews(preview)), None)
    }

    fn render_image(file: &Path, renderer: &Renderer) -> Thumbnails {
        renderer.render(file, MediaKind::Image, None, None).unwrap()
    }

    /// `(width, decoded size)` of each thumbnail; fails unless they are WebP.
    pub(crate) fn sizes(thumbnails: &Thumbnails) -> Vec<(u32, (u32, u32))> {
        thumbnails
            .images
            .iter()
            .map(|(width, data)| {
                let image = image::load_from_memory_with_format(data, image::ImageFormat::WebP).unwrap();
                (*width, image.dimensions())
            })
            .collect()
    }

    pub(crate) fn average_color(thumbnails: &Thumbnails) -> [u8; 3] {
        let hash = base64::engine::general_purpose::STANDARD
            .decode(&thumbnails.thumbhash)
            .unwrap();
        let (r, g, b, _) = thumbhash::thumb_hash_to_average_rgba(&hash).unwrap();
        [r, g, b].map(|channel| (channel * 255.0).round() as u8)
    }

    fn reddish(color: [u8; 3]) -> bool {
        color[0] > 200 && color[1] < 50 && color[2] < 50
    }

    fn bluish(color: [u8; 3]) -> bool {
        color[0] < 50 && color[1] < 50 && color[2] > 200
    }

    /// REQ-005-03: auto-orientation of a 2 × 3 image with one distinct pixel per position (expected pixels pinned).
    #[test]
    fn req_005_03_turns_every_orientation_upright() {
        let expected: [(u8, (u32, u32), [u8; 6]); 8] = [
            (1, (2, 3), [0, 1, 2, 3, 4, 5]),
            (2, (2, 3), [1, 0, 3, 2, 5, 4]),
            (3, (2, 3), [5, 4, 3, 2, 1, 0]),
            (4, (2, 3), [4, 5, 2, 3, 0, 1]),
            (5, (3, 2), [0, 2, 4, 1, 3, 5]),
            (6, (3, 2), [4, 2, 0, 5, 3, 1]),
            (7, (3, 2), [5, 3, 1, 4, 2, 0]),
            (8, (3, 2), [1, 3, 5, 0, 2, 4]),
        ];
        let pixels: Vec<u8> = (0..6u8).flat_map(|i| [i * 40, 0, 0]).collect();
        let image = DynamicImage::ImageRgb8(RgbImage::from_raw(2, 3, pixels).unwrap());
        for (orientation, size, reds) in expected {
            let oriented = orient(image.clone(), orientation).into_rgb8();
            let actual: Vec<u8> = oriented.pixels().map(|p| p[0] / 40).collect();
            assert_eq!(
                (oriented.dimensions(), actual),
                (size, reds.to_vec()),
                "orientation {orientation}"
            );
        }
    }

    #[test]
    fn req_005_03_turns_jpegs_upright_by_their_exif_orientation() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("rotated.jpg");
        std::fs::write(&file, with_orientation(&jpeg(&solid(1000, 500, [255, 0, 0])), 6)).unwrap();

        let thumbnails = render_image(&file, &renderer(None));

        assert_eq!(sizes(&thumbnails), [(400, (400, 800)), (800, (500, 1000))]);
    }

    /// REQ-004-12, REQ-005-03: a photo oriented only in its embedded XMP, or only in its sidecar, is turned upright;
    /// an EXIF orientation wins over both.
    #[test]
    fn req_004_12_req_005_03_turns_photos_upright_by_an_orientation_only_in_xmp() {
        let dir = tempfile::tempdir().unwrap();
        let plain = jpeg(&solid(1000, 500, [255, 0, 0]));
        let upright = [(400, (400, 800)), (800, (500, 1000))];
        let sideways = [(400, (400, 200)), (800, (800, 400))];

        let embedded = dir.path().join("embedded.jpg");
        std::fs::write(&embedded, with_xmp_orientation(&plain, 6)).unwrap();
        assert_eq!(
            sizes(&render_image(&embedded, &renderer(None))),
            upright,
            "embedded XMP"
        );

        let bare = dir.path().join("bare.jpg");
        std::fs::write(&bare, &plain).unwrap();
        let sidecar = renderer(None).render(&bare, MediaKind::Image, None, Some(6)).unwrap();
        assert_eq!(sizes(&sidecar), upright, "sidecar");

        let png = dir.path().join("bare.png");
        solid(1000, 500, [255, 0, 0]).save(&png).unwrap();
        let sidecar_png = renderer(None).render(&png, MediaKind::Image, None, Some(6)).unwrap();
        assert_eq!(sizes(&sidecar_png), upright, "sidecar of a PNG");

        let both = dir.path().join("both.jpg");
        std::fs::write(&both, with_orientation(&with_xmp_orientation(&plain, 6), 1)).unwrap();
        let exif_wins = renderer(None).render(&both, MediaKind::Image, None, Some(6)).unwrap();
        assert_eq!(sizes(&exif_wins), sideways, "EXIF first");
    }

    #[test]
    fn req_005_01_renders_every_width_as_webp_without_upscaling_plus_a_thumbhash() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("wide.png");
        solid(1000, 500, [255, 0, 0]).save(&file).unwrap();

        let thumbnails = render_image(&file, &renderer(None));

        assert_eq!(sizes(&thumbnails), [(400, (400, 200)), (800, (800, 400))]);
        assert!(reddish(average_color(&thumbnails)), "{:?}", average_color(&thumbnails));

        let small = dir.path().join("small.png");
        solid(300, 300, [0, 255, 0]).save(&small).unwrap();
        let thumbnails = render_image(&small, &renderer(None));
        assert_eq!(sizes(&thumbnails), [(400, (300, 300)), (800, (300, 300))]);
    }

    #[test]
    fn req_005_01_fits_tall_strips_into_2400_pixels() {
        let thumbnails = render_thumbnails(solid(1000, 10_000, [0, 0, 255])).unwrap();
        assert_eq!(sizes(&thumbnails), [(400, (240, 2400)), (800, (240, 2400))]);
    }

    #[test]
    fn keeps_transparency() {
        let image = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(500, 500, image::Rgba([255, 0, 0, 0])));
        let thumbnails = render_thumbnails(image).unwrap();
        let decoded = image::load_from_memory(&thumbnails.images[0].1).unwrap();
        assert!(decoded.color().has_alpha());
        assert_eq!(decoded.to_rgba8().get_pixel(10, 10)[3], 0);
    }

    #[test]
    fn decodes_gif_and_webp() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.gif", "a.webp"] {
            let file = dir.path().join(name);
            solid(500, 250, [0, 0, 255]).save(&file).unwrap();
            let thumbnails = render_image(&file, &renderer(None));
            assert_eq!(sizes(&thumbnails)[0], (400, (400, 200)), "{name}");
            assert!(bluish(average_color(&thumbnails)), "{name}");
        }
    }

    /// REQ-005-04: a large JPEG decodes shrunk, to the same size as a full decode would give.
    #[test]
    fn req_005_04_shrinks_large_jpegs_while_decoding() {
        let data = jpeg(&solid(4000, 3000, [255, 0, 0]));
        let decoded = decode_jpeg(&data, None, None).unwrap();
        assert_eq!(decoded.dimensions(), (1000, 750));
        let thumbnails = render_thumbnails(decoded).unwrap();
        assert_eq!(sizes(&thumbnails)[1], (800, (800, 600)));
    }

    #[test]
    fn req_005_04_decodes_a_truncated_jpeg_partially() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cut.jpg");
        let data = jpeg(&solid(800, 600, [255, 0, 0]));
        std::fs::write(&file, &data[..data.len() * 6 / 10]).unwrap();

        let thumbnails = render_image(&file, &renderer(None));

        let decoded = image::load_from_memory(&thumbnails.images[1].1).unwrap().to_rgb8();
        assert_eq!(decoded.dimensions(), (800, 600));
        assert!(reddish(decoded.get_pixel(400, 10).0), "the top was decoded");
    }

    #[test]
    fn req_005_04_uses_a_large_enough_embedded_preview_with_the_original_orientation() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("camera.jpg");
        std::fs::write(&file, with_orientation(&jpeg(&solid(1600, 1200, [255, 0, 0])), 6)).unwrap();
        let preview = jpeg(&solid(1200, 900, [0, 0, 255]));

        let thumbnails = render_image(&file, &renderer(Some(preview)));

        assert_eq!(sizes(&thumbnails)[1], (800, (800, 1067)));
        assert!(bluish(average_color(&thumbnails)), "{:?}", average_color(&thumbnails));
    }

    #[test]
    fn req_005_04_ignores_small_differently_shaped_or_invalid_previews() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("camera.jpg");
        std::fs::write(&file, jpeg(&solid(1600, 1200, [255, 0, 0]))).unwrap();
        let previews = [
            ("too small", jpeg(&solid(160, 120, [0, 0, 255]))),
            ("other shape", jpeg(&solid(1200, 800, [0, 0, 255]))),
            ("not an image", b"not an image".to_vec()),
        ];
        for (name, preview) in previews {
            let thumbnails = render_image(&file, &renderer(Some(preview)));
            assert!(reddish(average_color(&thumbnails)), "{name}");
        }
    }

    #[test]
    fn fails_on_files_that_are_no_images() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["broken.jpg", "broken.png"] {
            let file = dir.path().join(name);
            std::fs::write(&file, b"not an image").unwrap();
            assert!(matches!(
                renderer(None).render(&file, MediaKind::Image, None, None),
                Err(RenderError::Decode(_))
            ));
        }
    }
}
