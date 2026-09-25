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
    Migration {
        version: 4,
        name: "accounts and login sessions",
        // The mode lives in a one row table rather than in `settings`, because settings are edited through
        // the settings endpoint and whether a login is required must never be changeable that way. Every
        // existing install arrives here as undecided, so its owner is asked the question on the next visit.
        //
        // Login sessions store a SHA-256 of the cookie value, never the value itself, so a copied database
        // file cannot be used to sign in.
        sql: r#"
        CREATE TABLE auth_state (
            id            INTEGER PRIMARY KEY CHECK (id = 1),
            mode          TEXT NOT NULL CHECK (mode IN ('undecided', 'open', 'accounts')),
            decided_at_ms INTEGER
        );

        INSERT INTO auth_state (id, mode, decided_at_ms) VALUES (1, 'undecided', NULL);

        CREATE TABLE users (
            id                     INTEGER PRIMARY KEY AUTOINCREMENT,
            email                  TEXT NOT NULL UNIQUE COLLATE NOCASE,
            password_hash          TEXT NOT NULL,
            role                   TEXT NOT NULL CHECK (role IN ('admin', 'listener')),
            created_at_ms          INTEGER NOT NULL,
            password_changed_at_ms INTEGER NOT NULL
        );

        CREATE TABLE auth_sessions (
            token_hash      BLOB PRIMARY KEY,
            user_id         INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
            created_at_ms   INTEGER NOT NULL,
            expires_at_ms   INTEGER NOT NULL,
            last_seen_at_ms INTEGER NOT NULL
        );

        CREATE INDEX idx_auth_sessions_user ON auth_sessions (user_id);
    "#,
    },
    Migration {
        version: 5,
        name: "two factor sign in",
        // The authenticator secret has to be readable, because the server recomputes the code to check
        // it, so it is stored as is, like every authenticator app stores it. A pending secret waits here
        // between showing the QR code and the first code that proves it was scanned. The last accepted
        // time step is what stops a code being replayed while it is still on the screen.
        //
        // Recovery codes are one time and high entropy, so a SHA-256 of each is enough, and a used code
        // is kept with its time rather than deleted, so "how many are left" is a count, not a guess.
        sql: r#"
        ALTER TABLE users ADD COLUMN totp_secret BLOB;
        ALTER TABLE users ADD COLUMN totp_pending_secret BLOB;
        ALTER TABLE users ADD COLUMN totp_last_step INTEGER;

        CREATE TABLE recovery_codes (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id    INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
            code_hash  BLOB NOT NULL,
            used_at_ms INTEGER
        );

        CREATE INDEX idx_recovery_codes_user ON recovery_codes (user_id);
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
