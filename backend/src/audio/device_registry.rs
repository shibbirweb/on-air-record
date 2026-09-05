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

        Ok(listed)
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
