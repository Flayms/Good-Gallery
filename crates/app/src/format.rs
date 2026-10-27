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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

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
