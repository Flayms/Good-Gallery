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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

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
