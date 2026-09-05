//! Reads and writes runtime preferences, and pushes the ones that act immediately.
//!
//! Settings are read on every capture rollover and on every status poll, so they are cached in memory and
//! the database is only touched on write. The cache is the source of truth for readers, which also means
//! a caller never has to handle a database error just to find out the current gain.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::audio::GainControl;
use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::models::{Settings, SettingsPatch};
use crate::repositories::SettingsRepository;

pub struct SettingsService {
    repository: Arc<SettingsRepository>,
    config: Arc<AppConfig>,
    cache: RwLock<Settings>,
    gain: GainControl,
}

impl SettingsService {
    /// Load the stored settings, seeding defaults on a fresh database.
    pub fn load(repository: Arc<SettingsRepository>, config: Arc<AppConfig>) -> AppResult<Self> {
        repository.seed_defaults()?;
        let settings = repository.load()?;
        let gain = GainControl::new(settings.gain);

        Ok(Self {
            repository,
            config,
            cache: RwLock::new(settings),
            gain,
        })
    }

    /// Where segments are written under the current settings.
    pub fn effective_recordings_dir(&self) -> PathBuf {
        self.config
            .effective_recordings_dir(self.current().recordings_dir.as_deref())
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

        // A directory that cannot be written to must be refused here. Accepting it would leave the
        // recorder failing on every segment with nothing in the UI to explain why.
        if patch.recordings_dir.is_some() {
            let resolved = self
                .config
                .effective_recordings_dir(updated.recordings_dir.as_deref());
            check_writable(&resolved)?;
        }

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

/// Confirm a directory exists, or can be made, and that we can actually write into it.
///
/// Creating a probe file is the only honest test. A directory can exist and be readable while still
/// refusing writes because of permissions, a read only mount, or a full disk, and every one of those
/// would otherwise surface as a stream of failed segments instead of a message on the settings page.
fn check_writable(dir: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dir).map_err(|error| {
        AppError::bad_request(format!(
            "cannot use '{}' for recordings: {error}",
            dir.display()
        ))
    })?;

    let probe = dir.join(".oar-write-test");
    std::fs::write(&probe, b"on-air-record").map_err(|error| {
        AppError::bad_request(format!("'{}' is not writable: {error}", dir.display()))
    })?;
    let _ = std::fs::remove_file(&probe);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    /// A config rooted in a scratch directory, so writability checks touch nothing real.
    fn config() -> Arc<AppConfig> {
        let data_dir = std::env::temp_dir().join(format!("oar-settings-{}", std::process::id()));
        std::fs::create_dir_all(&data_dir).expect("data dir");
        Arc::new(AppConfig {
            data_dir,
            ..AppConfig::default()
        })
    }

    fn service() -> SettingsService {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let repository = Arc::new(SettingsRepository::new(database));
        SettingsService::load(repository, config()).expect("service")
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
        let service = SettingsService::load(repository.clone(), config()).expect("service");

        service
            .set_input_device(Some("Scarlett Solo USB".to_string()))
            .expect("select");

        let reloaded = SettingsService::load(repository, config()).expect("reload");
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
                retention_hours: Some(168),
                auto_start: false,
                frame_ms: 40,
                recording_sample_rate: None,
                recordings_dir: None,
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
        let service = SettingsService::load(repository.clone(), config()).expect("service");
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
        let reloaded = SettingsService::load(repository, config()).expect("reload");
        assert_eq!(reloaded.current(), Settings::default());
    }

    #[test]
    fn a_writable_recordings_directory_is_accepted() {
        let service = service();
        let target = std::env::temp_dir().join(format!("oar-recdir-{}", std::process::id()));
        std::fs::remove_dir_all(&target).ok();

        let stored = service
            .update(&SettingsPatch {
                recordings_dir: Some(Some(target.to_string_lossy().to_string())),
                ..SettingsPatch::default()
            })
            .expect("accepted");

        assert_eq!(
            stored.recordings_dir,
            Some(target.to_string_lossy().to_string())
        );
        // Accepting the setting also creates the directory, so the recorder does not have to.
        assert!(target.is_dir());
        assert_eq!(service.effective_recordings_dir(), target);

        std::fs::remove_dir_all(&target).ok();
    }

    #[test]
    fn an_unusable_recordings_directory_is_refused() {
        let service = service();
        let before = service.current();

        // A path under a regular file can never be a directory.
        let blocker = std::env::temp_dir().join(format!("oar-blocker-{}", std::process::id()));
        std::fs::write(&blocker, b"not a directory").expect("blocker");

        let outcome = service.update(&SettingsPatch {
            recordings_dir: Some(Some(blocker.join("nested").to_string_lossy().to_string())),
            ..SettingsPatch::default()
        });

        assert!(outcome.is_err());
        // The rejected value must not have been persisted on the way out.
        assert_eq!(service.current(), before);

        std::fs::remove_file(&blocker).ok();
    }

    #[test]
    fn clearing_the_directory_returns_to_the_default() {
        let service = service();
        let target = std::env::temp_dir().join(format!("oar-recdir-clear-{}", std::process::id()));

        service
            .update(&SettingsPatch {
                recordings_dir: Some(Some(target.to_string_lossy().to_string())),
                ..SettingsPatch::default()
            })
            .expect("set");
        service
            .update(&SettingsPatch {
                recordings_dir: Some(None),
                ..SettingsPatch::default()
            })
            .expect("clear");

        assert_eq!(service.current().recordings_dir, None);
        assert!(service.effective_recordings_dir().ends_with("recordings"));

        std::fs::remove_dir_all(&target).ok();
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
