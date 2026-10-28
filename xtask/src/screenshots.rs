//! `cargo xtask screenshots [--keep <dir>]`: renders the README screenshots (`docs/screenshots/`) from a generated demo
//! library: 56 landscapes in four trips with keywords, People, Places, ratings and capture dates, plus two videos. The
//! library lies on a spare drive letter (`subst`), so no user folder shows in the viewer's info panel. Needs exiftool
//! and ffmpeg.

use anyhow::{Context as _, bail};
use image::{Rgb, RgbImage};
use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "usage: cargo xtask screenshots [--keep <dir>]";

#[derive(Clone, Copy)]
struct Palette {
    sky: [u32; 2],
    sun: u32,
    hills: [u32; 3],
    water: Option<u32>,
}

const fn palette(sky: [u32; 2], sun: u32, hills: [u32; 3], water: Option<u32>) -> Palette {
    Palette { sky, sun, hills, water }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Sunset,
    Dawn,
    Alpine,
    Forest,
    Night,
    Fjord,
    Desert,
    Lake,
}

impl Look {
    fn palette(self) -> Palette {
        match self {
            Self::Sunset => palette([0x1e1b4b, 0xf97316], 0xfde68a, [0x7c2d12, 0x431407, 0x1c0a04], None),
            Self::Dawn => palette([0x93c5fd, 0xfbcfe8], 0xfff7ed, [0xa78bfa, 0x6d28d9, 0x3b0764], None),
            Self::Alpine => palette([0x0ea5e9, 0xe0f2fe], 0xffffff, [0xcbd5e1, 0x64748b, 0x1e293b], None),
            Self::Forest => palette([0xbbf7d0, 0xfef9c3], 0xfffbeb, [0x4ade80, 0x15803d, 0x052e16], None),
            Self::Night => palette([0x020617, 0x1e3a8a], 0xe2e8f0, [0x1e293b, 0x0f172a, 0x020617], None),
            Self::Fjord => palette(
                [0x38bdf8, 0xf0f9ff],
                0xfefce8,
                [0x94a3b8, 0x334155, 0x0f172a],
                Some(0x0369a1),
            ),
            Self::Desert => palette([0xfb923c, 0xfef3c7], 0xfffbeb, [0xf59e0b, 0xb45309, 0x78350f], None),
            Self::Lake => palette(
                [0xa5b4fc, 0xfdf2f8],
                0xfff1f2,
                [0x818cf8, 0x4338ca, 0x1e1b4b],
                Some(0x6366f1),
            ),
        }
    }

    fn keywords(self) -> [&'static str; 2] {
        match self {
            Self::Sunset => ["Sunset", "Mountains"],
            Self::Dawn => ["Sunrise", "Mountains"],
            Self::Alpine => ["Mountains", "Snow"],
            Self::Forest => ["Forest", "Hiking"],
            Self::Night => ["Night", "Stars"],
            Self::Fjord => ["Fjord", "Water"],
            Self::Desert => ["Desert", "Dunes"],
            Self::Lake => ["Lake", "Water"],
        }
    }
}

struct Trip {
    folder: &'static str,
    place: &'static str,
    looks: [Look; 3],
    people: &'static [&'static str],
}

const TRIPS: [Trip; 4] = [
    Trip {
        folder: "Trips/2024 Lofoten",
        place: "Places|Norway|Lofoten",
        looks: [Look::Fjord, Look::Night, Look::Dawn],
        people: &["Mia"],
    },
    Trip {
        folder: "Trips/2025 Dolomites",
        place: "Places|Italy|Dolomites",
        looks: [Look::Alpine, Look::Sunset, Look::Dawn],
        people: &["Mia", "Jonas"],
    },
    Trip {
        folder: "Trips/2025 Atacama",
        place: "Places|Chile|Atacama",
        looks: [Look::Desert, Look::Night, Look::Sunset],
        people: &[],
    },
    Trip {
        folder: "Home/Black Forest",
        place: "Places|Germany|Black Forest",
        looks: [Look::Forest, Look::Lake, Look::Sunset],
        people: &["Jonas"],
    },
];

const RATIOS: [f64; 8] = [
    3.0 / 2.0,
    2.0 / 3.0,
    4.0 / 3.0,
    16.0 / 9.0,
    1.0,
    3.0 / 4.0,
    3.0 / 2.0,
    16.0 / 9.0,
];

struct Demo {
    file: String,
    look: Look,
    ratio: f64,
    /// exiftool's `YYYY:MM:DD hh:mm:ss`.
    date: String,
    places: &'static str,
    person: Option<&'static str>,
    rating: u8,
}

/// The demo library: 14 photos per trip, a few per day, so the gallery shows date groups.
fn demo_library() -> Vec<Demo> {
    let mut demos = Vec::new();
    let mut n = 0usize;
    for (t, trip) in TRIPS.iter().enumerate() {
        for i in 0..14 {
            let look = trip.looks[(i * 2 + i / 3) % trip.looks.len()];
            let person = (!trip.people.is_empty() && i % 4 == 0).then(|| trip.people[i % trip.people.len()]);
            demos.push(Demo {
                file: format!("{}/IMG_{:04}.jpg", trip.folder, 4100 + n * 7),
                look,
                ratio: RATIOS[(n * 5) % RATIOS.len()],
                date: format!(
                    "{}:0{}:{:02} {:02}:12:00",
                    2024 + t / 2,
                    (t % 2) * 3 + 4,
                    1 + i / 4,
                    7 + (i % 4) * 3
                ),
                places: trip.place,
                person,
                rating: [5, 3, 4, 0, 2][n % 5],
            });
            n += 1;
        }
    }
    demos
}

/// Deterministic pseudo-random numbers in 0..1, so every run renders the same library.
struct Random(u64);

impl Random {
    fn next(&mut self) -> f64 {
        self.0 = (self.0 * 1_664_525 + 1_013_904_223) % 4_294_967_296;
        self.0 as f64 / 4_294_967_296.0
    }
}

fn rgb(color: u32) -> [f64; 3] {
    [
        f64::from((color >> 16) as u8),
        f64::from((color >> 8) as u8),
        f64::from(color as u8),
    ]
}

fn mix(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t.clamp(0.0, 1.0))
}

fn pixel(color: [f64; 3]) -> Rgb<u8> {
    Rgb(color.map(|c| c.round().clamp(0.0, 255.0) as u8))
}

/// A drawn landscape: sky gradient, sun with a glow, three hill layers, water in some palettes.
fn landscape(palette: Palette, width: u32, height: u32, seed: u64) -> RgbImage {
    let mut random = Random(seed);
    let (w, h) = (f64::from(width), f64::from(height));
    let horizon = h * if palette.water.is_some() { 0.62 } else { 0.72 };
    // Each hill layer is a polyline from left to right; everything below it is filled.
    let hills: Vec<Vec<(f64, f64)>> = (0..palette.hills.len())
        .map(|layer| {
            let layer_f = layer as f64;
            let base = horizon - h * (0.28 - layer_f * 0.1);
            let steps = 6 + (random.next() * 6.0) as usize;
            (0..=steps)
                .map(|s| {
                    let y = base + (random.next() - 0.5) * h * (0.3 - layer_f * 0.07);
                    ((s as f64 / steps as f64 * w).round(), y.round())
                })
                .collect()
        })
        .collect();
    let sun_x = w * (0.2 + random.next() * 0.6);
    let sun_y = horizon * (0.25 + random.next() * 0.35);
    let sun_r = w.min(h) * 0.08;
    let (sky, sun) = ([rgb(palette.sky[0]), rgb(palette.sky[1])], rgb(palette.sun));
    let hill_colors = palette.hills.map(rgb);
    let ridge = |line: &[(f64, f64)], x: f64| {
        let i = line.windows(2).position(|p| x <= p[1].0).unwrap_or(line.len() - 2);
        let ((x0, y0), (x1, y1)) = (line[i], line[i + 1]);
        y0 + (y1 - y0) * ((x - x0) / (x1 - x0).max(1.0))
    };
    RgbImage::from_fn(width, height, |px, py| {
        let (x, y) = (f64::from(px) + 0.5, f64::from(py) + 0.5);
        let mut color = mix(sky[0], sky[1], y / h);
        let distance = ((x - sun_x).powi(2) + (y - sun_y).powi(2)).sqrt();
        let glow = 0.6 * (1.0 - distance / (sun_r * 4.0)).max(0.0);
        color = mix(color, sun, glow);
        if distance <= sun_r {
            color = sun;
        }
        for (line, hill) in hills.iter().zip(hill_colors) {
            if y >= ridge(line, x) {
                color = hill;
            }
        }
        if let Some(water) = palette.water
            && y >= horizon
        {
            let t = (y - horizon) / (h - horizon);
            let top = mix(color, rgb(water), 0.85);
            color = mix(top, hill_colors[2], t);
        }
        pixel(color)
    })
}

fn write_jpeg(image: &RgbImage, path: &Path) -> anyhow::Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(file, 85);
    encoder.encode_image(image)?;
    Ok(())
}

fn create_library(dir: &Path, exiftool: &Path, ffmpeg: &Path, scratch: &Path) -> anyhow::Result<()> {
    let mut args = Vec::new();
    for (index, demo) in demo_library().iter().enumerate() {
        let file = dir.join(&demo.file);
        std::fs::create_dir_all(file.parent().context("demo without folder")?)?;
        let long = 1800.0;
        let (width, height) = if demo.ratio >= 1.0 {
            (long, (long / demo.ratio).round())
        } else {
            ((long * demo.ratio).round(), long)
        };
        write_jpeg(
            &landscape(demo.look.palette(), width as u32, height as u32, index as u64 + 1),
            &file,
        )?;
        for keyword in demo.look.keywords().into_iter().chain(["Landscape"]) {
            args.push(format!("-XMP-dc:Subject={keyword}"));
        }
        args.push(format!("-XMP-lr:HierarchicalSubject={}", demo.places));
        if let Some(person) = demo.person {
            args.push(format!("-XMP-lr:HierarchicalSubject=People|{person}"));
        }
        args.push(format!("-XMP-xmp:Rating={}", demo.rating));
        args.push(format!("-EXIF:DateTimeOriginal={}", demo.date));
        args.push("-overwrite_original".to_owned());
        args.push(file.to_str().context("library path isn't UTF-8")?.to_owned());
        args.push("-execute".to_owned());
    }
    // One exiftool run for all photos; an argument file keeps the values intact.
    let arg_file = scratch.join("tags.args");
    std::fs::write(&arg_file, args.join("\n"))?;
    crate::process::run_command(
        Command::new(exiftool)
            .args(["-q", "-charset", "utf8", "-@"])
            .arg(&arg_file),
    )
    .context("tagging the demo photos")?;

    // A slow pan over a still landscape stands in for a video.
    let videos = [
        ("Trips/2024 Lofoten/VID_0007.mp4", Look::Fjord, "2024-04-03T09:00:00Z"),
        ("Trips/2025 Atacama/VID_0012.mp4", Look::Desert, "2025-04-03T18:00:00Z"),
    ];
    for (index, (file, look, date)) in videos.into_iter().enumerate() {
        let still = scratch.join(format!("still-{index}.png"));
        landscape(look.palette(), 1920, 1080, 90 + index as u64).save(&still)?;
        crate::process::run_command(
            Command::new(ffmpeg)
                .args(["-v", "error", "-y", "-loop", "1", "-i"])
                .arg(&still)
                .args(["-t", "4", "-r", "24", "-vf", "crop=1280:720:'(in_w-out_w)*t/4':0"])
                .args(["-pix_fmt", "yuv420p", "-metadata", &format!("creation_time={date}")])
                .arg(dir.join(file)),
        )
        .context("rendering a demo video")?;
    }
    Ok(())
}

/// A drive letter that isn't in use, mapped to `dir` until dropped.
struct Subst(String);

impl Subst {
    fn new(dir: &Path) -> anyhow::Result<Self> {
        let letter = ('P'..='Z')
            .map(|letter| format!("{letter}:"))
            .find(|drive| !Path::new(&format!("{drive}\\")).exists())
            .context("no free drive letter for subst")?;
        crate::process::run_command(Command::new("subst").arg(&letter).arg(dir))?;
        Ok(Self(letter))
    }
}

impl Drop for Subst {
    fn drop(&mut self) {
        let _ = Command::new("subst").arg(&self.0).arg("/D").status();
    }
}

/// Runs the app on `data` until it has saved `out`.
fn screenshot(app: &Path, data: &Path, out: &Path, extra: &[&str]) -> anyhow::Result<()> {
    crate::process::run_command(
        Command::new(app)
            .arg("--data-dir")
            .arg(data)
            .arg("--screenshot")
            .arg(out)
            .args(["--screenshot-after", "6"])
            .args(extra),
    )?;
    println!("{}", out.display());
    Ok(())
}

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let root = std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join(".."))?;
    let keep = match args {
        [] => None,
        [flag, dir] if flag == "--keep" => Some(PathBuf::from(dir)),
        _ => bail!(USAGE),
    };
    let exiftool = media::exiftool::find_exiftool().context("exiftool wasn't found")?;
    let ffmpeg = media::ffmpeg::find_ffmpeg().context("ffmpeg wasn't found")?;
    let temp = tempfile::tempdir()?;
    let work = keep.unwrap_or_else(|| temp.path().to_path_buf());
    let (drive_dir, data, scratch) = (work.join("drive"), work.join("data"), work.join("scratch"));
    for dir in [&drive_dir, &data, &scratch] {
        // A kept folder from an earlier run would add the library a second time.
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        std::fs::create_dir_all(dir)?;
    }
    let drive = Subst::new(&drive_dir)?;
    let library = format!("{}\\Photos", drive.0);
    create_library(Path::new(&library), &exiftool, &ffmpeg, &scratch)?;
    let index = data.join("gallery.db");
    crate::process::index_library(exiftool, Path::new(&library), &index, "Photos")?;

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    if !Command::new(cargo)
        .args(["build", "-p", "app"])
        .current_dir(&root)
        .status()?
        .success()
    {
        bail!("building the app failed");
    }
    let app = root.join(r"target\debug\app.exe");
    let out = root.join(r"docs\screenshots");
    std::fs::create_dir_all(&out)?;
    // A first run renders the thumbnails, so the screenshots don't catch placeholders.
    screenshot(&app, &data, &scratch.join("warm-up.png"), &[])?;
    let conn = db::open(&index)?;
    for view in ["masonry", "justified", "grid", "list"] {
        db::settings::set_value(&conn, "gallery.view", &serde_json::Value::from(view))?;
        screenshot(&app, &data, &out.join(format!("{view}.png")), &[])?;
    }
    db::settings::set_value(&conn, "gallery.view", &serde_json::Value::from("masonry"))?;
    db::settings::set_value(&conn, "viewer.filmstrip", &serde_json::Value::from(true))?;
    screenshot(&app, &data, &out.join("viewer.png"), &["--view", "3", "--info"])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_library_has_four_trips_of_photos() {
        let demos = demo_library();
        assert_eq!(demos.len(), 56);
        assert_eq!(demos[0].file, "Trips/2024 Lofoten/IMG_4100.jpg");
        assert_eq!(demos[0].date, "2024:04:01 07:12:00");
        assert_eq!(demos[0].person, Some("Mia"));
        assert_eq!(demos[55].file, "Home/Black Forest/IMG_4485.jpg");
        assert_eq!(demos[55].date, "2025:07:04 10:12:00");
        assert_eq!(demos.iter().filter(|demo| demo.rating == 5).count(), 12);
    }

    #[test]
    fn draws_sky_sun_and_hills() {
        let image = landscape(Look::Sunset.palette(), 300, 200, 1);
        // The top is the sky's first color, the bottom row the darkest hill.
        let top = image.get_pixel(0, 0).0;
        assert!(
            top.iter().zip([0x1e, 0x1b, 0x4b]).all(|(a, b)| a.abs_diff(b) < 8),
            "{top:?}"
        );
        assert_eq!(image.get_pixel(150, 199).0, [0x1c, 0x0a, 0x04]);
    }
}
