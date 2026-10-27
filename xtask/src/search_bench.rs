//! `cargo xtask search-bench [--items 500000] [--out <index>]`: the search speed of REQ-007-14. Builds a synthetic
//! index (500k media items, 2k tags in 40 hierarchies, ≈ 1M tag links with a few very common tags and many rare ones)
//! and times typical searches and tag suggestions, printing the 95th percentile of each.

use anyhow::{Context as _, bail};
use chrono::NaiveDate;
use db::libraries::{self, LibraryStatus};
use db::rusqlite::Connection;
use domain::media::MediaKind;
use domain::search::{GallerySearch, SortBy};
use std::path::PathBuf;
use std::time::Instant;

const USAGE: &str = "usage: cargo xtask search-bench [--items 500000] [--out <index file>]";
const TAGS: usize = 2_000;
const TAG_ROOTS: usize = 40;
const RUNS: usize = 20;
/// Budgets of REQ-007-14, ms.
const SEARCH_BUDGET: f64 = 50.0;
const SUGGEST_BUDGET: f64 = 10.0;

/// A small deterministic generator, so every run builds the same index.
struct Random(u64);

impl Random {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Tag index with a skewed popularity: low indexes are common, high ones rare.
fn popular_tag(random: &mut Random) -> usize {
    ((TAGS as f64) * random.next().powi(3)) as usize
}

fn tag_name(i: usize) -> String {
    format!("tag {i}")
}

fn build(conn: &Connection, items: usize) -> anyhow::Result<()> {
    let root = libraries::insert(conn, "B:\\search-bench", "Bench", LibraryStatus::Offline)?.id;
    let tx = conn.unchecked_transaction()?;
    {
        let mut tag = tx.prepare("INSERT INTO tags (id, name, name_norm, parent_id) VALUES (?1, ?2, ?3, ?4)")?;
        for i in 0..TAGS {
            // The first tags are roots, the others spread below them (two levels).
            let parent = match i {
                i if i < TAG_ROOTS => None,
                i if i < TAG_ROOTS * 10 => Some((i % TAG_ROOTS) as i64 + 1),
                i => Some((TAG_ROOTS + i % (TAG_ROOTS * 9)) as i64 + 1),
            };
            let name = tag_name(i);
            tag.execute((i as i64 + 1, &name, &name, parent))?;
        }
        let mut media = tx.prepare(
            "INSERT INTO media (id, root_id, rel_path, file_name, kind, size, mtime, width, height, taken_at, rating)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )?;
        let mut link = tx.prepare("INSERT OR IGNORE INTO media_tags (media_id, tag_id) VALUES (?1, ?2)")?;
        let mut random = Random(42);
        let start = NaiveDate::from_ymd_opt(2014, 1, 1)
            .expect("date")
            .and_hms_opt(0, 0, 0)
            .expect("time");
        let start = start.and_utc().timestamp_millis();
        for i in 0..items {
            let id = i as i64 + 1;
            let kind = if random.next() < 0.05 {
                MediaKind::Video
            } else {
                MediaKind::Image
            };
            let taken = start + (random.next() * 10.0 * 365.0 * 86_400_000.0) as i64;
            let rating = (random.next() < 0.3).then(|| 1 + (random.next() * 5.0) as i64);
            media.execute((
                id,
                root,
                format!("{}/{}/IMG_{i:06}.jpg", 2014 + i % 10, i / 1000 % 100),
                format!("IMG_{i:06}.jpg"),
                kind.as_str(),
                (random.next() * 8_000_000.0) as i64,
                taken + 1000,
                4000,
                if i % 3 == 0 { 3000 } else { 6000 },
                (random.next() < 0.9).then_some(taken),
                rating,
            ))?;
            for _ in 0..2 {
                link.execute((id, popular_tag(&mut random) as i64 + 1))?;
            }
        }
    }
    tx.commit()?;
    let links: i64 = conn.query_row("SELECT count(*) FROM media_tags", [], |row| row.get(0))?;
    println!("built {items} media items, {TAGS} tags, {links} tag links");
    Ok(())
}

fn p95(mut times: Vec<f64>) -> f64 {
    times.sort_by(f64::total_cmp);
    times[((times.len() - 1) as f64 * 0.95).round() as usize]
}

pub fn run(args: &[String]) -> anyhow::Result<bool> {
    let (mut items, mut out) = (500_000usize, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--items" => items = args.next().context(USAGE)?.parse().context(USAGE)?,
            "--out" => out = Some(PathBuf::from(args.next().context(USAGE)?)),
            other => bail!("unknown argument {other} ({USAGE})"),
        }
    }
    let temp = tempfile::tempdir()?;
    let path = out.unwrap_or_else(|| temp.path().join("gallery.db"));
    if !path.exists() {
        let started = Instant::now();
        build(&db::open(&path)?, items)?;
        println!("in {:.0} s", started.elapsed().as_secs_f64());
    }
    // A reused index must be a complete bench index of this size (not an interrupted build or a real index).
    let check = db::open_reader(&path)?;
    let (media, tags): (i64, i64) = check.query_row(
        "SELECT (SELECT count(*) FROM media), (SELECT count(*) FROM tags)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if media != items as i64 || tags != TAGS as i64 {
        bail!(
            "{} holds {media} media items and {tags} tags, not a bench index of {items}",
            path.display()
        );
    }
    drop(check);
    let conn = db::open_reader(&path)?;
    // The most and a rarely used tag, and one in between, by their real counts.
    let by_use = |order: &str| -> anyhow::Result<String> {
        Ok(conn.query_row(
            &format!("SELECT name FROM tags WHERE media_count > 0 ORDER BY media_count {order} LIMIT 1"),
            [],
            |row| row.get(0),
        )?)
    };
    let (common, rare) = (by_use("DESC")?, by_use("ASC")?);
    let middle: String = conn.query_row(
        "SELECT name FROM tags WHERE media_count BETWEEN 2000 AND 20000 ORDER BY media_count LIMIT 1",
        [],
        |row| row.get(0),
    )?;
    let tags = |include: &[&str], exclude: &[&str]| GallerySearch {
        tags: include.iter().map(|s| s.to_string()).collect(),
        exclude: exclude.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    };
    let year = |y| NaiveDate::from_ymd_opt(y, 1, 1);
    let searches: Vec<(&str, GallerySearch)> = vec![
        ("a common tag", tags(&[&common], &[])),
        ("a rare tag", tags(&[&rare], &[])),
        ("a mid-sized tag", tags(&[&middle], &[])),
        ("two tags", tags(&[&common, &middle], &[])),
        ("a tag and an exclusion", tags(&[&middle], &[&common])),
        ("a tag root (with descendants)", tags(&["tag 1"], &[])),
        (
            "a file name",
            GallerySearch {
                name: Some("IMG_0123".into()),
                ..Default::default()
            },
        ),
        (
            "rating 5",
            GallerySearch {
                ratings: vec![5],
                ..Default::default()
            },
        ),
        (
            "videos of one year",
            GallerySearch {
                kind: Some(MediaKind::Video),
                from: year(2020),
                to: NaiveDate::from_ymd_opt(2020, 12, 31),
                ..Default::default()
            },
        ),
        (
            "a tag, by name",
            GallerySearch {
                sort_by: SortBy::Name,
                descending: false,
                ..tags(&[&middle], &[])
            },
        ),
        ("everything", GallerySearch::default()),
        (
            "everything, by size",
            GallerySearch {
                sort_by: SortBy::Size,
                ..Default::default()
            },
        ),
    ];
    let mut slowest = 0.0f64;
    println!("{:<34} {:>9} {:>9}", "search", "matches", "p95 ms");
    for (name, search) in &searches {
        let mut times = Vec::new();
        let mut matches = 0;
        for _ in 0..RUNS {
            let started = Instant::now();
            matches = db::gallery::layout(&conn, search)?.ids.len();
            times.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        let p = p95(times);
        // Everything (no filter) lists the whole index; REQ-007-14 is about filtered searches.
        if !name.starts_with("everything") {
            slowest = slowest.max(p);
        }
        println!("{name:<34} {matches:>9} {p:>9.1}");
    }
    let mut suggest = Vec::new();
    for prefix in ["", "t", "tag 1", "tag 12", "tag 199", "x"] {
        for _ in 0..RUNS {
            let started = Instant::now();
            db::tags::autocomplete(&conn, prefix, 10)?;
            suggest.push(started.elapsed().as_secs_f64() * 1000.0);
        }
    }
    let suggest = p95(suggest);
    println!("tag suggestions p95 {suggest:.1} ms (budget {SUGGEST_BUDGET} ms)");
    println!("filtered searches p95 at most {slowest:.1} ms (budget {SEARCH_BUDGET} ms)");
    Ok(slowest < SEARCH_BUDGET && suggest < SUGGEST_BUDGET)
}
