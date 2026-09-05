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

    /// Restore the shipped defaults.
    ///
    /// The selected input device is deliberately preserved. It is chosen from a different part of the UI,
    /// and silently moving the recorder onto another microphone is not what someone resetting the tuning
    /// sliders is asking for.
    pub fn reset(&self) -> AppResult<Settings> {
        let input_device_id = self.current().input_device_id;
        self.replace(Settings {
            input_device_id,
            ..Settings::default()
        })
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

        service
            .set_input_device(Some("Scarlett Solo USB".to_string()))
            .expect("select");

        let reloaded = SettingsService::load(repository).expect("reload");
        assert_eq!(
            reloaded.current().input_device_id,
            Some("Scarlett Solo USB".to_string())
        );
    }

    #[test]
    fn reset_restores_the_defaults_but_keeps_the_device() {
        let service = service();
        service
            .replace(Settings {
                input_device_id: Some("Scarlett Solo USB".to_string()),
                gain: 3.5,
                segment_seconds: 60,
                retention_hours: 168,
                auto_start: false,
                frame_ms: 40,
            })
            .expect("configure");

        let reset = service.reset().expect("reset");

        assert_eq!(reset.gain, Settings::default().gain);
        assert_eq!(reset.segment_seconds, Settings::default().segment_seconds);
        assert_eq!(reset.retention_hours, Settings::default().retention_hours);
        assert_eq!(reset.auto_start, Settings::default().auto_start);
        assert_eq!(reset.frame_ms, Settings::default().frame_ms);
        // The microphone is chosen elsewhere, so resetting the tuning must not move it.
        assert_eq!(reset.input_device_id, Some("Scarlett Solo USB".to_string()));
    }

    #[test]
    fn reset_is_persisted_and_moves_the_live_gain() {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let repository = Arc::new(SettingsRepository::new(database));
        let service = SettingsService::load(repository.clone()).expect("service");
        let gain = service.gain_control();

        service
            .update(&SettingsPatch {
                gain: Some(3.0),
                ..SettingsPatch::default()
            })
            .expect("update");
        assert_eq!(gain.get(), 3.0);

        service.reset().expect("reset");

        assert_eq!(gain.get(), Settings::default().gain);
        let reloaded = SettingsService::load(repository).expect("reload");
        assert_eq!(reloaded.current(), Settings::default());
    }

    #[test]
    fn selecting_no_device_returns_to_the_system_default() {
        let service = service();
        service
            .set_input_device(Some("mic".to_string()))
            .expect("select");
        service.set_input_device(None).expect("clear");
        assert_eq!(service.current().input_device_id, None);
    }
}
