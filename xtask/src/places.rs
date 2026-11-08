//! `cargo xtask places [--from <dir>]`: builds `crates/domain/data/places.deflate`, the places the app names from GPS
//! positions (spec 042), from GeoNames' cities with at least 5,000 people, their regions and countries (CC BY 4.0,
//! <https://www.geonames.org>). Downloads them with `curl`, or reads `cities5000.zip`, `admin1CodesASCII.txt` and
//! `countryInfo.txt` from `--from`.

use anyhow::{Context as _, bail};
use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "usage: cargo xtask places [--from <dir>]";
const BASE: &str = "https://download.geonames.org/export/dump/";
const FILES: [&str; 3] = ["cities5000.zip", "admin1CodesASCII.txt", "countryInfo.txt"];

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let dir = match args {
        [] => {
            for file in FILES {
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
    let text = places_text(&dir)?;
    let compressed = miniz_oxide::deflate::compress_to_vec(text.as_bytes(), 10);
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/domain/data/places.deflate");
    std::fs::create_dir_all(out.parent().context("no data folder")?)?;
    std::fs::write(&out, &compressed)?;
    println!(
        "{}: {} places, {} KB",
        out.display(),
        text.lines().filter(|l| l.starts_with('P')).count(),
        compressed.len() / 1024
    );
    Ok(())
}

/// The places as `domain::places` reads them: countries (`C`), regions (`R`) and cities (`P`, latitude and longitude
/// in ten-thousandths of a degree), one per line, fields separated by tabs.
fn places_text(dir: &Path) -> anyhow::Result<String> {
    let mut countries = BTreeMap::new();
    for line in std::fs::read_to_string(dir.join("countryInfo.txt"))?.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if line.starts_with('#') || fields.len() < 5 {
            continue;
        }
        countries.insert(fields[0].to_owned(), fields[4].to_owned());
    }
    let mut regions = BTreeMap::new();
    for line in std::fs::read_to_string(dir.join("admin1CodesASCII.txt"))?.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() >= 2 {
            regions.insert(fields[0].to_owned(), fields[1].to_owned());
        }
    }
    let mut cities = String::new();
    std::fs::File::open(dir.join("cities5000.zip"))
        .map(zip::ZipArchive::new)??
        .by_name("cities5000.txt")?
        .read_to_string(&mut cities)?;
    let mut out = String::from("# GeoNames, CC BY 4.0, https://www.geonames.org: cities of 5,000 people or more\n");
    let mut used_countries = BTreeMap::new();
    let mut used_regions = BTreeMap::new();
    let mut places = Vec::new();
    for line in cities.lines() {
        // geonameid, name, asciiname, alternatenames, latitude, longitude, class, code, country, cc2, admin1, …
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 11 {
            continue;
        }
        let (Ok(latitude), Ok(longitude)) = (fields[4].parse::<f64>(), fields[5].parse::<f64>()) else {
            continue;
        };
        let country = fields[8];
        let Some(country_name) = countries.get(country) else {
            continue;
        };
        used_countries.insert(country.to_owned(), country_name.clone());
        let region = format!("{country}.{}", fields[10]);
        let region = match regions.get(&region) {
            Some(name) => {
                used_regions.insert(region.clone(), name.clone());
                region
            }
            None => String::new(),
        };
        places.push(format!(
            "P\t{}\t{}\t{country}\t{region}\t{}\n",
            (latitude * 1e4).round() as i32,
            (longitude * 1e4).round() as i32,
            fields[1].trim()
        ));
    }
    for (code, name) in used_countries {
        out.push_str(&format!("C\t{code}\t{name}\n"));
    }
    for (code, name) in used_regions {
        out.push_str(&format!("R\t{code}\t{name}\n"));
    }
    places.sort();
    out.extend(places);
    Ok(out)
}
