//! The time zone and today overrides are process-wide, so their tests have a binary of their own.

use chrono::{FixedOffset, Local, TimeZone};
use domain::time_zone::{earliest, set_fixed, set_today, today, wall_clock};

#[test]
fn a_fixed_offset_replaces_the_computers_zone_until_unset() {
    let ms = 2_000_000_000_000; // 2033-05-18 03:33:20 UTC
    let local = Local.timestamp_millis_opt(ms).unwrap().naive_local();
    assert_eq!(wall_clock(ms), Some(local));

    set_fixed(FixedOffset::east_opt(-5 * 3600));
    let wall = wall_clock(ms).unwrap();
    assert_eq!(wall.to_string(), "2033-05-17 22:33:20");
    assert_eq!(earliest(&wall), Some(ms));

    set_fixed(None);
    assert_eq!(wall_clock(ms), Some(local));
    assert_eq!(earliest(&local), Some(ms));
}

/// Snapshots that show today (the calendar under _Filters_) don't change with the date they run on.
#[test]
fn a_fixed_day_replaces_the_clocks_until_unset() {
    let day = chrono::NaiveDate::from_ymd_opt(2001, 2, 3).unwrap();
    set_today(Some(day));
    assert_eq!(today(), day);
    set_today(None);
    assert_ne!(today(), day);
}
