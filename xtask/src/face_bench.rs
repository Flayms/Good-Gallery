//! Measures face models (spec 032) with the app's own pipeline, for `docs/face-models.md`. One model per process, so a
//! model's peak memory is its own, and results are written as they come, so a small machine can stop and go on.
//!
//! - `face-bench speed <kind> <model>`: load time, time per run, peak memory.
//! - `face-bench landmarks <kind> <model> <lfw folder> <pairs.txt> <out.tsv>`: finds the faces of LFW's pairs.
//! - `face-bench embed <kind> <model> <lfw folder> <landmarks.tsv> <out.tsv>`: recognizes them.
//! - `face-bench evaluate <pairs.txt> <vectors.tsv>`: accuracy, and the threshold for a rate of false matches.
//! - `face-bench wider <kind> <model> <images folder> <gt.txt> <every Nth>`: faces found and falsely found.
//!
//! - `face-bench people <vectors.tsv> <threshold>…`: what suggestions and groups would do, with the app's own functions.
//! - `face-bench decode <photo>`: how long decoding a photo takes, as the search does it.
//!
//! `kind` is `yunet`, `retinaface`, `arcface` or `sface`.

use anyhow::{Context, Result, bail};
use domain::face_models::{CROP_SIDE, Detection, DetectorFormat, INPUT_SIDE, InputStyle, cosine, group, suggest};
use media::face_models::{Detector, Recognizer};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::time::Instant;

fn format_of(kind: &str) -> Result<DetectorFormat> {
    match kind {
        "yunet" => Ok(DetectorFormat::YuNet),
        "retinaface" => Ok(DetectorFormat::RetinaFace),
        _ => bail!("{kind} is no detection format"),
    }
}

fn style_of(kind: &str) -> Result<InputStyle> {
    match kind {
        "arcface" => Ok(InputStyle::ArcFace),
        "sface" => Ok(InputStyle::SFace),
        _ => bail!("{kind} is no recognition style"),
    }
}

/// Peak memory of this process so far, in megabytes (Linux only).
fn peak_megabytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
    let kilobytes: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kilobytes / 1024)
}

/// A photo with something to look at, so no shortcut through empty input is measured.
fn noise(side: u32) -> image::RgbImage {
    let mut seed = 12345u32;
    image::RgbImage::from_fn(side, side, |_, _| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let [a, b, c, _] = seed.to_le_bytes();
        image::Rgb([a, b, c])
    })
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn speed(kind: &str, model: &Path) -> Result<()> {
    const RUNS: usize = 20;
    let bytes = std::fs::metadata(model)?.len();
    let start = Instant::now();
    let mut times = Vec::new();
    if let Ok(format) = format_of(kind) {
        let detector = Detector::load(model, format).map_err(anyhow::Error::msg)?;
        let loaded = start.elapsed().as_secs_f64();
        let photo = noise(INPUT_SIDE as u32);
        for _ in 0..RUNS {
            let start = Instant::now();
            detector.find_one_look(&photo).map_err(anyhow::Error::msg)?;
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        println!("load_s={loaded:.2}");
    } else {
        let recognizer = Recognizer::load(model, style_of(kind)?).map_err(anyhow::Error::msg)?;
        let loaded = start.elapsed().as_secs_f64();
        let photo = noise(CROP_SIDE as u32);
        let landmarks = domain::face_models::TEMPLATE;
        let mut length = 0;
        for _ in 0..RUNS {
            let start = Instant::now();
            length = recognizer.embed(&photo, &landmarks).map_err(anyhow::Error::msg)?.len();
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        println!("load_s={loaded:.2} vector={length}");
    }
    println!(
        "megabytes_on_disk={:.1} ms_per_run={:.0} peak_memory_mb={}",
        bytes as f64 / 1e6,
        median(times),
        peak_megabytes().map_or("?".to_owned(), |mb| mb.to_string())
    );
    Ok(())
}

/// LFW's pairs: `(same, first image, second image)`.
fn read_pairs(file: &Path) -> Result<Vec<(bool, String, String)>> {
    let text = std::fs::read_to_string(file).with_context(|| file.display().to_string())?;
    let image = |name: &str, number: &str| -> Result<String> {
        let number: u32 = number.parse()?;
        Ok(format!("{name}/{name}_{number:04}.jpg"))
    };
    let mut pairs = Vec::new();
    for line in text.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.as_slice() {
            [name, a, b] => pairs.push((true, image(name, a)?, image(name, b)?)),
            [first, a, second, b] => pairs.push((false, image(first, a)?, image(second, b)?)),
            _ => bail!("unreadable pair: {line}"),
        }
    }
    Ok(pairs)
}

/// What a tab-separated results file already holds, by its first column.
fn done_in(file: &Path) -> HashSet<String> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split('\t').next().map(str::to_owned))
        .collect()
}

fn append(file: &Path) -> Result<std::fs::File> {
    Ok(std::fs::OpenOptions::new().create(true).append(true).open(file)?)
}

/// The face LFW's image is about: the one nearest its center.
fn central(found: Vec<Detection>, width: f32, height: f32) -> Option<Detection> {
    let distance = |face: &Detection| {
        let (cx, cy) = (face.rect[0] + face.rect[2] / 2.0, face.rect[1] + face.rect[3] / 2.0);
        (cx - width / 2.0).powi(2) + (cy - height / 2.0).powi(2)
    };
    found.into_iter().min_by(|a, b| distance(a).total_cmp(&distance(b)))
}

fn landmarks(kind: &str, model: &Path, lfw: &Path, pairs: &Path, out: &Path) -> Result<()> {
    let detector = Detector::load(model, format_of(kind)?).map_err(anyhow::Error::msg)?;
    let pairs = read_pairs(pairs)?;
    let images: HashSet<&String> = pairs.iter().flat_map(|(_, a, b)| [a, b]).collect();
    let mut images: Vec<&String> = images.into_iter().collect();
    images.sort();
    let done = done_in(out);
    let mut file = append(out)?;
    let start = Instant::now();
    let (mut seen, mut missing, mut searched) = (0, 0, 0);
    for name in images.iter().filter(|name| !done.contains(name.as_str())) {
        searched += 1;
        let photo = image::open(lfw.join(name)).with_context(|| name.to_string())?.to_rgb8();
        // Low, as LFW's faces are known to be there: this finds their landmarks, it doesn't judge the detector.
        let found = detector.find_from(&photo, 0.3).map_err(anyhow::Error::msg)?;
        match central(found, photo.width() as f32, photo.height() as f32) {
            Some(face) => {
                let numbers: Vec<String> = face.landmarks.iter().flatten().map(|n| format!("{n:.2}")).collect();
                writeln!(file, "{name}\t{}", numbers.join("\t"))?;
                seen += 1;
            }
            None => {
                writeln!(file, "{name}")?;
                missing += 1;
            }
        }
    }
    println!(
        "{} images in all, searched {searched}: {seen} with a face, {missing} without, {:.0} ms each, peak {:?} MB",
        images.len(),
        start.elapsed().as_secs_f64() * 1000.0 / f64::from(searched.max(1)),
        peak_megabytes()
    );
    Ok(())
}

fn embed(kind: &str, model: &Path, lfw: &Path, landmarks: &Path, out: &Path) -> Result<()> {
    let recognizer = Recognizer::load(model, style_of(kind)?).map_err(anyhow::Error::msg)?;
    let done = done_in(out);
    let mut file = append(out)?;
    let start = Instant::now();
    let mut count = 0;
    for line in std::fs::read_to_string(landmarks)?.lines() {
        let mut parts = line.split('\t');
        let Some(name) = parts.next() else { continue };
        let numbers: Vec<f32> = parts.filter_map(|n| n.parse().ok()).collect();
        if numbers.len() != 10 || done.contains(name) {
            continue;
        }
        let points: [[f32; 2]; 5] = std::array::from_fn(|n| [numbers[2 * n], numbers[2 * n + 1]]);
        let photo = image::open(lfw.join(name)).with_context(|| name.to_owned())?.to_rgb8();
        let vector = recognizer.embed(&photo, &points).map_err(anyhow::Error::msg)?;
        let text: Vec<String> = vector.iter().map(|n| format!("{n:.5}")).collect();
        writeln!(file, "{name}\t{}", text.join(" "))?;
        count += 1;
    }
    println!(
        "{count} faces, {:.0} ms each with decoding, peak {:?} MB",
        start.elapsed().as_secs_f64() * 1000.0 / f64::from(count.max(1)),
        peak_megabytes()
    );
    Ok(())
}

/// The share of pairs a threshold judges rightly.
fn accuracy(scores: &[(bool, f32)], threshold: f32) -> f64 {
    let right = scores
        .iter()
        .filter(|(same, score)| *same == (*score >= threshold))
        .count();
    right as f64 / scores.len().max(1) as f64
}

fn best_threshold(scores: &[(bool, f32)]) -> f32 {
    (-100..=100)
        .map(|n| n as f32 / 100.0)
        .max_by(|a, b| accuracy(scores, *a).total_cmp(&accuracy(scores, *b)))
        .unwrap_or(0.0)
}

fn evaluate(pairs: &Path, vectors: &Path) -> Result<()> {
    let pairs = read_pairs(pairs)?;
    let text = std::fs::read_to_string(vectors)?;
    let mut embeddings: HashMap<&str, Vec<f32>> = HashMap::new();
    for line in text.lines() {
        if let Some((name, numbers)) = line.split_once('\t') {
            embeddings.insert(name, numbers.split(' ').filter_map(|n| n.parse().ok()).collect());
        }
    }
    // A pair with a face nobody found counts as unlike: wrong for the same person, right for two.
    let scores: Vec<(bool, f32)> = pairs
        .iter()
        .map(|(same, a, b)| {
            let score = match (embeddings.get(a.as_str()), embeddings.get(b.as_str())) {
                (Some(a), Some(b)) => cosine(a, b),
                _ => -1.0,
            };
            (*same, score)
        })
        .collect();
    let unseen = pairs
        .iter()
        .filter(|(_, a, b)| !embeddings.contains_key(a.as_str()) || !embeddings.contains_key(b.as_str()))
        .count();
    // LFW's ten folds: each judged with the threshold that suits the other nine.
    let fold_size = scores.len() / 10;
    let mut folds = Vec::new();
    for fold in 0..10 {
        let range = fold * fold_size..(fold + 1) * fold_size;
        let others: Vec<(bool, f32)> = scores
            .iter()
            .enumerate()
            .filter(|(n, _)| !range.contains(n))
            .map(|(_, score)| *score)
            .collect();
        folds.push(accuracy(&scores[range], best_threshold(&others)));
    }
    let mean = folds.iter().sum::<f64>() / folds.len() as f64;
    println!(
        "pairs={} unseen={unseen} lfw_accuracy={:.2}% best_threshold={:.2}",
        pairs.len(),
        mean * 100.0,
        best_threshold(&scores)
    );

    // Far more unrelated faces than LFW's pairs, as a library holds: random pairs of different people.
    let people: Vec<(&str, &Vec<f32>)> = embeddings.iter().map(|(name, vector)| (*name, vector)).collect();
    let mut people = people;
    people.sort_by_key(|(name, _)| *name);
    let person = |name: &str| name.split('/').next().unwrap_or(name).to_owned();
    const RANDOM_PAIRS: usize = 2_000_000;
    let mut seed = 42u64;
    let mut next = |limit: usize| {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((seed >> 33) as usize) % limit
    };
    let mut unrelated = Vec::with_capacity(RANDOM_PAIRS);
    while unrelated.len() < RANDOM_PAIRS {
        let (a, b) = (people[next(people.len())], people[next(people.len())]);
        if person(a.0) != person(b.0) {
            unrelated.push(cosine(a.1, b.1));
        }
    }
    unrelated.sort_by(|a, b| b.total_cmp(a));
    let genuine: Vec<f32> = scores
        .iter()
        .filter(|(same, _)| *same)
        .map(|(_, score)| *score)
        .collect();
    for rate in [0.01, 0.001, 0.0001] {
        let threshold = unrelated[((RANDOM_PAIRS as f64 * rate) as usize).min(RANDOM_PAIRS - 1)];
        let kept = genuine.iter().filter(|score| **score >= threshold).count();
        println!(
            "false_matches={:.2}% threshold={threshold:.3} same_person_found={:.1}%",
            rate * 100.0,
            kept as f64 * 100.0 / genuine.len().max(1) as f64
        );
    }
    Ok(())
}

/// A face of WIDER FACE's list.
struct Truth {
    rect: [f32; 4],
    invalid: bool,
}

fn read_wider(file: &Path) -> Result<Vec<(String, Vec<Truth>)>> {
    let text = std::fs::read_to_string(file)?;
    let mut lines = text.lines();
    let mut images = Vec::new();
    while let Some(name) = lines.next() {
        let count: usize = lines.next().context("no count")?.trim().parse()?;
        let mut faces = Vec::new();
        // An image without faces still has one line of zeros.
        for _ in 0..count.max(1) {
            let numbers: Vec<f32> = lines
                .next()
                .context("no face")?
                .split_whitespace()
                .filter_map(|n| n.parse().ok())
                .collect();
            if count > 0 && numbers.len() >= 8 {
                faces.push(Truth {
                    rect: [numbers[0], numbers[1], numbers[2], numbers[3]],
                    invalid: numbers[7] > 0.0,
                });
            }
        }
        images.push((name.to_owned(), faces));
    }
    Ok(images)
}

fn overlap(a: [f32; 4], b: [f32; 4]) -> f32 {
    let w = (a[0] + a[2]).min(b[0] + b[2]) - a[0].max(b[0]);
    let h = (a[1] + a[3]).min(b[1] + b[3]) - a[1].max(b[1]);
    let shared = w.max(0.0) * h.max(0.0);
    let union = a[2] * a[3] + b[2] * b[3] - shared;
    if union > 0.0 { shared / union } else { 0.0 }
}

/// Faces found and falsely found. Unlike WIDER's own test this counts what the app keeps: small faces it drops on
/// purpose aren't misses, and a box on an unlabeled tiny or invalid face isn't false.
fn wider(kind: &str, model: &Path, images: &Path, truth: &Path, every: usize) -> Result<()> {
    const THRESHOLDS: [f32; 9] = [0.3, 0.5, 0.6, 0.7, 0.75, 0.8, 0.85, 0.9, 0.95];
    const SIZES: [(&str, f32, f32); 3] = [
        ("24-63 px", 24.0, 64.0),
        ("64-159 px", 64.0, 160.0),
        ("160+ px", 160.0, f32::MAX),
    ];
    const SAME: f32 = 0.5;
    let detector = Detector::load(model, format_of(kind)?).map_err(anyhow::Error::msg)?;
    let all = read_wider(truth)?;
    let chosen: Vec<&(String, Vec<Truth>)> = all.iter().step_by(every.max(1)).collect();
    let mut found_all: Vec<Vec<Detection>> = Vec::new();
    let mut seconds = 0.0;
    for (name, _) in &chosen {
        let photo = image::open(images.join(name)).with_context(|| name.clone())?.to_rgb8();
        let start = Instant::now();
        found_all.push(detector.find_from(&photo, THRESHOLDS[0]).map_err(anyhow::Error::msg)?);
        seconds += start.elapsed().as_secs_f64();
    }
    println!(
        "images={} ms_per_photo={:.0} peak_memory_mb={:?}",
        chosen.len(),
        seconds * 1000.0 / chosen.len() as f64,
        peak_megabytes()
    );
    for threshold in THRESHOLDS {
        let (mut hits, mut totals) = ([0u32; 3], [0u32; 3]);
        let (mut false_found, mut kept) = (0u32, 0u32);
        for ((_, truths), found) in chosen.iter().zip(&found_all) {
            let mut used = vec![false; truths.len()];
            for face in found.iter().filter(|face| face.score >= threshold) {
                kept += 1;
                let best = truths
                    .iter()
                    .enumerate()
                    .filter(|(n, _)| !used[*n])
                    .map(|(n, truth)| (n, overlap(face.rect, truth.rect)))
                    .max_by(|a, b| a.1.total_cmp(&b.1));
                match best {
                    Some((n, shared)) if shared >= SAME => used[n] = true,
                    // Overlapping a face too small or invalid to count, it isn't false either.
                    _ if truths.iter().any(|truth| overlap(face.rect, truth.rect) >= 0.2) => {}
                    _ => false_found += 1,
                }
            }
            for (n, truth) in truths.iter().enumerate() {
                let side = truth.rect[2].min(truth.rect[3]);
                if truth.invalid {
                    continue;
                }
                for (bucket, (_, low, high)) in SIZES.iter().enumerate() {
                    if side >= *low && side < *high {
                        totals[bucket] += 1;
                        hits[bucket] += u32::from(used[n]);
                    }
                }
            }
        }
        let shares: Vec<String> = SIZES
            .iter()
            .enumerate()
            .map(|(n, (label, _, _))| {
                format!(
                    "{label} {:.1}% of {}",
                    f64::from(hits[n]) * 100.0 / f64::from(totals[n].max(1)),
                    totals[n]
                )
            })
            .collect();
        println!(
            "score>={threshold:.2}: {} | boxes {kept}, false {false_found} ({:.1} per 100 photos)",
            shares.join(", "),
            f64::from(false_found) * 100.0 / chosen.len() as f64
        );
    }
    Ok(())
}

/// A library as a simulation: of the people with several photos, every other person is known by all photos but one, and
/// that one asks for a suggestion. The rest are strangers, who should get none.
fn people(vectors: &Path, thresholds: &[f32]) -> Result<()> {
    let text = std::fs::read_to_string(vectors)?;
    let mut by_person: std::collections::BTreeMap<&str, Vec<Vec<f32>>> = std::collections::BTreeMap::new();
    for line in text.lines() {
        if let Some((name, numbers)) = line.split_once('\t') {
            let person = name.split('/').next().unwrap_or(name);
            let vector = numbers.split(' ').filter_map(|n| n.parse().ok()).collect();
            by_person.entry(person).or_default().push(vector);
        }
    }
    let mut known: Vec<(&str, &[f32])> = Vec::new();
    let mut probes: Vec<(&str, &[f32])> = Vec::new();
    let mut strangers: Vec<&[f32]> = Vec::new();
    let mut keep = false;
    for (person, faces) in &by_person {
        if faces.len() >= 2 {
            keep = !keep;
        }
        if faces.len() >= 2 && keep {
            let (probe, rest) = faces.split_first().context("no faces")?;
            probes.push((*person, probe.as_slice()));
            known.extend(rest.iter().map(|face| (*person, face.as_slice())));
        } else {
            strangers.extend(faces.iter().map(Vec::as_slice));
        }
    }
    println!(
        "known_faces={} known_probes={} stranger_faces={}",
        known.len(),
        probes.len(),
        strangers.len()
    );
    for threshold in thresholds {
        let (mut right, mut wrong, mut none) = (0u32, 0u32, 0u32);
        for (person, face) in &probes {
            match suggest(face, &known, *threshold, &[]) {
                Some((name, _)) if name == *person => right += 1,
                Some(_) => wrong += 1,
                None => none += 1,
            }
        }
        let sampled: Vec<&&[f32]> = strangers.iter().step_by(4).collect();
        let strangers_suggested = sampled
            .iter()
            .filter(|face| suggest(face, &known, *threshold, &[]).is_some())
            .count();
        let share = |n: u32| f64::from(n) * 100.0 / probes.len().max(1) as f64;
        // Groups of everyone, as the People page would make them.
        let (faces, owners): (Vec<&[f32]>, Vec<&str>) = by_person
            .iter()
            .flat_map(|(person, faces)| faces.iter().map(move |face| (face.as_slice(), *person)))
            .unzip();
        let groups = group(&faces, *threshold, 3);
        let (mut grouped, mut majority) = (0usize, 0usize);
        for members in &groups {
            let mut counts: HashMap<&str, usize> = HashMap::new();
            for member in members {
                *counts.entry(owners[*member]).or_default() += 1;
            }
            grouped += members.len();
            majority += counts.values().max().copied().unwrap_or(0);
        }
        println!(
            "threshold={threshold:.3} right={:.1}% wrong={:.1}% none={:.1}% strangers_suggested={:.1}% groups={} faces_in_groups={grouped} group_purity={:.1}%",
            share(right),
            share(wrong),
            share(none),
            strangers_suggested as f64 * 100.0 / sampled.len().max(1) as f64,
            groups.len(),
            majority as f64 * 100.0 / grouped.max(1) as f64,
        );
    }
    Ok(())
}

fn decode(photo: &Path) -> Result<()> {
    let mut times = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        media::original::load_original(photo, None, 8000, None).map_err(|error| anyhow::anyhow!("{error}"))?;
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    println!(
        "ms_per_decode={:.0} peak_memory_mb={:?}",
        median(times),
        peak_megabytes()
    );
    Ok(())
}

pub fn run(args: &[String]) -> Result<()> {
    let path = |n: usize| {
        args.get(n)
            .map(Path::new)
            .context("missing argument, see the top of face_bench.rs")
    };
    match args.first().map(String::as_str) {
        Some("speed") => speed(&args[1], path(2)?),
        Some("landmarks") => landmarks(&args[1], path(2)?, path(3)?, path(4)?, path(5)?),
        Some("embed") => embed(&args[1], path(2)?, path(3)?, path(4)?, path(5)?),
        Some("evaluate") => evaluate(path(1)?, path(2)?),
        Some("decode") => decode(path(1)?),
        Some("people") => {
            let thresholds: Vec<f32> = args[2..].iter().filter_map(|n| n.parse().ok()).collect();
            people(path(1)?, &thresholds)
        }
        Some("wider") => wider(&args[1], path(2)?, path(3)?, path(4)?, args[5].parse()?),
        _ => bail!("face-bench speed|landmarks|embed|evaluate|wider, see the top of face_bench.rs"),
    }
}
