//! Reads and writes runtime preferences, and pushes the ones that act immediately.
//!
//! Settings are read on every capture rollover and on every status poll, so they are cached in memory and
//! the database is only touched on write. The cache is the source of truth for readers, which also means
//! a caller never has to handle a database error just to find out the current gain.

use std::sync::{Arc, RwLock};

use crate::audio::GainControl;
use crate::error::{AppError, AppResult};
use crate::models::{Settings, SettingsPatch};
use crate::repositories::SettingsRepository;

pub struct SettingsService {
    repository: Arc<SettingsRepository>,
    cache: RwLock<Settings>,
    gain: GainControl,
}

impl SettingsService {
    /// Load the stored settings, seeding defaults on a fresh database.
    pub fn load(repository: Arc<SettingsRepository>) -> AppResult<Self> {
        repository.seed_defaults()?;
        let settings = repository.load()?;
        let gain = GainControl::new(settings.gain);

        Ok(Self {
            repository,
            cache: RwLock::new(settings),
            gain,
        })
    }

    /// Current settings. Cheap enough to call per request.
    pub fn current(&self) -> Settings {
        match self.cache.read() {
            Ok(guard) => guard.clone(),
            // A poisoned lock means a writer panicked mid update. Serving the defaults keeps the service
            // answering instead of failing every request until it is restarted.
            Err(_) => Settings::default(),
        }
    }

    /// The gain handle shared with the audio callback, so a gain change takes effect on the next buffer
    /// without reopening the device.
    pub fn gain_control(&self) -> GainControl {
        self.gain.clone()
    }

    /// Apply a partial update, persist it, and return the new settings.
    pub fn update(&self, patch: &SettingsPatch) -> AppResult<Settings> {
        let updated = patch.apply_to(&self.current());
        self.replace(updated)
    }

    /// Store a complete settings object.
    pub fn replace(&self, settings: Settings) -> AppResult<Settings> {
        let settings = settings.clamped();
        self.repository.save(&settings)?;

        let mut guard = self
            .cache
            .write()
            .map_err(|_| AppError::internal("settings cache lock was poisoned"))?;
        *guard = settings.clone();
        drop(guard);

        self.gain.set(settings.gain);
        Ok(settings)
    }

    /// Convenience used by the device endpoint, which only ever changes one field.
    pub fn set_input_device(&self, device_id: Option<String>) -> AppResult<Settings> {
        self.update(&SettingsPatch {
            input_device_id: Some(device_id),
            ..SettingsPatch::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn service() -> SettingsService {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let repository = Arc::new(SettingsRepository::new(database));
        SettingsService::load(repository).expect("service")
    }

    #[test]
    fn starts_from_the_stored_defaults() {
        assert_eq!(service().current(), Settings::default());
    }

    #[test]
    fn updating_gain_moves_the_shared_gain_control() {
        let service = service();
        let gain = service.gain_control();
        assert_eq!(gain.get(), 1.0);

        service
            .update(&SettingsPatch {
                gain: Some(2.5),
                ..SettingsPatch::default()
            })
            .expect("update");

        assert_eq!(gain.get(), 2.5);
        assert_eq!(service.current().gain, 2.5);
    }

    #[test]
    fn updates_are_persisted_and_survive_a_reload() {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let repository = Arc::new(SettingsRepository::new(database));
        let service = SettingsService::load(repository.clone()).expect("service");

        service.set_input_device(Some("Scarlett Solo USB".to_string())).expect("select");

        let reloaded = SettingsService::load(repository).expect("reload");
        assert_eq!(
            reloaded.current().input_device_id,
            Some("Scarlett Solo USB".to_string())
        );
    }

    #[test]
    fn selecting_no_device_returns_to_the_system_default() {
        let service = service();
        service.set_input_device(Some("mic".to_string())).expect("select");
        service.set_input_device(None).expect("clear");
        assert_eq!(service.current().input_device_id, None);
    }
}
