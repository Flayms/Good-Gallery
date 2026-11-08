//! Saving and reverting edits (spec 021). A save renders the recipe from the unedited original at full resolution in
//! 32-bit float, encodes it in the original's format into a temporary file beside the target, copies the original's
//! metadata into it with exiftool (with the recipe in our own XMP namespace) and renames it into place. Replacing an
//! original first copies it into the library's backup folder; reverting moves the backup back.

use crate::thumbnail::{decode_error, limits};
use domain::edit::{
    Geometry, Mask, MtimeOnSave, Params, Pyramid, Recipe, backup_rel_path, blur_lod, blur_pixel, copy_name, dither,
    lens_weighted,
};
use image::{DynamicImage, ImageReader, Rgba32FImage};
use std::io::{Cursor, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// exiftool's definition of our XMP namespace, which it needs to write `gg:` tags (reading needs none).
const EXIFTOOL_CONFIG: &str = r"%Image::ExifTool::UserDefined = (
    'Image::ExifTool::XMP::Main' => {
        gg => { SubDirectory => { TagTable => 'Image::ExifTool::UserDefined::gg' } },
    },
);
%Image::ExifTool::UserDefined::gg = (
    GROUPS => { 0 => 'XMP', 1 => 'XMP-gg', 2 => 'Image' },
    NAMESPACE => { 'gg' => 'https://github.com/Flayms/Good-Gallery/ns/edit/1.0/' },
    WRITABLE => 'string',
    EditRecipe => { },
    EditSource => { },
    EditHistory => { },
);
1;
";

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Render(#[from] crate::thumbnail::RenderError),
    #[error("can't encode the photo: {0}")]
    Encode(String),
    #[error("can't copy the metadata: {0}")]
    Metadata(String),
    #[error("this file type can't be edited")]
    Unsupported,
    #[error("the unedited original is missing")]
    NoOriginal,
    #[error("the path leaves its library")]
    UnsafePath,
    #[error("can't find the subject: {0}")]
    Subject(String),
    #[error(transparent)]
    Replace(#[from] crate::replace::Error),
}

/// Writes exiftool's configuration for the `gg` namespace into `dir` (the app's data folder, not a shared temporary
/// folder: exiftool runs the file as Perl) and returns its path.
pub fn exiftool_config(dir: &Path) -> std::io::Result<PathBuf> {
    let path = dir.join("exiftool-edit.config");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(EXIFTOOL_CONFIG) {
        std::fs::create_dir_all(dir)?;
        std::fs::write(&path, EXIFTOOL_CONFIG)?;
    }
    Ok(path)
}

/// What saving needs to know (REQ-021-09, REQ-021-10).
#[derive(Debug, Clone)]
pub struct SaveRequest {
    /// The library's folder.
    pub library: PathBuf,
    /// The edited media item's file, relative to the library.
    pub rel_path: String,
    /// Its unedited original, relative to the library: the file itself, its backup, or the source of a copy.
    pub source: String,
    pub recipe: Recipe,
    /// Save as a copy beside the file, or replace the file.
    pub copy: bool,
    /// Replacing keeps the unedited original as a backup (REQ-021-10); without, the file keeps no edit and an old
    /// backup is deleted (REQ-021-22).
    pub keep_original: bool,
    pub mtime: MtimeOnSave,
    /// The subject as the preview showed it, for a recipe with parts (REQ-022-05); found again without one.
    pub subject: Option<Arc<Mask>>,
    /// Turns an original without an orientation of its own (REQ-004-12).
    pub sidecar_orientation: Option<u8>,
    /// The file's versions so far, ending with the edit it holds; none for an unedited photo (REQ-021-24).
    pub history: Vec<domain::versions::Version>,
}

/// exiftool and its configuration for the `gg` namespace.
#[derive(Debug, Clone)]
pub struct Tools {
    pub exiftool: PathBuf,
    pub config: PathBuf,
}

/// Saves an edit; returns the written file, relative to the library.
pub fn save(tools: &Tools, request: &SaveRequest) -> Result<String, EditError> {
    let path = |rel: &str| db_path(&request.library, rel).ok_or(EditError::UnsafePath);
    let file = path(&request.rel_path)?;
    let mut source_rel = request.source.clone();
    let mut source = path(&source_rel)?;
    if !source.is_file() {
        return Err(EditError::NoOriginal);
    }
    let format = Format::of(&file).ok_or(EditError::Unsupported)?;
    let original_mtime = std::fs::metadata(&source)?.modified()?;
    let folder = file.parent().ok_or(EditError::UnsafePath)?.to_path_buf();

    let target_rel = if request.copy {
        let name = file_name(&request.rel_path);
        let copy = copy_name(name, |candidate| folder.join(candidate).exists());
        sibling(&request.rel_path, &copy)
    } else {
        request.rel_path.clone()
    };
    let target = path(&target_rel)?;
    // Each exiftool step writes a new file (REQ-021-16): the rendered photo, then with the metadata, then with the
    // regions moved.
    let rendered = crate::replace::beside(&target, "edit");
    let tagged = crate::replace::beside(&target, "meta");
    let turned = crate::replace::beside(&target, "regions");
    let mut made_backup = None;
    let written = (|| {
        let encoded = render_file(
            &source,
            &request.recipe,
            request.subject.as_deref(),
            format,
            request.sidecar_orientation,
        )?;
        write_synced(&rendered, &encoded)?;
        // Replacing an unedited original: keep it first (REQ-021-10). A file already edited keeps its first backup;
        // a copy saved over has its source elsewhere.
        if !request.copy && request.keep_original && source_rel == request.rel_path {
            source_rel = backup_rel_path(&request.rel_path);
            let backup = path(&source_rel)?;
            if !backup.exists() {
                make_backup(&file, &backup, &request.library)?;
                made_backup = Some(backup.clone());
            }
            source = backup;
        }
        // Without a backup there is no original to edit again from, so the file keeps no edit (REQ-021-22). A copy
        // starts its own versions, a replacement adds one to the file's (REQ-021-24).
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs() as i64);
        let earlier: &[domain::versions::Version] = if request.copy { &[] } else { &request.history };
        let history = domain::versions::to_json(&domain::versions::after_save(earlier, &request.recipe, now));
        let edit = (request.copy || request.keep_original).then_some(SavedFields {
            recipe: &request.recipe,
            source: source_rel.as_str(),
            history: &history,
        });
        copy_metadata(tools, &source, &rendered, &tagged, edit)?;
        let _ = std::fs::remove_file(&rendered);
        let new = if move_regions(
            tools,
            &source,
            &tagged,
            &turned,
            &request.recipe,
            request.sidecar_orientation,
        )? {
            let _ = std::fs::remove_file(&tagged);
            &turned
        } else {
            &tagged
        };
        if request.mtime == MtimeOnSave::Keep {
            set_mtime(new, original_mtime)?;
        }
        // The old file or the new one is in place, never an empty or partial one (REQ-021-16).
        crate::replace::replace(new, &target)?;
        Ok::<_, EditError>(())
    })();
    if let Err(error) = written {
        for temp in [&rendered, &tagged, &turned] {
            let _ = std::fs::remove_file(temp);
        }
        // The file wasn't replaced, so a backup made for it isn't needed (and won't be mistaken for its original).
        if let Some(backup) = made_backup {
            let _ = std::fs::remove_file(&backup);
            remove_empty_backup_folders(&request.library, &backup);
        }
        return Err(error);
    }
    if request.copy {
        copy_sidecar(&folder, file_name(&request.rel_path), file_name(&target_rel));
    }
    // The edited photo is in place: an earlier backup isn't wanted any more (REQ-021-22). The original of a copy is a
    // file of its own and stays.
    if !request.copy && !request.keep_original && source_rel == backup_rel_path(&request.rel_path) {
        std::fs::remove_file(&source)?;
        remove_empty_backup_folders(&request.library, &source);
    }
    Ok(target_rel)
}

/// Keeps only the last of `versions` in the file's history (REQ-021-26). Only the tag changes: the photo, its other
/// metadata and its date modified stay, and the file is replaced safely (REQ-021-16).
pub fn clear_history(
    tools: &Tools,
    library: &Path,
    rel_path: &str,
    versions: &[domain::versions::Version],
) -> Result<(), EditError> {
    let file = db_path(library, rel_path).ok_or(EditError::UnsafePath)?;
    let modified = std::fs::metadata(&file)?.modified()?;
    let history = domain::versions::to_json(&domain::versions::cleared(versions));
    let temp = crate::replace::beside(&file, "history");
    let args = [
        "-charset".into(),
        "filename=utf8".into(),
        format!("-XMP-gg:EditHistory={history}"),
    ];
    let written = crate::replace::exiftool_into(&tools.exiftool, Some(&tools.config), &args, &file, &temp)
        .map_err(metadata_error)
        .and_then(|()| set_mtime(&temp, modified).map_err(EditError::from))
        .and_then(|()| crate::replace::replace(&temp, &file).map_err(EditError::from));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

/// Puts the unedited original back in place of `rel_path` and removes its backup (REQ-021-12).
pub fn revert(library: &Path, rel_path: &str, mtime: MtimeOnSave) -> Result<(), EditError> {
    let file = db_path(library, rel_path).ok_or(EditError::UnsafePath)?;
    let backup = db_path(library, &backup_rel_path(rel_path)).ok_or(EditError::UnsafePath)?;
    if !backup.is_file() {
        return Err(EditError::NoOriginal);
    }
    // A copy takes the edited file's place, so the backup stays until the original is back (REQ-021-16).
    let temp = crate::replace::beside(&file, "revert");
    let modified = std::fs::metadata(&backup)?.modified()?;
    let copied = std::fs::copy(&backup, &temp).and_then(|_| set_mtime(&temp, modified));
    if let Err(error) = copied {
        let _ = std::fs::remove_file(&temp);
        return Err(error.into());
    }
    crate::replace::replace(&temp, &file)?;
    std::fs::remove_file(&backup)?;
    if mtime == MtimeOnSave::Update {
        set_mtime(&file, SystemTime::now())?;
    }
    remove_empty_backup_folders(library, &backup);
    Ok(())
}

/// The file of `rel_path` in `library`, if the path stays inside it (REQ-011-08).
fn db_path(library: &Path, rel_path: &str) -> Option<PathBuf> {
    domain::media::is_safe_rel_path(rel_path).then(|| {
        let mut path = library.to_path_buf();
        path.extend(rel_path.split('/'));
        path
    })
}

fn file_name(rel_path: &str) -> &str {
    rel_path.rsplit('/').next().unwrap_or(rel_path)
}

/// `name` in the folder of `rel_path`.
fn sibling(rel_path: &str, name: &str) -> String {
    match rel_path.rfind('/') {
        Some(slash) => format!("{}/{name}", &rel_path[..slash]),
        None => name.to_owned(),
    }
}

fn write_synced(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(data)?;
    file.sync_all()
}

fn set_mtime(path: &Path, time: SystemTime) -> std::io::Result<()> {
    std::fs::File::options().write(true).open(path)?.set_modified(time)
}

/// Copies `file` to `backup` (complete, on disk and read back before it counts, with its date modified), and hides
/// the backup folder on Windows.
fn make_backup(file: &Path, backup: &Path, library: &Path) -> Result<(), EditError> {
    let folder = backup.parent().expect("a backup has a folder");
    std::fs::create_dir_all(folder)?;
    hide(&library.join(domain::edit::BACKUP_DIR.split('/').next().unwrap_or_default()));
    let temp = crate::replace::beside(backup, "backup");
    let modified = std::fs::metadata(file)?.modified()?;
    let made = std::fs::copy(file, &temp)
        .and_then(|_| set_mtime(&temp, modified))
        .map_err(EditError::from)
        .and_then(|()| crate::replace::replace(&temp, backup).map_err(EditError::from));
    if made.is_err() {
        let _ = std::fs::remove_file(&temp);
        remove_empty_backup_folders(library, backup);
    }
    made
}

#[cfg(windows)]
fn hide(folder: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new("attrib")
        .arg("+H")
        .arg(folder)
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

#[cfg(not(windows))]
fn hide(_folder: &Path) {}

/// Removes the backup's folders up to and including `.good-gallery`, as far as they are empty.
fn remove_empty_backup_folders(library: &Path, backup: &Path) {
    let top = library.join(domain::edit::BACKUP_DIR.split('/').next().unwrap_or_default());
    let mut folder = backup.parent();
    while let Some(dir) = folder {
        if !dir.starts_with(&top) || std::fs::remove_dir(dir).is_err() {
            return;
        }
        if dir == top {
            return;
        }
        folder = dir.parent();
    }
}

/// A copy gets a copy of the original's sidecar under its own name, in the same naming scheme (REQ-021-14).
fn copy_sidecar(folder: &Path, original: &str, copy: &str) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    let mut sidecars: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if domain::media::is_sidecar(&name) {
            sidecars.entry(name.to_lowercase()).or_default().push(name);
        }
    }
    let Some(sidecar) = domain::media::find_sidecar(&sidecars, original) else {
        return;
    };
    let stem = |name: &str| match name.rfind('.') {
        Some(dot) if dot > 0 => name[..dot].to_owned(),
        _ => name.to_owned(),
    };
    // `IMG_1.jpg.xmp` → `IMG_1 (edited).jpg.xmp`, `IMG_1.xmp` → `IMG_1 (edited).xmp`.
    let name = if stem(sidecar).eq_ignore_ascii_case(original) {
        format!("{copy}.xmp")
    } else {
        format!("{}.xmp", stem(copy))
    };
    let target = folder.join(name);
    if !target.exists()
        && let Err(error) = std::fs::copy(folder.join(sidecar), &target)
    {
        tracing::warn!("can't copy the sidecar {sidecar}: {error}");
    }
}

/// The formats an edit is saved in: the original's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Jpeg,
    Png,
    Webp,
}

impl Format {
    fn of(file: &Path) -> Option<Self> {
        match file.extension()?.to_string_lossy().to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "webp" => Some(Self::Webp),
            _ => None,
        }
    }
}

/// The decoded original: pixels in 32-bit float, upright, and what the output keeps of it.
struct Decoded {
    image: Rgba32FImage,
    alpha: bool,
    sixteen_bit: bool,
    lossless_webp: bool,
}

fn decode(data: &[u8], sidecar_orientation: Option<u8>) -> Result<Decoded, EditError> {
    let mut reader = ImageReader::new(Cursor::new(data)).with_guessed_format()?;
    reader.limits(limits());
    let mut decoder = reader.into_decoder().map_err(decode_error)?;
    let orientation = crate::thumbnail::decoder_orientation(&mut decoder, sidecar_orientation);
    let mut image = DynamicImage::from_decoder(decoder).map_err(decode_error)?;
    if image.width() == 0 || image.height() == 0 {
        return Err(decode_error("empty image").into());
    }
    // Upright in the pixels; the saved file gets orientation 1 (REQ-021-14).
    image.apply_orientation(orientation);
    let color = image.color();
    Ok(Decoded {
        alpha: color.has_alpha(),
        sixteen_bit: color.bytes_per_pixel() / color.channel_count() >= 2,
        lossless_webp: is_lossless_webp(data),
        image: image.into_rgba32f(),
    })
}

/// Whether a WebP file holds a lossless (`VP8L`) image.
fn is_lossless_webp(data: &[u8]) -> bool {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return false;
    }
    let mut at = 12;
    while at + 8 <= data.len() {
        let id = &data[at..at + 4];
        let size = u32::from_le_bytes([data[at + 4], data[at + 5], data[at + 6], data[at + 7]]) as usize;
        match id {
            b"VP8L" => return true,
            b"VP8 " => return false,
            _ => at += 8 + size + (size & 1),
        }
    }
    false
}

/// Applies `params` to every pixel, on all cores, the subject's and the background's own adjustments by `subject`
/// (REQ-022-03); a blur first, from a pyramid of the photo (REQ-023-04).
pub fn render(params: &Params, image: &mut Rgba32FImage, subject: Option<&Mask>) {
    let (width, height) = image.dimensions();
    let size = [width as f32, height as f32];
    let pyramid = (params.blur > 0.0 || params.halation > 0.0).then(|| {
        let pixels = image.pixels().map(|pixel| pixel.0).collect();
        Pyramid::new(width, height, pixels)
    });
    let pyramid = pyramid.as_ref();
    // Lights spread into discs: a pyramid of weighted pixels (REQ-023-05).
    let lens = (params.blur > 0.0 && params.blur_lens).then(|| {
        let pixels = image.pixels().map(|pixel| lens_weighted(pixel.0)).collect();
        Pyramid::new(width, height, pixels)
    });
    let lens = lens.as_ref();
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let row = width as usize * 4;
    let rows_per_chunk = (height as usize).div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for (chunk, pixels) in image.chunks_mut(rows_per_chunk * row).enumerate() {
            scope.spawn(move || {
                for (i, pixel) in pixels.chunks_exact_mut(4).enumerate() {
                    let x = (i % width as usize) as f32;
                    let y = (chunk * rows_per_chunk + i / width as usize) as f32;
                    let uv = [(x + 0.5) / size[0], (y + 0.5) / size[1]];
                    let inside = subject.map_or(0.0, |mask| mask.sample(uv));
                    let mut glow = [0.0; 3];
                    if let Some(pyramid) = pyramid {
                        let sharp = [pixel[0], pixel[1], pixel[2], pixel[3]];
                        if params.halation > 0.0 {
                            let lod = blur_lod(Params::halation_sigma(size), pyramid.levels());
                            let around = pyramid.blurred(sharp, uv, lod);
                            glow =
                                params.halation_glow([sharp[0], sharp[1], sharp[2]], [around[0], around[1], around[2]]);
                        }
                        let sigma = params.blur_sigma(uv, size, inside);
                        // Transparency stays sharp, as in the preview.
                        pixel[..3].copy_from_slice(&blur_pixel(pyramid, lens, sharp, uv, sigma, size)[..3]);
                    }
                    let rgb = [pixel[0], pixel[1], pixel[2]];
                    let [r, g, b] = params.pixel_glowing(rgb, glow, uv, size, subject.map(|_| inside));
                    pixel[..3].copy_from_slice(&[r, g, b]);
                }
            });
        }
    });
}

/// The cropped, turned output of a rendered photo (REQ-029-06).
fn apply_geometry(image: Rgba32FImage, geometry: &Geometry) -> Rgba32FImage {
    if geometry.is_identity() {
        return image;
    }
    let [x, y, w, h] = geometry.crop_pixels(image.width(), image.height());
    let cropped = image::imageops::crop_imm(&image, x, y, w, h).to_image();
    match geometry.quarter_turns() {
        1 => image::imageops::rotate90(&cropped),
        2 => image::imageops::rotate180(&cropped),
        3 => image::imageops::rotate270(&cropped),
        _ => cropped,
    }
}

/// The original at `source` with `recipe` applied, encoded as `format`.
fn render_file(
    source: &Path,
    recipe: &Recipe,
    subject: Option<&Mask>,
    format: Format,
    sidecar_orientation: Option<u8>,
) -> Result<Vec<u8>, EditError> {
    let data = std::fs::read(source)?;
    let mut decoded = decode(&data, sidecar_orientation)?;
    drop(data);
    let found;
    let subject = match subject {
        _ if !recipe.needs_subject() => None,
        Some(mask) => Some(mask),
        None => {
            let rgba = DynamicImage::ImageRgba32F(decoded.image.clone()).into_rgba8();
            found = crate::subject::find(&rgba).map_err(EditError::Subject)?;
            Some(&found)
        }
    };
    let size = [decoded.image.width() as f32, decoded.image.height() as f32];
    let paintings = domain::paint::draw_all(&recipe.paintings, size);
    render(
        &Params::new(recipe).with_paintings(&paintings),
        &mut decoded.image,
        subject,
    );
    // Last, so pixels outside the crop still feed the blur (REQ-029-04).
    decoded.image = apply_geometry(decoded.image, &recipe.geometry());
    decoded.image = scale_down(decoded.image, recipe.long_edge);
    encode(&decoded, format, recipe.quality())
}

/// 8 bits per channel with a dither below half a step, so smooth gradients don't band and unchanged values stay.
fn quantize8(image: &Rgba32FImage, alpha: bool) -> DynamicImage {
    let (width, height) = image.dimensions();
    let channels = if alpha { 4 } else { 3 };
    let mut bytes = Vec::with_capacity(width as usize * height as usize * channels);
    for (x, y, pixel) in image.enumerate_pixels() {
        for c in 0..channels {
            let noise = if c < 3 { dither(x, y, c as u32) * 0.49 } else { 0.0 };
            bytes.push((pixel[c] * 255.0 + noise).round().clamp(0.0, 255.0) as u8);
        }
    }
    if alpha {
        DynamicImage::ImageRgba8(image::RgbaImage::from_raw(width, height, bytes).expect("sized"))
    } else {
        DynamicImage::ImageRgb8(image::RgbImage::from_raw(width, height, bytes).expect("sized"))
    }
}

fn quantize16(image: &Rgba32FImage, alpha: bool) -> DynamicImage {
    let converted = DynamicImage::ImageRgba32F(image.clone());
    if alpha {
        DynamicImage::ImageRgba16(converted.into_rgba16())
    } else {
        DynamicImage::ImageRgb16(converted.into_rgb16())
    }
}

/// The photo with its long edge at most `long_edge` (REQ-029-09).
fn scale_down(image: Rgba32FImage, long_edge: Option<u32>) -> Rgba32FImage {
    let (width, height) = domain::edit::scaled_size(image.width(), image.height(), long_edge);
    if (width, height) == image.dimensions() {
        return image;
    }
    image::imageops::resize(&image, width, height, image::imageops::FilterType::Lanczos3)
}

fn encode(decoded: &Decoded, format: Format, quality: u8) -> Result<Vec<u8>, EditError> {
    let encode_error = |error: image::ImageError| EditError::Encode(error.to_string());
    let mut out = Vec::new();
    match format {
        Format::Jpeg => {
            let image = quantize8(&decoded.image, false);
            image
                .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality))
                .map_err(encode_error)?;
        }
        Format::Png => {
            let image = if decoded.sixteen_bit {
                quantize16(&decoded.image, decoded.alpha)
            } else {
                quantize8(&decoded.image, decoded.alpha)
            };
            image
                .write_with_encoder(image::codecs::png::PngEncoder::new(&mut out))
                .map_err(encode_error)?;
        }
        Format::Webp => {
            let image = quantize8(&decoded.image, decoded.alpha);
            let (width, height) = (image.width(), image.height());
            let encoder = match &image {
                DynamicImage::ImageRgba8(rgba) => webp::Encoder::from_rgba(rgba.as_raw(), width, height),
                DynamicImage::ImageRgb8(rgb) => webp::Encoder::from_rgb(rgb.as_raw(), width, height),
                _ => return Err(EditError::Encode("unexpected pixel format".into())),
            };
            let encoded = encoder
                .encode_simple(decoded.lossless_webp, f32::from(quality))
                .map_err(|error| EditError::Encode(format!("{error:?}")))?;
            out.extend_from_slice(&encoded);
        }
    }
    Ok(out)
}

/// The arguments that copy `source`'s metadata into a photo and record the edit: everything but embedded previews
/// (they would show the unedited photo), the color profile, orientation 1 (the pixels are upright).
/// With `edit` the recipe, where its original is and the versions, else no edit at all (REQ-021-22).
fn metadata_args(source: &Path, edit: Option<SavedFields>) -> Vec<String> {
    let path = |path: &Path| path.to_string_lossy().into_owned();
    let (recipe, source_rel, history) = match edit {
        Some(edit) => (edit.recipe.to_json(), edit.source.to_string(), edit.history.to_string()),
        // Empty values delete the tags, also ones copied from the source.
        None => (String::new(), String::new(), String::new()),
    };
    vec![
        "-charset".into(),
        "filename=utf8".into(),
        "-tagsFromFile".into(),
        path(source),
        "-all:all".into(),
        "-icc_profile".into(),
        "--ThumbnailImage".into(),
        "--PreviewImage".into(),
        "--MPF:all".into(),
        "-Orientation#=1".into(),
        format!("-XMP-gg:EditRecipe={recipe}"),
        format!("-XMP-gg:EditSource={source_rel}"),
        format!("-XMP-gg:EditHistory={history}"),
        "-ignoreMinorErrors".into(),
    ]
}

/// What a saved file records of its edit in our namespace.
#[derive(Clone, Copy)]
struct SavedFields<'a> {
    recipe: &'a Recipe,
    source: &'a str,
    /// `gg:EditHistory` (REQ-021-24).
    history: &'a str,
}

/// Writes `from` with `source`'s metadata and the recipe into the new file `to` with exiftool (REQ-011-09).
fn copy_metadata(
    tools: &Tools,
    source: &Path,
    from: &Path,
    to: &Path,
    edit: Option<SavedFields>,
) -> Result<(), EditError> {
    let args = metadata_args(source, edit);
    crate::replace::exiftool_into(&tools.exiftool, Some(&tools.config), &args, from, to).map_err(metadata_error)
}

fn metadata_error(error: crate::replace::Error) -> EditError {
    match error {
        crate::replace::Error::Exiftool(error) => EditError::Metadata(error),
        error => error.into(),
    }
}

/// Moves the regions just copied from `source` into `from` onto its pixels, which were turned upright and through the
/// recipe's crop and turns (REQ-029-08, REQ-021-14), writing the new file `to`; `false` if nothing had to move. A
/// person who loses every region keeps their People tag.
fn move_regions(
    tools: &Tools,
    source: &Path,
    from: &Path,
    to: &Path,
    recipe: &Recipe,
    sidecar_orientation: Option<u8>,
) -> Result<bool, EditError> {
    let path = |path: &Path| path.to_string_lossy().into_owned();
    let read = crate::exiftool::run_once(
        &tools.exiftool,
        None,
        &[
            "-charset".into(),
            "filename=utf8".into(),
            "-json".into(),
            "-struct".into(),
            "-Orientation#".into(),
            "-XMP-mwg-rs:RegionInfo".into(),
            "-XMP-MP:RegionInfoMP".into(),
            path(source),
        ],
    )
    .map_err(EditError::Metadata)?;
    let parsed: serde_json::Value =
        serde_json::from_str(&read).map_err(|error| EditError::Metadata(error.to_string()))?;
    let Some(tags) = parsed.get(0).and_then(serde_json::Value::as_object) else {
        return Ok(false);
    };
    let orientation = tags
        .get("Orientation")
        .and_then(serde_json::Value::as_f64)
        .map(|n| n as u8)
        .or(sidecar_orientation)
        .unwrap_or(1);
    let geometry = recipe.geometry();
    if geometry.is_identity() && orientation == 1 {
        return Ok(false);
    }
    let size = image::image_dimensions(from).map_err(|error| EditError::Encode(error.to_string()))?;
    let Some(regions) = meta::rules::regions_on_output(tags, orientation, &geometry, size) else {
        return Ok(false);
    };
    let mut args = vec![
        "-charset".to_string(),
        "filename=utf8".into(),
        "-XMP-MP:RegionInfoMP=".into(),
    ];
    // Through a file, so names need no escaping for exiftool's struct syntax.
    let file = from.with_extension("gg-regions.json");
    match &regions.info {
        Some(info) => {
            let json = serde_json::json!([{ "SourceFile": "*", "RegionInfo": info }]);
            std::fs::write(&file, json.to_string())?;
            args.push(format!("-json={}", path(&file)));
        }
        None => args.push("-XMP-mwg-rs:RegionInfo=".into()),
    }
    for name in regions.lost_people.iter().filter(|name| !name.contains(['\n', '\r'])) {
        // Removing first keeps a name that is there already from appearing twice.
        args.push(format!("-XMP-iptcExt:PersonInImage-={name}"));
        args.push(format!("-XMP-iptcExt:PersonInImage+={name}"));
    }
    args.push("-ignoreMinorErrors".into());
    let written = crate::replace::exiftool_into(&tools.exiftool, Some(&tools.config), &args, from, to);
    let _ = std::fs::remove_file(&file);
    written.map(|()| true).map_err(metadata_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::edit::Adjustment;
    use image::ImageDecoder as _;
    use serde_json::Value;
    use std::process::Command;

    struct Library {
        dir: tempfile::TempDir,
        tools: Tools,
    }

    fn library() -> Option<Library> {
        let exiftool = crate::exiftool::tests::exiftool()?;
        drop(exiftool);
        let dir = tempfile::tempdir().unwrap();
        let config = exiftool_config(&dir.path().join("data")).unwrap();
        let tools = Tools {
            exiftool: crate::exiftool::find_exiftool().unwrap(),
            config,
        };
        std::fs::create_dir(dir.path().join("photos")).unwrap();
        Some(Library { dir, tools })
    }

    impl Library {
        fn root(&self) -> PathBuf {
            self.dir.path().join("photos")
        }

        fn path(&self, rel: &str) -> PathBuf {
            db_path(&self.root(), rel).unwrap()
        }

        /// Writes tags into a file with exiftool.
        fn tag(&self, rel: &str, args: &[&str]) {
            let status = Command::new(&self.tools.exiftool)
                .args(args)
                .args(["-overwrite_original", "-q"])
                .arg(self.path(rel))
                .status()
                .unwrap();
            assert!(status.success());
        }

        fn read(&self, rel: &str) -> serde_json::Map<String, Value> {
            let output = Command::new(&self.tools.exiftool)
                .args(["-j", "-n", "-G1", "-a"])
                .arg(self.path(rel))
                .output()
                .unwrap();
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            value[0].as_object().unwrap().clone()
        }

        fn request(&self, rel: &str, recipe: Recipe, copy: bool) -> SaveRequest {
            SaveRequest {
                library: self.root(),
                rel_path: rel.into(),
                source: rel.into(),
                recipe,
                copy,
                keep_original: true,
                mtime: MtimeOnSave::Keep,
                subject: None,
                sidecar_orientation: None,
                history: Vec::new(),
            }
        }

        fn without_backup(&self, rel: &str, recipe: Recipe) -> SaveRequest {
            SaveRequest {
                keep_original: false,
                ..self.request(rel, recipe, false)
            }
        }
    }

    fn size_of(path: &Path) -> (u32, u32) {
        image::image_dimensions(path).unwrap()
    }

    /// REQ-029-09: the saved photo has the chosen long edge after the crop, never more than the crop.
    #[test]
    fn req_029_09_saves_at_the_smaller_size() {
        let Some(lib) = library() else { return };
        jpeg_file(&lib, "a.jpg");
        let smaller = Recipe {
            long_edge: Some(16),
            ..brighter()
        };
        let written = save(&lib.tools, &lib.request("a.jpg", smaller, true)).unwrap();
        assert_eq!(size_of(&lib.path(&written)), (16, 8));
        // A half-width crop of 32 × 32 is already below 40.
        let cropped = Recipe {
            crop: Some([0.0, 0.0, 0.5, 1.0]),
            long_edge: Some(40),
            ..Recipe::default()
        };
        let written = save(&lib.tools, &lib.request("a.jpg", cropped, true)).unwrap();
        assert_eq!(size_of(&lib.path(&written)), (32, 32));
    }

    /// REQ-021-21: a lower quality makes a smaller JPEG.
    #[test]
    fn req_021_21_a_lower_quality_makes_a_smaller_file() {
        let Some(lib) = library() else { return };
        let path = lib.path("big.jpg");
        let noise = image::RgbImage::from_fn(256, 256, |x, y| {
            let n = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)) as u8;
            image::Rgb([n, n.wrapping_mul(3), n.wrapping_add(x as u8)])
        });
        DynamicImage::ImageRgb8(noise).save(&path).unwrap();
        let length = |rel: &str| std::fs::metadata(lib.path(rel)).unwrap().len();
        let default = save(&lib.tools, &lib.request("big.jpg", brighter(), true)).unwrap();
        let low = Recipe {
            quality: Some(30),
            ..brighter()
        };
        let low = save(&lib.tools, &lib.request("big.jpg", low, true)).unwrap();
        assert!(
            length(&low) * 2 < length(&default),
            "{} vs {}",
            length(&low),
            length(&default)
        );
    }

    /// REQ-021-22: saving without a backup replaces the file, keeps no edit in it, and deletes an earlier backup.
    #[test]
    fn req_021_22_saves_without_a_backup_and_deletes_an_old_one() {
        let Some(lib) = library() else { return };
        jpeg_file(&lib, "a.jpg");
        jpeg_file(&lib, "b.jpg");
        let original = std::fs::read(lib.path("a.jpg")).unwrap();

        save(&lib.tools, &lib.without_backup("a.jpg", brighter())).unwrap();
        assert_ne!(std::fs::read(lib.path("a.jpg")).unwrap(), original);
        assert!(!lib.root().join(".good-gallery").exists(), "no backup");
        let tags = lib.read("a.jpg");
        assert!(!tags.contains_key("XMP-gg:EditRecipe") && !tags.contains_key("XMP-gg:EditSource"));

        // Saved with a backup before: the backup goes, and the edit is rendered from it once more.
        save(&lib.tools, &lib.request("b.jpg", brighter(), false)).unwrap();
        assert!(lib.path(".good-gallery/originals/b.jpg").is_file());
        let mut again = lib.without_backup("b.jpg", brighter());
        again.source = ".good-gallery/originals/b.jpg".into();
        save(&lib.tools, &again).unwrap();
        assert!(!lib.root().join(".good-gallery").exists(), "the old backup is deleted");
        assert!(mean_red(&lib.path("b.jpg")) < 200.0, "the edit doesn't stack");
        assert!(!lib.read("b.jpg").contains_key("XMP-gg:EditRecipe"));
    }

    /// REQ-021-22: the original of an edited copy is a file of its own, and stays.
    #[test]
    fn req_021_22_an_edited_copy_leaves_its_original() {
        let Some(lib) = library() else { return };
        jpeg_file(&lib, "a.jpg");
        let original = std::fs::read(lib.path("a.jpg")).unwrap();
        let copy = save(&lib.tools, &lib.request("a.jpg", brighter(), true)).unwrap();
        let mut request = lib.without_backup(&copy, brighter());
        request.source = "a.jpg".into();
        save(&lib.tools, &request).unwrap();
        assert_eq!(std::fs::read(lib.path("a.jpg")).unwrap(), original);
        assert!(!lib.read(&copy).contains_key("XMP-gg:EditSource"));
    }

    fn brighter() -> Recipe {
        let mut recipe = Recipe::default();
        Adjustment::Exposure.set(&mut recipe, 1.0);
        recipe
    }

    fn gradient(width: u32, height: u32) -> image::RgbImage {
        image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x * 255 / width) as u8, (y * 255 / height) as u8, 90])
        })
    }

    fn jpeg_file(lib: &Library, rel: &str) {
        let path = lib.path(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        DynamicImage::ImageRgb8(gradient(64, 32)).save(&path).unwrap();
    }

    fn mtime(path: &Path) -> SystemTime {
        std::fs::metadata(path).unwrap().modified().unwrap()
    }

    fn mean_red(path: &Path) -> f64 {
        let image = image::open(path).unwrap().into_rgb8();
        image.pixels().map(|p| f64::from(p[0])).sum::<f64>() / f64::from(image.width() * image.height())
    }

    #[test]
    fn req_021_09_req_021_14_saves_a_copy_beside_the_original_with_its_metadata_and_sidecar() {
        let Some(lib) = library() else { return };
        jpeg_file(&lib, "trip/a.jpg");
        lib.tag(
            "trip/a.jpg",
            &["-Rating=4", "-Keywords=beach", "-GPSLatitude=48.1", "-GPSLatitudeRef=N"],
        );
        std::fs::write(lib.path("trip/a.xmp"), "<x:xmpmeta xmlns:x='adobe:ns:meta/'/>").unwrap();
        let original = std::fs::read(lib.path("trip/a.jpg")).unwrap();

        let written = save(&lib.tools, &lib.request("trip/a.jpg", brighter(), true)).unwrap();

        assert_eq!(written, "trip/a (edited).jpg");
        assert_eq!(
            std::fs::read(lib.path("trip/a.jpg")).unwrap(),
            original,
            "the original stays"
        );
        assert!(mean_red(&lib.path(&written)) > mean_red(&lib.path("trip/a.jpg")) + 10.0);
        let tags = lib.read(&written);
        assert_eq!(tags["XMP-xmp:Rating"], 4);
        assert_eq!(tags["IPTC:Keywords"], "beach");
        assert!(tags.contains_key("GPS:GPSLatitude"));
        assert_eq!(tags["XMP-gg:EditSource"], "trip/a.jpg");
        assert_eq!(
            Recipe::from_json(tags["XMP-gg:EditRecipe"].as_str().unwrap()),
            Some(brighter())
        );
        assert!(lib.path("trip/a (edited).xmp").is_file(), "the sidecar comes along");
        // A second copy is numbered; no temporary files are left.
        assert_eq!(
            save(&lib.tools, &lib.request("trip/a.jpg", brighter(), true)).unwrap(),
            "trip/a (edited 2).jpg"
        );
        let names: Vec<String> = std::fs::read_dir(lib.path("trip"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|name| !name.starts_with('.')), "{names:?}");
    }

    #[test]
    fn req_021_10_req_021_12_replacing_keeps_a_backup_and_reverting_restores_the_original_byte_for_byte() {
        let Some(lib) = library() else { return };
        jpeg_file(&lib, "a.jpg");
        lib.tag("a.jpg", &["-Rating=2"]);
        let original = std::fs::read(lib.path("a.jpg")).unwrap();
        let before = mtime(&lib.path("a.jpg"));

        assert_eq!(
            save(&lib.tools, &lib.request("a.jpg", brighter(), false)).unwrap(),
            "a.jpg"
        );

        let backup = lib.path(".good-gallery/originals/a.jpg");
        assert_eq!(std::fs::read(&backup).unwrap(), original);
        assert_ne!(std::fs::read(lib.path("a.jpg")).unwrap(), original);
        let tags = lib.read("a.jpg");
        assert_eq!(tags["XMP-gg:EditSource"], ".good-gallery/originals/a.jpg");
        assert_eq!(tags["XMP-xmp:Rating"], 2);

        // Saved again, from the backup: the first backup stays, the edit doesn't stack (REQ-021-11).
        let mut again = lib.request("a.jpg", brighter(), false);
        again.source = ".good-gallery/originals/a.jpg".into();
        save(&lib.tools, &again).unwrap();
        assert_eq!(std::fs::read(&backup).unwrap(), original);
        let once = mean_red(&lib.path("a.jpg"));
        assert!(
            once < 200.0,
            "rendered from the original, not from the edited file: {once}"
        );

        revert(&lib.root(), "a.jpg", MtimeOnSave::Keep).unwrap();
        assert_eq!(std::fs::read(lib.path("a.jpg")).unwrap(), original);
        assert_eq!(mtime(&lib.path("a.jpg")), before);
        assert!(
            !lib.root().join(".good-gallery").exists(),
            "empty backup folders are removed"
        );
        assert!(matches!(
            revert(&lib.root(), "a.jpg", MtimeOnSave::Keep),
            Err(EditError::NoOriginal)
        ));
    }

    /// The versions a file holds (`gg:EditHistory`).
    fn versions_of(lib: &Library, rel: &str) -> Vec<domain::versions::Version> {
        domain::versions::parse(
            lib.read(rel)
                .get("XMP-gg:EditHistory")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
    }

    /// REQ-021-24, REQ-021-26 (#247): saving over the original adds a dated version to the file's history, a copy starts
    /// its own, saving without a backup keeps none, and clearing keeps only the current one without touching the photo.
    #[test]
    fn req_021_24_req_021_26_saves_carry_the_versions_and_clearing_keeps_the_last() {
        let Some(lib) = library() else { return };
        jpeg_file(&lib, "a.jpg");
        save(&lib.tools, &lib.request("a.jpg", brighter(), false)).unwrap();
        let first = versions_of(&lib, "a.jpg");
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].recipe, brighter());
        assert!(first[0].saved_at.is_some_and(|t| t > 1_700_000_000), "dated");

        let mut darker = Recipe::default();
        Adjustment::Exposure.set(&mut darker, -1.0);
        let mut again = lib.request("a.jpg", darker.clone(), false);
        again.source = ".good-gallery/originals/a.jpg".into();
        again.history = first.clone();
        save(&lib.tools, &again).unwrap();
        let both = versions_of(&lib, "a.jpg");
        assert_eq!(both.len(), 2);
        assert_eq!((&both[0], &both[1].recipe), (&first[0], &darker));

        // A copy of it starts its own history.
        let mut copy = again.clone();
        copy.copy = true;
        let copy = save(&lib.tools, &copy).unwrap();
        assert_eq!(versions_of(&lib, &copy).len(), 1);

        let pixels = std::fs::read(lib.path("a.jpg")).unwrap();
        let before = mtime(&lib.path("a.jpg"));
        clear_history(&lib.tools, &lib.root(), "a.jpg", &both).unwrap();
        assert_eq!(versions_of(&lib, "a.jpg"), vec![both[1].clone()]);
        let tags = lib.read("a.jpg");
        assert_eq!(
            tags["XMP-gg:EditSource"], ".good-gallery/originals/a.jpg",
            "the edit stays"
        );
        assert_eq!(mtime(&lib.path("a.jpg")), before, "not a change of the photo");
        assert_eq!(
            image::open(lib.path("a.jpg")).unwrap().to_rgb8(),
            image::load_from_memory(&pixels).unwrap().to_rgb8()
        );

        // Without a backup the file keeps no edit, so no versions either (REQ-021-22).
        let mut without = lib.without_backup("a.jpg", darker);
        without.source = ".good-gallery/originals/a.jpg".into();
        without.history = both;
        save(&lib.tools, &without).unwrap();
        assert!(!lib.read("a.jpg").contains_key("XMP-gg:EditHistory"));
    }

    #[test]
    fn req_021_14_bakes_the_orientation_into_the_pixels() {
        let Some(lib) = library() else { return };
        jpeg_file(&lib, "turned.jpg");
        lib.tag("turned.jpg", &["-Orientation#=6"]);
        let written = save(&lib.tools, &lib.request("turned.jpg", brighter(), true)).unwrap();
        let turned = image::open(lib.path(&written)).unwrap();
        assert_eq!((turned.width(), turned.height()), (32, 64));
        assert_eq!(lib.read(&written)["IFD0:Orientation"], 1);
    }

    fn cropped_and_turned() -> Recipe {
        Recipe {
            crop: Some([0.5, 0.0, 0.5, 0.5]),
            quarter_turns: 1,
            ..Default::default()
        }
    }

    /// The saved file's metadata as the index would read it.
    fn indexed(lib: &Library, rel: &str) -> domain::media::MediaMetadata {
        let output = Command::new(&lib.tools.exiftool)
            .args(["-j", "-struct", "-n"])
            .arg(lib.path(rel))
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        meta::rules::extract_metadata(value[0].as_object().unwrap(), None)
    }

    /// REQ-029-06: only the cropped, turned pixels, upright by the tag.
    #[test]
    fn req_029_06_saves_the_cropped_turned_pixels() {
        let Some(lib) = library() else { return };
        jpeg_file(&lib, "a.jpg");
        let written = save(&lib.tools, &lib.request("a.jpg", cropped_and_turned(), true)).unwrap();
        let saved = image::open(lib.path(&written)).unwrap().into_rgb8();
        assert_eq!(saved.dimensions(), (16, 32));
        assert_eq!(lib.read(&written)["IFD0:Orientation"], 1);
        // The gradient's red grows to the right, its green downwards. Turned clockwise, the crop's top right (the
        // photo's) is the output's bottom right, and red now grows downwards.
        let (top, bottom) = (saved.get_pixel(8, 1), saved.get_pixel(8, 30));
        assert!(bottom[0] > top[0] + 80, "{top:?} {bottom:?}");
        assert!(top[0] > 110, "only the right half: {top:?}");
    }

    /// REQ-029-06: the turn works the same on a 16-bit PNG, which keeps its depth.
    #[test]
    fn req_029_06_keeps_16_bits() {
        let Some(lib) = library() else { return };
        let path = lib.path("deep.png");
        DynamicImage::ImageRgb16(image::ImageBuffer::from_fn(40, 20, |x, _| {
            image::Rgb([(x * 1600) as u16, 300, 300])
        }))
        .save(&path)
        .unwrap();
        let written = save(&lib.tools, &lib.request("deep.png", cropped_and_turned(), true)).unwrap();
        let saved = image::open(lib.path(&written)).unwrap();
        assert!(matches!(saved, DynamicImage::ImageRgb16(_)), "{:?}", saved.color());
        assert_eq!((saved.width(), saved.height()), (10, 20));
    }

    /// REQ-029-07: saving again starts from the whole original, and reverting brings it back.
    #[test]
    fn req_029_07_editing_again_and_reverting_bring_back_the_whole_photo() {
        let Some(lib) = library() else { return };
        jpeg_file(&lib, "a.jpg");
        let original = std::fs::read(lib.path("a.jpg")).unwrap();
        save(&lib.tools, &lib.request("a.jpg", cropped_and_turned(), false)).unwrap();
        assert_eq!(image::image_dimensions(lib.path("a.jpg")).unwrap(), (16, 32));

        let mut again = lib.request("a.jpg", Recipe::default(), false);
        again.source = backup_rel_path("a.jpg");
        again.recipe.quarter_turns = 1;
        save(&lib.tools, &again).unwrap();
        assert_eq!(
            image::image_dimensions(lib.path("a.jpg")).unwrap(),
            (32, 64),
            "the whole photo, turned"
        );

        revert(&lib.root(), "a.jpg", MtimeOnSave::Keep).unwrap();
        assert_eq!(std::fs::read(lib.path("a.jpg")).unwrap(), original);
    }

    const REGIONS: [&str; 2] = [
        "-XMP-mwg-rs:RegionInfo={AppliedToDimensions={W=64,H=32,Unit=pixel},RegionList=[\
         {Area={X=0.75,Y=0.25,W=0.2,H=0.2,Unit=normalized},Name=Anna,Type=Face},\
         {Area={X=0.25,Y=0.75,W=0.2,H=0.2,Unit=normalized},Name=Bob,Type=Face}]}",
        "-XMP-MP:RegionInfoMP={Regions=[{PersonDisplayName=Anna,Rectangle=0.65|, 0.15|, 0.2|, 0.2}]}",
    ];

    /// REQ-029-08: in the saved file the regions mark the same faces; who was cropped away keeps their People tag.
    #[test]
    fn req_029_08_saved_regions_mark_the_same_faces() {
        let Some(lib) = library() else { return };
        jpeg_file(&lib, "a.jpg");
        lib.tag("a.jpg", &REGIONS);
        let recipe = Recipe {
            crop: Some([0.5, 0.0, 0.5, 0.5]),
            ..Default::default()
        };
        let written = save(&lib.tools, &lib.request("a.jpg", recipe, true)).unwrap();
        let saved = indexed(&lib, &written);
        assert_eq!(saved.faces.len(), 1, "{:?}", saved.faces);
        let anna = &saved.faces[0];
        assert_eq!(anna.name, "Anna");
        assert!((anna.x - 0.3).abs() < 0.01 && (anna.y - 0.3).abs() < 0.01, "{anna:?}");
        assert!((anna.w - 0.4).abs() < 0.01 && (anna.h - 0.4).abs() < 0.01, "{anna:?}");
        let people: Vec<String> = saved.tags.iter().map(|tag| tag.join("|")).collect();
        assert!(people.iter().any(|tag| tag.contains("Bob")), "{people:?}");
        assert!(
            lib.read(&written)
                .keys()
                .all(|key| !key.contains("RegionInfoMP") && !key.contains("Regions"))
        );
        let names: Vec<String> = std::fs::read_dir(lib.root())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|name| !name.contains("gg-regions")), "{names:?}");
    }

    /// REQ-021-14: baking the orientation into the pixels moves the regions with them, also without a crop.
    #[test]
    fn req_021_14_baking_the_orientation_moves_the_regions() {
        let Some(lib) = library() else { return };
        jpeg_file(&lib, "turned.jpg");
        lib.tag("turned.jpg", &["-Orientation#=6", REGIONS[0]]);
        let written = save(&lib.tools, &lib.request("turned.jpg", brighter(), true)).unwrap();
        let faces = indexed(&lib, &written).faces;
        // Orientation 6: the stored top right (0.75, 0.25) is the upright bottom right.
        let anna = faces.iter().find(|face| face.name == "Anna").expect("Anna");
        assert!((anna.x + anna.w / 2.0 - 0.75).abs() < 0.01, "{anna:?}");
        assert!((anna.y + anna.h / 2.0 - 0.75).abs() < 0.01, "{anna:?}");
    }

    #[test]
    fn req_021_15_keeps_or_updates_the_date_modified() {
        let Some(lib) = library() else { return };
        jpeg_file(&lib, "a.jpg");
        let old = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_500_000_000);
        set_mtime(&lib.path("a.jpg"), old).unwrap();

        let kept = save(&lib.tools, &lib.request("a.jpg", brighter(), true)).unwrap();
        assert_eq!(mtime(&lib.path(&kept)), old);

        let mut update = lib.request("a.jpg", brighter(), false);
        update.mtime = MtimeOnSave::Update;
        save(&lib.tools, &update).unwrap();
        assert!(mtime(&lib.path("a.jpg")) > old + std::time::Duration::from_secs(86_400));
        assert_eq!(
            mtime(&lib.path(".good-gallery/originals/a.jpg")),
            old,
            "the backup keeps its date"
        );

        revert(&lib.root(), "a.jpg", MtimeOnSave::Update).unwrap();
        assert!(mtime(&lib.path("a.jpg")) > old + std::time::Duration::from_secs(86_400));
    }

    #[test]
    fn req_021_13_keeps_16_bits_the_color_profile_and_the_colors_of_unchanged_pixels() {
        let Some(lib) = library() else { return };
        // A 16-bit gradient with an embedded profile.
        let deep = image::ImageBuffer::<image::Rgb<u16>, _>::from_fn(300, 4, |x, _| {
            image::Rgb([(x * 218) as u16, 30_000, (65_535 - x * 218) as u16])
        });
        let mut png = Vec::new();
        let mut encoder = image::codecs::png::PngEncoder::new(&mut png);
        image::ImageEncoder::set_icc_profile(&mut encoder, icc_profile()).unwrap();
        DynamicImage::ImageRgb16(deep.clone())
            .write_with_encoder(encoder)
            .unwrap();
        std::fs::write(lib.path("deep.png"), &png).unwrap();

        let written = save(&lib.tools, &lib.request("deep.png", Recipe::default(), true)).unwrap();

        let saved = image::open(lib.path(&written)).unwrap();
        assert_eq!(saved.color(), image::ColorType::Rgb16);
        let saved = saved.into_rgb16();
        // Neutral: the same values (f32 round trips through linear light stay within one 16-bit level here).
        let worst = deep
            .pixels()
            .zip(saved.pixels())
            .flat_map(|(a, b)| (0..3).map(move |c| (i32::from(a[c]) - i32::from(b[c])).abs()))
            .max()
            .unwrap();
        assert!(worst <= 2, "{worst}");
        let mut decoder =
            image::codecs::png::PngDecoder::new(Cursor::new(std::fs::read(lib.path(&written)).unwrap())).unwrap();
        assert_eq!(decoder.icc_profile().unwrap(), Some(icc_profile()));

        // 8 bits: within one level, and the dither leaves unchanged values exactly as they were.
        DynamicImage::ImageRgb8(gradient(64, 32))
            .save(lib.path("flat.png"))
            .unwrap();
        let written = save(&lib.tools, &lib.request("flat.png", Recipe::default(), true)).unwrap();
        assert_eq!(image::open(lib.path(&written)).unwrap().into_rgb8(), gradient(64, 32));
    }

    /// REQ-022-05: a recipe with parts renders them by the subject handed over, or by the subject found again.
    #[test]
    fn req_022_05_saving_applies_the_parts_by_the_subject() {
        let mut recipe = Recipe::default();
        Adjustment::Exposure.set(recipe.part_mut(domain::edit::Part::Subject), 1.0);
        let params = Params::new(&recipe);
        // Handed over: the right half is the subject.
        let mut image = Rgba32FImage::from_pixel(4, 2, image::Rgba([0.4, 0.4, 0.4, 1.0]));
        let mask = Mask::new(2, 1, vec![0.0, 1.0]).unwrap();
        render(&params, &mut image, Some(&mask));
        assert!((image.get_pixel(0, 0)[0] - 0.4).abs() < 1e-4);
        assert!(image.get_pixel(3, 0)[0] > 0.5);

        // Found again: a disc on a plain wall.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("disc.png");
        let disc = image::RgbImage::from_fn(240, 180, |x, y| {
            let (dx, dy) = (x as f32 - 120.0, y as f32 - 90.0);
            if dx * dx + dy * dy < 45.0 * 45.0 {
                image::Rgb([200, 70, 40])
            } else {
                image::Rgb([60, 80, 100])
            }
        });
        disc.save(&path).unwrap();
        let encoded = render_file(&path, &recipe, None, Format::of(&path).unwrap(), None).unwrap();
        let saved = image::load_from_memory(&encoded).unwrap().into_rgb8();
        assert!(
            saved.get_pixel(120, 90)[1] > disc.get_pixel(120, 90)[1] + 20,
            "the subject is brighter"
        );
        assert_eq!(saved.get_pixel(3, 3), disc.get_pixel(3, 3), "the background stays");
    }

    /// REQ-030-14: as much painting as a photo can have fits its saved metadata, and comes back whole.
    #[test]
    fn req_030_14_a_full_painting_fits_the_saved_file() {
        let Some(lib) = library() else { return };
        // A wide spread of points, each with all four decimals.
        let points: Vec<[f32; 2]> = (0..domain::paint::MAX_POINTS)
            .map(|i| {
                let t = i as f32 / domain::paint::MAX_POINTS as f32;
                [0.1234 + t * 0.7531, 0.1357 + (t * 97.0).fract() * 0.7].map(|c| (c * 10_000.0).round() / 10_000.0)
            })
            .collect();
        let mut mask = domain::edit::LocalMask::new(domain::edit::MaskShape::Painted {
            layer: 0,
            freeform: false,
        });
        Adjustment::Exposure.set(&mut mask.recipe, 0.5);
        let recipe = Recipe {
            paintings: vec![domain::paint::Painting {
                strokes: vec![domain::paint::Stroke {
                    points,
                    size: 2.5,
                    softness: 50.0,
                    erase: false,
                    closed: false,
                }],
            }],
            masks: vec![mask],
            ..Recipe::default()
        };
        assert!(recipe.to_json().len() < 40_000, "{}", recipe.to_json().len());
        jpeg_file(&lib, "a.jpg");
        let written = save(&lib.tools, &lib.request("a.jpg", recipe.clone(), true)).unwrap();
        let tags = lib.read(&written);
        assert_eq!(
            Recipe::from_json(tags["XMP-gg:EditRecipe"].as_str().unwrap()),
            Some(recipe),
            "the whole painting comes back"
        );
    }

    /// REQ-030-08: saving applies the masks: the gradient's side changes, the other side doesn't.
    #[test]
    fn req_030_08_saving_applies_the_masks() {
        let mut mask = domain::edit::LocalMask::new(domain::edit::MaskShape::Linear {
            from: [0.0, 0.2],
            to: [0.0, 0.6],
        });
        Adjustment::Exposure.set(&mut mask.recipe, 1.0);
        let recipe = Recipe {
            masks: vec![mask],
            ..Recipe::default()
        };
        let mut image = Rgba32FImage::from_pixel(40, 40, image::Rgba([0.4, 0.4, 0.4, 1.0]));
        render(&Params::new(&recipe), &mut image, None);
        assert!(image.get_pixel(20, 2)[0] > 0.5, "{:?}", image.get_pixel(20, 2));
        assert!(
            (image.get_pixel(20, 38)[0] - 0.4).abs() < 1e-3,
            "{:?}",
            image.get_pixel(20, 38)
        );
    }

    /// REQ-028-08: saving lets a bright light glow red beside it, and leaves what's far from it alone.
    #[test]
    fn req_028_08_saving_lets_bright_lights_glow() {
        let (width, height) = (800, 600);
        let photo = || {
            Rgba32FImage::from_fn(width, height, |x, _| {
                let v = if (390..410).contains(&x) { 1.0 } else { 0.1 };
                image::Rgba([v, v, v, 1.0])
            })
        };
        let render_with = |recipe: &Recipe| {
            let mut image = photo();
            render(&Params::new(recipe), &mut image, None);
            image
        };
        let look = domain::look::LookChoice {
            strength: 100.0,
            grain: 0.0,
            ..domain::look::LookChoice::new(domain::look::Look::Gold200)
        };
        let with = render_with(&Recipe {
            look: Some(look),
            ..Recipe::default()
        });
        let mut without = Params::new(&Recipe {
            look: Some(look),
            ..Recipe::default()
        });
        without.halation = 0.0;
        let mut plain = photo();
        render(&without, &mut plain, None);
        let (beside, far) = ((387, 300), (40, 300));
        let red = |image: &Rgba32FImage, (x, y): (u32, u32)| image.get_pixel(x, y).0;
        assert!(red(&with, beside)[0] > red(&plain, beside)[0] + 0.01);
        assert!((red(&with, far)[0] - red(&plain, far)[0]).abs() < 1e-4);
    }

    /// REQ-023-04: saving blurs from the photo's pyramid: away from the focus a hard edge softens, at the focus it
    /// stays.
    #[test]
    fn req_023_04_saving_blurs_away_from_the_focus() {
        let edge = |x: u32| if x < 128 { 0.1 } else { 0.9 };
        let mut image = Rgba32FImage::from_fn(256, 256, |x, _| image::Rgba([edge(x), edge(x), edge(x), 1.0]));
        let recipe = Recipe {
            blur_kind: domain::edit::BlurKind::TiltShift,
            blur_focus: [0.5, 0.1],
            blur: 100.0,
            ..Recipe::default()
        };
        render(&Params::new(&recipe), &mut image, None);
        // In the focus: still hard. Far from it: in between next to the edge, but flat far from it.
        assert_eq!(image.get_pixel(126, 25)[0], 0.1);
        let soft = image.get_pixel(126, 240)[0];
        assert!(soft > 0.15 && soft < 0.5, "{soft}");
        assert!((image.get_pixel(20, 240)[0] - 0.1).abs() < 1e-4);
    }

    /// REQ-023-05: saving with a lens blur spreads a small light into a disc, which a soft blur leaves faint.
    #[test]
    fn req_023_05_saving_spreads_a_light_into_a_disc() {
        let light = |x: u32, y: u32| (127..=129).contains(&x) && (127..=129).contains(&y);
        let photo = || {
            Rgba32FImage::from_fn(256, 256, |x, y| {
                let v = if light(x, y) { 1.0 } else { 0.1 };
                image::Rgba([v, v, v, 1.0])
            })
        };
        let saved = |lens: bool| {
            let recipe = Recipe {
                blur_kind: domain::edit::BlurKind::Radial,
                blur_focus: [0.0, 0.0],
                blur_size: 0.0,
                blur: 100.0,
                blur_lens: lens,
                ..Recipe::default()
            };
            let mut image = photo();
            render(&Params::new(&recipe), &mut image, None);
            image
        };
        let (lens, soft) = (saved(true), saved(false));
        // A quarter of the disc's radius from the light, and three quarters.
        let radius = 0.015 * 256.0 * domain::edit::LENS_RADIUS_PER_SIGMA;
        for at in [0.25, 0.75] {
            let x = 128 + (radius * at) as u32;
            let (with, without) = (lens.get_pixel(x, 128)[0] - 0.1, soft.get_pixel(x, 128)[0] - 0.1);
            assert!(with > 0.01 && with > 3.0 * without, "{at}: lens {with}, soft {without}");
        }
        assert!((lens.get_pixel(20, 20)[0] - 0.1).abs() < 1e-3, "flat stays flat");
    }

    #[test]
    fn req_021_13_smooth_gradients_dont_band_after_an_edit() {
        // A dark 16-bit ramp brightened a lot: without dithering, 8-bit output would show long flat steps.
        let ramp = Rgba32FImage::from_fn(1024, 8, |x, _| image::Rgba([0.02 + x as f32 / 1024.0 * 0.06; 4]));
        let mut decoded = Decoded {
            image: ramp,
            alpha: false,
            sixteen_bit: false,
            lossless_webp: false,
        };
        let mut recipe = Recipe::default();
        Adjustment::Exposure.set(&mut recipe, 3.0);
        render(&Params::new(&recipe), &mut decoded.image, None);
        let image = quantize8(&decoded.image, false).into_rgb8();
        // Along a row, neighbouring rows differ: the steps are broken up.
        let differing = (0..1024)
            .filter(|&x| image.get_pixel(x, 0)[0] != image.get_pixel(x, 1)[0])
            .count();
        assert!(differing > 100, "{differing}");
    }

    #[test]
    fn req_021_16_a_failed_save_leaves_the_original_and_no_temporary_file() {
        let Some(lib) = library() else { return };
        jpeg_file(&lib, "a.jpg");
        let original = std::fs::read(lib.path("a.jpg")).unwrap();
        // exiftool can't run, after the backup was made.
        let failing = Tools {
            exiftool: lib.dir.path().join("no-exiftool"),
            config: lib.tools.config.clone(),
        };
        assert!(save(&failing, &lib.request("a.jpg", brighter(), false)).is_err());
        assert_eq!(std::fs::read(lib.path("a.jpg")).unwrap(), original);
        let names: Vec<_> = std::fs::read_dir(lib.root())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["a.jpg"], "no temporary file, no backup left");
        // A missing original and an unsupported format fail before anything is written.
        let mut gone = lib.request("a.jpg", brighter(), true);
        gone.source = "gone.jpg".into();
        assert!(matches!(save(&lib.tools, &gone), Err(EditError::NoOriginal)));
        std::fs::write(lib.path("b.gif"), b"GIF89a").unwrap();
        assert!(matches!(
            save(&lib.tools, &lib.request("b.gif", brighter(), true)),
            Err(EditError::Unsupported)
        ));
        assert!(matches!(
            save(&lib.tools, &lib.request("../a.jpg", brighter(), true)),
            Err(EditError::UnsafePath)
        ));
    }

    /// REQ-021-16: a file system that loses the saved file while it says it renamed it into place (#140) leaves the
    /// original, without a backup or temporary files.
    #[test]
    fn req_021_16_a_lost_replacement_leaves_the_original() {
        let Some(lib) = library() else { return };
        jpeg_file(&lib, "a.jpg");
        let original = std::fs::read(lib.path("a.jpg")).unwrap();

        let saved =
            crate::replace::tests::losing_renames(|| save(&lib.tools, &lib.request("a.jpg", brighter(), false)));
        assert!(saved.is_err());
        assert_eq!(std::fs::read(lib.path("a.jpg")).unwrap(), original);
        let names: Vec<_> = std::fs::read_dir(lib.root())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["a.jpg"], "no temporary file, no backup left");

        // Saved again from its backup, only the edited file is renamed into place.
        save(&lib.tools, &lib.request("a.jpg", brighter(), false)).unwrap();
        let edited = std::fs::read(lib.path("a.jpg")).unwrap();
        let mut again = lib.request("a.jpg", brighter(), false);
        again.source = ".good-gallery/originals/a.jpg".into();
        let saved = crate::replace::tests::losing_renames(|| save(&lib.tools, &again));
        assert!(
            matches!(saved, Err(EditError::Replace(crate::replace::Error::Lost))),
            "{saved:?}"
        );
        assert_eq!(std::fs::read(lib.path("a.jpg")).unwrap(), edited);
        assert_eq!(
            std::fs::read(lib.path(".good-gallery/originals/a.jpg")).unwrap(),
            original
        );
        let mut names: Vec<_> = std::fs::read_dir(lib.root())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, [".good-gallery", "a.jpg"], "no temporary file");
    }

    /// REQ-021-16: a revert whose file is lost keeps the edited file and the backup.
    #[test]
    fn req_021_16_reverting_never_loses_the_backup() {
        let Some(lib) = library() else { return };
        jpeg_file(&lib, "a.jpg");
        let original = std::fs::read(lib.path("a.jpg")).unwrap();
        save(&lib.tools, &lib.request("a.jpg", brighter(), false)).unwrap();
        let edited = std::fs::read(lib.path("a.jpg")).unwrap();

        let reverted = crate::replace::tests::losing_renames(|| revert(&lib.root(), "a.jpg", MtimeOnSave::Keep));
        assert!(reverted.is_err());
        assert_eq!(std::fs::read(lib.path("a.jpg")).unwrap(), edited);
        assert_eq!(
            std::fs::read(lib.path(".good-gallery/originals/a.jpg")).unwrap(),
            original
        );

        revert(&lib.root(), "a.jpg", MtimeOnSave::Keep).unwrap();
        assert_eq!(std::fs::read(lib.path("a.jpg")).unwrap(), original);
    }

    #[test]
    fn req_021_16_a_read_only_folder_fails_and_changes_nothing() {
        let Some(lib) = library() else { return };
        jpeg_file(&lib, "locked/a.jpg");
        let original = std::fs::read(lib.path("locked/a.jpg")).unwrap();
        let folder = lib.path("locked");
        let mut permissions = std::fs::metadata(&folder).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&folder, permissions.clone()).unwrap();
        // Root ignores permissions; nothing to check then.
        if std::fs::write(folder.join("probe"), b"").is_ok() {
            return;
        }
        let result = save(&lib.tools, &lib.request("locked/a.jpg", brighter(), true));
        #[expect(
            clippy::permissions_set_readonly_false,
            reason = "undo for the temporary folder's cleanup"
        )]
        permissions.set_readonly(false);
        std::fs::set_permissions(&folder, permissions).unwrap();
        assert!(result.is_err());
        assert_eq!(std::fs::read(lib.path("locked/a.jpg")).unwrap(), original);
    }

    #[test]
    fn req_021_13_keeps_lossless_webp_lossless() {
        let image = DynamicImage::ImageRgb8(gradient(16, 16));
        let rgb = image.to_rgb8();
        let lossless = webp::Encoder::from_rgb(rgb.as_raw(), 16, 16).encode_lossless().to_vec();
        let lossy = webp::Encoder::from_rgb(rgb.as_raw(), 16, 16).encode(80.0).to_vec();
        assert!(is_lossless_webp(&lossless));
        assert!(!is_lossless_webp(&lossy));
        assert!(!is_lossless_webp(b"RIFF"));
        let decoded = decode(&lossless, None).unwrap();
        let encoded = encode(&decoded, Format::Webp, domain::edit::DEFAULT_QUALITY).unwrap();
        assert!(is_lossless_webp(&encoded));
        assert_eq!(image::load_from_memory(&encoded).unwrap().into_rgb8(), rgb);
    }

    /// A minimal ICC profile: a header without tags.
    fn icc_profile() -> Vec<u8> {
        let mut profile = vec![0u8; 132];
        profile[0..4].copy_from_slice(&132u32.to_be_bytes());
        profile[8] = 2;
        profile[12..16].copy_from_slice(b"mntr");
        profile[16..20].copy_from_slice(b"RGB ");
        profile[20..24].copy_from_slice(b"XYZ ");
        profile[36..40].copy_from_slice(b"acsp");
        profile
    }
}
