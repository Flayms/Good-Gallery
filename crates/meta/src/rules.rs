//! Metadata of a media file from exiftool-shaped tags (spec 004), with the parts of exiftool-vendored's date parsing
//! and time-zone inference that earlier versions relied on. The desktop app gets the tags from exiftool, the web build
//! (spec 019) from this crate's readers.

use chrono::{FixedOffset, Local, NaiveDate, NaiveDateTime, TimeZone};
use domain::edit::Geometry;
use domain::faces::FaceRegion;
use domain::media::MediaMetadata;
use domain::tags::{PEOPLE_TAG_ROOT, clean_tag, normalize_tag};
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::sync::LazyLock;

/// Tags by exiftool's names, as `exiftool -json -n -api struct=1` returns them.
pub type Tags = Map<String, Value>;

const DATE_TAGS: [&str; 4] = [
    "SubSecDateTimeOriginal",
    "DateTimeOriginal",
    "CreateDate",
    "MediaCreateDate",
];

fn positive(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64).filter(|n| n.is_finite() && *n > 0.0)
}

/// Strings and numbers as text (exiftool prints numeric-looking keywords such as "2024" as numbers); one value or a
/// list. Anything else is ignored.
fn text_values(value: Option<&Value>) -> Vec<String> {
    let one = |value: &Value| match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(one).collect(),
        Some(value) => one(value).into_iter().collect(),
        None => Vec::new(),
    }
}

/// Merges tags from all sources into deduplicated tag paths, outermost first.
pub fn extract_tags(sources: &[&Tags]) -> Vec<Vec<String>> {
    let mut hierarchical: Vec<Vec<String>> = Vec::new();
    let mut flat: Vec<String> = Vec::new();
    let mut people: Vec<String> = Vec::new();
    for tags in sources {
        for value in text_values(tags.get("HierarchicalSubject")) {
            hierarchical.push(value.split('|').map(String::from).collect());
        }
        flat.extend(text_values(tags.get("Keywords")));
        flat.extend(text_values(tags.get("Subject")));
        for value in text_values(tags.get("XPKeywords")) {
            flat.extend(value.split(';').map(String::from));
        }
        // Face regions: an IPTC field plus the MWG and Microsoft Photo region structs (Lightroom, Picasa, digiKam,
        // Windows Photo Gallery). With `-api struct=1` exiftool returns the structs; the flattened fields are what
        // it returns without, read too so either form works.
        people.extend(text_values(tags.get("PersonInImage")));
        people.extend(text_values(tags.get("RegionPersonDisplayName")));
        let types = text_values(tags.get("RegionType"));
        for (i, name) in text_values(tags.get("RegionName")).into_iter().enumerate() {
            if types.get(i).map(String::as_str) == Some("Face") {
                people.push(name);
            }
        }
        people.extend(face_regions(tags));
    }

    let mut paths: Vec<Vec<String>> = Vec::new();
    let mut keys = HashSet::new();
    let mut add = |path: &[String]| {
        let names: Vec<String> = path
            .iter()
            .map(|name| clean_tag(name))
            .filter(|name| !name.is_empty())
            .collect();
        let key = names
            .iter()
            .map(|name| normalize_tag(name))
            .collect::<Vec<_>>()
            .join("|");
        if !names.is_empty() && keys.insert(key) {
            paths.push(names);
        }
    };
    for path in &hierarchical {
        add(path);
    }
    for name in &people {
        add(&[PEOPLE_TAG_ROOT.to_string(), name.clone()]);
    }
    // Lightroom also writes every hierarchy level (and some tools every face name) as a flat keyword.
    let named: HashSet<String> = hierarchical
        .iter()
        .flatten()
        .chain(&people)
        .map(|name| normalize_tag(name))
        .collect();
    for name in &flat {
        if !named.contains(&normalize_tag(name)) {
            add(std::slice::from_ref(name));
        }
    }
    paths
}

/// Names of `Face` regions in the MWG struct (`RegionInfo.RegionList[]`) and of all regions in the Microsoft Photo
/// struct (`RegionInfoMP.Regions[]`, which only holds people). Earlier versions read only the flattened fields, which
/// exiftool doesn't return with its own arguments, so they never found these (REQ-004-09).
fn face_regions(tags: &Tags) -> Vec<String> {
    let list = |value: Option<&Value>| match value {
        Some(Value::Array(items)) => items.clone(),
        Some(item @ Value::Object(_)) => vec![item.clone()],
        _ => Vec::new(),
    };
    let mut names = Vec::new();
    let mwg = tags.get("RegionInfo").and_then(|info| info.get("RegionList"));
    for region in list(mwg) {
        if region.get("Type").and_then(Value::as_str) == Some("Face") {
            names.extend(text_values(region.get("Name")));
        }
    }
    let microsoft = tags.get("RegionInfoMP").and_then(|info| info.get("Regions"));
    for region in list(microsoft) {
        names.extend(text_values(region.get("PersonDisplayName")));
    }
    names
}

/// Named face regions with their places on the upright photo (REQ-004-13): `Face` regions of the MWG struct with a
/// normalized `Area` (center and size), and Microsoft Photo regions with a `Rectangle` ("x, y, w, h" from the
/// top-left). Both are relative to the photo as stored.
fn face_positions(tags: &Tags, orientation: u8) -> Vec<FaceRegion> {
    let list = |value: Option<&Value>| match value {
        Some(Value::Array(items)) => items.clone(),
        Some(item @ Value::Object(_)) => vec![item.clone()],
        _ => Vec::new(),
    };
    let number = |value: Option<&Value>| {
        match value {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(text)) => text.trim().parse().ok(),
            _ => None,
        }
        .map(|n| n as f32)
    };
    let name = |region: &Value, field: &str| text_values(region.get(field)).into_iter().next();
    let mut faces = Vec::new();
    let mwg = tags.get("RegionInfo").and_then(|info| info.get("RegionList"));
    for region in list(mwg) {
        let area = region.get("Area");
        let field = |key: &str| number(area.and_then(|area| area.get(key)));
        let normalized = area.and_then(|area| area.get("Unit")).and_then(Value::as_str) == Some("normalized");
        if region.get("Type").and_then(Value::as_str) == Some("Face")
            && normalized
            && let (Some(name), Some(x), Some(y), Some(w), Some(h)) =
                (name(&region, "Name"), field("X"), field("Y"), field("W"), field("H"))
        {
            faces.extend(FaceRegion::from_center(&name, x, y, w, h, orientation));
        }
    }
    let microsoft = tags.get("RegionInfoMP").and_then(|info| info.get("Regions"));
    for region in list(microsoft) {
        let rectangle: Vec<f32> = region
            .get("Rectangle")
            .and_then(Value::as_str)
            .map(|text| text.split(',').filter_map(|n| n.trim().parse().ok()).collect())
            .unwrap_or_default();
        if let (Some(name), [x, y, w, h]) = (name(&region, "PersonDisplayName"), rectangle.as_slice()) {
            faces.extend(FaceRegion::new(&name, *x, *y, *w, *h, orientation));
        }
    }
    faces
}

/// The regions of a saved file, placed on its pixels (REQ-029-08).
#[derive(Debug, Clone, PartialEq)]
pub struct SavedRegions {
    /// The MWG `RegionInfo` struct; `None` if no region remains.
    pub info: Option<Value>,
    /// People whose every region was cropped away, so their names have to stay some other way.
    pub lost_people: Vec<String>,
}

/// `tags`' MWG and Microsoft Photo regions, on the photo as stored in the original, moved onto an output of `size` that
/// was turned upright by `orientation` and through `geometry`. Microsoft's become MWG's, so a stale copy can't disagree;
/// regions without a normalized area can't be placed and go. `None` if `tags` has no regions.
pub fn regions_on_output(
    tags: &Tags,
    orientation: u8,
    geometry: &Geometry,
    (width, height): (u32, u32),
) -> Option<SavedRegions> {
    let list = |value: Option<&Value>| match value {
        Some(Value::Array(items)) => items.clone(),
        Some(item @ Value::Object(_)) => vec![item.clone()],
        _ => Vec::new(),
    };
    let number = |value: Option<&Value>| match value {
        Some(Value::Number(n)) => n.as_f64().map(|n| n as f32),
        Some(Value::String(text)) => text.trim().parse().ok(),
        _ => None,
    };
    let mwg = list(tags.get("RegionInfo").and_then(|info| info.get("RegionList")));
    let microsoft = list(tags.get("RegionInfoMP").and_then(|info| info.get("Regions")));
    if mwg.is_empty() && microsoft.is_empty() {
        return None;
    }
    // Regions as (the struct's other fields, the area on the stored photo as `x, y, w, h`, the person's name).
    let mut found: Vec<(Value, Option<[f32; 4]>, Option<String>)> = Vec::new();
    for region in mwg {
        let area = region.get("Area");
        let field = |key: &str| number(area.and_then(|area| area.get(key)));
        let normalized = area.and_then(|area| area.get("Unit")).and_then(Value::as_str) == Some("normalized");
        let rect = match (normalized, field("X"), field("Y"), field("W"), field("H")) {
            (true, Some(x), Some(y), Some(w), Some(h)) => Some([x - w / 2.0, y - h / 2.0, w, h]),
            _ => None,
        };
        let face = region.get("Type").and_then(Value::as_str) == Some("Face");
        let name = face
            .then(|| text_values(region.get("Name")).into_iter().next())
            .flatten();
        found.push((region, rect, name));
    }
    let named_in_mwg: HashSet<String> = found.iter().filter_map(|(_, _, name)| name.clone()).collect();
    for region in microsoft {
        let Some(name) = text_values(region.get("PersonDisplayName")).into_iter().next() else {
            continue;
        };
        if named_in_mwg.contains(&name) {
            continue;
        }
        let rectangle: Vec<f32> = region
            .get("Rectangle")
            .and_then(Value::as_str)
            .map(|text| text.split(',').filter_map(|n| n.trim().parse().ok()).collect())
            .unwrap_or_default();
        let rect = <[f32; 4]>::try_from(rectangle.as_slice()).ok();
        let fields = serde_json::json!({ "Name": name, "Type": "Face" });
        found.push((fields, rect, Some(name)));
    }
    let mut regions = Vec::new();
    let mut lost_people = Vec::new();
    let mut kept_people = HashSet::new();
    for (mut region, rect, name) in found {
        let placed = rect
            .filter(|rect| rect.iter().all(|n| n.is_finite()) && rect[2] > 0.0 && rect[3] > 0.0)
            .and_then(|rect| geometry.place(domain::faces::upright_rect(rect, orientation)));
        let Some([x, y, w, h]) = placed else {
            lost_people.extend(name);
            continue;
        };
        region["Area"] = serde_json::json!({
            "X": x + w / 2.0, "Y": y + h / 2.0, "W": w, "H": h, "Unit": "normalized",
        });
        kept_people.extend(name);
        regions.push(region);
    }
    lost_people.retain(|name| !kept_people.contains(name));
    lost_people.sort();
    lost_people.dedup();
    let info = (!regions.is_empty()).then(|| {
        serde_json::json!({
            "AppliedToDimensions": { "W": width, "H": height, "Unit": "pixel" },
            "RegionList": regions,
        })
    });
    Some(SavedRegions { info, lost_people })
}

/// First valid rating; -1 (rejected) and 0 (unrated) both mean "no rating".
fn extract_rating(sources: &[&Tags]) -> Option<i64> {
    sources
        .iter()
        .find_map(|tags| tags.get("Rating").and_then(Value::as_f64).filter(|n| n.is_finite()))
        .and_then(|rating| (rating > 0.0).then(|| rating.round().min(5.0) as i64))
}

/// Sidecar values fill gaps in the embedded metadata; tags from both are merged.
pub fn extract_metadata(tags: &Tags, sidecar: Option<&Tags>) -> MediaMetadata {
    let mut sources = vec![tags];
    sources.extend(sidecar);
    let sidecar_orientation = sidecar.and_then(orientation);
    // The file's orientation, EXIF before XMP, else the sidecar's (REQ-004-12).
    let orientation = orientation(tags).or(sidecar_orientation).unwrap_or(1);
    let (mut width, mut height) = (None, None);
    if let (Some(w), Some(h)) = (positive(tags.get("ImageWidth")), positive(tags.get("ImageHeight"))) {
        // Orientations 5–8 and 90° / 270° video rotations swap the displayed axes.
        let rotation = tags.get("Rotation").and_then(Value::as_f64);
        let swapped = orientation >= 5 || rotation == Some(90.0) || rotation == Some(270.0);
        let (w, h) = if swapped { (h, w) } else { (w, h) };
        (width, height) = (Some(w as i64), Some(h as i64));
    }
    let taken_at = sources.iter().find_map(|source| {
        let zone = file_zone(source);
        DATE_TAGS.iter().find_map(|name| date_millis(source.get(*name)?, zone))
    });
    let edit = domain::edit::SavedEdit::parse(
        // Only the file's own: a copy of a sidecar doesn't carry another file's edit.
        tags.get("EditRecipe").and_then(Value::as_str),
        tags.get("EditSource").and_then(Value::as_str),
    )
    .map(Box::new);
    // A saved edit moved the file's regions onto its pixels, but a sidecar's still describe the unedited photo, whose
    // orientation the file no longer tells (REQ-029-08).
    let sidecar_faces = |sidecar: &Tags| {
        let Some(edit) = &edit else {
            return face_positions(sidecar, orientation);
        };
        let geometry = edit.recipe().geometry();
        face_positions(sidecar, sidecar_orientation.unwrap_or(1))
            .into_iter()
            .filter_map(|face| {
                let [x, y, w, h] = geometry.place([face.x, face.y, face.w, face.h])?;
                Some(FaceRegion { x, y, w, h, ..face })
            })
            .collect()
    };
    let faces = Some(face_positions(tags, orientation))
        .filter(|faces| !faces.is_empty())
        .or_else(|| sidecar.map(sidecar_faces))
        .unwrap_or_default();
    MediaMetadata {
        width,
        height,
        duration: positive(tags.get("Duration")),
        taken_at,
        rating: extract_rating(&sources),
        sidecar_orientation,
        tags: extract_tags(&sources),
        edit,
        faces,
    }
}

/// An orientation as EXIF numbers it (1–8); others count as none.
fn orientation(tags: &Tags) -> Option<u8> {
    let value = tags.get("Orientation")?.as_f64()?;
    (1.0..=8.0).contains(&value).then_some(value as u8)
}

// --- Dates, as exiftool-vendored (v38) turns them into instants ---

/// `2024:07:02 07:12:59.123+02:00`, also with `-` separators or `T`, without seconds, with `Z` / `UTC`.
static DATE_TIME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(\d{1,4})[:-](\d{1,2})[:-](\d{1,2})[ T](\d{1,2}):(\d{2})(?::(\d{2})(?:\.(\d+))?)?\s*((?:Z|UTC|GMT)|(?:UTC)?[+−-]\d\d?(?::?\d\d)?)?(?:\s+[A-Za-z]{2,5})?$",
    )
    .unwrap()
});

/// A trailing offset in a string (`exiftool-vendored`'s `tzRe`).
static OFFSET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:(Z)|(?:UTC)?([+−-])(\d\d?)(?::?(\d\d)(?::(\d\d))?)?)$").unwrap());

/// Offsets real places use (exiftool-vendored's `ValidTimezoneOffsets`), in minutes.
const VALID_OFFSETS: [i32; 38] = [
    -660, -600, -570, -540, -510, -480, -420, -360, -300, -240, -210, -180, -150, -120, -60, 0, 60, 120, 180, 210, 240,
    270, 300, 330, 345, 360, 390, 420, 480, 510, 525, 540, 570, 585, 600, 630, 660, 720,
];
const MORE_VALID_OFFSETS: [i32; 3] = [765, 780, 840];

fn is_valid_offset(minutes: i32) -> bool {
    VALID_OFFSETS.contains(&minutes) || MORE_VALID_OFFSETS.contains(&minutes)
}

/// The UTC offset (minutes) a value names: a number of hours, `Z` / `UTC` / `GMT`, or a trailing `±HH[:MM]`.
fn zone_of(value: &Value) -> Option<i32> {
    match value {
        Value::Array(items) => items.iter().find(|item| !item.is_null()).and_then(zone_of),
        Value::Number(hours) => {
            let minutes = (hours.as_f64()? * 60.0).round() as i32;
            is_valid_offset(minutes).then_some(minutes)
        }
        Value::String(text) => {
            let text = text.trim();
            if matches!(text, "Z" | "UTC" | "GMT" | "UTC+0" | "UTC-0") {
                return Some(0);
            }
            // Over-specified values end with an abbreviation ("+02:00 CEST"); it doesn't change the offset.
            let text = strip_abbreviation(text);
            let captures = OFFSET.captures(text)?;
            if captures.get(1).is_some() {
                return Some(0);
            }
            let sign = if &captures[2] == "+" { 1 } else { -1 };
            let number = |i: usize| captures.get(i).map_or(0, |m| m.as_str().parse::<i32>().unwrap_or(0));
            let minutes = sign * (number(3) * 60 + number(4));
            is_valid_offset(minutes).then_some(minutes)
        }
        _ => None,
    }
}

fn strip_abbreviation(text: &str) -> &str {
    static ABBREVIATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s[A-Za-z]{2,5}$").unwrap());
    static ENDS_IN_UTC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[.\d\s](?:UTC|Z)$").unwrap());
    if ENDS_IN_UTC.is_match(text) {
        return text;
    }
    ABBREVIATION.find(text).map_or(text, |m| &text[..m.start()])
}

/// Tags whose offset belongs to the capture date, in exiftool-vendored's order (`OffsetTime` is the file's
/// modification date, so it is left out on purpose).
const ZONE_TAGS: [&str; 4] = [
    "TimeZone",
    "OffsetTimeOriginal",
    "OffsetTimeDigitized",
    "TimeZoneOffset",
];

/// Capture dates that may carry an offset of their own.
const CAPTURED_AT: [&str; 8] = [
    "SubSecDateTimeOriginal",
    "SubSecCreateDate",
    "DateTimeOriginal",
    "CreateDate",
    "MediaCreateDate",
    "CreationDate",
    "DateTimeCreated",
    "TimeCreated",
];

fn is_video(tags: &Tags) -> bool {
    tags.get("MIMEType")
        .and_then(Value::as_str)
        .is_some_and(|mime| mime.starts_with("video/"))
}

/// The zone (UTC offset in minutes) dates without their own offset are in. `None` means the computer's local zone.
/// Order as in exiftool-vendored: zone tags, offsets of other capture dates, UTC for videos, the difference to a
/// GPS / UTC timestamp. Its GPS position lookup (geo-tz) is not ported, see spec 004's plan.
fn file_zone(tags: &Tags) -> Option<i32> {
    if is_video(tags) {
        // Videos store zoneless dates in UTC (`defaultVideosToUTC`), which also overrides any inferred zone.
        return Some(0);
    }
    let from_tags = ZONE_TAGS.iter().find_map(|name| {
        let minutes = zone_of(tags.get(*name)?)?;
        Some(minutes + daylight_saving_adjustment(tags))
    });
    from_tags
        .or_else(|| {
            CAPTURED_AT
                .iter()
                .find_map(|name| zone_of(tags.get(*name)?).filter(|minutes| *minutes != 0))
        })
        .or_else(|| zone_from_utc_timestamp(tags))
}

/// Nikon writes `TimeZone` without daylight saving time and says so in `DaylightSavings`.
fn daylight_saving_adjustment(tags: &Tags) -> i32 {
    static NIKON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bnikon\b").unwrap());
    let dst = match tags.get("DaylightSavings") {
        Some(Value::Bool(yes)) => *yes,
        Some(Value::String(text)) => matches!(text.to_lowercase().as_str(), "yes" | "true" | "on" | "1"),
        Some(Value::Number(n)) => n.as_f64() == Some(1.0),
        _ => false,
    };
    let make = tags.get("Make").and_then(Value::as_str).unwrap_or_default();
    if dst && NIKON.is_match(make) { 60 } else { 0 }
}

/// The offset between a zoneless capture date and a UTC timestamp of the same moment (GPS), rounded to the nearest
/// real offset within 30 minutes.
fn zone_from_utc_timestamp(tags: &Tags) -> Option<i32> {
    let text = |name: &str| tags.get(name).and_then(Value::as_str).filter(|s| !s.trim().is_empty());
    let gps_stamp = text("GPSDateStamp")
        .zip(text("GPSTimeStamp"))
        .map(|(d, t)| format!("{d} {t}"));
    let utc = [
        text("GPSDateTime").map(String::from),
        text("DateTimeUTC").map(String::from),
        gps_stamp,
        text("SonyDateTime2").map(String::from),
    ]
    .into_iter()
    .flatten()
    .find_map(|value| {
        let (local, offset) = parse_date_time(&value)?;
        (offset.is_none() || offset == Some(0)).then(|| local.and_utc().timestamp())
    })?;
    let captured = CAPTURED_AT.iter().find_map(|name| {
        let (local, offset) = parse_date_time(text(name)?)?;
        offset.is_none().then(|| local.and_utc().timestamp())
    })?;
    let delta = (captured - utc) as f64 / 60.0;
    VALID_OFFSETS
        .iter()
        .chain(&MORE_VALID_OFFSETS)
        .map(|offset| (*offset, (f64::from(*offset) - delta).abs()))
        .filter(|(_, diff)| *diff <= 30.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(offset, _)| offset)
}

/// A date with a time, and its own offset in minutes if it has one.
fn parse_date_time(text: &str) -> Option<(NaiveDateTime, Option<i32>)> {
    let c = DATE_TIME.captures(text.trim())?;
    let number = |i: usize| c.get(i).map_or(Some(0), |m| m.as_str().parse::<u32>().ok());
    let year = number(1)? as i32;
    // Year 0 and 1 are placeholders of cameras without a clock.
    if year <= 1 {
        return None;
    }
    let millis = c.get(7).map_or(0, |m| {
        let digits: String = m.as_str().chars().chain("000".chars()).take(3).collect();
        digits.parse::<u32>().unwrap_or(0)
    });
    let date = NaiveDate::from_ymd_opt(year, number(2)?, number(3)?)?;
    let time = chrono::NaiveTime::from_hms_milli_opt(number(4)?, number(5)?, number(6)?, millis)?;
    let offset = c.get(8).and_then(|m| zone_of(&Value::String(m.as_str().to_string())));
    Some((NaiveDateTime::new(date, time), offset))
}

/// Milliseconds since the epoch of a capture date; zeroed and pre-1970 dates count as missing.
fn date_millis(value: &Value, zone: Option<i32>) -> Option<i64> {
    let (local, own_offset) = parse_date_time(value.as_str()?)?;
    let millis = match own_offset.or(zone) {
        Some(minutes) => FixedOffset::east_opt(minutes * 60)?
            .from_local_datetime(&local)
            .single()?
            .timestamp_millis(),
        None => local_millis(&local)?,
    };
    (millis > 0).then_some(millis)
}

/// A wall-clock time in the computer's zone. A time in the spring-forward gap uses the offset before the gap (it moves
/// forward, as Luxon does); a repeated autumn time takes its first occurrence.
fn local_millis(local: &NaiveDateTime) -> Option<i64> {
    match Local.from_local_datetime(local) {
        chrono::LocalResult::Single(at) | chrono::LocalResult::Ambiguous(at, _) => Some(at.timestamp_millis()),
        chrono::LocalResult::None => {
            let before = Local
                .from_local_datetime(&(*local - chrono::Duration::hours(3)))
                .earliest()?;
            Some(local.and_utc().timestamp_millis() - i64::from(before.offset().local_minus_utc()) * 1000)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::edit::Recipe;
    use serde_json::json;

    fn tags(value: Value) -> Tags {
        value.as_object().unwrap().clone()
    }

    fn strings(paths: &[&[&str]]) -> Vec<Vec<String>> {
        paths
            .iter()
            .map(|path| path.iter().map(|s| s.to_string()).collect())
            .collect()
    }

    fn utc(text: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(text).unwrap().timestamp_millis()
    }

    #[test]
    fn req_004_06_merges_keyword_fields_case_insensitively_and_keeps_the_first_spelling() {
        let t =
            tags(json!({ "Keywords": ["Beach", " sunset "], "Subject": ["beach"], "XPKeywords": "Family;;SUNSET" }));

        assert_eq!(extract_tags(&[&t]), strings(&[&["Beach"], &["sunset"], &["Family"]]));
    }

    #[test]
    fn req_004_07_keeps_hierarchies_and_drops_flat_keywords_that_are_part_of_one() {
        let t = tags(json!({
            "HierarchicalSubject": ["Places|Germany|Berlin", "Places|Germany"],
            "Keywords": ["Places", "Germany", "Berlin", "Travel"],
        }));

        assert_eq!(
            extract_tags(&[&t]),
            strings(&[&["Places", "Germany", "Berlin"], &["Places", "Germany"], &["Travel"]])
        );
    }

    #[test]
    fn req_021_11_reads_the_saved_edit_from_the_file_but_not_from_its_sidecar() {
        let recipe = domain::edit::Recipe::default().to_json();
        let file = tags(json!({ "EditRecipe": recipe, "EditSource": ".good-gallery/originals/a.jpg" }));
        let edit = extract_metadata(&file, None).edit.unwrap();
        assert_eq!(edit.source, ".good-gallery/originals/a.jpg");
        assert_eq!(extract_metadata(&tags(json!({})), Some(&file)).edit, None);
        let outside = tags(json!({ "EditRecipe": recipe, "EditSource": "../a.jpg" }));
        assert_eq!(extract_metadata(&outside, None).edit, None);
    }

    #[test]
    fn req_004_06_accepts_numeric_keywords() {
        assert_eq!(
            extract_tags(&[&tags(json!({ "Keywords": [2024, "x"] }))]),
            strings(&[&["2024"], &["x"]])
        );
    }

    #[test]
    fn req_004_09_adds_face_region_names_as_people_tags_from_any_of_the_fields_apps_use() {
        let people = strings(&[&["People", "Anton"]]);
        assert_eq!(extract_tags(&[&tags(json!({ "PersonInImage": "Anton" }))]), people);
        assert_eq!(
            extract_tags(&[&tags(json!({ "RegionPersonDisplayName": ["Anton"] }))]),
            people
        );
        let regions = tags(json!({ "RegionName": ["Anton", "My Dog"], "RegionType": ["Face", "Pet"] }));
        assert_eq!(extract_tags(&[&regions]), people);
    }

    #[test]
    fn req_004_09_does_not_duplicate_a_face_region_already_covered_by_a_keyword_hierarchy() {
        let t =
            tags(json!({ "HierarchicalSubject": ["People|Anton"], "PersonInImage": "Anton", "Keywords": ["Anton"] }));

        assert_eq!(extract_tags(&[&t]), strings(&[&["People", "Anton"]]));
    }

    /// REQ-004-09: the region structs exiftool returns with `-api struct=1`.
    #[test]
    fn req_004_09_reads_face_regions_from_the_mwg_and_microsoft_structs() {
        let t = tags(json!({
            "RegionInfo": { "RegionList": [{ "Name": "Anna", "Type": "Face" }, { "Name": "Rex", "Type": "Pet" }] },
            "RegionInfoMP": { "Regions": [{ "PersonDisplayName": "Ben" }] },
        }));

        assert_eq!(extract_tags(&[&t]), strings(&[&["People", "Anna"], &["People", "Ben"]]));
    }

    fn rect(face: &FaceRegion) -> (&str, [f32; 4]) {
        let round = |n: f32| (n * 100.0).round() / 100.0;
        (face.name.as_str(), [face.x, face.y, face.w, face.h].map(round))
    }

    /// REQ-004-13: the region structs as exiftool returns them with `-n -api struct=1`.
    /// REQ-029-08: a sidecar's regions describe the unedited photo, so they follow the file's saved crop and turn.
    #[test]
    fn req_029_08_moves_a_sidecars_regions_by_the_saved_edit() {
        let recipe = Recipe {
            crop: Some([0.5, 0.0, 0.5, 1.0]),
            ..Default::default()
        };
        let file = tags(json!({ "EditRecipe": recipe.to_json(), "EditSource": "a.jpg" }));
        let sidecar = tags(json!({ "RegionInfo": { "RegionList": [
            { "Type": "Face", "Name": "Anna", "Area": { "X": 0.75, "Y": 0.5, "W": 0.2, "H": 0.2, "Unit": "normalized" } },
            { "Type": "Face", "Name": "Bob", "Area": { "X": 0.25, "Y": 0.5, "W": 0.2, "H": 0.2, "Unit": "normalized" } },
        ] } }));
        let faces = extract_metadata(&file, Some(&sidecar)).faces;
        assert_eq!(faces.len(), 1, "Bob is cropped away: {faces:?}");
        assert_eq!(rect(&faces[0]), ("Anna", [0.3, 0.4, 0.4, 0.2]));
    }

    /// REQ-029-08: regions land on the saved pixels; Microsoft's become MWG's, and people left without a region are
    /// named so that their People tag stays.
    #[test]
    fn req_029_08_places_regions_on_the_output_and_names_who_is_gone() {
        let source = tags(json!({
            "RegionInfo": { "RegionList": [
                { "Type": "Face", "Name": "Anna", "Area": { "X": 0.75, "Y": 0.5, "W": 0.2, "H": 0.2, "Unit": "normalized" } },
                { "Type": "Face", "Name": "Bob", "Area": { "X": 0.25, "Y": 0.5, "W": 0.2, "H": 0.2, "Unit": "normalized" } },
                { "Type": "Focus", "Area": { "X": 0.8, "Y": 0.5, "W": 0.1, "H": 0.1, "Unit": "normalized" } },
            ] },
            "RegionInfoMP": { "Regions": [
                { "PersonDisplayName": "Anna", "Rectangle": "0.65, 0.4, 0.2, 0.2" },
                { "PersonDisplayName": "Cleo", "Rectangle": "0.5, 0, 0.1, 0.1" },
            ] },
        }));
        let geometry = Geometry::new(Some([0.5, 0.0, 0.5, 1.0]), 0);
        let saved = regions_on_output(&source, 1, &geometry, (50, 100)).unwrap();
        assert_eq!(saved.lost_people, ["Bob"]);
        let info = saved.info.unwrap();
        assert_eq!(info["AppliedToDimensions"]["W"], 50);
        let names: Vec<_> = info["RegionList"]
            .as_array()
            .unwrap()
            .iter()
            .map(|region| region["Name"].as_str().unwrap_or("-"))
            .collect();
        assert_eq!(names, ["Anna", "-", "Cleo"], "Anna once, the focus region kept");
        let anna = &info["RegionList"][0]["Area"];
        assert!((anna["X"].as_f64().unwrap() - 0.5).abs() < 1e-5);
        assert!((anna["W"].as_f64().unwrap() - 0.4).abs() < 1e-5);
        assert_eq!(regions_on_output(&tags(json!({})), 1, &geometry, (50, 100)), None);
    }

    #[test]
    fn req_004_13_keeps_named_face_regions_with_their_place_on_the_upright_photo() {
        let area = |x: f64, y: f64, unit: &str| json!({ "X": x, "Y": y, "W": 0.2, "H": 0.4, "Unit": unit });
        let file = tags(json!({
            "Orientation": 6,
            "RegionInfo": { "RegionList": [
                { "Name": "Anna", "Type": "Face", "Area": area(0.3, 0.4, "normalized") },
                { "Name": "Rex", "Type": "Pet", "Area": area(0.5, 0.5, "normalized") },
                { "Name": "Pixels", "Type": "Face", "Area": area(30.0, 40.0, "pixel") },
                { "Type": "Face", "Area": area(0.5, 0.5, "normalized") },
            ] },
            "RegionInfoMP": { "Regions": [
                { "PersonDisplayName": "Ben", "Rectangle": "0.1, 0.2, 0.3, 0.4" },
                { "PersonDisplayName": "Broken", "Rectangle": "0.1, 0.2" },
            ] },
        }));

        let faces = extract_metadata(&file, None).faces;
        // Turned 90° clockwise: the stored photo's left edge is the upright photo's top.
        assert_eq!(
            faces.iter().map(rect).collect::<Vec<_>>(),
            [("Anna", [0.4, 0.2, 0.4, 0.2]), ("Ben", [0.4, 0.1, 0.4, 0.3])]
        );

        // From the sidecar when the file has none.
        let sidecar = tags(
            json!({ "RegionInfoMP": { "Regions": { "PersonDisplayName": "Cleo", "Rectangle": "0, 0, 0.5, 0.5" } } }),
        );
        let faces = extract_metadata(&tags(json!({})), Some(&sidecar)).faces;
        assert_eq!(
            faces.iter().map(rect).collect::<Vec<_>>(),
            [("Cleo", [0.0, 0.0, 0.5, 0.5])]
        );
    }

    #[test]
    fn a_time_in_the_spring_forward_gap_still_has_an_instant() {
        let local = NaiveDate::from_ymd_opt(2024, 3, 31)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        // Whatever the machine's zone, every local time maps to some instant.
        assert!(local_millis(&local).is_some());
    }

    #[test]
    fn req_004_02_swaps_dimensions_for_rotated_images_and_videos() {
        let rotated = extract_metadata(
            &tags(json!({ "ImageWidth": 4000, "ImageHeight": 3000, "Orientation": 6 })),
            None,
        );
        assert_eq!((rotated.width, rotated.height), (Some(3000), Some(4000)));
        let upright = extract_metadata(
            &tags(json!({ "ImageWidth": 4000, "ImageHeight": 3000, "Orientation": 1 })),
            None,
        );
        assert_eq!((upright.width, upright.height), (Some(4000), Some(3000)));
        let video = extract_metadata(
            &tags(json!({ "ImageWidth": 1920, "ImageHeight": 1080, "Rotation": 270 })),
            None,
        );
        assert_eq!((video.width, video.height), (Some(1080), Some(1920)));
        let unknown = extract_metadata(&tags(json!({ "ImageWidth": 0, "ImageHeight": 3000 })), None);
        assert_eq!((unknown.width, unknown.height), (None, None));
    }

    /// REQ-004-12: an orientation only in the sidecar swaps the dimensions too and is kept for the decoders; the
    /// file's own (EXIF, else its embedded XMP, as exiftool prefers them) wins.
    #[test]
    fn req_004_12_takes_the_orientation_from_the_sidecar_when_the_file_has_none() {
        let file = tags(json!({ "ImageWidth": 4000, "ImageHeight": 3000 }));
        let turned = tags(json!({ "Orientation": 6 }));
        let from_sidecar = extract_metadata(&file, Some(&turned));
        assert_eq!((from_sidecar.width, from_sidecar.height), (Some(3000), Some(4000)));
        assert_eq!(from_sidecar.sidecar_orientation, Some(6));

        let upright = tags(json!({ "ImageWidth": 4000, "ImageHeight": 3000, "Orientation": 1 }));
        let own_wins = extract_metadata(&upright, Some(&turned));
        assert_eq!((own_wins.width, own_wins.height), (Some(4000), Some(3000)));

        let invalid = extract_metadata(&file, Some(&tags(json!({ "Orientation": 9 }))));
        assert_eq!((invalid.width, invalid.sidecar_orientation), (Some(4000), None));
    }

    #[test]
    fn req_004_03_prefers_embedded_capture_dates_and_falls_back_to_the_sidecar() {
        let embedded =
            tags(json!({ "DateTimeOriginal": "2020:01:02 03:04:05Z", "CreateDate": "2021:01:01 00:00:00Z" }));
        let sidecar = tags(json!({ "DateTimeOriginal": "2019:01:01 00:00:00Z" }));

        assert_eq!(
            extract_metadata(&embedded, Some(&sidecar)).taken_at,
            Some(utc("2020-01-02T03:04:05Z"))
        );
        assert_eq!(
            extract_metadata(&Tags::new(), Some(&sidecar)).taken_at,
            Some(utc("2019-01-01T00:00:00Z"))
        );
        let zeroed = tags(json!({ "DateTimeOriginal": "0000:00:00 00:00:00" }));
        assert_eq!(extract_metadata(&zeroed, None).taken_at, None);
        let before_1970 = tags(json!({ "DateTimeOriginal": "1960:01:01 00:00:00Z" }));
        assert_eq!(extract_metadata(&before_1970, None).taken_at, None);
    }

    #[test]
    fn req_004_03_takes_the_first_valid_date_in_order_of_preference() {
        let t = tags(json!({
            "DateTimeOriginal": "garbage",
            "CreateDate": "2021:05:06 07:08:09+01:00",
            "SubSecDateTimeOriginal": "2021:05:06 07:08:09.250+02:00",
        }));

        assert_eq!(
            extract_metadata(&t, None).taken_at,
            Some(utc("2021-05-06T05:08:09.250Z"))
        );
    }

    #[test]
    fn req_004_03_uses_offset_tags_for_zoneless_dates() {
        let t = tags(json!({ "DateTimeOriginal": "2021:05:06 07:08:09", "OffsetTimeOriginal": "-05:00" }));
        assert_eq!(extract_metadata(&t, None).taken_at, Some(utc("2021-05-06T12:08:09Z")));

        let numeric = tags(json!({ "DateTimeOriginal": "2021:05:06 07:08:09", "TimeZoneOffset": [9, 9] }));
        assert_eq!(
            extract_metadata(&numeric, None).taken_at,
            Some(utc("2021-05-05T22:08:09Z"))
        );

        let nikon = tags(json!({
            "DateTimeOriginal": "2021:05:06 07:08:09", "TimeZone": "+01:00", "DaylightSavings": "Yes",
            "Make": "NIKON CORPORATION",
        }));
        assert_eq!(
            extract_metadata(&nikon, None).taken_at,
            Some(utc("2021-05-06T05:08:09Z"))
        );
    }

    #[test]
    fn req_004_03_infers_the_zone_from_another_capture_date_or_the_gps_time() {
        let other =
            tags(json!({ "DateTimeOriginal": "2021:05:06 07:08:09", "CreateDate": "2021:05:06 07:08:09+03:00" }));
        assert_eq!(
            extract_metadata(&other, None).taken_at,
            Some(utc("2021-05-06T04:08:09Z"))
        );

        let gps = tags(json!({ "DateTimeOriginal": "2023:01:15 11:30:45", "GPSDateTime": "2023:01:15 19:29:58Z" }));
        assert_eq!(extract_metadata(&gps, None).taken_at, Some(utc("2023-01-15T19:30:45Z")));
    }

    #[test]
    fn req_004_03_reads_zoneless_video_dates_as_utc() {
        let t = tags(
            json!({ "MIMEType": "video/mp4", "CreateDate": "2021:05:06 07:08:09", "OffsetTimeOriginal": "+02:00" }),
        );

        assert_eq!(extract_metadata(&t, None).taken_at, Some(utc("2021-05-06T07:08:09Z")));
    }

    #[test]
    fn req_004_03_reads_zoneless_photo_dates_in_local_time() {
        let t = tags(json!({ "DateTimeOriginal": "2021:05:06 07:08:09" }));
        let local = NaiveDate::from_ymd_opt(2021, 5, 6)
            .unwrap()
            .and_hms_opt(7, 8, 9)
            .unwrap();

        assert_eq!(
            extract_metadata(&t, None).taken_at,
            Some(Local.from_local_datetime(&local).earliest().unwrap().timestamp_millis())
        );
    }

    #[test]
    fn req_004_04_reads_video_duration() {
        assert_eq!(
            extract_metadata(&tags(json!({ "Duration": 12.5 })), None).duration,
            Some(12.5)
        );
        assert_eq!(
            extract_metadata(&tags(json!({ "Duration": "0 s" })), None).duration,
            None
        );
    }

    #[test]
    fn req_004_05_normalizes_the_rating_falling_back_to_the_sidecar() {
        let rating = |t: Value, sidecar: Option<Value>| extract_metadata(&tags(t), sidecar.map(tags).as_ref()).rating;
        assert_eq!(rating(json!({ "Rating": 5 }), None), Some(5));
        assert_eq!(rating(json!({ "Rating": 2.4 }), None), Some(2));
        assert_eq!(rating(json!({ "Rating": 7 }), None), Some(5));
        assert_eq!(rating(json!({ "Rating": 0 }), None), None);
        assert_eq!(rating(json!({ "Rating": -1 }), None), None);
        assert_eq!(rating(json!({}), None), None);
        assert_eq!(rating(json!({}), Some(json!({ "Rating": 3 }))), Some(3));
    }

    /// REQ-011-10: values of unexpected types and absurd sizes are ignored or kept, never a panic.
    #[test]
    fn req_011_10_survives_hostile_values() {
        let long = "x".repeat(100_000);
        let deep = vec!["a"; 10_000].join("|");
        let t = tags(json!({
            "ImageWidth": "wide", "ImageHeight": [1, 2], "Orientation": {"a": 1}, "Rating": "five",
            "Keywords": [null, true, {"x": 1}, long], "HierarchicalSubject": deep, "RegionName": "Anton",
            "RegionType": 7, "DateTimeOriginal": 2021, "CreateDate": "9999:99:99 99:99:99", "Duration": -3,
            "TimeZoneOffset": "nonsense",
        }));

        let metadata = extract_metadata(&t, None);

        assert_eq!(
            (metadata.width, metadata.rating, metadata.taken_at, metadata.duration),
            (None, None, None, None)
        );
        assert_eq!(metadata.tags.len(), 2);
    }

    #[test]
    fn parses_exif_and_iso_date_forms() {
        let p = |s: &str| parse_date_time(s).map(|(dt, offset)| (dt.to_string(), offset));
        assert_eq!(p("2024:07:02 07:12:59"), Some(("2024-07-02 07:12:59".into(), None)));
        assert_eq!(p("2024-07-02T07:12"), Some(("2024-07-02 07:12:00".into(), None)));
        assert_eq!(
            p("2024:07:02 07:12:59.5-08:00"),
            Some(("2024-07-02 07:12:59.500".into(), Some(-480)))
        );
        assert_eq!(
            p("2024:07:02 07:12:59+0530"),
            Some(("2024-07-02 07:12:59".into(), Some(330)))
        );
        assert_eq!(
            p("2024:07:02 07:12:59 UTC"),
            Some(("2024-07-02 07:12:59".into(), Some(0)))
        );
        assert_eq!(p("2024:07:02"), None);
        assert_eq!(p("0001:01:01 00:00:00"), None);
    }
}
