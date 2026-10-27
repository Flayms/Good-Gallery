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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

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
