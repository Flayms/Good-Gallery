//! Fixing the date taken of many photos (spec 038): where the new date comes from, reading it from a file name, and
//! what makes a date unusable. Dates are wall-clock times with the offset the photo or the source has, if any
//! (REQ-038-07).

use crate::media::MediaKind;
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use regex::Regex;
use std::sync::{Arc, LazyLock};

/// A date as a photo stores it: the wall-clock time, and its offset from UTC if it has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub at: NaiveDateTime,
    pub offset: Option<FixedOffset>,
}

impl Stamp {
    pub fn zoneless(at: NaiveDateTime) -> Self {
        Self { at, offset: None }
    }

    /// exiftool's form of the wall-clock time, `2023:05:12 10:15:00`.
    pub fn exif_text(&self) -> String {
        self.at.format("%Y:%m:%d %H:%M:%S").to_string()
    }

    /// The offset as EXIF writes it, `+02:00`.
    pub fn offset_text(&self) -> Option<String> {
        self.offset.map(|offset| offset.to_string())
    }
}

/// A date field of a photo that the new date taken can come from (REQ-038-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The file's date modified.
    Modified,
    /// EXIF `CreateDate`, also called DateTimeDigitized.
    Created,
    /// `XMP-photoshop:DateCreated`.
    XmpCreated,
    /// `GPSDateTime`, in UTC.
    Gps,
}

impl Field {
    pub const ALL: [Self; 4] = [Self::Modified, Self::Created, Self::XmpCreated, Self::Gps];

    pub fn label(self) -> &'static str {
        match self {
            Self::Modified => "Date modified",
            Self::Created => "Date created",
            Self::XmpCreated => "XMP date created",
            Self::Gps => "GPS time",
        }
    }
}

/// Where the new date taken comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Another date of the photo (REQ-038-02).
    Field(Field),
    /// A date in the file name: a pattern in the rename date format, or the built-in ones when empty (REQ-038-03).
    FileName(String),
    /// The date taken moved by this many minutes (REQ-038-04).
    Shift(i64),
}

/// The dates of a photo read from its file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Read {
    /// EXIF `DateTimeOriginal`, with its offset.
    pub taken: Option<Stamp>,
    pub created: Option<Stamp>,
    pub xmp_created: Option<Stamp>,
    /// UTC.
    pub gps: Option<NaiveDateTime>,
}

/// A selected media item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: i64,
    pub rel_path: String,
    pub kind: MediaKind,
    /// Date modified, milliseconds since the Unix epoch.
    pub mtime: i64,
    /// The date taken the index has, as a wall-clock time: shown until the file is read, and used when the file has
    /// no `DateTimeOriginal` of its own (a date from the sidecar or another field).
    pub indexed: Option<NaiveDateTime>,
    /// What the file says, once read; `None` while it is read.
    pub read: Option<Read>,
}

impl Item {
    /// The date taken shown as the old one.
    pub fn old(&self) -> Option<Stamp> {
        self.read
            .as_ref()
            .and_then(|read| read.taken)
            .or(self.indexed.map(Stamp::zoneless))
    }
}

/// Why a row can't be fixed (REQ-038-06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    Video,
    /// The file is still being read.
    Reading,
    /// It has no value in the field.
    NoValue(Field),
    NoDateTaken,
    NoDateInName,
    /// Digits that look like a date, but no such day or time exists.
    Impossible,
    TooEarly,
    InFuture,
    /// The pattern has no year, month and day.
    BadPattern,
}

impl Problem {
    pub fn text(&self) -> String {
        match self {
            Self::Video => "Videos are left out".into(),
            Self::Reading => "Reading…".into(),
            Self::NoValue(field) => format!("Has no {}", field.label().to_lowercase()),
            Self::NoDateTaken => "Has no date taken to shift".into(),
            Self::NoDateInName => "No date in the name".into(),
            Self::Impossible => "No such date".into(),
            Self::TooEarly => "Before 1900".into(),
            Self::InFuture => "In the future".into(),
            Self::BadPattern => "The pattern needs yyyy, MM and dd".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Unchanged,
    Changed(Stamp),
    Failed(Problem),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: i64,
    pub old: Option<Stamp>,
    pub outcome: Outcome,
}

/// The new date of `item` from `source`, or why there is none (REQ-038-02 … -07). `now` is the wall-clock time
/// now; `local` gives the computer's offset at a moment (ms since the epoch), for dates that are moments.
pub fn new_date(
    source: &Source,
    item: &Item,
    now: NaiveDateTime,
    local: impl Fn(i64) -> Option<FixedOffset>,
) -> Outcome {
    if item.kind == MediaKind::Video {
        return Outcome::Failed(Problem::Video);
    }
    let Some(read) = &item.read else {
        return Outcome::Failed(Problem::Reading);
    };
    let old = item.old();
    let own = read.taken.and_then(|taken| taken.offset);
    let new = match source {
        Source::Field(Field::Modified) => {
            // A moment: the photo's own offset, else the computer's time without one (read back as local).
            let Some(utc) = DateTime::from_timestamp_millis(item.mtime) else {
                return Outcome::Failed(Problem::NoValue(Field::Modified));
            };
            match own {
                Some(offset) => Stamp {
                    at: utc.with_timezone(&offset).naive_local(),
                    offset: Some(offset),
                },
                None => match local(item.mtime) {
                    Some(offset) => Stamp::zoneless(utc.with_timezone(&offset).naive_local()),
                    None => return Outcome::Failed(Problem::NoValue(Field::Modified)),
                },
            }
        }
        Source::Field(field @ Field::Created) => match read.created {
            Some(created) => created,
            None => return Outcome::Failed(Problem::NoValue(*field)),
        },
        Source::Field(field @ Field::XmpCreated) => match read.xmp_created {
            Some(created) => created,
            None => return Outcome::Failed(Problem::NoValue(*field)),
        },
        Source::Field(Field::Gps) => {
            let Some(utc) = read.gps else {
                return Outcome::Failed(Problem::NoValue(Field::Gps));
            };
            let ms = utc.and_utc().timestamp_millis();
            let Some(offset) = own.or_else(|| local(ms)) else {
                return Outcome::Failed(Problem::NoValue(Field::Gps));
            };
            Stamp {
                at: utc.and_utc().with_timezone(&offset).naive_local(),
                offset: Some(offset),
            }
        }
        Source::FileName(pattern) => {
            let name = item.rel_path.rsplit('/').next().unwrap_or(&item.rel_path);
            match from_name(name, pattern) {
                Ok(at) => Stamp::zoneless(at),
                Err(problem) => return Outcome::Failed(problem),
            }
        }
        Source::Shift(minutes) => match old.and_then(|old| {
            let at = old.at.checked_add_signed(Duration::minutes(*minutes))?;
            Some(Stamp { at, ..old })
        }) {
            Some(shifted) => shifted,
            None => return Outcome::Failed(Problem::NoDateTaken),
        },
    };
    if new.at.year() < 1900 {
        return Outcome::Failed(Problem::TooEarly);
    }
    // Later today is a clock a little ahead or another zone; a later day is wrong.
    if new.at.date() > now.date() {
        return Outcome::Failed(Problem::InFuture);
    }
    // Without an offset of its own, the photo's stays (REQ-038-07).
    let same = old.is_some_and(|old| old.at == new.at && (new.offset.is_none() || new.offset == old.offset));
    if same {
        Outcome::Unchanged
    } else {
        Outcome::Changed(new)
    }
}

/// Every item with its new date, or why it keeps its own (REQ-038-05).
pub fn preview(
    source: &Source,
    items: &[Item],
    now: NaiveDateTime,
    local: impl Fn(i64) -> Option<FixedOffset> + Copy,
) -> Vec<Row> {
    items
        .iter()
        .map(|item| Row {
            id: item.id,
            old: item.old(),
            outcome: new_date(source, item, now, local),
        })
        .collect()
}

/// The date and time in a file name: by `pattern` in the rename date format (`yyyy-MM-dd HH-mm-ss`), anywhere in
/// the name, or by the built-in patterns when it is empty. A date without a time is at 12:00 (REQ-038-03).
pub fn from_name(name: &str, pattern: &str) -> Result<NaiveDateTime, Problem> {
    if pattern.trim().is_empty() {
        built_in(name)
    } else {
        by_pattern(name, pattern)
    }
}

/// A date at `hms`, or at noon without one, so it stays on its day in every time zone.
fn date_at(ymd: (i32, u32, u32), hms: Option<(u32, u32, u32)>) -> Option<NaiveDateTime> {
    let date = NaiveDate::from_ymd_opt(ymd.0, ymd.1, ymd.2)?;
    let (h, m, s) = hms.unwrap_or((12, 0, 0));
    Some(date.and_time(NaiveTime::from_hms_opt(h, m, s)?))
}

/// Phones, cameras and screenshots: `20230512_101500`, `2023-05-12 10.15.00`, `2023-05-12 at 10-15-00`,
/// `IMG-20230512-WA0001`, `2023_05_12`.
fn built_in(name: &str) -> Result<NaiveDateTime, Problem> {
    static DATE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?:^|\D)(\d{4})[-_.]?(\d{2})[-_.]?(\d{2})(?:(?:[ _T-]|\s?at\s?)(\d{2})[-_.:h]?(\d{2})[-_.:m]?(\d{2}))?",
        )
        .unwrap()
    });
    let mut impossible = false;
    for captures in DATE.captures_iter(name) {
        let whole = captures.get(0).expect("the whole match");
        let number = |i: usize| captures.get(i).and_then(|m| m.as_str().parse::<u32>().ok());
        let time = match (number(4), number(5), number(6)) {
            (Some(h), Some(m), Some(s)) => Some((h, m, s)),
            _ => None,
        };
        // A date alone that runs on into more digits is part of a longer number.
        if time.is_none() && name[whole.end()..].starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        let (Some(y), Some(mo), Some(d)) = (number(1), number(2), number(3)) else {
            continue;
        };
        match date_at((y as i32, mo, d), time) {
            Some(at) => return Ok(at),
            None => impossible = true,
        }
    }
    Err(if impossible {
        Problem::Impossible
    } else {
        Problem::NoDateInName
    })
}

/// A user pattern as a regular expression, with the date part each group holds.
struct Compiled {
    regex: Regex,
    order: Vec<&'static str>,
}

/// `pattern` compiled, or why it can't be used.
fn compile(pattern: &str) -> Result<Compiled, Problem> {
    let mut source = String::new();
    let mut order = Vec::new();
    let mut rest = pattern;
    while let Some(c) = rest.chars().next() {
        match crate::rename::DATE_TOKENS
            .into_iter()
            .find(|token| rest.starts_with(token))
        {
            Some(token) => {
                source.push_str(if token == "yyyy" { r"(\d{4})" } else { r"(\d{2})" });
                order.push(token);
                rest = &rest[token.len()..];
            }
            None => {
                source.push_str(&regex::escape(&c.to_string()));
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    let has = |token: &str| order.contains(&token);
    if !(has("yyyy") || has("yy")) || !has("MM") || !has("dd") {
        return Err(Problem::BadPattern);
    }
    let regex = Regex::new(&source).map_err(|_| Problem::BadPattern)?;
    Ok(Compiled { regex, order })
}

/// A compiled pattern, shared with the rows of one preview.
type Cached = Arc<Result<Compiled, Problem>>;

/// A pattern in the rename date format, found anywhere in the name. The last pattern stays compiled: the preview
/// asks for it once per row (REQ-038-05).
fn by_pattern(name: &str, pattern: &str) -> Result<NaiveDateTime, Problem> {
    static LAST: std::sync::Mutex<Option<(String, Cached)>> = std::sync::Mutex::new(None);
    let compiled = {
        let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        match &*last {
            Some((cached, compiled)) if cached == pattern => compiled.clone(),
            _ => {
                let compiled = Arc::new(compile(pattern));
                *last = Some((pattern.to_owned(), compiled.clone()));
                compiled
            }
        }
    };
    let Compiled { regex, order } = compiled.as_ref().as_ref().map_err(Clone::clone)?;
    let has = |token: &str| order.contains(&token);
    let captures = regex.captures(name).ok_or(Problem::NoDateInName)?;
    let value = |token: &str| -> Option<u32> {
        let at = order.iter().position(|t| *t == token)?;
        captures.get(at + 1)?.as_str().parse().ok()
    };
    let year = match (value("yyyy"), value("yy")) {
        (Some(year), _) => year as i32,
        // As chrono's `%y`: 69–99 are the 1900s.
        (None, Some(yy)) if yy >= 69 => 1900 + yy as i32,
        (None, Some(yy)) => 2000 + yy as i32,
        (None, None) => return Err(Problem::BadPattern),
    };
    let (Some(month), Some(day)) = (value("MM"), value("dd")) else {
        return Err(Problem::BadPattern);
    };
    let time = has("HH").then(|| {
        (
            value("HH").unwrap_or(0),
            value("mm").unwrap_or(0),
            value("ss").unwrap_or(0),
        )
    });
    date_at((year, month, day), time).ok_or(Problem::Impossible)
}

/// A date as exiftool prints it: `2023:05:12 10:15:00`, with optional sub-seconds and an offset (`+02:00`, `Z`).
pub fn parse_exif(text: &str) -> Option<Stamp> {
    static EXIF: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(\d{4})[:-](\d{2})[:-](\d{2})[ T](\d{2}):(\d{2}):(\d{2})(?:\.\d+)?\s*(Z|[+-]\d{2}:?\d{2})?$")
            .unwrap()
    });
    let captures = EXIF.captures(text.trim())?;
    let number = |i: usize| captures.get(i)?.as_str().parse::<u32>().ok();
    let at = date_at(
        (number(1)? as i32, number(2)?, number(3)?),
        Some((number(4)?, number(5)?, number(6)?)),
    )?;
    let offset = match captures.get(7).map(|m| m.as_str()) {
        None => None,
        Some("Z") => FixedOffset::east_opt(0),
        Some(offset) => {
            let sign = if offset.starts_with('-') { -1 } else { 1 };
            let digits: String = offset.chars().filter(char::is_ascii_digit).collect();
            let (hours, minutes) = (digits[..2].parse::<i32>().ok()?, digits[2..].parse::<i32>().ok()?);
            Some(FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60))?)
        }
    };
    Some(Stamp { at, offset })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn plus(hours: i32) -> FixedOffset {
        FixedOffset::east_opt(hours * 3600).unwrap()
    }

    const NOW: &str = "2026-11-07 12:00:00";

    fn item(read: Option<Read>) -> Item {
        Item {
            id: 1,
            rel_path: "2023/IMG_20230512_101500.jpg".into(),
            kind: MediaKind::Image,
            // 2024-01-02 03:04:05 UTC
            mtime: 1_704_164_645_000,
            indexed: Some(at("2020-01-01 08:00:00")),
            read,
        }
    }

    fn fix(source: &Source, item: &Item) -> Outcome {
        new_date(source, item, at(NOW), |_| Some(plus(1)))
    }

    fn changed(text: &str, offset: Option<FixedOffset>) -> Outcome {
        Outcome::Changed(Stamp { at: at(text), offset })
    }

    /// REQ-038-03: built-in patterns of phones and cameras, a date alone at noon, and the first real date in the name.
    #[test]
    fn req_038_03_finds_dates_in_names() {
        for (name, expected) in [
            ("IMG_20230512_101500.jpg", "2023-05-12 10:15:00"),
            ("PXL_20230512_101500123.jpg", "2023-05-12 10:15:00"),
            ("2023-05-12 10.15.00.jpg", "2023-05-12 10:15:00"),
            ("Screenshot 2023-05-12 at 10-15-00.png", "2023-05-12 10:15:00"),
            ("IMG-20230512-WA0001.jpg", "2023-05-12 12:00:00"),
            ("2023_05_12.jpg", "2023-05-12 12:00:00"),
            ("holiday 20230512.jpg", "2023-05-12 12:00:00"),
            ("scan 0042 20230512.jpg", "2023-05-12 12:00:00"),
        ] {
            assert_eq!(from_name(name, ""), Ok(at(expected)), "{name}");
        }
        assert_eq!(from_name("IMG_4711.jpg", ""), Err(Problem::NoDateInName));
        assert_eq!(from_name("IMG_20230231_101500.jpg", ""), Err(Problem::Impossible));
    }

    /// REQ-038-03: a pattern of the user's own, in the rename date format, anywhere in the name.
    #[test]
    fn req_038_03_reads_a_pattern_of_the_users_own() {
        assert_eq!(
            from_name("Party 12.05.2023 22h15.jpg", "dd.MM.yyyy HHhmm"),
            Ok(at("2023-05-12 22:15:00"))
        );
        assert_eq!(
            from_name("Party 12.05.23.jpg", "dd.MM.yy"),
            Ok(at("2023-05-12 12:00:00"))
        );
        assert_eq!(from_name("Party.jpg", "dd.MM.yyyy"), Err(Problem::NoDateInName));
        assert_eq!(from_name("31.02.2023.jpg", "dd.MM.yyyy"), Err(Problem::Impossible));
        assert_eq!(from_name("x.jpg", "HH-mm"), Err(Problem::BadPattern));
    }

    /// REQ-038-02, REQ-038-07: another date of the photo, with its offset; GPS time and the date modified are moments,
    /// shown in the photo's own offset, else the computer's.
    #[test]
    fn req_038_02_req_038_07_takes_another_date() {
        let read = Read {
            taken: Some(Stamp::zoneless(at("2020-01-01 08:00:00"))),
            created: Some(Stamp {
                at: at("2021-02-03 04:05:06"),
                offset: Some(plus(2)),
            }),
            xmp_created: None,
            gps: Some(at("2021-02-03 02:05:06")),
        };
        let item = item(Some(read.clone()));
        assert_eq!(
            fix(&Source::Field(Field::Created), &item),
            changed("2021-02-03 04:05:06", Some(plus(2)))
        );
        assert_eq!(
            fix(&Source::Field(Field::XmpCreated), &item),
            Outcome::Failed(Problem::NoValue(Field::XmpCreated))
        );
        // No offset of its own: the computer's (+01:00 here).
        assert_eq!(
            fix(&Source::Field(Field::Gps), &item),
            changed("2021-02-03 03:05:06", Some(plus(1)))
        );
        assert_eq!(
            fix(&Source::Field(Field::Modified), &item),
            changed("2024-01-02 04:04:05", None)
        );

        // The photo's own offset wins for moments.
        let own = Item {
            read: Some(Read {
                taken: Some(Stamp {
                    at: at("2020-01-01 08:00:00"),
                    offset: Some(plus(-5)),
                }),
                ..read
            }),
            ..item
        };
        assert_eq!(
            fix(&Source::Field(Field::Gps), &own),
            changed("2021-02-02 21:05:06", Some(plus(-5)))
        );
    }

    /// REQ-038-04: a shift moves the date taken and keeps its offset.
    #[test]
    fn req_038_04_shifts_the_date_taken() {
        let read = Read {
            taken: Some(Stamp {
                at: at("2020-01-01 08:00:00"),
                offset: Some(plus(2)),
            }),
            ..Read::default()
        };
        let item = item(Some(read));
        let shift = Source::Shift(-(24 * 60 + 90));
        assert_eq!(fix(&shift, &item), changed("2019-12-31 06:30:00", Some(plus(2))));
        // Without a DateTimeOriginal, the indexed date taken is shifted.
        let indexed = Item {
            read: Some(Read::default()),
            ..item.clone()
        };
        assert_eq!(fix(&Source::Shift(60), &indexed), changed("2020-01-01 09:00:00", None));
        let none = Item {
            indexed: None,
            ..indexed
        };
        assert_eq!(fix(&Source::Shift(60), &none), Outcome::Failed(Problem::NoDateTaken));
        assert_eq!(fix(&Source::Shift(0), &item), Outcome::Unchanged);
    }

    /// REQ-038-06, REQ-038-01: videos, files still being read, and dates before 1900 or in the future are left out.
    #[test]
    fn req_038_06_dates_that_cant_be_used() {
        let video = Item {
            kind: MediaKind::Video,
            ..item(Some(Read::default()))
        };
        assert_eq!(fix(&Source::Shift(60), &video), Outcome::Failed(Problem::Video));
        assert_eq!(fix(&Source::Shift(60), &item(None)), Outcome::Failed(Problem::Reading));

        let named = |name: &str| Item {
            rel_path: name.into(),
            ..item(Some(Read::default()))
        };
        let name = Source::FileName(String::new());
        assert_eq!(
            fix(&name, &named("18991231_120000.jpg")),
            Outcome::Failed(Problem::TooEarly)
        );
        assert_eq!(
            fix(&name, &named("20261109_120000.jpg")),
            Outcome::Failed(Problem::InFuture)
        );
        // The same day, later than now: a clock a little ahead or another zone, not a wrong date.
        assert_eq!(
            fix(&name, &named("20261107_230000.jpg")),
            changed("2026-11-07 23:00:00", None)
        );
        assert_eq!(fix(&name, &named("2023/x/20200101_080000.jpg")), Outcome::Unchanged);
    }

    /// REQ-038-05: the preview of 50,000 photos is worked out again on every key press, so it must stay quick.
    #[test]
    fn req_038_05_previews_50000_photos_quickly() {
        let items: Vec<Item> = (0..50_000)
            .map(|i| Item {
                id: i,
                rel_path: format!("2023/IMG_202305{:02}_{:06}.jpg", i % 28 + 1, i % 235_959),
                ..item(Some(Read::default()))
            })
            .collect();
        for source in [
            Source::FileName(String::new()),
            Source::FileName("yyyyMMdd_HHmmss".into()),
        ] {
            let started = std::time::Instant::now();
            let rows = preview(&source, &items, at(NOW), |_| Some(plus(1)));
            let took = started.elapsed();
            assert_eq!(rows.len(), 50_000);
            eprintln!("{source:?}: 50,000 rows in {took:?}");
            // Generous for a debug build on a slow machine; a release build takes a fraction of it.
            assert!(took < std::time::Duration::from_secs(3), "{source:?}: {took:?}");
        }
    }

    /// REQ-038-07: exiftool's dates with sub-seconds and offsets.
    #[test]
    fn req_038_07_reads_exiftool_dates() {
        assert_eq!(
            parse_exif("2023:05:12 10:15:00"),
            Some(Stamp::zoneless(at("2023-05-12 10:15:00")))
        );
        assert_eq!(
            parse_exif("2023:05:12 10:15:00.123+02:00"),
            Some(Stamp {
                at: at("2023-05-12 10:15:00"),
                offset: Some(plus(2))
            })
        );
        assert_eq!(
            parse_exif("2023:05:12 10:15:00Z"),
            Some(Stamp {
                at: at("2023-05-12 10:15:00"),
                offset: Some(plus(0))
            })
        );
        assert_eq!(parse_exif("2023-05-12T10:15:00-05:00").unwrap().offset, Some(plus(-5)));
        assert_eq!(parse_exif("0000:00:00 00:00:00"), None);
        assert_eq!(parse_exif("garbage"), None);
        let stamp = parse_exif("2023:05:12 10:15:00+02:00").unwrap();
        assert_eq!(
            (stamp.exif_text(), stamp.offset_text()),
            ("2023:05:12 10:15:00".into(), Some("+02:00".into()))
        );
    }
}
