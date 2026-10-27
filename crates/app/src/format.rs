//! Formatting of dates, sizes and counts for display.

use chrono::{Local, TimeZone};

/// Medium date with hours and minutes, without seconds, in local time: "2 Jul 2025, 07:12". `ms` since the Unix epoch.
/// Follows the reference's English output; locale-dependent formats come with REQ-013-13.
pub fn medium_date_time(ms: i64) -> String {
    match Local.timestamp_millis_opt(ms).single() {
        Some(at) => at.format("%-d %b %Y, %H:%M").to_string(),
        None => String::new(),
    }
}

/// A count with thousands separators: "12,345". Follows the reference's English output; locale-dependent grouping
/// comes with REQ-013-13.
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
    fn formats_without_seconds() {
        let local = NaiveDate::from_ymd_opt(2025, 7, 2)
            .unwrap()
            .and_hms_opt(7, 12, 59)
            .unwrap();
        let ms = Local.from_local_datetime(&local).single().unwrap().timestamp_millis();

        assert_eq!(medium_date_time(ms), "2 Jul 2025, 07:12");
    }
}
