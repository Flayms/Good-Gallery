//! The date calendar under _Filters_ (spec 041): a month's weeks, picking a range with clicks, quick ranges and a
//! year's months.

use crate::date_groups::DateGroup;
use chrono::{Datelike, Days, Months, NaiveDate};

/// The first day of `day`'s month.
pub fn month_of(day: NaiveDate) -> NaiveDate {
    day.with_day(1).unwrap_or(day)
}

/// The days a month's calendar shows: whole weeks from Monday, from the week of its first day to the week of its
/// last (REQ-041-01).
pub fn weeks(month: NaiveDate) -> Vec<NaiveDate> {
    let first = month_of(month);
    let last = first
        .checked_add_months(Months::new(1))
        .and_then(|next| next.pred_opt())
        .unwrap_or(first);
    let start = first - Days::new(u64::from(first.weekday().num_days_from_monday()));
    let end = last + Days::new(u64::from(6 - last.weekday().num_days_from_monday()));
    start.iter_days().take_while(|day| *day <= end).collect()
}

/// A range being picked with clicks: its days and the day of a first click still waiting for the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Picking {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub anchor: Option<NaiveDate>,
}

/// A click on `day`: the first picks that day alone and waits, the second makes the range between both days, either
/// way round (REQ-041-01).
pub fn pick(picking: Picking, day: NaiveDate) -> Picking {
    match picking.anchor {
        Some(anchor) => Picking {
            from: Some(anchor.min(day)),
            to: Some(anchor.max(day)),
            anchor: None,
        },
        None => Picking {
            from: Some(day),
            to: Some(day),
            anchor: Some(day),
        },
    }
}

/// The quick ranges (REQ-041-04).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quick {
    ThisMonth,
    Last30Days,
    ThisYear,
    LastYear,
}

impl Quick {
    pub const ALL: [Self; 4] = [Self::ThisMonth, Self::Last30Days, Self::ThisYear, Self::LastYear];

    pub fn label(self) -> &'static str {
        match self {
            Self::ThisMonth => "This month",
            Self::Last30Days => "Last 30 days",
            Self::ThisYear => "This year",
            Self::LastYear => "Last year",
        }
    }

    /// Its first and last day, up to `today` where it would end later.
    pub fn range(self, today: NaiveDate) -> (NaiveDate, NaiveDate) {
        let year = |year: i32| NaiveDate::from_ymd_opt(year, 1, 1).unwrap_or(today);
        match self {
            Self::ThisMonth => (month_of(today), today),
            Self::Last30Days => (today - Days::new(29), today),
            Self::ThisYear => (year(today.year()), today),
            Self::LastYear => (year(today.year() - 1), year(today.year()).pred_opt().unwrap_or(today)),
        }
    }
}

/// Photos and videos in each month of `year`, January first, from date groups (REQ-041-03).
pub fn months(year: i32, groups: &[DateGroup]) -> [(u32, u32); 12] {
    let mut months = [(0, 0); 12];
    for group in groups.iter().filter(|group| group.day.year() == year) {
        let month = &mut months[group.day.month0() as usize];
        month.0 += group.photos;
        month.1 += group.videos;
    }
    months
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Weekday;

    fn day(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }

    #[test]
    fn req_041_01_shows_whole_weeks_from_monday() {
        // July 2025 starts on a Tuesday and ends on a Thursday.
        let days = weeks(day("2025-07-01"));
        assert_eq!(days.first(), Some(&day("2025-06-30")));
        assert_eq!(days.last(), Some(&day("2025-08-03")));
        assert_eq!(days.len(), 35);
        // February 2027 starts on a Monday and fills four weeks.
        assert_eq!(weeks(day("2027-02-01")).len(), 28);
        assert!(
            weeks(day("2025-07-01"))
                .iter()
                .step_by(7)
                .all(|d| d.weekday() == Weekday::Mon)
        );
    }

    #[test]
    fn req_041_01_two_clicks_make_a_range_either_way_round() {
        let first = pick(Picking::default(), day("2025-07-10"));
        assert_eq!(
            first,
            Picking {
                from: Some(day("2025-07-10")),
                to: Some(day("2025-07-10")),
                anchor: Some(day("2025-07-10"))
            },
            "one day, waiting for the second"
        );
        let second = pick(first, day("2025-07-03"));
        assert_eq!(
            second,
            Picking {
                from: Some(day("2025-07-03")),
                to: Some(day("2025-07-10")),
                anchor: None
            }
        );
        let third = pick(second, day("2025-08-01"));
        assert_eq!(
            (third.from, third.to),
            (Some(day("2025-08-01")), Some(day("2025-08-01"))),
            "starts over"
        );
        let same = pick(third, day("2025-08-01"));
        assert_eq!(
            (same.from, same.to, same.anchor),
            (third.from, third.to, None),
            "one day, done"
        );
    }

    #[test]
    fn req_041_04_quick_ranges_end_today_at_the_latest() {
        let today = day("2026-11-08");
        assert_eq!(Quick::ThisMonth.range(today), (day("2026-11-01"), today));
        assert_eq!(Quick::Last30Days.range(today), (day("2026-10-10"), today));
        assert_eq!(Quick::ThisYear.range(today), (day("2026-01-01"), today));
        assert_eq!(Quick::LastYear.range(today), (day("2025-01-01"), day("2025-12-31")));
    }

    #[test]
    fn req_041_03_counts_a_years_months() {
        let group = |text: &str, photos, videos| DateGroup {
            day: day(text),
            start: 0,
            photos,
            videos,
        };
        let months = months(
            2025,
            &[
                group("2024-12-31", 9, 9),
                group("2025-01-05", 2, 0),
                group("2025-01-20", 1, 1),
                group("2025-07-03", 4, 0),
                group("2026-01-01", 9, 9),
            ],
        );
        assert_eq!(months[0], (3, 1));
        assert_eq!(months[6], (4, 0));
        assert_eq!(months[1], (0, 0));
    }
}
