//! Persistence for the key value settings table.

use std::collections::HashMap;
use std::sync::Arc;

use crate::db::Database;
use crate::error::AppResult;
use crate::models::Settings;
use crate::util::time::now_ms;

pub struct SettingsRepository {
    database: Arc<Database>,
}

impl SettingsRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    /// Every stored key, unparsed.
    pub fn load_pairs(&self) -> AppResult<HashMap<String, String>> {
        self.database.with_connection(|conn| {
            let mut statement = conn.prepare("SELECT key, value FROM settings")?;
            let rows = statement.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;

            let mut pairs = HashMap::new();
            for row in rows {
                let (key, value) = row?;
                pairs.insert(key, value);
            }
            Ok(pairs)
        })
    }

    /// Typed settings, with defaults filled in for anything not stored yet.
    pub fn load(&self) -> AppResult<Settings> {
        Ok(Settings::from_pairs(&self.load_pairs()?))
    }

    /// Write the whole settings object in one transaction, so a partially applied update is impossible.
    pub fn save(&self, settings: &Settings) -> AppResult<()> {
        let pairs = settings.clone().clamped().to_pairs();
        let timestamp = now_ms();

        self.database.with_connection(|conn| {
            let transaction = conn.unchecked_transaction()?;
            {
                let mut statement = transaction.prepare(
                    "INSERT INTO settings (key, value, updated_at_ms)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at_ms = excluded.updated_at_ms",
                )?;
                for (key, value) in &pairs {
                    statement.execute(rusqlite::params![key, value, timestamp])?;
                }
            }
            transaction.commit()?;
            Ok(())
        })
    }

    /// Write the defaults for any key that has never been stored. Called once at startup so a fresh
    /// install has a fully populated table to read.
    pub fn seed_defaults(&self) -> AppResult<()> {
        let defaults = Settings::default().to_pairs();
        let timestamp = now_ms();

        self.database.with_connection(|conn| {
            let transaction = conn.unchecked_transaction()?;
            {
                let mut statement = transaction.prepare(
                    "INSERT OR IGNORE INTO settings (key, value, updated_at_ms) VALUES (?1, ?2, ?3)",
                )?;
                for (key, value) in &defaults {
                    statement.execute(rusqlite::params![key, value, timestamp])?;
                }
            }
            transaction.commit()?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> SettingsRepository {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        SettingsRepository::new(database)
    }

    #[test]
    fn load_returns_defaults_when_empty() {
        assert_eq!(repository().load().expect("load"), Settings::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let repository = repository();
        let settings = Settings {
            input_device_id: Some("Built-in Microphone".to_string()),
            gain: 2.0,
            segment_seconds: 30,
            retention_hours: 12,
            auto_start: false,
            frame_ms: 60,
        };
        repository.save(&settings).expect("save");
        assert_eq!(repository.load().expect("load"), settings);
    }

    #[test]
    fn seed_does_not_overwrite_existing_values() {
        let repository = repository();
        let settings = Settings {
            gain: 3.0,
            ..Settings::default()
        };
        repository.save(&settings).expect("save");
        repository.seed_defaults().expect("seed");
        assert_eq!(repository.load().expect("load").gain, 3.0);
    }
}
