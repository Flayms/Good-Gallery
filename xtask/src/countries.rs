//! `cargo xtask countries`: builds `crates/domain/data/countries.deflate`, every country's name in the languages
//! Unicode CLDR covers fully (Unicode License v3, <https://github.com/unicode-org/cldr-json>), so a place named from a
//! GPS position takes the name the user's own Places tags give its country (spec 042). Downloads them with `curl`.

use anyhow::{Context as _, bail};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

const BASE: &str = "https://raw.githubusercontent.com/unicode-org/cldr-json/main/cldr-json/";

fn fetch(path: &str) -> anyhow::Result<Value> {
    let output = Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--location"])
        .arg(format!("{BASE}{path}"))
        .output()
        .context("can't run curl")?;
    if !output.status.success() {
        bail!("downloading {path} failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub fn run(args: &[String]) -> anyhow::Result<()> {
    if !args.is_empty() {
        bail!("usage: cargo xtask countries");
    }
    let coverage = fetch("cldr-core/coverageLevels.json")?;
    // Languages, not their regional variants: those repeat the names.
    let languages: Vec<String> = coverage["effectiveCoverageLevels"]
        .as_object()
        .context("no coverage levels")?
        .iter()
        .filter(|(locale, level)| level.as_str() == Some("modern") && !locale.contains(['-', '_']) && *locale != "und")
        .map(|(locale, _)| locale.clone())
        .collect();
    let mut names: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for language in &languages {
        let file = fetch(&format!("cldr-localenames-full/main/{language}/territories.json"))?;
        let territories = file["main"][language.as_str()]["localeDisplayNames"]["territories"]
            .as_object()
            .with_context(|| format!("no territories for {language}"))?;
        for (key, name) in territories {
            // Countries by their two letters, with short and variant names ("US-alt-short"); not regions ("001").
            let code = key.split("-alt-").next().unwrap_or(key);
            if code.len() == 2
                && code.chars().all(|c| c.is_ascii_uppercase())
                && let Some(name) = name.as_str()
            {
                names.entry(code.to_owned()).or_default().insert(name.to_owned());
            }
        }
    }
    let mut text = String::from("# Unicode CLDR, Unicode License v3: country names in every fully covered language\n");
    for (code, names) in &names {
        text.push_str(code);
        for name in names {
            text.push('\t');
            text.push_str(name);
        }
        text.push('\n');
    }
    let compressed = miniz_oxide::deflate::compress_to_vec(text.as_bytes(), 10);
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/domain/data/countries.deflate");
    std::fs::write(&out, &compressed)?;
    println!(
        "{}: {} countries in {} languages, {} KB",
        out.display(),
        names.len(),
        languages.len(),
        compressed.len() / 1024
    );
    Ok(())
}
