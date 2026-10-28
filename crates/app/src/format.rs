//! Formatting of dates, sizes and counts for display.

use chrono::{Datelike, Local, TimeZone, Timelike};
use icu_datetime::DateTimeFormatter;
use icu_datetime::fieldsets::{YMD, YMDT};
use icu_datetime::input::{Date, DateTime, Time};
use icu_locale_core::Locale;
use std::cell::RefCell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Used when the system's locale is unknown or can't be read: the design guide's examples are in it.
const FALLBACK_LOCALE: &str = "en-GB";

/// The locale set with [`set_locale`]; `None` formats in the user's locale.
static LOCALE: Mutex<Option<String>> = Mutex::new(None);
/// Counts [`set_locale`] calls, so formatting only looks at [`LOCALE`] after a change.
static LOCALE_VERSION: AtomicU64 = AtomicU64::new(0);

/// The [`LOCALE_VERSION`] a formatter was made for, and the formatter, if one could be made.
type CachedFormatter = (u64, Option<DateTimeFormatter<YMDT>>);

thread_local! {
    /// The formatter for the locale in use, made once (dates show on every list row); it can't move between threads.
    static DATES: RefCell<Option<CachedFormatter>> = const { RefCell::new(None) };
}

fn parse_locale(name: &str) -> Option<Locale> {
    // POSIX names ("de_DE.UTF-8") as BCP 47.
    let name = name.split(['.', '@']).next()?.replace('_', "-");
    Locale::try_from_str(&name).ok()
}

/// The user's locale (REQ-013-13).
fn system_locale() -> Locale {
    sys_locale::get_locale()
        .and_then(|name| parse_locale(&name))
        .or_else(|| parse_locale(FALLBACK_LOCALE))
        .unwrap_or(Locale::UNKNOWN)
}

fn formatter(locale: &Locale) -> Option<DateTimeFormatter<YMDT>> {
    let fields: YMDT = YMD::medium().with_time_hm();
    DateTimeFormatter::try_new(locale.into(), fields)
        .ok()
        .or_else(|| DateTimeFormatter::try_new(parse_locale(FALLBACK_LOCALE)?.into(), fields).ok())
}

/// Formats dates in `name` (a BCP 47 locale such as "en-GB") from now on instead of the user's locale; for tests and
/// screenshots that must not depend on the machine.
pub fn set_locale(name: &str) {
    *LOCALE.lock().unwrap_or_else(|p| p.into_inner()) = Some(name.to_owned());
    LOCALE_VERSION.fetch_add(1, Ordering::SeqCst);
}

fn format_with(formatter: &DateTimeFormatter<YMDT>, ms: i64) -> String {
    let Some(at) = Local.timestamp_millis_opt(ms).single() else {
        return String::new();
    };
    let date = Date::try_new_iso(at.year(), at.month() as u8, at.day() as u8);
    let time = Time::try_new(at.hour() as u8, at.minute() as u8, 0, 0);
    match (date, time) {
        (Ok(date), Ok(time)) => formatter.format(&DateTime { date, time }).to_string(),
        _ => String::new(),
    }
}

/// Medium date with hours and minutes, without seconds, in local time and the user's locale (REQ-013-13): "2 Jul
/// 2025, 07:12" in British English, "Jul 2, 2025, 7:12 AM" in American English. `ms` since the Unix epoch.
pub fn medium_date_time(ms: i64) -> String {
    let version = LOCALE_VERSION.load(Ordering::SeqCst);
    DATES.with_borrow_mut(|dates| {
        if dates.as_ref().is_none_or(|(made_for, _)| *made_for != version) {
            let wanted = LOCALE.lock().unwrap_or_else(|p| p.into_inner()).clone();
            let locale = wanted.as_deref().and_then(parse_locale).unwrap_or_else(system_locale);
            *dates = Some((version, formatter(&locale)));
        }
        match dates {
            Some((_, Some(formatter))) => format_with(formatter, ms),
            _ => String::new(),
        }
    })
}

/// [`medium_date_time`] in the locale `name`.
pub fn medium_date_time_in(name: &str, ms: i64) -> String {
    let locale = parse_locale(name).unwrap_or(Locale::UNKNOWN);
    formatter(&locale).map_or_else(String::new, |formatter| format_with(&formatter, ms))
}

/// A count with thousands separators: "12,345". English grouping; locale-dependent grouping isn't part of REQ-013-13.
pub fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A video's duration: `m:ss`, or `h:mm:ss` from one hour on (REQ-006-09). Rounded to whole seconds.
pub fn duration(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The weekday and the full date of a day: "Monday, 26 October 2026" (REQ-016-02). English until REQ-013-13.
pub fn day(day: chrono::NaiveDate) -> String {
    day.format("%A, %-d %B %Y").to_string()
}

fn count_of(n: u32, one: &str, many: &str) -> String {
    format!("{} {}", count(u64::from(n)), if n == 1 { one } else { many })
}

/// Media items of a date group by kind: "42 photos", "1 video", "42 photos · 3 videos" (REQ-016-02).
pub fn group_count(photos: u32, videos: u32) -> String {
    let mut parts = Vec::new();
    if photos > 0 || videos == 0 {
        parts.push(count_of(photos, "photo", "photos"));
    }
    if videos > 0 {
        parts.push(count_of(videos, "video", "videos"));
    }
    parts.join(" · ")
}

/// A rating in words: "Unrated", "1 star", "4 stars" (the bundled fonts have no star glyph).
pub fn rating(rating: u8) -> String {
    match rating {
        0 => "Unrated".into(),
        1 => "1 star".into(),
        n => format!("{n} stars"),
    }
}

/// The rating filter: "Any rating", "Unrated", "3 and 5 stars", "Unrated and 2 stars" (REQ-007-09).
pub fn ratings(ratings: &[u8]) -> String {
    let mut stars: Vec<u8> = ratings.iter().copied().filter(|&r| r > 0).collect();
    stars.sort_unstable();
    let stars = match stars.as_slice() {
        [] => None,
        [one] => Some(rating(*one)),
        [rest @ .., last] => Some(format!(
            "{} and {last} stars",
            rest.iter().map(u8::to_string).collect::<Vec<_>>().join(", ")
        )),
    };
    match (ratings.contains(&0), stars) {
        (false, None) => "Any rating".into(),
        (true, None) => "Unrated".into(),
        (false, Some(stars)) => stars,
        (true, Some(stars)) => format!("Unrated and {stars}"),
    }
}

/// Binary units with one decimal from KB on: "1.5 MB".
pub fn bytes(bytes: i64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes.max(0) as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn req_016_02_names_the_day_with_its_weekday() {
        assert_eq!(
            day(NaiveDate::from_ymd_opt(2026, 10, 26).unwrap()),
            "Monday, 26 October 2026"
        );
    }

    #[test]
    fn req_016_02_counts_photos_and_videos_of_a_group() {
        assert_eq!(group_count(42, 0), "42 photos");
        assert_eq!(group_count(0, 1), "1 video");
        assert_eq!(group_count(1, 3), "1 photo · 3 videos");
        assert_eq!(group_count(1234, 0), "1,234 photos");
        assert_eq!(group_count(0, 0), "0 photos");
    }

    #[test]
    fn req_007_09_names_the_rating_filter() {
        assert_eq!(ratings(&[]), "Any rating");
        assert_eq!(ratings(&[0]), "Unrated");
        assert_eq!(ratings(&[1]), "1 star");
        assert_eq!(ratings(&[5, 3]), "3 and 5 stars");
        assert_eq!(ratings(&[0, 3]), "Unrated and 3 stars");
        assert_eq!(ratings(&[2, 4, 5]), "2, 4 and 5 stars");
    }

    #[test]
    fn sizes_use_binary_units() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn req_006_09_formats_durations_as_minutes_or_hours() {
        assert_eq!(duration(0.4), "0:00");
        assert_eq!(duration(5.5), "0:06");
        assert_eq!(duration(65.0), "1:05");
        assert_eq!(duration(3599.0), "59:59");
        assert_eq!(duration(3600.0), "1:00:00");
        assert_eq!(duration(3725.0), "1:02:05");
    }

    #[test]
    fn counts_have_thousands_separators() {
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1234), "1,234");
        assert_eq!(count(1_234_567), "1,234,567");
    }

    #[test]
    fn req_013_13_formats_medium_dates_in_the_locale_without_seconds() {
        let local = NaiveDate::from_ymd_opt(2025, 7, 2)
            .unwrap()
            .and_hms_opt(7, 12, 59)
            .unwrap();
        let ms = Local.from_local_datetime(&local).single().unwrap().timestamp_millis();

        assert_eq!(medium_date_time_in("en-GB", ms), "2 Jul 2025, 07:12");
        assert_eq!(medium_date_time_in("en_GB.UTF-8", ms), "2 Jul 2025, 07:12");
        assert_eq!(medium_date_time_in("de-DE", ms), "02.07.2025, 07:12");
        let us = medium_date_time_in("en-US", ms);
        assert!(us.starts_with("Jul 2, 2025"), "{us}");
        assert!(us.contains("7:12") && us.contains("AM") && !us.contains(":59"), "{us}");
    }
}
