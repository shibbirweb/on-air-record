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

    #[test]
    fn with_no_devices_and_nothing_configured_the_list_is_empty() {
        assert!(arrange(Vec::new(), None).is_empty());
    }
}
