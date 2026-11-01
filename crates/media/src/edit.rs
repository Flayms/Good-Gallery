//! Saving and reverting edits (spec 021). A save renders the recipe from the unedited original at full resolution in
//! 32-bit float, encodes it in the original's format into a temporary file beside the target, copies the original's
//! metadata into it with exiftool (with the recipe in our own XMP namespace) and renames it into place. Replacing an
//! original first copies it into the library's backup folder; reverting moves the backup back.

use crate::thumbnail::{decode_error, limits};
use domain::edit::{Mask, MtimeOnSave, Params, Pyramid, Recipe, backup_rel_path, blur_lod, copy_name, dither};
use image::{DynamicImage, ImageReader, Rgba32FImage};
use std::io::{Cursor, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::SystemTime;

const JPEG_QUALITY: u8 = 95;
const WEBP_QUALITY: f32 = 95.0;

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
    pub mtime: MtimeOnSave,
    /// The subject as the preview showed it, for a recipe with parts (REQ-022-05); found again without one.
    pub subject: Option<Arc<Mask>>,
    /// Turns an original without an orientation of its own (REQ-004-12).
    pub sidecar_orientation: Option<u8>,
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
    let temp = folder.join(format!(".{}.gg-edit.{}", file_name(&target_rel), format.extension()));
    let mut made_backup = None;
    let written = (|| {
        let encoded = render_file(
            &source,
            &request.recipe,
            request.subject.as_deref(),
            format,
            request.sidecar_orientation,
        )?;
        write_synced(&temp, &encoded)?;
        // Replacing an unedited original: keep it first (REQ-021-10). A file already edited keeps its first backup;
        // a copy saved over has its source elsewhere.
        if !request.copy && source_rel == request.rel_path {
            source_rel = backup_rel_path(&request.rel_path);
            let backup = path(&source_rel)?;
            if !backup.exists() {
                make_backup(&file, &backup, &request.library)?;
                made_backup = Some(backup.clone());
            }
            source = backup;
        }
        copy_metadata(tools, &source, &temp, &request.recipe, &source_rel)?;
        if request.mtime == MtimeOnSave::Keep {
            set_mtime(&temp, original_mtime)?;
        }
        // Atomic: the old file or the new one is in place, never a partial one (REQ-021-16).
        std::fs::rename(&temp, &target)?;
        Ok::<_, EditError>(())
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temp);
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
    Ok(target_rel)
}

/// Puts the unedited original back in place of `rel_path` and removes its backup (REQ-021-12).
pub fn revert(library: &Path, rel_path: &str, mtime: MtimeOnSave) -> Result<(), EditError> {
    let file = db_path(library, rel_path).ok_or(EditError::UnsafePath)?;
    let backup = db_path(library, &backup_rel_path(rel_path)).ok_or(EditError::UnsafePath)?;
    if !backup.is_file() {
        return Err(EditError::NoOriginal);
    }
    if std::fs::rename(&backup, &file).is_err() {
        // Another volume (a mount inside the library): copy, then replace atomically.
        let temp = file.with_file_name(format!(".{}.gg-revert", file_name(rel_path)));
        let modified = std::fs::metadata(&backup)?.modified()?;
        let copied = std::fs::copy(&backup, &temp)
            .and_then(|_| std::fs::File::options().write(true).open(&temp)?.sync_all())
            .and_then(|()| set_mtime(&temp, modified))
            .and_then(|()| std::fs::rename(&temp, &file));
        if let Err(error) = copied {
            let _ = std::fs::remove_file(&temp);
            return Err(error.into());
        }
        std::fs::remove_file(&backup)?;
    }
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

/// Copies `file` to `backup` (complete and on disk before it counts, with its date modified), and hides the backup
/// folder on Windows.
fn make_backup(file: &Path, backup: &Path, library: &Path) -> std::io::Result<()> {
    let folder = backup.parent().expect("a backup has a folder");
    std::fs::create_dir_all(folder)?;
    hide(&library.join(domain::edit::BACKUP_DIR.split('/').next().unwrap_or_default()));
    let name = backup.file_name().unwrap_or_default().to_string_lossy();
    let temp = folder.join(format!(".{name}.gg-backup"));
    let modified = std::fs::metadata(file)?.modified()?;
    let copied = std::fs::copy(file, &temp)
        .and_then(|_| std::fs::File::options().write(true).open(&temp)?.sync_all())
        .and_then(|()| set_mtime(&temp, modified))
        .and_then(|()| {
            if std::fs::metadata(&temp)?.len() != std::fs::metadata(file)?.len() {
                return Err(std::io::Error::other("the backup is incomplete"));
            }
            std::fs::rename(&temp, backup)
        });
    if copied.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    copied
}

#[cfg(windows)]
fn hide(folder: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = Command::new("attrib")
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

    fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
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
    let pyramid = (params.blur > 0.0).then(|| {
        let pixels = image.pixels().map(|pixel| pixel.0).collect();
        Pyramid::new(width, height, pixels)
    });
    let pyramid = pyramid.as_ref();
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
                    if let Some(pyramid) = pyramid {
                        let lod = blur_lod(params.blur_sigma(uv, size, inside), pyramid.levels());
                        let sharp = [pixel[0], pixel[1], pixel[2], pixel[3]];
                        // Transparency stays sharp, as in the preview.
                        pixel[..3].copy_from_slice(&pyramid.blurred(sharp, uv, lod)[..3]);
                    }
                    let rgb = [pixel[0], pixel[1], pixel[2]];
                    let [r, g, b] = match subject {
                        Some(_) => params.pixel_masked(rgb, uv, size, inside),
                        None => params.pixel(rgb, uv, size),
                    };
                    pixel[..3].copy_from_slice(&[r, g, b]);
                }
            });
        }
    });
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
    render(&Params::new(recipe), &mut decoded.image, subject);
    encode(&decoded, format)
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

fn encode(decoded: &Decoded, format: Format) -> Result<Vec<u8>, EditError> {
    let encode_error = |error: image::ImageError| EditError::Encode(error.to_string());
    let mut out = Vec::new();
    match format {
        Format::Jpeg => {
            let image = quantize8(&decoded.image, false);
            image
                .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut out,
                    JPEG_QUALITY,
                ))
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
                .encode_simple(decoded.lossless_webp, WEBP_QUALITY)
                .map_err(|error| EditError::Encode(format!("{error:?}")))?;
            out.extend_from_slice(&encoded);
        }
    }
    Ok(out)
}

/// The arguments that copy `source`'s metadata into `target` and record the edit: everything but embedded previews
/// (they would show the unedited photo), the color profile, orientation 1 (the pixels are upright).
fn metadata_args(source: &Path, target: &Path, recipe: &Recipe, source_rel: &str) -> Vec<String> {
    let path = |path: &Path| path.to_string_lossy().into_owned();
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
        format!("-XMP-gg:EditRecipe={}", recipe.to_json()),
        format!("-XMP-gg:EditSource={source_rel}"),
        "-overwrite_original".into(),
        "-ignoreMinorErrors".into(),
        path(target),
    ]
}

/// Runs exiftool once, with the arguments on stdin (one per line, never through a shell: REQ-011-09).
fn copy_metadata(
    tools: &Tools,
    source: &Path,
    target: &Path,
    recipe: &Recipe,
    source_rel: &str,
) -> Result<(), EditError> {
    let args = metadata_args(source, target, recipe, source_rel);
    if args.iter().any(|arg| arg.contains(['\n', '\r'])) {
        return Err(EditError::Metadata("a file name contains a line break".into()));
    }
    let mut command = Command::new(&tools.exiftool);
    // `-config` must come first, before the argument file.
    command
        .arg("-config")
        .arg(&tools.config)
        .args(["-@", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = crate::helpers::spawn_helper(command).map_err(|error| EditError::Metadata(error.to_string()))?;
    let mut input = args.join("\n");
    input.push('\n');
    child
        .stdin
        .take()
        .expect("piped")
        .write_all(input.as_bytes())
        .map_err(|error| EditError::Metadata(error.to_string()))?;
    let output = child
        .wait_with_output()
        .map_err(|error| EditError::Metadata(error.to_string()))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() || stderr.lines().any(|line| line.starts_with("Error")) {
        return Err(EditError::Metadata(stderr.trim().to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::edit::Adjustment;
    use image::ImageDecoder as _;
    use serde_json::Value;

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
                mtime: MtimeOnSave::Keep,
                subject: None,
                sidecar_orientation: None,
            }
        }
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
        let encoded = encode(&decoded, Format::Webp).unwrap();
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
