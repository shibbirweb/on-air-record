//! Versioned schema migrations.
//!
//! The schema version is kept in SQLite's own `user_version` pragma, so there is no bootstrap table and no
//! chicken and egg problem on a fresh database. Migrations are append only: never edit an existing entry,
//! add a new one, because installed services will already have run the old version.

use rusqlite::Connection;

use crate::error::AppResult;

struct Migration {
    version: i32,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial schema",
        sql: r#"
        CREATE TABLE settings (
            key           TEXT PRIMARY KEY,
            value         TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE sessions (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id     TEXT NOT NULL,
            device_name   TEXT NOT NULL,
            sample_rate   INTEGER NOT NULL,
            channels      INTEGER NOT NULL,
            started_at_ms INTEGER NOT NULL,
            ended_at_ms   INTEGER
        );

        CREATE INDEX idx_sessions_started_at ON sessions (started_at_ms);

        CREATE TABLE segments (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id    INTEGER NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
            sequence      INTEGER NOT NULL,
            path          TEXT NOT NULL,
            started_at_ms INTEGER NOT NULL,
            ended_at_ms   INTEGER NOT NULL,
            sample_rate   INTEGER NOT NULL,
            channels      INTEGER NOT NULL,
            byte_len      INTEGER NOT NULL,
            peaks         BLOB NOT NULL
        );

        CREATE INDEX idx_segments_range ON segments (started_at_ms, ended_at_ms);
        CREATE UNIQUE INDEX idx_segments_session_sequence ON segments (session_id, sequence);
    "#,
    },
    Migration {
        version: 2,
        name: "index segments by local calendar day",
        // The backfill uses SQLite's own `localtime` modifier so segments recorded before this migration
        // land on the same day they would have been filed under had the column always existed. Their
        // files stay where they are: `path` is stored per row, so the old flat layout keeps resolving.
        sql: r#"
        ALTER TABLE segments ADD COLUMN day TEXT NOT NULL DEFAULT '';

        UPDATE segments
        SET day = date(started_at_ms / 1000, 'unixepoch', 'localtime')
        WHERE day = '';

        CREATE INDEX idx_segments_day ON segments (day, started_at_ms);
    "#,
    },
    Migration {
        version: 3,
        name: "timeline bookmarks",
        // Bookmarks point at a moment rather than at a segment. Deliberately no foreign key: a moment can
        // be marked before the segment covering it has closed, and the timestamp stays meaningful even
        // once the audio around it is gone.
        sql: r#"
        CREATE TABLE bookmarks (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp_ms  INTEGER NOT NULL,
            label         TEXT NOT NULL,
            note          TEXT,
            created_at_ms INTEGER NOT NULL
        );

        CREATE INDEX idx_bookmarks_timestamp ON bookmarks (timestamp_ms);
    "#,
    },
];

/// Apply every migration newer than the database's recorded version.
pub fn run(connection: &Connection) -> AppResult<()> {
    let current: i32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    for migration in MIGRATIONS.iter().filter(|item| item.version > current) {
        tracing::info!(
            version = migration.version,
            name = migration.name,
            "applying migration"
        );
        connection.execute_batch(migration.sql)?;
        connection.pragma_update(None, "user_version", migration.version)?;
    }

    Ok(())
}

/// Highest migration version this build knows about.
pub fn latest_version() -> i32 {
    MIGRATIONS
        .iter()
        .map(|item| item.version)
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_idempotent() {
        let connection = Connection::open_in_memory().expect("open");
        run(&connection).expect("first run");
        run(&connection).expect("second run");
        let version: i32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("version");
        assert_eq!(version, latest_version());
    }

    #[test]
    fn the_day_column_is_backfilled_from_the_start_timestamp() {
        let connection = Connection::open_in_memory().expect("open");

        // Stand up a database exactly as the previous release left it: the original schema, stamped at
        // version 1, holding a row written under the old flat path layout.
        connection
            .execute_batch(MIGRATIONS[0].sql)
            .expect("initial schema");
        connection
            .pragma_update(None, "user_version", 1)
            .expect("stamp version");
        connection
            .execute_batch(
                "INSERT INTO sessions (device_id, device_name, sample_rate, channels, started_at_ms)
                 VALUES ('mic', 'mic', 48000, 1, 1757030400000);
                 INSERT INTO segments
                     (session_id, sequence, path, started_at_ms, ended_at_ms, sample_rate, channels, byte_len, peaks)
                 VALUES (1, 0, 'recordings/1/000000.pcm', 1757030400000, 1757030410000, 48000, 1, 960000, x'00');",
            )
            .expect("legacy row");

        run(&connection).expect("upgrade");

        let (day, path): (String, String) = connection
            .query_row("SELECT day, path FROM segments WHERE id = 1", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .expect("row");

        assert_eq!(day, crate::util::day::local_day(1_757_030_400_000));
        // The file itself was not moved, so the old path must survive the upgrade untouched.
        assert_eq!(path, "recordings/1/000000.pcm");
    }

    #[test]
    fn bookmarks_survive_an_upgrade_from_the_previous_version() {
        let connection = Connection::open_in_memory().expect("open");
        connection.execute_batch(MIGRATIONS[0].sql).expect("v1");
        connection.execute_batch(MIGRATIONS[1].sql).expect("v2");
        connection
            .pragma_update(None, "user_version", 2)
            .expect("stamp version");

        run(&connection).expect("upgrade");

        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM bookmarks", [], |row| row.get(0))
            .expect("bookmarks table exists");
        assert_eq!(count, 0);
    }

    #[test]
    fn migration_versions_are_unique_and_ordered() {
        let mut previous = 0;
        for migration in MIGRATIONS {
            assert!(migration.version > previous, "versions must ascend");
            previous = migration.version;
        }
    }
}
