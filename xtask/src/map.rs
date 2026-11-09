//! `cargo xtask map [--from <dir>]`: builds `crates/app/data/world.deflate`, the world map the app draws (spec 044),
//! from Natural Earth's 1:50m land and country borders (public domain, <https://www.naturalearthdata.com>). The land
//! is triangulated here, once, as egui fills only convex shapes. Downloads the GeoJSON with `curl`, or reads
//! `ne_50m_land.geojson` and `ne_50m_admin_0_boundary_lines_land.geojson` from `--from`.
//!
//! The file, deflated, little-endian: `GGMAP1`; the number of land polygons, each with its bounds (west, south, east,
//! north as f32), its vertices (count, then longitude and latitude as f32), its triangles (count of indices, then u32
//! indices into its vertices) and its rings (count, then each ring's vertex count; the rings follow each other in the
//! vertices, outer ring first); then the number of border lines, each with its points (count, then f32 pairs).

use anyhow::{Context as _, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "usage: cargo xtask map [--from <dir>]";
const BASE: &str = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/";
const LAND: &str = "ne_50m_land.geojson";
const BORDERS: &str = "ne_50m_admin_0_boundary_lines_land.geojson";

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let dir = match args {
        [] => {
            for file in [LAND, BORDERS] {
                let status = Command::new("curl")
                    .args(["--fail", "--silent", "--show-error", "--location", "--output"])
                    .arg(temp.path().join(file))
                    .arg(format!("{BASE}{file}"))
                    .status()
                    .context("can't run curl")?;
                if !status.success() {
                    bail!("downloading {file} failed");
                }
            }
            temp.path().to_path_buf()
        }
        [flag, dir] if flag == "--from" => PathBuf::from(dir),
        _ => bail!(USAGE),
    };
    let bytes = world(&dir)?;
    let compressed = miniz_oxide::deflate::compress_to_vec(&bytes, 10);
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/app/data/world.deflate");
    std::fs::create_dir_all(out.parent().context("no data folder")?)?;
    std::fs::write(&out, &compressed)?;
    println!("{}: {} KB", out.display(), compressed.len() / 1024);
    Ok(())
}

fn read(path: &Path) -> anyhow::Result<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

/// The geometries of a FeatureCollection.
fn geometries(collection: &Value) -> impl Iterator<Item = &Value> {
    collection["features"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|feature| &feature["geometry"])
}

fn points(ring: &Value) -> Vec<[f32; 2]> {
    ring.as_array()
        .into_iter()
        .flatten()
        .filter_map(|point| Some([point[0].as_f64()? as f32, point[1].as_f64()? as f32]))
        .collect()
}

fn world(dir: &Path) -> anyhow::Result<Vec<u8>> {
    let mut out = b"GGMAP1".to_vec();
    let push_u32 = |out: &mut Vec<u8>, n: usize| out.extend((n as u32).to_le_bytes());
    let push_f32 = |out: &mut Vec<u8>, x: f32| out.extend(x.to_le_bytes());

    let land = read(&dir.join(LAND))?;
    let mut polygons: Vec<Vec<Vec<[f32; 2]>>> = Vec::new();
    for geometry in geometries(&land) {
        let coordinates = &geometry["coordinates"];
        match geometry["type"].as_str() {
            Some("Polygon") => polygons.push(coordinates.as_array().into_iter().flatten().map(points).collect()),
            Some("MultiPolygon") => {
                for polygon in coordinates.as_array().into_iter().flatten() {
                    polygons.push(polygon.as_array().into_iter().flatten().map(points).collect());
                }
            }
            _ => {}
        }
    }
    push_u32(&mut out, polygons.len());
    let mut earcut = earcut::Earcut::new();
    for rings in &polygons {
        // GeoJSON closes each ring with its first point again.
        let rings: Vec<&[[f32; 2]]> = rings.iter().map(|ring| &ring[..ring.len().saturating_sub(1)]).collect();
        let vertices: Vec<[f32; 2]> = rings.iter().flat_map(|ring| ring.iter().copied()).collect();
        let mut holes = Vec::new();
        let mut start = 0;
        for ring in &rings {
            if start > 0 {
                holes.push(start as u32);
            }
            start += ring.len();
        }
        let mut triangles: Vec<u32> = Vec::new();
        // On the projected map, where they are drawn: triangles of longitude and latitude bend when projected and
        // overlap near the poles (#297).
        earcut.earcut(
            vertices
                .iter()
                .map(|[x, y]| domain::map::project(f64::from(*x), f64::from(*y))),
            &holes,
            &mut triangles,
        );
        let (mut west, mut south, mut east, mut north) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for [x, y] in &vertices {
            (west, south, east, north) = (west.min(*x), south.min(*y), east.max(*x), north.max(*y));
        }
        for bound in [west, south, east, north] {
            push_f32(&mut out, bound);
        }
        push_u32(&mut out, vertices.len());
        for [x, y] in &vertices {
            push_f32(&mut out, *x);
            push_f32(&mut out, *y);
        }
        push_u32(&mut out, triangles.len());
        for index in triangles {
            push_u32(&mut out, index as usize);
        }
        push_u32(&mut out, rings.len());
        for ring in &rings {
            push_u32(&mut out, ring.len());
        }
    }

    let borders = read(&dir.join(BORDERS))?;
    let mut lines: Vec<Vec<[f32; 2]>> = Vec::new();
    for geometry in geometries(&borders) {
        let coordinates = &geometry["coordinates"];
        match geometry["type"].as_str() {
            Some("LineString") => lines.push(points(coordinates)),
            Some("MultiLineString") => lines.extend(coordinates.as_array().into_iter().flatten().map(points)),
            _ => {}
        }
    }
    push_u32(&mut out, lines.len());
    for line in &lines {
        push_u32(&mut out, line.len());
        for [x, y] in line {
            push_f32(&mut out, *x);
            push_f32(&mut out, *y);
        }
    }
    Ok(out)
}
