//! The time zone dates are shown, grouped and searched in: the computer's, or a fixed offset set with [`set_fixed`]
//! so that tests and snapshots don't depend on the machine.

use chrono::{DateTime, FixedOffset, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use std::sync::atomic::{AtomicI64, Ordering};

/// No fixed offset: the computer's zone. Offsets are within a day, so this is never one.
const COMPUTER: i64 = i64::MIN;

/// The fixed offset in seconds east of UTC, or [`COMPUTER`].
static FIXED: AtomicI64 = AtomicI64::new(COMPUTER);

/// Uses `offset` instead of the computer's zone from now on; `None` goes back to the computer's.
pub fn set_fixed(offset: Option<FixedOffset>) {
    let seconds = offset.map_or(COMPUTER, |offset| i64::from(offset.local_minus_utc()));
    FIXED.store(seconds, Ordering::Relaxed);
}

fn fixed() -> Option<FixedOffset> {
    match FIXED.load(Ordering::Relaxed) {
        COMPUTER => None,
        seconds => FixedOffset::east_opt(i32::try_from(seconds).ok()?),
    }
}

/// The wall-clock time of `ms` (milliseconds since the Unix epoch).
pub fn wall_clock(ms: i64) -> Option<NaiveDateTime> {
    let at = DateTime::from_timestamp_millis(ms)?;
    Some(match fixed() {
        Some(offset) => at.with_timezone(&offset).naive_local(),
        None => at.with_timezone(&Local).naive_local(),
    })
}

/// The earliest moment, in ms since the Unix epoch, with the wall-clock time `local`; `None` if a daylight saving gap
/// skips it.
pub fn earliest(local: &NaiveDateTime) -> Option<i64> {
    let at = match fixed() {
        Some(offset) => offset.from_local_datetime(local).earliest()?.timestamp_millis(),
        None => Local.from_local_datetime(local).earliest()?.timestamp_millis(),
    };
    Some(at)
}

/// Today's date.
pub fn today() -> NaiveDate {
    wall_clock(Utc::now().timestamp_millis()).map_or_else(|| Local::now().date_naive(), |now| now.date())
}
