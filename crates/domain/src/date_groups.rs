//! Date groups (spec 016): runs of consecutive gallery items whose sort date falls on the same local calendar day.

use chrono::{DateTime, Local, NaiveDate, TimeZone};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateGroup {
    /// Local calendar day.
    pub day: NaiveDate,
    /// Gallery index of the group's first item.
    pub start: usize,
    pub photos: u32,
    pub videos: u32,
}

/// The local day of `ms` (milliseconds since the Unix epoch) and where it starts and ends, in ms.
fn local_day(ms: i64) -> (NaiveDate, i64, i64) {
    let day = DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .with_timezone(&Local)
        .date_naive();
    // The earliest local time of the day: midnight, or later if a daylight saving gap swallows it.
    let start_of = |day: NaiveDate| {
        (0..24)
            .find_map(|hour| Local.from_local_datetime(&day.and_hms_opt(hour, 0, 0)?).earliest())
            .map_or(i64::MIN, |time| time.timestamp_millis())
    };
    let next = day.succ_opt().unwrap_or(day);
    (day, start_of(day), start_of(next))
}

/// Groups items (in gallery order) by the local day of their date (REQ-016-01). The day is only worked out again
/// when a date leaves the current day's range, so the cost is about one time zone lookup per group.
pub fn date_groups(dates: &[i64], is_video: impl Fn(usize) -> bool) -> Vec<DateGroup> {
    let mut groups: Vec<DateGroup> = Vec::new();
    let (mut day_start, mut day_end) = (0, 0);
    for (index, &date) in dates.iter().enumerate() {
        if groups.is_empty() || date < day_start || date >= day_end {
            let (day, start, end) = local_day(date);
            (day_start, day_end) = (start, end);
            if groups.last().is_none_or(|group| group.day != day) {
                groups.push(DateGroup {
                    day,
                    start: index,
                    photos: 0,
                    videos: 0,
                });
            }
        }
        let group = groups.last_mut().expect("pushed above");
        if is_video(index) {
            group.videos += 1;
        } else {
            group.photos += 1;
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
        Local
            .with_ymd_and_hms(y, m, d, h, min, 0)
            .earliest()
            .unwrap()
            .timestamp_millis()
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn req_016_01_groups_consecutive_items_by_local_day_and_counts_kinds() {
        let dates = [
            local(2026, 10, 26, 23, 59),
            local(2026, 10, 26, 0, 0),
            local(2026, 10, 25, 12, 0),
            local(2026, 10, 25, 8, 0),
            local(2026, 10, 25, 7, 0),
        ];
        let videos = [false, true, false, false, true];

        let groups = date_groups(&dates, |i| videos[i]);

        assert_eq!(
            groups,
            [
                DateGroup {
                    day: day(2026, 10, 26),
                    start: 0,
                    photos: 1,
                    videos: 1
                },
                DateGroup {
                    day: day(2026, 10, 25),
                    start: 2,
                    photos: 2,
                    videos: 1
                },
            ]
        );
    }

    /// REQ-016-01: items keep the search's order; a day that comes back later starts a new group.
    #[test]
    fn req_016_01_keeps_the_order_and_starts_a_group_when_the_day_changes() {
        let dates = [
            local(2026, 1, 1, 10, 0),
            local(2026, 1, 2, 10, 0),
            local(2026, 1, 1, 11, 0),
        ];
        let starts: Vec<usize> = date_groups(&dates, |_| false).iter().map(|g| g.start).collect();
        assert_eq!(starts, [0, 1, 2]);
        assert!(date_groups(&[], |_| false).is_empty());
    }

    #[test]
    fn req_016_01_days_span_local_midnight_to_midnight_across_daylight_saving_changes() {
        // Both the start and the end of a day with a time change, wherever the computer is.
        for (m, d) in [(3, 29), (10, 25), (3, 8), (11, 1)] {
            let dates = [local(2026, m, d, 0, 0), local(2026, m, d, 23, 59)];
            assert_eq!(date_groups(&dates, |_| false).len(), 1, "2026-{m}-{d}");
        }
    }
}
