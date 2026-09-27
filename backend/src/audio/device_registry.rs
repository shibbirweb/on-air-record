//! Enumerates and resolves host input devices.
//!
//! `cpal` exposes a device by name only. That is unfortunate as an identifier, but it is the one handle
//! that exists on CoreAudio, ALSA, and WASAPI alike and that survives a restart, so the name doubles as
//! the persisted id. Two identical interfaces plugged into the same machine will therefore share an id,
//! which is a known limitation of the platform APIs rather than of this code.

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{Device, SupportedStreamConfig};

use crate::error::{AppError, AppResult};
use crate::models::InputDevice;

pub struct DeviceRegistry;

impl DeviceRegistry {
    /// Every input the host currently offers, with `selected_id` marked and, when that id is not present,
    /// appended as an unavailable entry so the UI can say the configured microphone is unplugged.
    pub fn list(selected_id: Option<&str>) -> AppResult<Vec<InputDevice>> {
        home_the_device_enumerator();
        let host = cpal::default_host();
        let default_name = host
            .default_input_device()
            .and_then(|device| device.name().ok());

        let devices = host.input_devices().map_err(|error| {
            AppError::audio(format!("could not enumerate input devices: {error}"))
        })?;

        let mut listed = Vec::new();
        for device in devices {
            let Ok(name) = device.name() else {
                // A device whose name cannot be read cannot be selected later either, so skip it rather
                // than showing an entry that would fail on click.
                continue;
            };

            let config = device.default_input_config().ok();
            listed.push(InputDevice {
                is_default: default_name.as_deref() == Some(name.as_str()),
                available: true,
                channels: config.as_ref().map(|item| item.channels()).unwrap_or(0),
                sample_rate: config
                    .as_ref()
                    .map(|item| item.sample_rate().0)
                    .unwrap_or(0),
                id: name.clone(),
                name,
            });
        }

        Ok(arrange(listed, selected_id))
    }

    /// Find the device to capture from.
    ///
    /// A configured device that has gone missing falls back to the system default rather than failing,
    /// because an unplugged interface should not stop the service from recording anything at all. The
    /// caller learns which device was actually opened from the returned descriptor.
    pub fn resolve(
        preferred_id: Option<&str>,
    ) -> AppResult<(Device, SupportedStreamConfig, InputDevice)> {
        home_the_device_enumerator();
        let host = cpal::default_host();

        let device = match preferred_id {
            Some(wanted) => find_by_name(&host, wanted).or_else(|| {
                tracing::warn!(
                    device_id = wanted,
                    "configured input device is not available, falling back to the default"
                );
                host.default_input_device()
            }),
            None => host.default_input_device(),
        };

        let device = device.ok_or_else(|| {
            AppError::audio("no input device is available on this machine".to_string())
        })?;

        let name = device
            .name()
            .unwrap_or_else(|_| "unknown input device".to_string());

        let config = device.default_input_config().map_err(|error| {
            AppError::audio(format!(
                "device '{name}' has no usable input config: {error}"
            ))
        })?;

        let descriptor = InputDevice {
            id: name.clone(),
            name,
            is_default: preferred_id.is_none(),
            available: true,
            channels: config.channels(),
            sample_rate: config.sample_rate().0,
        };

        Ok((device, config, descriptor))
    }
}

/// On Windows, create cpal's device enumerator on a thread that lives as long as the process.
///
/// cpal 0.16 keeps one `IMMDeviceEnumerator` for the whole process, made in the COM apartment of whichever
/// thread first lists devices, and it sets COM up per thread and undoes it when the thread ends. Devices
/// are listed here from short lived blocking threads, so the end of the first one took the enumerator's
/// apartment with it, and the next listing, or opening the microphone, crashed the service with an access
/// violation. Made first on a thread that parks for good, the enumerator stays valid for every thread.
/// Other platforms have no COM, so there it does nothing, but it compiles everywhere so it is checked
/// everywhere.
fn home_the_device_enumerator() {
    if !cfg!(windows) {
        return;
    }
    static HOMED: std::sync::Once = std::sync::Once::new();
    HOMED.call_once(|| {
        let (ready, is_ready) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("oar-audio-devices".to_string())
            .spawn(move || {
                // Any listing makes the enumerator; what it finds does not matter here.
                let _ = cpal::default_host()
                    .input_devices()
                    .map(|devices| devices.count());
                let _ = ready.send(());
                loop {
                    std::thread::park();
                }
            });
        match spawned {
            Ok(_) => {
                let _ = is_ready.recv();
            }
            Err(error) => tracing::warn!(%error, "could not start the audio device thread"),
        }
    });
}

fn find_by_name(host: &cpal::Host, wanted: &str) -> Option<Device> {
    host.input_devices()
        .ok()?
        .find(|device| device.name().map(|name| name == wanted).unwrap_or(false))
}

/// The configured device appended as unavailable when the host does not offer it, so the UI can say the
/// microphone is unplugged rather than silently showing another one selected; then the system default
/// first and the rest by name, case aside. Apart from `list` so it can be tested without audio hardware.
fn arrange(mut listed: Vec<InputDevice>, selected_id: Option<&str>) -> Vec<InputDevice> {
    if let Some(selected) = selected_id {
        if !listed.iter().any(|device| device.id == selected) {
            listed.push(InputDevice::unavailable(selected));
        }
    }
    listed.sort_by(|left, right| {
        right
            .is_default
            .cmp(&left.is_default)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    listed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str, is_default: bool) -> InputDevice {
        InputDevice {
            id: name.to_string(),
            name: name.to_string(),
            is_default,
            available: true,
            channels: 1,
            sample_rate: 48_000,
        }
    }

    fn names(devices: &[InputDevice]) -> Vec<&str> {
        devices.iter().map(|device| device.name.as_str()).collect()
    }

    #[test]
    fn the_system_default_comes_first_and_the_rest_by_name_whatever_the_case() {
        let arranged = arrange(
            vec![
                device("zoom H1", false),
                device("Built-in", true),
                device("apogee", false),
            ],
            None,
        );
        assert_eq!(names(&arranged), ["Built-in", "apogee", "zoom H1"]);
    }

    #[test]
    fn a_configured_device_that_is_unplugged_is_listed_as_unavailable() {
        let arranged = arrange(vec![device("Built-in", true)], Some("Scarlett Solo"));
        let missing = arranged
            .iter()
            .find(|device| device.id == "Scarlett Solo")
            .expect("still listed");
        assert!(!missing.available);
        assert!(!missing.is_default);
        assert_eq!(arranged.len(), 2);
    }

    #[test]
    fn a_configured_device_that_is_present_is_not_listed_twice() {
        let arranged = arrange(
            vec![device("Built-in", true), device("Scarlett Solo", false)],
            Some("Scarlett Solo"),
        );
        assert_eq!(arranged.len(), 2);
        assert!(arranged.iter().all(|device| device.available));
    }

    /// The service lists devices from short lived worker threads. On Windows the first such thread's end
    /// used to take cpal's shared device enumerator with it, and the next listing crashed the process;
    /// found by CI, whose Windows runners have no sound hardware. Harmless elsewhere, it is the pattern
    /// that crashed, so it runs everywhere.
    #[test]
    fn listing_devices_from_threads_that_come_and_go_never_crashes() {
        for _ in 0..3 {
            std::thread::spawn(|| {
                let _ = DeviceRegistry::list(None);
            })
            .join()
            .expect("the listing thread finished");
        }
        // And from this thread, after all of them have gone.
        let _ = DeviceRegistry::list(None);
    }

    #[test]
    fn with_no_devices_and_nothing_configured_the_list_is_empty() {
        assert!(arrange(Vec::new(), None).is_empty());
    }
}
