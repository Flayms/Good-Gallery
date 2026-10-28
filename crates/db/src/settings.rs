//! User settings, stored as JSON values in the `settings` table.

use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// Size cap of the thumbnail cache (GiB), 0.5–1024.
    pub thumb_cache_gib: f64,
    /// Quick scan of all libraries for changes the watchers missed; `0` checks only at startup. At most 7 days.
    pub quick_check_interval_minutes: u32,
    /// Parallel file reads (scanning, metadata, thumbnails), 1–16.
    pub io_concurrency: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            thumb_cache_gib: 5.0,
            quick_check_interval_minutes: 60,
            io_concurrency: 4,
        }
    }
}

const THUMB_CACHE: &str = "thumbCacheGiB";
const QUICK_CHECK: &str = "quickCheckIntervalMinutes";
const IO_CONCURRENCY: &str = "ioConcurrency";
const MAX_QUICK_CHECK: u32 = 7 * 24 * 60;

fn thumb_cache(value: &Value) -> Option<f64> {
    value.as_f64().filter(|gib| (0.5..=1024.0).contains(gib))
}

/// An integer in `range`; `2.0` counts as an integer, `2.5` doesn't.
fn integer(value: &Value, range: std::ops::RangeInclusive<u32>) -> Option<u32> {
    let n = value.as_f64().filter(|n| n.fract() == 0.0)?;
    (n >= f64::from(*range.start()) && n <= f64::from(*range.end())).then_some(n as u32)
}

impl Settings {
    /// Rejects values outside the allowed ranges.
    pub fn validate(&self) -> Result<()> {
        let invalid = |name: &str| Err(Error::InvalidSetting(format!("{name} is out of range")));
        if thumb_cache(&Value::from(self.thumb_cache_gib)).is_none() {
            return invalid("Thumbnail cache size");
        }
        if self.quick_check_interval_minutes > MAX_QUICK_CHECK {
            return invalid("Quick check interval");
        }
        if !(1..=16).contains(&self.io_concurrency) {
            return invalid("Parallel file reads");
        }
        Ok(())
    }
}

/// The stored settings. Missing or no longer valid values fall back to their defaults.
pub fn load(conn: &Connection) -> Result<Settings> {
    let mut settings = Settings::default();
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    for row in rows {
        let (key, text) = row?;
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        match key.as_str() {
            THUMB_CACHE => settings.thumb_cache_gib = thumb_cache(&value).unwrap_or(settings.thumb_cache_gib),
            QUICK_CHECK => {
                settings.quick_check_interval_minutes =
                    integer(&value, 0..=MAX_QUICK_CHECK).unwrap_or(settings.quick_check_interval_minutes)
            }
            IO_CONCURRENCY => settings.io_concurrency = integer(&value, 1..=16).unwrap_or(settings.io_concurrency),
            _ => {}
        }
    }
    Ok(settings)
}

/// Validates and stores all settings; nothing changes if any value is invalid.
pub fn save(conn: &Connection, settings: &Settings) -> Result<()> {
    settings.validate()?;
    let tx = conn.unchecked_transaction()?;
    for (key, value) in [
        (THUMB_CACHE, Value::from(settings.thumb_cache_gib)),
        (QUICK_CHECK, Value::from(settings.quick_check_interval_minutes)),
        (IO_CONCURRENCY, Value::from(settings.io_concurrency)),
    ] {
        tx.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            (key, value.to_string()),
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// A stored value outside the settings above, e.g. a view choice of the gallery (`gallery.columns`), kept as is. `None`
/// if missing or unreadable.
pub fn value(conn: &Connection, key: &str) -> Result<Option<Value>> {
    let text: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0))
        .optional()?;
    Ok(text.and_then(|text| serde_json::from_str(&text).ok()))
}

pub fn set_value(conn: &Connection, key: &str, value: &Value) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        (key, value.to_string()),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_other_values_under_their_own_keys() {
        let conn = crate::open_in_memory().unwrap();
        assert_eq!(value(&conn, "gallery.columns").unwrap(), None);
        set_value(&conn, "gallery.columns", &Value::from(7)).unwrap();
        set_value(&conn, "gallery.columns", &Value::from(8)).unwrap();
        assert_eq!(value(&conn, "gallery.columns").unwrap(), Some(Value::from(8)));
        assert_eq!(load(&conn).unwrap(), Settings::default());
    }

    #[test]
    fn req_010_01_starts_with_defaults_and_persists_updates() {
        let conn = crate::open_in_memory().unwrap();
        assert_eq!(load(&conn).unwrap(), Settings::default());
        assert_eq!(
            Settings::default(),
            Settings {
                thumb_cache_gib: 5.0,
                quick_check_interval_minutes: 60,
                io_concurrency: 4
            }
        );

        let changed = Settings {
            thumb_cache_gib: 20.0,
            quick_check_interval_minutes: 0,
            io_concurrency: 8,
        };
        save(&conn, &changed).unwrap();

        assert_eq!(load(&conn).unwrap(), changed);
    }

    #[test]
    fn req_010_02_rejects_out_of_range_values() {
        let conn = crate::open_in_memory().unwrap();
        for invalid in [
            Settings {
                thumb_cache_gib: 0.25,
                ..Default::default()
            },
            Settings {
                quick_check_interval_minutes: MAX_QUICK_CHECK + 1,
                ..Default::default()
            },
            Settings {
                io_concurrency: 17,
                ..Default::default()
            },
            Settings {
                io_concurrency: 0,
                ..Default::default()
            },
        ] {
            assert!(save(&conn, &invalid).is_err(), "{invalid:?}");
        }
        assert_eq!(load(&conn).unwrap(), Settings::default());
    }

    #[test]
    fn req_010_02_falls_back_to_defaults_for_stored_values_that_no_longer_validate() {
        let conn = crate::open_in_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO settings (key, value) VALUES ('thumbCacheGiB', '2000'), ('ioConcurrency', '2.5'),
             ('quickCheckIntervalMinutes', '\"often\"'), ('unknown', '1')",
        )
        .unwrap();

        assert_eq!(load(&conn).unwrap(), Settings::default());
    }

    #[test]
    fn stores_json_values_under_camel_case_keys() {
        let conn = crate::open_in_memory().unwrap();
        save(&conn, &Settings::default()).unwrap();

        let stored: String = conn
            .query_row("SELECT value FROM settings WHERE key = 'ioConcurrency'", [], |r| {
                r.get(0)
            })
            .unwrap();

        assert_eq!(stored, "4");
    }
}
