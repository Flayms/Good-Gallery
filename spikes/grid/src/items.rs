//! Item list: every WebP in the copied thumbnail cache, with its size from the file header.

use rayon::prelude::*;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

pub struct Thumb {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

/// Width and height from the first chunk of a WebP file (VP8, VP8L or VP8X).
pub fn webp_size(header: &[u8]) -> Option<(u32, u32)> {
    if header.len() < 30 || &header[0..4] != b"RIFF" || &header[8..12] != b"WEBP" {
        return None;
    }
    let data = &header[20..];
    match &header[12..16] {
        b"VP8 " => {
            if data[3..6] != [0x9d, 0x01, 0x2a] {
                return None;
            }
            let w = u16::from_le_bytes([data[6], data[7]]) & 0x3fff;
            let h = u16::from_le_bytes([data[8], data[9]]) & 0x3fff;
            Some((w as u32, h as u32))
        }
        b"VP8L" => {
            if data[0] != 0x2f {
                return None;
            }
            let bits = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8X" => {
            let w = u32::from_le_bytes([data[4], data[5], data[6], 0]) + 1;
            let h = u32::from_le_bytes([data[7], data[8], data[9], 0]) + 1;
            Some((w, h))
        }
        _ => None,
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => collect(&path, out),
            Ok(_) if path.extension().is_some_and(|e| e == "webp") => out.push(path),
            _ => {}
        }
    }
}

/// Like `scan`, but cached in `results/items.tsv`: a cold header scan takes minutes (Defender scans each first read).
pub fn scan_cached(dir: &Path) -> Vec<Thumb> {
    let cache = Path::new("results/items.tsv");
    if let Ok(text) = std::fs::read_to_string(cache) {
        let thumbs: Vec<Thumb> = text
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('\t');
                let path = dir.join(parts.next()?);
                Some(Thumb {
                    path,
                    width: parts.next()?.parse().ok()?,
                    height: parts.next()?.parse().ok()?,
                })
            })
            .collect();
        if !thumbs.is_empty() {
            return thumbs;
        }
    }
    let thumbs = scan(dir);
    let text: String = thumbs
        .iter()
        .map(|t| {
            format!(
                "{}\t{}\t{}\n",
                t.path.strip_prefix(dir).unwrap().display(),
                t.width,
                t.height
            )
        })
        .collect();
    let _ = std::fs::create_dir_all("results");
    let _ = std::fs::write(cache, text);
    thumbs
}

/// All readable thumbnails under `dir`, sorted by path so runs are reproducible.
pub fn scan(dir: &Path) -> Vec<Thumb> {
    let start = std::time::Instant::now();
    let mut paths = Vec::new();
    collect(dir, &mut paths);
    paths.sort();
    eprintln!("walk: {} files in {:?}", paths.len(), start.elapsed());
    paths
        .into_par_iter()
        .filter_map(|path| {
            let mut header = [0u8; 30];
            File::open(&path).ok()?.read_exact(&mut header).ok()?;
            let (width, height) = webp_size(&header)?;
            Some(Thumb {
                path,
                width,
                height,
            })
        })
        .collect()
}
