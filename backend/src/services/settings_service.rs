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

    /// Report what would happen if recordings were pointed at `configured`, without changing anything.
    ///
    /// Deliberately non mutating, unlike the check on save. A test that created the directory would
    /// litter the disk with empty folders every time somebody tried a path and thought better of it, so a
    /// missing directory is judged by whether its nearest existing ancestor would accept one.
    pub fn probe_recordings_dir(&self, configured: Option<&str>) -> DirectoryProbe {
        let resolved = self.config.effective_recordings_dir(configured);

        if resolved.exists() {
            if !resolved.is_dir() {
                return DirectoryProbe::failed(
                    &resolved,
                    true,
                    "That path is a file, not a directory.".to_string(),
                );
            }

            let readable = std::fs::read_dir(&resolved).is_ok();
            if let Err(error) = write_probe(&resolved) {
                return DirectoryProbe {
                    readable,
                    ..DirectoryProbe::failed(
                        &resolved,
                        true,
                        format!("The directory exists but cannot be written to: {error}"),
                    )
                };
            }

            return DirectoryProbe {
                ok: readable,
                resolved_path: resolved.to_string_lossy().to_string(),
                exists: true,
                will_create: false,
                readable,
                writable: true,
                message: if readable {
                    "Ready to use. The directory exists and is readable and writable.".to_string()
                } else {
                    "Writable, but its contents cannot be listed, so playback of existing recordings may fail."
                        .to_string()
                },
            };
        }

        // Nothing at the path yet. Whether it could be created is a question about its parent.
        let Some(ancestor) = resolved.ancestors().skip(1).find(|path| path.exists()) else {
            return DirectoryProbe::failed(
                &resolved,
                false,
                "No part of that path exists, so it cannot be created.".to_string(),
            );
        };

        if !ancestor.is_dir() {
            return DirectoryProbe::failed(
                &resolved,
                false,
                format!(
                    "'{}' is a file, so nothing can be created inside it.",
                    ancestor.display()
                ),
            );
        }

        match write_probe(ancestor) {
            Ok(()) => DirectoryProbe {
                ok: true,
                resolved_path: resolved.to_string_lossy().to_string(),
                exists: false,
                will_create: true,
                readable: true,
                writable: true,
                message: format!(
                    "Does not exist yet. It will be created inside '{}' when you save.",
                    ancestor.display()
                ),
            },
            Err(error) => DirectoryProbe::failed(
                &resolved,
                false,
                format!(
                    "It cannot be created: '{}' is not writable: {error}",
                    ancestor.display()
                ),
            ),
        }
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

/// What a directory would do if recordings were pointed at it.
#[derive(Debug, Clone)]
pub struct DirectoryProbe {
    pub ok: bool,
    /// The absolute path the setting resolves to, which is what the operator should be shown.
    pub resolved_path: String,
    pub exists: bool,
    /// True when the directory is missing but its parent would allow it to be created on save.
    pub will_create: bool,
    pub readable: bool,
    pub writable: bool,
    /// A sentence fit to put in front of a person.
    pub message: String,
}

impl DirectoryProbe {
    fn failed(resolved: &Path, exists: bool, message: String) -> Self {
        Self {
            ok: false,
            resolved_path: resolved.to_string_lossy().to_string(),
            exists,
            will_create: false,
            readable: false,
            writable: false,
            message,
        }
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

    write_probe(dir).map_err(|error| {
        AppError::bad_request(format!("'{}' is not writable: {error}", dir.display()))
    })
}

/// Write a small file into `dir` and remove it again.
///
/// The only honest test of writability. A directory can exist and be readable while still refusing
/// writes because of permissions, a read only mount, or a full disk, and every one of those would
/// otherwise surface as a stream of failed segments rather than a message on the settings page.
fn write_probe(dir: &Path) -> Result<(), std::io::Error> {
    let probe = dir.join(".oar-write-test");
    std::fs::write(&probe, b"on-air-record")?;
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
    fn probing_an_existing_writable_directory_reports_ready() {
        let service = service();
        let target = std::env::temp_dir().join(format!("oar-probe-ok-{}", std::process::id()));
        std::fs::create_dir_all(&target).expect("target");

        let probe = service.probe_recordings_dir(Some(&target.to_string_lossy()));

        assert!(probe.ok);
        assert!(probe.exists);
        assert!(!probe.will_create);
        assert!(probe.readable && probe.writable);
        assert_eq!(probe.resolved_path, target.to_string_lossy());

        std::fs::remove_dir_all(&target).ok();
    }

    #[test]
    fn probing_a_missing_directory_reports_that_it_would_be_created() {
        let service = service();
        let target = std::env::temp_dir()
            .join(format!("oar-probe-new-{}", std::process::id()))
            .join("nested");
        std::fs::remove_dir_all(target.parent().expect("parent")).ok();

        let probe = service.probe_recordings_dir(Some(&target.to_string_lossy()));

        assert!(probe.ok);
        assert!(!probe.exists);
        assert!(probe.will_create);
        // Testing must not leave empty directories behind for every path somebody tried.
        assert!(!target.exists());
    }

    #[test]
    fn probing_a_file_reports_the_reason_rather_than_a_bare_failure() {
        let service = service();
        let blocker = std::env::temp_dir().join(format!("oar-probe-file-{}", std::process::id()));
        std::fs::write(&blocker, b"not a directory").expect("blocker");

        let probe = service.probe_recordings_dir(Some(&blocker.to_string_lossy()));
        assert!(!probe.ok);
        assert!(probe.message.contains("file"));

        let nested = service.probe_recordings_dir(Some(&blocker.join("inside").to_string_lossy()));
        assert!(!nested.ok);
        assert!(nested.message.contains("file"));

        std::fs::remove_file(&blocker).ok();
    }

    #[test]
    fn probing_nothing_reports_the_default_location() {
        let service = service();
        let probe = service.probe_recordings_dir(None);

        assert!(probe.resolved_path.ends_with("recordings"));
        // The default lives under the data directory, which the service already owns.
        assert!(probe.ok);
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
