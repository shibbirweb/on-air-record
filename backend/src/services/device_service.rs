//! Input device listing and selection.

use std::sync::Arc;

use crate::error::{AppError, AppResult};
use crate::models::InputDevice;
use crate::services::{CaptureService, SettingsService};

pub struct DeviceService {
    settings: Arc<SettingsService>,
    capture: Arc<CaptureService>,
}

impl DeviceService {
    pub fn new(settings: Arc<SettingsService>, capture: Arc<CaptureService>) -> Self {
        Self { settings, capture }
    }

    /// Enumerate the host inputs, marking the configured one.
    ///
    /// Enumeration talks to the operating system audio service and can block for tens of milliseconds, so
    /// it runs on the blocking pool rather than on a runtime worker.
    pub async fn list(&self) -> AppResult<Vec<InputDevice>> {
        let selected = self.settings.current().input_device_id;

        tokio::task::spawn_blocking(move || {
            crate::audio::DeviceRegistry::list(selected.as_deref())
        })
        .await
        .map_err(|error| AppError::internal(format!("device enumeration task failed: {error}")))?
    }

    /// Which device id is stored in settings, if any.
    pub fn selected_id(&self) -> Option<String> {
        self.settings.current().input_device_id
    }

    /// Persist a device choice and, when capture is running, move the live stream onto it.
    ///
    /// The device is validated against the host list first. Storing an id that does not exist would make
    /// the service silently fall back to the default input, which looks like the selection was ignored.
    pub async fn select(&self, device_id: Option<String>) -> AppResult<()> {
        if let Some(wanted) = device_id.as_deref() {
            let devices = self.list().await?;
            let known = devices
                .iter()
                .any(|device| device.id == wanted && device.available);
            if !known {
                return Err(AppError::not_found(format!(
                    "input device '{wanted}' is not available on this machine"
                )));
            }
        }

        self.settings.set_input_device(device_id)?;

        if self.capture.is_active() {
            // A new device usually means a new sample rate, so this deliberately starts a fresh session
            // rather than splicing two formats into one timeline.
            self.capture.restart().await?;
        }

        Ok(())
    }
}
