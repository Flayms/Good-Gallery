//! Where the metadata sits in each container: JPEG segments, PNG chunks, RIFF (WebP) chunks, GIF's header and ISO
//! base media boxes (AVIF, MP4, MOV). Each reader fills exiftool-shaped [`Tags`] and asks for bytes it hasn't seen.

use crate::bytes::{Bytes, Missing, u16_be, u16_le, u32_be, u32_le, u64_be};
use crate::rules::Tags;
use crate::{tiff, xmp};
use serde_json::{Value, json};

/// Segments, chunks or boxes looked at per file at most (REQ-011-10).
const MAX_PARTS: usize = 4096;
/// A metadata part larger than this is skipped.
const MAX_PART: u64 = 16 << 20;

fn put(tags: &mut Tags, name: &str, value: Value) {
    tags.entry(name.to_string()).or_insert(value);
}

fn dimensions(tags: &mut Tags, width: u64, height: u64) {
    if width > 0 && height > 0 {
        put(tags, "ImageWidth", json!(width));
        put(tags, "ImageHeight", json!(height));
    }
}

// --- JPEG ---

const XMP_SIGNATURE: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";

pub fn jpeg(data: &Bytes, tags: &mut Tags) -> Result<(), Missing> {
    let mut at = 2u64;
    let (mut exif, mut packet, mut iptc) = (None, None, None);
    for _ in 0..MAX_PARTS {
        let Some(header) = data.exact(at, 4)? else { break };
        if header[0] != 0xff {
            break;
        }
        let marker = header[1];
        // Fill bytes and markers without a length.
        if marker == 0xff {
            at += 1;
            continue;
        }
        if marker == 0xd8 || (0xd0..=0xd7).contains(&marker) || marker == 0x01 {
            at += 2;
            continue;
        }
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        let len = u64::from(u16_be(header, 2).unwrap_or(0));
        if len < 2 {
            break;
        }
        let body_at = at + 4;
        let body_len = len - 2;
        match marker {
            // Start of frame (not DHT, JPG, DAC): the image size.
            0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                if let Some(frame) = data.exact(body_at, 5)? {
                    let height = u16_be(frame, 1).unwrap_or(0);
                    let width = u16_be(frame, 3).unwrap_or(0);
                    dimensions(tags, u64::from(width), u64::from(height));
                }
            }
            0xe1 if body_len <= MAX_PART => {
                if let Some(body) = data.exact(body_at, body_len)? {
                    if body.starts_with(b"Exif\0\0") && exif.is_none() {
                        exif = Some(body[6..].to_vec());
                    } else if body.starts_with(XMP_SIGNATURE) && packet.is_none() {
                        packet = Some(body[XMP_SIGNATURE.len()..].to_vec());
                    }
                }
            }
            0xed if body_len <= MAX_PART => {
                if let Some(body) = data.exact(body_at, body_len)?
                    && body.starts_with(b"Photoshop 3.0\0")
                    && iptc.is_none()
                {
                    iptc = Some(body[14..].to_vec());
                }
            }
            _ => {}
        }
        at = body_at + body_len;
    }
    put(tags, "MIMEType", json!("image/jpeg"));
    // exiftool's priority for tags of the same name: EXIF, then IPTC, then XMP.
    if let Some(exif) = exif {
        tiff::read(&exif, tags);
    }
    if let Some(iptc) = iptc {
        photoshop_iptc(&iptc, tags);
    }
    if let Some(packet) = packet {
        xmp::read(&packet, tags);
    }
    Ok(())
}

/// Photoshop image resources (`8BIM`); resource 0x0404 holds IPTC IIM, whose 2:25 records are the keywords.
fn photoshop_iptc(data: &[u8], tags: &mut Tags) {
    let mut at = 0usize;
    for _ in 0..MAX_PARTS {
        if data.get(at..at + 4) != Some(b"8BIM") {
            return;
        }
        let Some(id) = u16_be(data, at + 4) else { return };
        let Some(&name_len) = data.get(at + 6) else { return };
        // Pascal string, padded to an even length (with its length byte).
        let name_size = (usize::from(name_len) + 2) & !1;
        let size_at = at + 6 + name_size;
        let Some(size) = u32_be(data, size_at) else { return };
        let body_at = size_at + 4;
        let Some(body) = data.get(body_at..body_at + size as usize) else {
            return;
        };
        if id == 0x0404 {
            iptc_keywords(body, tags);
        }
        at = body_at + ((size as usize + 1) & !1);
    }
}

fn iptc_keywords(data: &[u8], tags: &mut Tags) {
    let mut keywords = Vec::new();
    let mut at = 0usize;
    while keywords.len() < 1000 && data.get(at) == Some(&0x1c) {
        let (Some(&record), Some(&dataset), Some(len)) = (data.get(at + 1), data.get(at + 2), u16_be(data, at + 3))
        else {
            break;
        };
        // Extended lengths (over 32 KB) aren't keywords.
        if len & 0x8000 != 0 {
            break;
        }
        let Some(value) = data.get(at + 5..at + 5 + usize::from(len)) else {
            break;
        };
        if (record, dataset) == (2, 25) {
            let text =
                String::from_utf8(value.to_vec()).unwrap_or_else(|_| value.iter().map(|&b| char::from(b)).collect());
            let text = text.trim().to_string();
            if !text.is_empty() {
                keywords.push(text);
            }
        }
        at += 5 + usize::from(len);
    }
    match keywords.len() {
        0 => {}
        1 => put(tags, "Keywords", Value::from(keywords.remove(0))),
        _ => put(tags, "Keywords", Value::from(keywords)),
    }
}

// --- PNG ---

pub fn png(data: &Bytes, tags: &mut Tags) -> Result<(), Missing> {
    let mut at = 8u64;
    let (mut exif, mut packet) = (None, None);
    for _ in 0..MAX_PARTS {
        let Some(header) = data.exact(at, 8)? else { break };
        let len = u64::from(u32_be(header, 0).unwrap_or(0));
        let kind = &header[4..8];
        let body_at = at + 8;
        match kind {
            b"IHDR" => {
                if let Some(body) = data.exact(body_at, 8)? {
                    dimensions(
                        tags,
                        u64::from(u32_be(body, 0).unwrap_or(0)),
                        u64::from(u32_be(body, 4).unwrap_or(0)),
                    );
                }
            }
            b"eXIf" if len <= MAX_PART => exif = data.exact(body_at, len)?.map(<[u8]>::to_vec),
            b"iTXt" if len <= MAX_PART => {
                if let Some(body) = data.exact(body_at, len)? {
                    // keyword \0 compression flag, method, language \0, translated keyword \0, text
                    if let Some(rest) = body.strip_prefix(b"XML:com.adobe.xmp\0")
                        && rest.first() == Some(&0)
                        && let Some(text) = rest.get(2..)
                    {
                        let mut parts = text.splitn(3, |&b| b == 0);
                        let (_, _, text) = (parts.next(), parts.next(), parts.next());
                        packet = text.map(<[u8]>::to_vec);
                    }
                }
            }
            // Metadata after the image data is rare; the pixels aren't read for it.
            b"IDAT" | b"IEND" => break,
            _ => {}
        }
        at = body_at + len + 4;
    }
    put(tags, "MIMEType", json!("image/png"));
    if let Some(exif) = exif {
        tiff::read(&exif, tags);
    }
    if let Some(packet) = packet {
        xmp::read(&packet, tags);
    }
    Ok(())
}

// --- WebP (RIFF) ---

pub fn webp(data: &Bytes, tags: &mut Tags) -> Result<(), Missing> {
    let mut at = 12u64;
    let (mut exif, mut packet) = (None, None);
    for _ in 0..MAX_PARTS {
        let Some(header) = data.exact(at, 8)? else { break };
        let len = u64::from(u32_le(header, 4).unwrap_or(0));
        let body_at = at + 8;
        match &header[0..4] {
            b"VP8X" => {
                if let Some(body) = data.exact(body_at, 10)? {
                    let side = |i: usize| u64::from(u32::from_le_bytes([body[i], body[i + 1], body[i + 2], 0])) + 1;
                    dimensions(tags, side(4), side(7));
                }
            }
            b"VP8 " => {
                if let Some(body) = data.exact(body_at, 10)? {
                    let width = u16_le(body, 6).unwrap_or(0) & 0x3fff;
                    let height = u16_le(body, 8).unwrap_or(0) & 0x3fff;
                    dimensions(tags, u64::from(width), u64::from(height));
                }
            }
            b"VP8L" => {
                if let Some(body) = data.exact(body_at, 5)? {
                    let bits = u32_le(body, 1).unwrap_or(0);
                    dimensions(tags, u64::from(bits & 0x3fff) + 1, u64::from((bits >> 14) & 0x3fff) + 1);
                }
            }
            b"EXIF" if len <= MAX_PART => {
                exif = data
                    .exact(body_at, len)?
                    .map(|body| body.strip_prefix(b"Exif\0\0").unwrap_or(body).to_vec());
            }
            b"XMP " if len <= MAX_PART => packet = data.exact(body_at, len)?.map(<[u8]>::to_vec),
            _ => {}
        }
        // Chunks are padded to an even length.
        at = body_at + len + (len & 1);
    }
    put(tags, "MIMEType", json!("image/webp"));
    if let Some(exif) = exif {
        tiff::read(&exif, tags);
    }
    if let Some(packet) = packet {
        xmp::read(&packet, tags);
    }
    Ok(())
}

// --- GIF ---

pub fn gif(data: &Bytes, tags: &mut Tags) -> Result<(), Missing> {
    if let Some(header) = data.exact(6, 4)? {
        dimensions(
            tags,
            u64::from(u16_le(header, 0).unwrap_or(0)),
            u64::from(u16_le(header, 2).unwrap_or(0)),
        );
    }
    put(tags, "MIMEType", json!("image/gif"));
    Ok(())
}

// --- ISO base media (AVIF, MP4, MOV) ---

/// A box: its type, where its body starts and how long the body is.
#[derive(Debug, Clone, Copy)]
struct BoxHeader {
    kind: [u8; 4],
    body: u64,
    len: u64,
}

fn box_at(data: &Bytes, at: u64, end: u64) -> Result<Option<BoxHeader>, Missing> {
    if at + 8 > end {
        return Ok(None);
    }
    let Some(header) = data.exact(at, 8)? else {
        return Ok(None);
    };
    let size = u64::from(u32_be(header, 0).unwrap_or(0));
    let kind = [header[4], header[5], header[6], header[7]];
    let (body, total) = match size {
        1 => {
            let Some(large) = data.exact(at + 8, 8)? else {
                return Ok(None);
            };
            (at + 16, u64_be(large, 0).unwrap_or(0))
        }
        0 => (at + 8, end - at),
        size => (at + 8, size),
    };
    if total < body - at || at + total > end {
        return Ok(None);
    }
    Ok(Some(BoxHeader {
        kind,
        body,
        len: total - (body - at),
    }))
}

/// The child boxes of a box body (or of the file).
fn children(data: &Bytes, start: u64, end: u64) -> Result<Vec<BoxHeader>, Missing> {
    let mut found = Vec::new();
    let mut at = start;
    while found.len() < MAX_PARTS {
        let Some(header) = box_at(data, at, end)? else { break };
        at = header.body + header.len;
        found.push(header);
    }
    Ok(found)
}

fn child(boxes: &[BoxHeader], kind: &[u8; 4]) -> Option<BoxHeader> {
    boxes.iter().find(|b| &b.kind == kind).copied()
}

/// Seconds since 1904 (QuickTime's epoch) as exiftool prints dates; 0 means unset.
fn quicktime_date(seconds: u64) -> Option<String> {
    const FROM_1904: i64 = 2_082_844_800;
    let unix = i64::try_from(seconds).ok()? - FROM_1904;
    if seconds == 0 || unix <= 0 {
        return None;
    }
    let at = chrono::DateTime::from_timestamp(unix, 0)?;
    Some(at.format("%Y:%m:%d %H:%M:%S").to_string())
}

/// `mvhd`, `tkhd` and `mdhd` begin with a version byte; version 1 has 64-bit times.
fn times(body: &[u8]) -> Option<(u64, u32, u64, usize)> {
    match body.first()? {
        0 => Some((
            u64::from(u32_be(body, 4)?),
            u32_be(body, 12)?,
            u64::from(u32_be(body, 16)?),
            20,
        )),
        1 => Some((u64_be(body, 4)?, u32_be(body, 20)?, u64_be(body, 24)?, 32)),
        _ => None,
    }
}

const XMP_UUID: [u8; 16] = [
    0xbe, 0x7a, 0xcf, 0xcb, 0x97, 0xa9, 0x42, 0xe8, 0x9c, 0x71, 0x99, 0x94, 0x91, 0xe3, 0xaf, 0xac,
];

/// MP4, M4V and MOV: dates, duration, size and rotation from `moov`; XMP from a `uuid` box or `udta/XMP_`.
pub fn movie(data: &Bytes, mime: &str, tags: &mut Tags) -> Result<(), Missing> {
    let top = children(data, 0, data.len())?;
    put(tags, "MIMEType", Value::from(mime));
    let mut packet = None;
    for header in top.iter().filter(|b| &b.kind == b"uuid" && b.len <= MAX_PART) {
        if let Some(body) = data.exact(header.body, header.len)?
            && body.starts_with(&XMP_UUID)
        {
            packet = Some(body[16..].to_vec());
        }
    }
    if let Some(moov) = child(&top, b"moov").filter(|moov| moov.len <= 4 * MAX_PART) {
        let Some(body) = data.exact(moov.body, moov.len)? else {
            return Ok(());
        };
        let whole = Bytes::whole(body.to_vec());
        let boxes = children(&whole, 0, whole.len())?;
        if let Some(mvhd) = child(&boxes, b"mvhd")
            && let Some(mvhd) = whole.exact(mvhd.body, mvhd.len)?
            && let Some((created, timescale, duration, _)) = times(mvhd)
        {
            if let Some(date) = quicktime_date(created) {
                put(tags, "CreateDate", Value::from(date));
            }
            if timescale > 0 && duration > 0 {
                put(tags, "Duration", json!(duration as f64 / f64::from(timescale)));
            }
        }
        for trak in boxes.iter().filter(|b| &b.kind == b"trak") {
            let trak_boxes = children(&whole, trak.body, trak.body + trak.len)?;
            let Some(tkhd) = child(&trak_boxes, b"tkhd") else {
                continue;
            };
            let Some(tkhd) = whole.exact(tkhd.body, tkhd.len)? else {
                continue;
            };
            // Version, flags 4; creation, modification (4 or 8 each), track id 4, reserved 4, duration (4 or 8);
            // reserved 8, layer 2, group 2, volume 2, reserved 2; matrix 36, width 4, height 4 (16.16 fixed).
            let matrix = match tkhd.first() {
                Some(0) => 40,
                Some(1) => 52,
                _ => continue,
            };
            let (Some(a), Some(b)) = (u32_be(tkhd, matrix), u32_be(tkhd, matrix + 4)) else {
                continue;
            };
            let (Some(width), Some(height)) = (u32_be(tkhd, matrix + 36), u32_be(tkhd, matrix + 40)) else {
                continue;
            };
            let (width, height) = (u64::from(width >> 16), u64::from(height >> 16));
            if width == 0 || height == 0 {
                continue;
            }
            dimensions(tags, width, height);
            let fixed = |v: u32| f64::from(v as i32) / 65536.0;
            let rotation = (fixed(b).atan2(fixed(a)).to_degrees().round() as i64).rem_euclid(360);
            put(tags, "Rotation", json!(rotation));
            if let Some(mdia) = child(&trak_boxes, b"mdia")
                && let Some(mdhd) = child(&children(&whole, mdia.body, mdia.body + mdia.len)?, b"mdhd")
                && let Some(mdhd) = whole.exact(mdhd.body, mdhd.len)?
                && let Some(date) = times(mdhd).and_then(|(created, ..)| quicktime_date(created))
            {
                put(tags, "MediaCreateDate", Value::from(date));
            }
            break;
        }
        if packet.is_none() {
            packet = xmp::find_packet(body).map(<[u8]>::to_vec);
        }
    }
    if let Some(packet) = packet {
        xmp::read(&packet, tags);
    }
    Ok(())
}

/// AVIF (and other HEIF): the image size and rotation from `ispe` / `irot`, EXIF and XMP items located by `iloc`.
pub fn heif(data: &Bytes, tags: &mut Tags) -> Result<(), Missing> {
    let top = children(data, 0, data.len())?;
    put(tags, "MIMEType", json!("image/avif"));
    let Some(meta) = child(&top, b"meta").filter(|meta| meta.len <= MAX_PART) else {
        return Ok(());
    };
    let Some(body) = data.exact(meta.body, meta.len)? else {
        return Ok(());
    };
    let whole = Bytes::whole(body.to_vec());
    // `meta` is a full box: version and flags first.
    let boxes = children(&whole, 4, whole.len())?;
    if let Some(iprp) = child(&boxes, b"iprp")
        && let Some(ipco) = child(&children(&whole, iprp.body, iprp.body + iprp.len)?, b"ipco")
    {
        let properties = children(&whole, ipco.body, ipco.body + ipco.len)?;
        if let Some(ispe) = child(&properties, b"ispe")
            && let Some(ispe) = whole.exact(ispe.body, 12)?
        {
            dimensions(
                tags,
                u64::from(u32_be(ispe, 4).unwrap_or(0)),
                u64::from(u32_be(ispe, 8).unwrap_or(0)),
            );
        }
        if let Some(irot) = child(&properties, b"irot")
            && let Some(irot) = whole.exact(irot.body, 1)?
        {
            put(tags, "Rotation", json!(u32::from(irot[0] & 3) * 90));
        }
    }
    let items = item_types(&whole, &boxes)?;
    let locations = item_locations(&whole, &boxes)?;
    let (mut exif, mut packet) = (None, None);
    for (id, kind) in items {
        let Some(&(offset, len)) = locations.iter().find(|(item, _)| *item == id).map(|(_, at)| at) else {
            continue;
        };
        if len > MAX_PART {
            continue;
        }
        let Some(item) = data.exact(offset, len)? else { continue };
        match kind {
            ItemKind::Exif => {
                // Starts with the offset of the TIFF header.
                let skip = u32_be(item, 0).unwrap_or(0) as usize + 4;
                exif = item.get(skip..).map(<[u8]>::to_vec);
            }
            ItemKind::Xmp => packet = Some(item.to_vec()),
        }
    }
    if let Some(exif) = exif {
        tiff::read(&exif, tags);
    }
    if let Some(packet) = packet {
        xmp::read(&packet, tags);
    }
    Ok(())
}

enum ItemKind {
    Exif,
    Xmp,
}

/// Item ids of the EXIF and XMP items in `iinf`.
fn item_types(meta: &Bytes, boxes: &[BoxHeader]) -> Result<Vec<(u32, ItemKind)>, Missing> {
    let Some(iinf) = child(boxes, b"iinf") else {
        return Ok(Vec::new());
    };
    let Some(head) = meta.exact(iinf.body, 4)? else {
        return Ok(Vec::new());
    };
    let count_size = if head[0] == 0 { 2 } else { 4 };
    let entries = children(meta, iinf.body + 4 + count_size, iinf.body + iinf.len)?;
    let mut found = Vec::new();
    for infe in entries.iter().filter(|b| &b.kind == b"infe") {
        let Some(body) = meta.exact(infe.body, infe.len)? else {
            continue;
        };
        // Versions 2 and 3: id (2 or 4 bytes), protection index 2, item type 4, name, then a content type for `mime`.
        let (id, at) = match body.first() {
            Some(2) => (u32::from(u16_be(body, 4).unwrap_or(0)), 6),
            Some(3) => (u32_be(body, 4).unwrap_or(0), 8),
            _ => continue,
        };
        let Some(kind) = body.get(at + 2..at + 6) else { continue };
        if kind == b"Exif" {
            found.push((id, ItemKind::Exif));
        } else if kind == b"mime" {
            let rest = &body[at + 6..];
            let mut parts = rest.split(|&b| b == 0);
            let _name = parts.next();
            if parts.next() == Some(b"application/rdf+xml") {
                found.push((id, ItemKind::Xmp));
            }
        }
    }
    Ok(found)
}

/// An item's ID, and its file offset and length.
type ItemLocation = (u32, (u64, u64));

/// File offset and length of items stored in one extent in the file (`iloc` construction method 0).
fn item_locations(meta: &Bytes, boxes: &[BoxHeader]) -> Result<Vec<ItemLocation>, Missing> {
    let Some(iloc) = child(boxes, b"iloc") else {
        return Ok(Vec::new());
    };
    let Some(body) = meta.exact(iloc.body, iloc.len)? else {
        return Ok(Vec::new());
    };
    let parse = || -> Option<Vec<ItemLocation>> {
        let version = *body.first()?;
        let sizes = *body.get(4)?;
        let (offset_size, length_size) = (usize::from(sizes >> 4), usize::from(sizes & 15));
        let more = *body.get(5)?;
        let (base_size, index_size) = (
            usize::from(more >> 4),
            if version >= 1 { usize::from(more & 15) } else { 0 },
        );
        let read = |at: usize, size: usize| -> Option<u64> {
            match size {
                0 => Some(0),
                4 => u32_be(body, at).map(u64::from),
                8 => u64_be(body, at),
                _ => None,
            }
        };
        let (count, mut at) = if version < 2 {
            (u32::from(u16_be(body, 6)?), 8)
        } else {
            (u32_be(body, 6)?, 10)
        };
        let mut found = Vec::new();
        for _ in 0..count.min(MAX_PARTS as u32) {
            let id = if version < 2 {
                let id = u32::from(u16_be(body, at)?);
                at += 2;
                id
            } else {
                let id = u32_be(body, at)?;
                at += 4;
                id
            };
            let method = if version >= 1 {
                let method = u16_be(body, at)? & 15;
                at += 2;
                method
            } else {
                0
            };
            at += 2; // data reference index
            let base = read(at, base_size)?;
            at += base_size;
            let extents = u16_be(body, at)?;
            at += 2;
            let mut first = None;
            for _ in 0..extents {
                at += index_size;
                let offset = read(at, offset_size)?;
                at += offset_size;
                let length = read(at, length_size)?;
                at += length_size;
                first.get_or_insert((base + offset, length));
            }
            if method == 0 && extents == 1 {
                found.extend(first.map(|location| (id, location)));
            }
        }
        Some(found)
    };
    Ok(parse().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiff::tests::{ascii, tiff};

    fn jpeg_with(segments: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let mut out = vec![0xff, 0xd8];
        for (marker, body) in segments {
            out.extend([0xff, *marker]);
            out.extend(((body.len() + 2) as u16).to_be_bytes());
            out.extend(body);
        }
        out.extend([0xff, 0xda, 0, 2]);
        out
    }

    fn iptc(keywords: &[&str]) -> Vec<u8> {
        let mut iim = Vec::new();
        for keyword in keywords {
            iim.extend([0x1c, 2, 25]);
            iim.extend((keyword.len() as u16).to_be_bytes());
            iim.extend(keyword.as_bytes());
        }
        let mut out = b"Photoshop 3.0\08BIM".to_vec();
        out.extend(0x0404u16.to_be_bytes());
        out.extend([0, 0]);
        out.extend((iim.len() as u32).to_be_bytes());
        out.extend(&iim);
        if iim.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    #[test]
    fn req_019_04_reads_a_jpegs_size_exif_iptc_and_xmp_in_exiftools_priority() {
        let mut exif = b"Exif\0\0".to_vec();
        exif.extend(tiff(
            &[(0x4746, 3, 1, 2u16.to_le_bytes().to_vec())],
            &[(0x8769, vec![(0x9003, 2, 20, ascii("2020:01:02 03:04:05"))])],
        ));
        let mut packet = XMP_SIGNATURE.to_vec();
        packet.extend(br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="5"/></rdf:RDF></x:xmpmeta>"#);
        let mut frame = vec![8];
        frame.extend(480u16.to_be_bytes());
        frame.extend(640u16.to_be_bytes());
        frame.push(3);
        let file = jpeg_with(&[
            (0xe1, exif),
            (0xe1, packet),
            (0xed, iptc(&["Beach", "Sea"])),
            (0xc0, frame),
        ]);
        let mut tags = Tags::new();
        jpeg(&Bytes::whole(file), &mut tags).unwrap();
        assert_eq!(tags["ImageWidth"], json!(640));
        assert_eq!(tags["ImageHeight"], json!(480));
        // EXIF's rating wins over XMP's.
        assert_eq!(tags["Rating"], json!(2));
        assert_eq!(tags["Keywords"], json!(["Beach", "Sea"]));
        assert_eq!(tags["DateTimeOriginal"], json!("2020:01:02 03:04:05"));
    }

    #[test]
    fn asks_for_the_bytes_it_needs() {
        let file = jpeg_with(&[(0xe1, vec![0; 60_000]), (0xc0, vec![8, 0, 10, 0, 20, 3])]);
        let mut bytes = Bytes::new(file.len() as u64);
        bytes.add(0, file[..1000].to_vec());
        let Err(missing) = jpeg(&bytes, &mut Tags::new()) else {
            panic!("read without the bytes")
        };
        bytes.add(
            missing.at,
            file[missing.at as usize..(missing.at + missing.len) as usize].to_vec(),
        );
        bytes.add(
            missing.at + missing.len,
            file[(missing.at + missing.len) as usize..].to_vec(),
        );
        let mut tags = Tags::new();
        jpeg(&bytes, &mut tags).unwrap();
        assert_eq!(tags["ImageWidth"], json!(20));
    }

    /// REQ-011-10: truncated and garbage files don't panic.
    #[test]
    fn req_011_10_survives_truncated_and_garbage_files() {
        let file = jpeg_with(&[(0xed, iptc(&["Beach"])), (0xc0, vec![8, 0, 10, 0, 20, 3])]);
        for cut in 0..file.len() {
            let mut tags = Tags::new();
            jpeg(&Bytes::whole(file[..cut].to_vec()), &mut tags).unwrap();
            for read in [png, webp, gif, heif] {
                read(&Bytes::whole(file[..cut].to_vec()), &mut tags).unwrap();
            }
            movie(&Bytes::whole(file[..cut].to_vec()), "video/mp4", &mut tags).unwrap();
        }
    }

    #[test]
    fn req_019_04_reads_a_movies_date_duration_size_and_rotation() {
        fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
            out.extend(kind);
            out.extend(body);
            out
        }
        // 2024-07-02 07:12:59 UTC in seconds since 1904.
        let created: u32 = (1_719_904_379i64 + 2_082_844_800) as u32;
        let mut mvhd = vec![0, 0, 0, 0];
        mvhd.extend(created.to_be_bytes());
        mvhd.extend(created.to_be_bytes());
        mvhd.extend(1000u32.to_be_bytes());
        mvhd.extend(12_500u32.to_be_bytes());
        mvhd.extend([0; 80]);
        let mut tkhd = vec![0, 0, 0, 7];
        tkhd.extend([0; 16]);
        tkhd.extend([0; 4]);
        tkhd.extend([0; 16]);
        // A 90° rotation: a = 0, b = 1, c = -1.
        for value in [0i32, 0x10000, 0, -0x10000, 0, 0, 0, 0, 0x4000_0000] {
            tkhd.extend(value.to_be_bytes());
        }
        tkhd.extend((1920u32 << 16).to_be_bytes());
        tkhd.extend((1080u32 << 16).to_be_bytes());
        let trak = boxed(b"trak", &boxed(b"tkhd", &tkhd));
        let moov = boxed(b"moov", &[boxed(b"mvhd", &mvhd), trak].concat());
        let file = [boxed(b"ftyp", b"isom"), boxed(b"mdat", &[0; 32]), moov].concat();
        let mut tags = Tags::new();
        movie(&Bytes::whole(file), "video/mp4", &mut tags).unwrap();
        assert_eq!(tags["CreateDate"], json!("2024:07:02 07:12:59"));
        assert_eq!(tags["Duration"], json!(12.5));
        assert_eq!(tags["ImageWidth"], json!(1920));
        assert_eq!(tags["Rotation"], json!(90));
        assert_eq!(tags["MIMEType"], json!("video/mp4"));
    }
}
