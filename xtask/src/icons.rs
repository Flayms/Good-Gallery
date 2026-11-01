//! `cargo xtask icons [--lucide <folder>]`: writes the app's icons from their sources.
//!
//! - The lucide icons named in `crates/app/src/icons.rs` (`icons!`) become `crates/app/assets/icons/<name>.svg`: lucide's
//!   shapes with `stroke="#fff"` and `stroke-width="1.75"` (`docs/design.md`), so egui can tint them. `<name>-filled`
//!   is `<name>` with `fill="#fff"`. The shapes come from lucide-static [`LUCIDE`] on npm (downloaded with `curl` and
//!   unpacked with `tar`), or from an unpacked lucide-static package at `--lucide`; its license goes beside the icons.
//! - `crates/app/assets/app-icon.svg` becomes `app-icon.png` (256 px, the window icon) and `app-icon.ico` (16–256 px
//!   PNG entries, the executable's and the installer's icon).
//!
//! On an unchanged checkout it writes the same bytes again.

use anyhow::{Context as _, bail};
use image::ImageEncoder as _;
use serde::de::{Deserializer, MapAccess, Visitor};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The lucide-static version the icons come from.
const LUCIDE: &str = "1.40.0";
/// The sizes in `app-icon.ico`.
const ICO_SIZES: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 128, 256];
/// The size of `app-icon.png`.
const PNG_SIZE: u32 = 256;
const USAGE: &str = "usage: cargo xtask icons [--lucide <unpacked lucide-static package>]";

/// An element's attributes in the order lucide lists them (a JSON object, whose order `serde_json` doesn't keep).
#[derive(Debug, PartialEq)]
struct Attributes(Vec<(String, String)>);

impl<'de> serde::Deserialize<'de> for Attributes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct InOrder;
        impl<'de> Visitor<'de> for InOrder {
            type Value = Attributes;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an object of attributes")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Attributes, A::Error> {
                let mut attributes = Vec::new();
                while let Some(entry) = map.next_entry::<String, String>()? {
                    attributes.push(entry);
                }
                Ok(Attributes(attributes))
            }
        }
        deserializer.deserialize_map(InOrder)
    }
}

/// lucide's `icon-nodes.json`: per icon, its elements as `[tag, attributes]`.
type IconNodes = BTreeMap<String, Vec<(String, Attributes)>>;

/// The SVG file of `name` from lucide's `nodes`.
fn icon_svg(nodes: &IconNodes, name: &str) -> anyhow::Result<String> {
    let (base, fill) = match name.strip_suffix("-filled") {
        Some(base) => (base, "#fff"),
        None => (name, "none"),
    };
    let elements = nodes
        .get(base)
        .with_context(|| format!("lucide {LUCIDE} has no icon `{base}`"))?;
    let mut svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="{fill}" stroke="#fff" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">"##
    );
    for (tag, Attributes(attributes)) in elements {
        svg.push('<');
        svg.push_str(tag);
        for (key, value) in attributes.iter().filter(|(key, _)| key != "key") {
            svg.push_str(&format!(r#" {key}="{value}""#));
        }
        svg.push_str("/>");
    }
    svg.push_str("</svg>\n");
    Ok(svg)
}

/// The file names in `icons!` of `crates/app/src/icons.rs` (`Variant => "name",`).
fn icon_names(source: &str) -> Vec<String> {
    let line = regex::Regex::new(r#"^\s*[A-Z]\w* => "([a-z0-9-]+)",\s*$"#).expect("valid regex");
    source
        .lines()
        .filter_map(|text| line.captures(text).map(|captures| captures[1].to_owned()))
        .collect()
}

/// `svg` drawn `size` pixels square, as PNG.
fn render_png(tree: &resvg::usvg::Tree, size: u32) -> anyhow::Result<Vec<u8>> {
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).context("empty icon size")?;
    let scale = size as f32 / tree.size().width().max(tree.size().height());
    resvg::render(
        tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia keeps premultiplied alpha; PNG wants it straight.
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect::<Vec<u8>>();
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new_with_quality(
        &mut png,
        image::codecs::png::CompressionType::Best,
        image::codecs::png::FilterType::Adaptive,
    )
    .write_image(&pixels, size, size, image::ExtendedColorType::Rgba8)
    .context("can't encode the icon as PNG")?;
    Ok(png)
}

/// An ICO file of `pngs` (size, PNG bytes): the header, one directory entry per image, then the PNG files.
fn ico(pngs: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // an icon
    out.extend_from_slice(&(pngs.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * pngs.len() as u32;
    for (size, png) in pngs {
        // 0 means 256.
        out.extend_from_slice(&[(size % 256) as u8, (size % 256) as u8, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // color planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in pngs {
        out.extend_from_slice(png);
    }
    out
}

/// `app-icon.png` and `app-icon.ico` from `app-icon.svg`.
fn app_icon(svg: &[u8]) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).context("app-icon.svg")?;
    let pngs = ICO_SIZES
        .iter()
        .map(|&size| Ok((size, render_png(&tree, size)?)))
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok((render_png(&tree, PNG_SIZE)?, ico(&pngs)))
}

/// Downloads and unpacks lucide-static [`LUCIDE`] into `dir`; the package's folder.
fn download_lucide(dir: &Path) -> anyhow::Result<PathBuf> {
    let archive = dir.join("lucide-static.tgz");
    let url = format!("https://registry.npmjs.org/lucide-static/-/lucide-static-{LUCIDE}.tgz");
    println!("downloading {url}");
    crate::process::run_command(Command::new("curl").args(["-fsSL", "-o"]).arg(&archive).arg(&url))?;
    crate::process::run_command(
        Command::new("tar")
            .arg("-xzf")
            .arg(&archive)
            .arg("-C")
            .arg(dir)
            .args(["package/icon-nodes.json", "package/LICENSE"]),
    )?;
    Ok(dir.join("package"))
}

/// Writes `bytes` to `path`; whether that changed it.
fn write(path: &Path, bytes: &[u8]) -> anyhow::Result<bool> {
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(false);
    }
    std::fs::write(path, bytes).with_context(|| format!("can't write {}", path.display()))?;
    Ok(true)
}

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let lucide = match args {
        [] => None,
        [flag, dir] if flag == "--lucide" => Some(PathBuf::from(dir)),
        _ => bail!(USAGE),
    };
    let app = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/app");
    let assets = app.join("assets");
    let icons = assets.join("icons");
    let temp = tempfile::tempdir()?;
    let package = match lucide {
        Some(dir) => dir,
        None => download_lucide(temp.path())?,
    };
    let json = std::fs::read(package.join("icon-nodes.json")).context("lucide's icon-nodes.json")?;
    let nodes: IconNodes = serde_json::from_slice(&json).context("lucide's icon-nodes.json")?;

    let names = icon_names(&std::fs::read_to_string(app.join("src/icons.rs"))?);
    if names.is_empty() {
        bail!("no icons found in crates/app/src/icons.rs");
    }
    let mut changed = 0;
    for name in &names {
        changed += usize::from(write(
            &icons.join(format!("{name}.svg")),
            icon_svg(&nodes, name)?.as_bytes(),
        )?);
    }
    for entry in std::fs::read_dir(&icons)? {
        let path = entry?.path();
        let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
        if path.extension().is_some_and(|ext| ext == "svg") && !names.iter().any(|name| name == stem) {
            println!("removed {} (not in icons!)", path.display());
            std::fs::remove_file(&path)?;
            changed += 1;
        }
    }
    let license = std::fs::read(package.join("LICENSE")).context("lucide's LICENSE")?;
    changed += usize::from(write(&icons.join("LICENSE.lucide.txt"), &license)?);

    let (png, ico) = app_icon(&std::fs::read(assets.join("app-icon.svg"))?)?;
    changed += usize::from(write(&assets.join("app-icon.png"), &png)?);
    changed += usize::from(write(&assets.join("app-icon.ico"), &ico)?);
    println!(
        "{} lucide {LUCIDE} icons and the app icon: {changed} files changed",
        names.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes() -> IconNodes {
        serde_json::from_str(
            r#"{"star": [["path", {"d": "M1 2z", "key": "k1"}]],
                "video": [["path", {"d": "m16 13", "key": "a"}], ["rect", {"x": "2", "y": "6", "width": "14", "rx": "2", "key": "b"}]]}"#,
        )
        .unwrap()
    }

    #[test]
    fn writes_lucide_shapes_with_the_apps_stroke_in_lucides_attribute_order() {
        assert_eq!(
            icon_svg(&nodes(), "video").unwrap(),
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><path d="m16 13"/><rect x="2" y="6" width="14" rx="2"/></svg>
"##
        );
        assert!(
            icon_svg(&nodes(), "star-filled")
                .unwrap()
                .contains(r##"fill="#fff" stroke="#fff""##)
        );
        assert!(icon_svg(&nodes(), "missing").is_err());
    }

    #[test]
    fn reads_the_icon_names_of_the_app() {
        let source = "icons! {\n    ArrowDown => \"arrow-down\",\n    Trash2 => \"trash-2\",\n}\n// Star => \"x\"\n";
        assert_eq!(icon_names(source), ["arrow-down", "trash-2"]);
        let app = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/app");
        let names = icon_names(&std::fs::read_to_string(app.join("src/icons.rs")).unwrap());
        for name in &names {
            assert!(app.join(format!("assets/icons/{name}.svg")).exists(), "{name}");
        }
    }

    #[test]
    fn ico_lists_each_png_with_its_size_and_place() {
        let file = ico(&[(16, vec![1, 2, 3]), (256, vec![4, 5])]);
        assert_eq!(&file[..6], [0, 0, 1, 0, 2, 0]);
        // First entry: 16 px, 3 bytes at 6 + 2 × 16.
        assert_eq!(&file[6..22], [16, 16, 0, 0, 1, 0, 32, 0, 3, 0, 0, 0, 38, 0, 0, 0]);
        // Second: 256 px is 0, 2 bytes after the first.
        assert_eq!(&file[22..38], [0, 0, 0, 0, 1, 0, 32, 0, 2, 0, 0, 0, 41, 0, 0, 0]);
        assert_eq!(&file[38..], [1, 2, 3, 4, 5]);
    }

    /// The committed app icons are what `cargo xtask icons` renders from `app-icon.svg`.
    #[test]
    fn the_app_icons_are_rendered_from_the_svg() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/app/assets");
        let (png, ico) = app_icon(&std::fs::read(assets.join("app-icon.svg")).unwrap()).unwrap();
        assert!(
            std::fs::read(assets.join("app-icon.png")).unwrap() == png,
            "run `cargo xtask icons`"
        );
        assert!(
            std::fs::read(assets.join("app-icon.ico")).unwrap() == ico,
            "run `cargo xtask icons`"
        );
    }
}
