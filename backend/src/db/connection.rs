//! Owns the single SQLite connection.
//!
//! Write volume is one row every few seconds (a closed segment) plus the occasional settings change, so a
//! connection pool would add moving parts for no throughput. A single connection behind a mutex also side
//! steps the `database is locked` failures that concurrent writers produce on network filesystems, which
//! matters because the data directory is often a mounted volume on a server install.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

pub struct Database {
    connection: Mutex<Connection>,
}

impl Database {
    /// Open the database at `path`, apply the connection pragmas, and run pending migrations.
    ///
    /// Any failure names the file. SQLite's own messages ("file is not a database", "attempt to write a
    /// readonly database") never say which file they mean, and this is what an operator reads when the
    /// service will not start.
    pub fn open(path: &Path) -> AppResult<Self> {
        Self::open_at(path).map_err(|error| {
            AppError::internal(format!(
                "could not open the database {}: {error}",
                path.display()
            ))
        })
    }

    fn open_at(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(path)?;
        // SQLite quietly falls back to read only when the file cannot be written. Carrying on would start
        // a service that plays old audio but indexes nothing it records, so refuse here instead.
        if connection.is_readonly(rusqlite::MAIN_DB)? {
            return Err(AppError::internal(
                "the file is read only, and the service has to write to it",
            ));
        }
        configure(&connection)?;

        let database = Self {
            connection: Mutex::new(connection),
        };
        database.with_connection(super::migrations::run)?;
        Ok(database)
    }

    /// Open an in memory database. Used by the unit tests and by a dry run mode.
    pub fn open_in_memory() -> AppResult<Self> {
        let connection = Connection::open_in_memory()?;
        configure(&connection)?;
        let database = Self {
            connection: Mutex::new(connection),
        };
        database.with_connection(super::migrations::run)?;
        Ok(database)
    }

    /// Run `operation` with exclusive access to the connection.
    ///
    /// A poisoned mutex means another thread panicked while holding a half applied statement, which is not
    /// something the caller can recover from, so it is surfaced as an internal error rather than hidden.
    pub fn with_connection<T, F>(&self, operation: F) -> AppResult<T>
    where
        F: FnOnce(&Connection) -> AppResult<T>,
    {
        let guard = self
            .connection
            .lock()
            .map_err(|_| AppError::internal("database connection lock was poisoned"))?;
        operation(&guard)
    }
}

fn configure(connection: &Connection) -> AppResult<()> {
    // WAL keeps readers from blocking the recorder's segment writes.
    connection.pragma_update(None, "journal_mode", "WAL")?;
    // NORMAL is the right trade for a media recorder: a crash can lose the last transaction, which is at
    // most one segment row, and the audio file itself is still on disk.
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_database_is_migrated() {
        let database = Database::open_in_memory().expect("open");
        let tables: i64 = database
            .with_connection(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('settings', 'sessions', 'segments')",
                    [],
                    |row| row.get(0),
                )
                .map_err(Into::into)
            })
            .expect("query");
        assert_eq!(tables, 3);
    }

    #[test]
    fn a_file_database_is_opened_in_wal_mode_with_foreign_keys_on() {
        // WAL is what lets the recorder write while listeners read, and an in memory database cannot use
        // it, so this needs a real file.
        let dir = std::env::temp_dir().join(format!("oar-db-wal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("folder");
        let database = Database::open(&dir.join("on-air-record.sqlite")).expect("open");
        let (mode, foreign_keys): (String, i64) = database
            .with_connection(|conn| {
                let mode = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
                let keys = conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
                Ok((mode, keys))
            })
            .expect("pragmas");
        drop(database);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(mode.to_lowercase(), "wal");
        assert_eq!(foreign_keys, 1, "bookmarks and segments rely on it");
    }
}
