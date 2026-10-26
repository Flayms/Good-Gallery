//! Schema migrations, compatible with drizzle-orm's SQLite migrator in the reference: the same SQL files, the same
//! `__drizzle_migrations` table, rows and ordering.

use crate::{Error, Result};
use rusqlite::Connection;
use rusqlite::types::Value;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The reference's `drizzle/*.sql`, copied byte for byte (the hash covers the exact bytes).
const FILES: &[(&str, &str)] = &[
    ("0000_init", include_str!("../migrations/0000_init.sql")),
    (
        "0001_sidecar_mtime",
        include_str!("../migrations/0001_sidecar_mtime.sql"),
    ),
    ("0002_thumb_index", include_str!("../migrations/0002_thumb_index.sql")),
    ("0003_search", include_str!("../migrations/0003_search.sql")),
    (
        "0004_tag_media_count",
        include_str!("../migrations/0004_tag_media_count.sql"),
    ),
    ("0005_settings", include_str!("../migrations/0005_settings.sql")),
    ("0006_folders", include_str!("../migrations/0006_folders.sql")),
    (
        "0007_curious_nextwave",
        include_str!("../migrations/0007_curious_nextwave.sql"),
    ),
    ("0008_lame_mongu", include_str!("../migrations/0008_lame_mongu.sql")),
    (
        "0009_retry_thumb_errors",
        include_str!("../migrations/0009_retry_thumb_errors.sql"),
    ),
];
const JOURNAL: &str = include_str!("../migrations/meta/_journal.json");

#[derive(Deserialize)]
struct Journal {
    entries: Vec<JournalEntry>,
}

#[derive(Deserialize)]
struct JournalEntry {
    tag: String,
    when: i64,
}

pub(crate) struct Migration {
    sql: &'static str,
    /// The journal's `when`, stored as `created_at`.
    when: i64,
    hash: String,
}

/// The migrations in journal order.
pub(crate) fn migrations() -> Result<Vec<Migration>> {
    let journal: Journal = serde_json::from_str(JOURNAL).map_err(|e| Error::Journal(e.to_string()))?;
    if journal.entries.len() != FILES.len() {
        return Err(Error::Journal(format!(
            "{} entries, {} embedded files",
            journal.entries.len(),
            FILES.len()
        )));
    }
    journal
        .entries
        .iter()
        .map(|entry| {
            let (_, sql) = FILES
                .iter()
                .find(|(tag, _)| *tag == entry.tag)
                .ok_or_else(|| Error::Journal(format!("{} isn't embedded", entry.tag)))?;
            Ok(Migration {
                sql,
                when: entry.when,
                hash: Sha256::digest(sql.as_bytes())
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect(),
            })
        })
        .collect()
}

/// Runs the migrations newer than the newest recorded one, in one transaction.
pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    let migrations = migrations()?;
    // Same text as drizzle's, tabs included: SQLite stores it in `sqlite_master`, and both apps should leave the
    // same schema behind.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS \"__drizzle_migrations\" (\n\t\t\t\tid SERIAL PRIMARY KEY,\n\t\t\t\thash text NOT NULL,\n\t\t\t\tcreated_at numeric\n\t\t\t)",
    )?;
    let last: Option<f64> = conn
        .query_row(
            "SELECT created_at FROM \"__drizzle_migrations\" ORDER BY created_at DESC LIMIT 1",
            [],
            |row| row.get::<_, Value>(0),
        )
        .map(|value| match value {
            Value::Integer(n) => Some(n as f64),
            Value::Real(n) => Some(n),
            Value::Text(text) => text.parse().ok(),
            _ => None,
        })
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            error => Err(error),
        })?;

    let tx = conn.transaction()?;
    for migration in migrations
        .iter()
        .filter(|m| last.is_none_or(|last| last < m.when as f64))
    {
        for statement in migration.sql.split("--> statement-breakpoint") {
            if !statement.trim().is_empty() {
                tx.execute_batch(statement)?;
            }
        }
        tx.execute(
            "INSERT INTO \"__drizzle_migrations\" (\"hash\", \"created_at\") VALUES (?1, ?2)",
            (&migration.hash, migration.when),
        )?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_journal_entry_is_embedded() {
        assert_eq!(migrations().unwrap().len(), FILES.len());
    }

    #[test]
    fn hashes_cover_the_exact_file_bytes() {
        // sha256 of `drizzle/0000_init.sql` as the reference computes it.
        assert_eq!(
            migrations().unwrap()[0].hash,
            "216a7df63b0d12887c37d7b053b42395ef10b07ec42ff025b9fbc12bf24fd101"
        );
    }
}
