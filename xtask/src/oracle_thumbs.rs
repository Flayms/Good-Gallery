//! The thumbnail part of `cargo xtask oracle`: renders a set of fixture images and a video with the reference's
//! `renderMedia` (sharp; `xtask/oracle/reference-thumbs.ts`) and with `media::thumbnail`, and compares each thumbnail
//! by size and pixels. Lossy WebP and different resamplers never match byte for byte, so pixels are compared within
//! a tolerance; placeholders by their average color.

use anyhow::{Context as _, bail};
use base64::Engine as _;
use domain::media::MediaKind;
use domain::thumbs::THUMB_WIDTHS;
use media::exiftool::ExifTool;
use media::thumbnail::Renderer;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

/// Mean absolute difference per channel (0–255) a thumbnail may have.
const MAX_MEAN_DIFFERENCE: f64 = 3.0;
/// The 99th percentile of per-channel differences: edges may move by a fraction of a pixel.
const MAX_P99_DIFFERENCE: u8 = 40;
/// Average colors of the placeholders, 0–1 per channel.
const MAX_PLACEHOLDER_DIFFERENCE: f32 = 0.03;

pub struct Item {
    name: &'static str,
    file: PathBuf,
    kind: MediaKind,
    duration: Option<f64>,
}

/// A photo-like pattern: smooth gradients, rings and hard-edged squares, so resampling differences show.
fn pattern(w: u32, h: u32) -> image::RgbImage {
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    image::RgbImage::from_fn(w, h, |x, y| {
        let ring = ((x as f32 - cx).hypot(y as f32 - cy) / 40.0).sin();
        let square = ((x / 64 + y / 64) % 2) as u8;
        image::Rgb([(x * 255 / w) as u8, (128.0 + 100.0 * ring) as u8, 40 + square * 160])
    })
}

fn ffmpeg_command(ffmpeg: &Path, args: &[&str], out: &Path) -> anyhow::Result<()> {
    let status = Command::new(ffmpeg)
        .args(["-v", "error", "-y"])
        .args(args)
        .arg(out)
        .status()?;
    if !status.success() {
        bail!("ffmpeg couldn't make {}", out.display());
    }
    Ok(())
}

/// Writes the thumbnail fixtures into `dir`.
pub fn generate(dir: &Path, exiftool: &Path, ffmpeg: &Path) -> anyhow::Result<Vec<Item>> {
    std::fs::create_dir_all(dir)?;
    let image = |name: &'static str| Item {
        name,
        file: dir.join(name),
        kind: MediaKind::Image,
        duration: None,
    };
    let jpeg = |name: &str, image: &image::RgbImage| -> anyhow::Result<()> {
        image.save_with_format(dir.join(name), image::ImageFormat::Jpeg)?;
        Ok(())
    };
    jpeg("photo.jpg", &pattern(3000, 2000))?;
    jpeg("portrait.jpg", &pattern(2000, 1500))?;
    let status = Command::new(exiftool)
        .args(["-q", "-overwrite_original", "-Orientation#=6"])
        .arg(dir.join("portrait.jpg"))
        .status()?;
    if !status.success() {
        bail!("exiftool couldn't tag portrait.jpg");
    }
    let mut truncated = Vec::new();
    pattern(1600, 1200).write_to(&mut std::io::Cursor::new(&mut truncated), image::ImageFormat::Jpeg)?;
    std::fs::write(dir.join("truncated.jpg"), &truncated[..truncated.len() * 6 / 10])?;
    let base = pattern(1200, 900);
    let alpha = image::RgbaImage::from_fn(1200, 900, |x, y| {
        let p = base.get_pixel(x, y).0;
        image::Rgba([p[0], p[1], p[2], (y * 255 / 900) as u8])
    });
    alpha.save(dir.join("alpha.png"))?;
    pattern(600, 4000).save(dir.join("strip.png"))?;
    pattern(300, 200).save(dir.join("small.png"))?;
    pattern(640, 480).save(dir.join("first-frame.gif"))?;
    pattern(1600, 1200).save(dir.join("photo.webp"))?;
    std::fs::write(dir.join("broken.jpg"), b"not a jpeg")?;
    pattern(1920, 1080).save(dir.join("source.png"))?;
    let source = dir.join("source.png").to_string_lossy().into_owned();
    ffmpeg_command(
        ffmpeg,
        &[
            "-i",
            &source,
            "-frames:v",
            "1",
            "-c:v",
            "libaom-av1",
            "-still-picture",
            "1",
        ],
        &dir.join("image.avif"),
    )?;
    ffmpeg_command(
        ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1280x720:rate=25",
            "-t",
            "10",
            "-pix_fmt",
            "yuv420p",
        ],
        &dir.join("clip.mp4"),
    )?;
    let mut items: Vec<Item> = [
        "photo.jpg",
        "portrait.jpg",
        "truncated.jpg",
        "alpha.png",
        "strip.png",
        "small.png",
        "first-frame.gif",
        "photo.webp",
        "broken.jpg",
        "image.avif",
    ]
    .into_iter()
    .map(image)
    .collect();
    items.push(Item {
        name: "clip.mp4",
        file: dir.join("clip.mp4"),
        kind: MediaKind::Video,
        duration: Some(10.0),
    });
    Ok(items)
}

fn render_with_reference(reference: &Path, work: &Path, items: &[Item], out: &Path) -> anyhow::Result<()> {
    let oracle = Path::new(env!("CARGO_MANIFEST_DIR")).join("oracle");
    let bundle = work.join("reference-thumbs.cjs");
    crate::oracle::run_command(
        Command::new("node")
            .arg(oracle.join("build-reference.mjs"))
            .arg(reference)
            .arg(&bundle)
            .arg("reference-thumbs.ts"),
    )
    .context("bundling the reference's thumbnail renderer")?;
    let list: Vec<serde_json::Value> = items
        .iter()
        .map(|item| {
            serde_json::json!({
                "name": item.name,
                "file": item.file,
                "kind": item.kind.as_str(),
                "duration": item.duration,
            })
        })
        .collect();
    let list_file = work.join("thumbs.json");
    std::fs::write(&list_file, serde_json::to_string(&list)?)?;
    crate::oracle::run_command(
        Command::new("node")
            .env("NODE_PATH", reference.join("node_modules"))
            .arg(&bundle)
            .arg(&list_file)
            .arg(out),
    )
}

fn render_with_ours(renderer: &Renderer, items: &[Item], out: &Path) -> anyhow::Result<()> {
    for item in items {
        match renderer.render(&item.file, item.kind, item.duration) {
            Ok(thumbnails) => {
                for (width, data) in &thumbnails.images {
                    std::fs::write(out.join(format!("{}-{width}.webp", item.name)), data)?;
                }
                std::fs::write(out.join(format!("{}.thumbhash", item.name)), &thumbnails.thumbhash)?;
            }
            Err(error) => std::fs::write(out.join(format!("{}.error", item.name)), error.to_string())?,
        }
    }
    Ok(())
}

struct PixelDifference {
    mean: f64,
    p99: u8,
}

/// Channels as they show: color premultiplied by alpha, since fully transparent pixels may keep any color (libwebp
/// drops it).
fn visible(image: &image::RgbaImage) -> Vec<u8> {
    image
        .pixels()
        .flat_map(|p| {
            let [r, g, b, a] = p.0;
            let premultiply = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
            [premultiply(r), premultiply(g), premultiply(b), a]
        })
        .collect()
}

fn pixel_difference(a: &image::RgbaImage, b: &image::RgbaImage) -> PixelDifference {
    let mut histogram = [0u64; 256];
    let mut sum = 0u64;
    for (x, y) in visible(a).iter().zip(&visible(b)) {
        let d = x.abs_diff(*y);
        histogram[d as usize] += 1;
        sum += u64::from(d);
    }
    let count = a.as_raw().len() as u64;
    let mut seen = 0;
    let mut p99 = 0;
    for (d, n) in histogram.iter().enumerate() {
        seen += n;
        if seen * 100 >= count * 99 {
            p99 = d as u8;
            break;
        }
    }
    PixelDifference {
        mean: sum as f64 / count as f64,
        p99,
    }
}

fn average_color(dir: &Path, name: &str) -> anyhow::Result<[f32; 4]> {
    let text = std::fs::read_to_string(dir.join(format!("{name}.thumbhash")))?;
    let hash = base64::engine::general_purpose::STANDARD.decode(text.trim())?;
    let (r, g, b, a) =
        thumbhash::thumb_hash_to_average_rgba(&hash).map_err(|()| anyhow::anyhow!("invalid ThumbHash"))?;
    Ok([r, g, b, a])
}

/// Compares what both renderers made of one item; the problems found, if any.
fn compare(reference: &Path, ours: &Path, item: &Item, report: &mut Vec<String>) -> anyhow::Result<Vec<String>> {
    let failed = |dir: &Path| dir.join(format!("{}.error", item.name)).exists();
    match (failed(reference), failed(ours)) {
        (true, true) => {
            report.push(format!("{:<16} both fail", item.name));
            return Ok(Vec::new());
        }
        (true, false) => return Ok(vec![format!("{}: only the reference fails", item.name)]),
        (false, true) => return Ok(vec![format!("{}: only ours fails", item.name)]),
        (false, false) => {}
    }
    let mut problems = Vec::new();
    for width in THUMB_WIDTHS {
        let file = format!("{}-{width}.webp", item.name);
        let a = image::open(reference.join(&file))?.into_rgba8();
        let b = image::open(ours.join(&file))?.into_rgba8();
        if a.dimensions() != b.dimensions() {
            problems.push(format!(
                "{file}: {:?} (reference) vs {:?}",
                a.dimensions(),
                b.dimensions()
            ));
            continue;
        }
        let difference = pixel_difference(&a, &b);
        report.push(format!(
            "{file:<22} {:>4} × {:<4} mean {:.2}, p99 {}",
            a.width(),
            a.height(),
            difference.mean,
            difference.p99
        ));
        if difference.mean > MAX_MEAN_DIFFERENCE || difference.p99 > MAX_P99_DIFFERENCE {
            problems.push(format!(
                "{file}: mean {:.2} (≤ {MAX_MEAN_DIFFERENCE}), p99 {} (≤ {MAX_P99_DIFFERENCE})",
                difference.mean, difference.p99
            ));
        }
    }
    let (a, b) = (average_color(reference, item.name)?, average_color(ours, item.name)?);
    let worst = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max);
    report.push(format!("{:<22} placeholder average color ±{worst:.3}", item.name));
    if worst > MAX_PLACEHOLDER_DIFFERENCE {
        problems.push(format!("{}: placeholder colors {a:?} vs {b:?}", item.name));
    }
    Ok(problems)
}

/// Renders and compares; false if a thumbnail is outside the tolerance.
pub fn run(reference: &Path, work: &Path, exiftool: &Path) -> anyhow::Result<bool> {
    let ffmpeg = media::ffmpeg::find_ffmpeg().context("ffmpeg not found (GOOD_GALLERY_FFMPEG or PATH)")?;
    let items = generate(&work.join("thumbs-library"), exiftool, &ffmpeg)?;
    let (reference_out, our_out) = (work.join("thumbs-reference"), work.join("thumbs-ours"));
    std::fs::create_dir_all(&reference_out)?;
    std::fs::create_dir_all(&our_out)?;
    render_with_reference(reference, work, &items, &reference_out)?;
    let renderer = Renderer::new(Arc::new(ExifTool::new(exiftool.to_path_buf(), 2)), Some(ffmpeg));
    render_with_ours(&renderer, &items, &our_out)?;

    let mut report = Vec::new();
    let mut problems = Vec::new();
    for item in &items {
        problems.extend(compare(&reference_out, &our_out, item, &mut report)?);
    }
    for line in &report {
        println!("thumbnail:  {line}");
    }
    for problem in &problems {
        println!("DIFFERENT:  {problem}");
    }
    println!(
        "{} thumbnail fixtures, {} outside the tolerance",
        items.len(),
        problems.len()
    );
    Ok(problems.is_empty())
}
