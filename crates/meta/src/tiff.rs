//! EXIF: the TIFF structure in JPEG `APP1`, PNG `eXIf`, WebP `EXIF` and HEIF `Exif` items. Only the tags spec 004
//! uses are read, under the names exiftool gives them; values as `exiftool -n` prints them.

use crate::rules::Tags;
use serde_json::{Value, json};

/// IFD entries read per directory at most; real ones have a few dozen.
const MAX_ENTRIES: usize = 512;

#[derive(Clone, Copy)]
struct Tiff<'a> {
    data: &'a [u8],
    little: bool,
}

impl Tiff<'_> {
    fn u16(&self, at: usize) -> Option<u16> {
        let bytes = self.data.get(at..at + 2)?.try_into().ok()?;
        Some(if self.little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        })
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let bytes = self.data.get(at..at + 4)?.try_into().ok()?;
        Some(if self.little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    }
}

/// One IFD entry: its type, count and where its value is.
struct Entry {
    kind: u16,
    count: usize,
    at: usize,
}

fn type_size(kind: u16) -> usize {
    match kind {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => 0,
    }
}

impl Tiff<'_> {
    fn entries(&self, offset: usize) -> Vec<(u16, Entry)> {
        let Some(count) = self.u16(offset) else {
            return Vec::new();
        };
        (0..usize::from(count).min(MAX_ENTRIES))
            .filter_map(|i| {
                let base = offset + 2 + i * 12;
                let tag = self.u16(base)?;
                let kind = self.u16(base + 2)?;
                let count = self.u32(base + 4)? as usize;
                let size = type_size(kind).checked_mul(count)?;
                let at = if size <= 4 {
                    base + 8
                } else {
                    self.u32(base + 8)? as usize
                };
                // The value must lie in the data.
                self.data.get(at..at.checked_add(size)?)?;
                Some((tag, Entry { kind, count, at }))
            })
            .collect()
    }

    fn number(&self, entry: &Entry, index: usize) -> Option<f64> {
        let at = entry.at + index * type_size(entry.kind);
        if index >= entry.count {
            return None;
        }
        Some(match entry.kind {
            1 | 7 => f64::from(*self.data.get(at)?),
            6 => f64::from(*self.data.get(at)? as i8),
            3 => f64::from(self.u16(at)?),
            8 => f64::from(self.u16(at)? as i16),
            4 => f64::from(self.u32(at)?),
            9 => f64::from(self.u32(at)? as i32),
            5 => {
                let (n, d) = (self.u32(at)?, self.u32(at + 4)?);
                if d == 0 {
                    return None;
                }
                f64::from(n) / f64::from(d)
            }
            10 => {
                let (n, d) = (self.u32(at)? as i32, self.u32(at + 4)? as i32);
                if d == 0 {
                    return None;
                }
                f64::from(n) / f64::from(d)
            }
            _ => return None,
        })
    }

    fn text(&self, entry: &Entry) -> Option<String> {
        let bytes = self.data.get(entry.at..entry.at + entry.count)?;
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        let text = String::from_utf8_lossy(&bytes[..end]).trim().to_string();
        (!text.is_empty()).then_some(text)
    }

    /// Windows' `XP*` tags: UCS-2 little endian in BYTE values.
    fn ucs2(&self, entry: &Entry) -> Option<String> {
        let bytes = self
            .data
            .get(entry.at..entry.at + entry.count * type_size(entry.kind))?;
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect();
        let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
        let text = String::from_utf16_lossy(&units[..end]).trim().to_string();
        (!text.is_empty()).then_some(text)
    }
}

fn insert(tags: &mut Tags, name: &str, value: Option<Value>) {
    if let Some(value) = value {
        tags.entry(name.to_string()).or_insert(value);
    }
}

/// A whole number as exiftool prints it with `-n` (no fraction for whole values).
fn number(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        json!(n as i64)
    } else {
        json!(n)
    }
}

/// Reads the EXIF tags of a TIFF structure (`II*\0` or `MM\0*`) into `tags`, keeping values already there.
pub fn read(data: &[u8], tags: &mut Tags) {
    let little = match data.get(0..4) {
        Some(b"II*\0") => true,
        Some(b"MM\0*") => false,
        _ => return,
    };
    let tiff = Tiff { data, little };
    let Some(ifd0) = tiff.u32(4) else { return };
    let mut exif_ifd = None;
    let mut gps_ifd = None;
    let mut found = Tags::new();
    for (tag, entry) in tiff.entries(ifd0 as usize) {
        match tag {
            0x010f => insert(&mut found, "Make", tiff.text(&entry).map(Value::from)),
            0x0112 => insert(&mut found, "Orientation", tiff.number(&entry, 0).map(number)),
            0x4746 => insert(&mut found, "Rating", tiff.number(&entry, 0).map(number)),
            0x9c9e => insert(&mut found, "XPKeywords", tiff.ucs2(&entry).map(Value::from)),
            0x8769 => exif_ifd = tiff.number(&entry, 0),
            0x8825 => gps_ifd = tiff.number(&entry, 0),
            _ => {}
        }
    }
    let mut sub_sec = (None, None);
    if let Some(offset) = exif_ifd {
        for (tag, entry) in tiff.entries(offset as usize) {
            match tag {
                0x9003 => insert(&mut found, "DateTimeOriginal", tiff.text(&entry).map(Value::from)),
                0x9004 => insert(&mut found, "CreateDate", tiff.text(&entry).map(Value::from)),
                0x9011 => insert(&mut found, "OffsetTimeOriginal", tiff.text(&entry).map(Value::from)),
                0x9012 => insert(&mut found, "OffsetTimeDigitized", tiff.text(&entry).map(Value::from)),
                0x9291 => sub_sec.0 = tiff.text(&entry),
                0x9292 => sub_sec.1 = tiff.text(&entry),
                0x882a => insert(&mut found, "TimeZoneOffset", tiff.number(&entry, 0).map(number)),
                _ => {}
            }
        }
    }
    if let Some(offset) = gps_ifd {
        for (tag, entry) in tiff.entries(offset as usize) {
            match tag {
                0x0007 if entry.count == 3 => {
                    let parts: Option<Vec<f64>> = (0..3).map(|i| tiff.number(&entry, i)).collect();
                    if let Some([h, m, s]) = parts.as_deref().map(|p| [p[0], p[1], p[2]]) {
                        let seconds = if s.fract() == 0.0 {
                            format!("{s:02}")
                        } else {
                            format!("{s:06.3}")
                        };
                        let time = format!("{:02}:{:02}:{seconds}", h as u32, m as u32);
                        insert(&mut found, "GPSTimeStamp", Some(Value::from(time)));
                    }
                }
                0x001d => insert(&mut found, "GPSDateStamp", tiff.text(&entry).map(Value::from)),
                _ => {}
            }
        }
    }
    // exiftool's composites of a date with its sub-seconds and offset.
    for (date, sub, offset, name) in [
        (
            "DateTimeOriginal",
            &sub_sec.0,
            "OffsetTimeOriginal",
            "SubSecDateTimeOriginal",
        ),
        ("CreateDate", &sub_sec.1, "OffsetTimeDigitized", "SubSecCreateDate"),
    ] {
        if let (Some(Value::String(date)), Some(sub)) = (found.get(date), sub) {
            let offset = found.get(offset).and_then(Value::as_str).unwrap_or_default();
            let value = format!("{date}.{sub}{offset}");
            found.insert(name.to_string(), Value::from(value));
        }
    }
    if let (Some(date), Some(time)) = (
        found.get("GPSDateStamp").and_then(Value::as_str),
        found.get("GPSTimeStamp").and_then(Value::as_str),
    ) {
        let value = format!("{date} {time}Z");
        found.insert("GPSDateTime".into(), Value::from(value));
    }
    for (name, value) in found {
        tags.entry(name).or_insert(value);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An IFD entry: `(tag, type, count, value bytes)`.
    pub type Entry = (u16, u16, u32, Vec<u8>);

    /// A little-endian TIFF with the given IFD0 entries `(tag, type, count, value bytes)`; values over 4 bytes go
    /// after the directory. Sub-IFDs are given as `(pointer tag, entries)`.
    pub fn tiff(ifd0: &[Entry], subs: &[(u16, Vec<Entry>)]) -> Vec<u8> {
        fn ifd(out: &mut Vec<u8>, at: usize, entries: &[Entry]) -> Vec<(usize, usize)> {
            // Returns where each entry's value slot is, so pointers can be patched.
            let mut slots = Vec::new();
            let mut extra: Vec<u8> = Vec::new();
            let extra_at = at + 2 + entries.len() * 12 + 4;
            out.extend((entries.len() as u16).to_le_bytes());
            for (tag, kind, count, value) in entries {
                out.extend(tag.to_le_bytes());
                out.extend(kind.to_le_bytes());
                out.extend(count.to_le_bytes());
                slots.push((out.len(), usize::from(*tag)));
                if value.len() <= 4 {
                    let mut slot = value.clone();
                    slot.resize(4, 0);
                    out.extend(slot);
                } else {
                    out.extend(((extra_at + extra.len()) as u32).to_le_bytes());
                    extra.extend(value);
                }
            }
            out.extend(0u32.to_le_bytes());
            out.extend(extra);
            slots
        }
        let mut out = b"II*\0".to_vec();
        out.extend(8u32.to_le_bytes());
        let mut entries = ifd0.to_vec();
        for (tag, _) in subs {
            entries.push((*tag, 4, 1, vec![0; 4]));
        }
        let slots = ifd(&mut out, 8, &entries);
        for (tag, sub) in subs {
            let at = out.len();
            let slot = slots.iter().find(|(_, t)| *t == usize::from(*tag)).unwrap().0;
            out[slot..slot + 4].copy_from_slice(&(at as u32).to_le_bytes());
            ifd(&mut out, at, sub);
        }
        out
    }

    pub fn ascii(text: &str) -> Vec<u8> {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        bytes
    }

    #[test]
    fn req_019_04_reads_dates_orientation_rating_and_keywords() {
        let xp: Vec<u8> = "Beach;Sea"
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        let data = tiff(
            &[
                (0x0112, 3, 1, 6u16.to_le_bytes().to_vec()),
                (0x4746, 3, 1, 4u16.to_le_bytes().to_vec()),
                (0x010f, 2, 6, ascii("Canon")),
                (0x9c9e, 1, xp.len() as u32, xp),
            ],
            &[(
                0x8769,
                vec![
                    (0x9003, 2, 20, ascii("2024:07:02 07:12:59")),
                    (0x9011, 2, 7, ascii("+02:00")),
                    (0x9291, 2, 3, ascii("12")),
                ],
            )],
        );
        let mut tags = Tags::new();
        read(&data, &mut tags);
        assert_eq!(tags["Orientation"], json!(6));
        assert_eq!(tags["Rating"], json!(4));
        assert_eq!(tags["Make"], json!("Canon"));
        assert_eq!(tags["XPKeywords"], json!("Beach;Sea"));
        assert_eq!(tags["DateTimeOriginal"], json!("2024:07:02 07:12:59"));
        assert_eq!(tags["SubSecDateTimeOriginal"], json!("2024:07:02 07:12:59.12+02:00"));
    }

    /// REQ-011-10: offsets outside the data, huge counts and loops are ignored.
    #[test]
    fn req_011_10_survives_broken_structures() {
        let mut data = tiff(&[(0x0112, 3, 1, 6u16.to_le_bytes().to_vec())], &[]);
        // A pointer to the directory itself and a value far outside the data.
        data.extend([0; 16]);
        let mut tags = Tags::new();
        for cut in 0..data.len() {
            read(&data[..cut], &mut tags);
        }
        let broken = tiff(
            &[
                (0x8769, 4, 1, 8u32.to_le_bytes().to_vec()),
                (0x9003, 2, 0xffff_ffff, vec![0; 4]),
            ],
            &[],
        );
        read(&broken, &mut Tags::new());
    }
}
