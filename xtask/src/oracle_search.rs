//! The search part of `cargo xtask oracle`: runs a list of searches, tag suggestions and the sidebar's queries with
//! the reference's own router (`xtask/oracle/reference-search.ts`) and with this app's queries, both on the reference's
//! index of the fixture library, so indexing differences can't show up here, and compares the results.

use anyhow::Context as _;
use chrono::NaiveDate;
use db::rusqlite::Connection;
use domain::media::MediaKind;
use domain::search::{GallerySearch, SortBy};
use domain::tags::TagCategory;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// The searches compared: every filter alone, in combination, every sort, unknown tags.
fn queries(library: i64) -> Vec<Value> {
    let base = json!({ "tags": [], "excludeTags": [], "ratings": [], "sortBy": "taken", "dir": "desc" });
    let with = |patch: Value| {
        let mut query = base.clone();
        for (key, value) in patch.as_object().expect("object") {
            query[key] = value.clone();
        }
        query
    };
    vec![
        base.clone(),
        with(json!({ "sortBy": "taken", "dir": "asc" })),
        with(json!({ "sortBy": "name", "dir": "asc" })),
        with(json!({ "sortBy": "name", "dir": "desc" })),
        with(json!({ "sortBy": "modified", "dir": "asc" })),
        with(json!({ "sortBy": "path", "dir": "asc" })),
        with(json!({ "sortBy": "size", "dir": "desc" })),
        with(json!({ "tags": ["places"] })),
        with(json!({ "tags": ["Rome"] })),
        with(json!({ "tags": ["people"] })),
        with(json!({ "tags": ["beach", " SUMMER "] })),
        with(json!({ "tags": ["café"] })),
        with(json!({ "excludeTags": ["places"] })),
        with(json!({ "tags": ["unknown tag"] })),
        with(json!({ "tags": ["beach"], "excludeTags": ["unknown tag"] })),
        with(json!({ "tags": ["places"], "excludeTags": ["rome"] })),
        with(json!({ "kind": "video" })),
        with(json!({ "kind": "image", "sortBy": "size", "dir": "asc" })),
        with(json!({ "name": "SIDECAR" })),
        with(json!({ "name": "%" })),
        with(json!({ "ratings": [0] })),
        with(json!({ "ratings": [4, 5] })),
        with(json!({ "from": "2021-01-01", "to": "2021-12-31" })),
        with(json!({ "from": "2021-07-15" })),
        with(json!({ "to": "2020-12-31", "dir": "asc" })),
        with(json!({ "rootId": library })),
        with(json!({ "rootId": library, "folder": "travel" })),
        with(json!({ "rootId": library, "folder": "2021" })),
        with(json!({ "tags": ["places"], "ratings": [5] })),
        with(json!({ "kind": "image", "name": "e", "ratings": [0, 2, 3], "sortBy": "name", "dir": "asc" })),
    ]
}

const SUGGEST: [&str; 6] = ["", "r", "ca", "NEW Y", "peo", "zzz"];

fn search_of(query: &Value) -> anyhow::Result<GallerySearch> {
    let strings = |key: &str| -> Vec<String> {
        query[key]
            .as_array()
            .map(|items| items.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    let day = |key: &str| -> anyhow::Result<Option<NaiveDate>> {
        query[key]
            .as_str()
            .map(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").context("day"))
            .transpose()
    };
    Ok(GallerySearch {
        library: query["rootId"].as_i64(),
        folder: query["folder"].as_str().map(String::from),
        kind: query["kind"].as_str().and_then(MediaKind::parse),
        name: query["name"].as_str().map(String::from),
        tags: strings("tags"),
        exclude: strings("excludeTags"),
        from: day("from")?,
        to: day("to")?,
        ratings: query["ratings"]
            .as_array()
            .map(|items| items.iter().filter_map(|v| v.as_u64().map(|r| r as u8)).collect())
            .unwrap_or_default(),
        sort_by: match query["sortBy"].as_str() {
            Some("name") => SortBy::Name,
            Some("modified") => SortBy::Modified,
            Some("path") => SortBy::Path,
            Some("size") => SortBy::Size,
            _ => SortBy::Taken,
        },
        descending: query["dir"] != "asc",
    })
}

/// This app's answers, in the reference script's shape.
fn ours(conn: &Connection, library: i64, queries: &[Value]) -> anyhow::Result<Value> {
    let mut stmt = conn.prepare("SELECT id, rel_path FROM media")?;
    let paths: HashMap<i64, String> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut stmt = conn.prepare("SELECT id, name FROM tags")?;
    let tag_names: HashMap<i64, String> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut layout = Vec::new();
    for query in queries {
        let ids = db::gallery::layout(conn, &search_of(query)?)?.ids;
        layout.push(ids.iter().map(|id| paths.get(id).cloned()).collect::<Vec<_>>());
    }
    let suggest: Vec<Value> = SUGGEST
        .iter()
        .map(|prefix| {
            Ok(json!(
                db::tags::autocomplete(conn, prefix, 10)?
                    .into_iter()
                    .map(|tag| json!([tag.name, tag.count]))
                    .collect::<Vec<_>>()
            ))
        })
        .collect::<anyhow::Result<_>>()?;
    let category = |category| -> anyhow::Result<Value> {
        Ok(json!(
            db::tags::category(conn, category)?
                .into_iter()
                .map(|tag| json!([tag.name, tag.parent_id.and_then(|id| tag_names.get(&id)), tag.count]))
                .collect::<Vec<_>>()
        ))
    };
    Ok(json!({
        "layout": layout,
        "suggest": suggest,
        "people": category(TagCategory::People)?,
        "places": category(TagCategory::Places)?,
        "popular": db::tags::popular(conn, 15)?.into_iter().map(|t| json!([t.name, t.count])).collect::<Vec<_>>(),
        "ratings": db::tags::rating_counts(conn)?.into_iter().map(|(r, n)| json!([r, n])).collect::<Vec<_>>(),
        "folders": db::tags::library_folders(conn, library)?.into_iter().map(|(p, n)| json!([p, n])).collect::<Vec<_>>(),
    }))
}

/// [`run`] on a copy of `index` in a temporary folder: the reference migrates the index it opens, and the user's
/// real index must never be opened in place.
pub fn run_on_copy(reference: &Path, index: &Path) -> anyhow::Result<bool> {
    let work = tempfile::tempdir()?;
    let copy = work.path().join("index.db");
    std::fs::copy(index, &copy).with_context(|| format!("can't copy {}", index.display()))?;
    for suffix in ["-wal", "-shm"] {
        let side = index.with_file_name(format!(
            "{}{suffix}",
            index.file_name().unwrap_or_default().to_string_lossy()
        ));
        if side.exists() {
            std::fs::copy(&side, work.path().join(format!("index.db{suffix}")))?;
        }
    }
    run(reference, work.path(), &copy)
}

/// Compares the reference's and this app's answers on `index` (the reference's); false if any differs.
pub fn run(reference: &Path, work: &Path, index: &Path) -> anyhow::Result<bool> {
    let conn = db::open_reader(index)?;
    let library: i64 = conn.query_row("SELECT id FROM library_roots ORDER BY id LIMIT 1", [], |row| row.get(0))?;
    let queries = queries(library);
    let list = work.join("searches.json");
    std::fs::write(
        &list,
        serde_json::to_string(&json!({ "layout": queries, "suggest": SUGGEST }))?,
    )?;

    let oracle = Path::new(env!("CARGO_MANIFEST_DIR")).join("oracle");
    let bundle = work.join("reference-search.cjs");
    crate::oracle::run_command(
        Command::new("node")
            .arg(oracle.join("build-reference.mjs"))
            .arg(reference)
            .arg(&bundle)
            .arg("reference-search.ts"),
    )
    .context("bundling the reference's router")?;
    let output = Command::new("node")
        .env("NODE_PATH", reference.join("node_modules"))
        .arg(&bundle)
        .arg(index)
        .arg(reference.join("drizzle"))
        .arg(&list)
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "the reference's searches failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line = String::from_utf8_lossy(&output.stdout);
    let theirs: Value = serde_json::from_str(line.lines().last().unwrap_or_default()).context("reference output")?;
    let ours = ours(&conn, library, &queries)?;

    let mut differences = 0;
    for (i, query) in queries.iter().enumerate() {
        if theirs["layout"][i] != ours["layout"][i] {
            differences += 1;
            println!(
                "DIFFERENT:  search {query}\n  reference {}\n  ours      {}",
                theirs["layout"][i], ours["layout"][i]
            );
        }
    }
    for (i, prefix) in SUGGEST.iter().enumerate() {
        if theirs["suggest"][i] != ours["suggest"][i] {
            differences += 1;
            println!(
                "DIFFERENT:  suggestions for {prefix:?}: {} vs {}",
                theirs["suggest"][i], ours["suggest"][i]
            );
        }
    }
    for key in ["people", "places", "popular", "ratings", "folders"] {
        if theirs[key] != ours[key] {
            differences += 1;
            println!("DIFFERENT:  {key}: {} vs {}", theirs[key], ours[key]);
        }
    }
    let matched: usize = ours["layout"]
        .as_array()
        .map_or(0, |all| all.iter().map(|ids| ids.as_array().map_or(0, Vec::len)).sum());
    println!(
        "searches:   {} searches ({matched} matches in all), {} suggestion prefixes, 5 sidebar lists; {differences} different",
        queries.len(),
        SUGGEST.len()
    );
    Ok(differences == 0)
}
