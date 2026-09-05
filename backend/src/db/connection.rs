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
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(path)?;
        configure(&connection)?;

        let database = Self {
            connection: Mutex::new(connection),
        };
        database.with_connection(|conn| super::migrations::run(conn))?;
        Ok(database)
    }

    /// Open an in memory database. Used by the unit tests and by a dry run mode.
    pub fn open_in_memory() -> AppResult<Self> {
        let connection = Connection::open_in_memory()?;
        configure(&connection)?;
        let database = Self {
            connection: Mutex::new(connection),
        };
        database.with_connection(|conn| super::migrations::run(conn))?;
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
}
