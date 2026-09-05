//! Input device payloads.

use serde::{Deserialize, Serialize};

use crate::models::InputDevice;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDto {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub is_selected: bool,
    pub available: bool,
    pub channels: u16,
    pub sample_rate: u32,
}

impl DeviceDto {
    pub fn from_model(device: InputDevice, selected_id: Option<&str>) -> Self {
        Self {
            is_selected: selected_id == Some(device.id.as_str()),
            id: device.id,
            name: device.name,
            is_default: device.is_default,
            available: device.available,
            channels: device.channels,
            sample_rate: device.sample_rate,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceListResponse {
    pub devices: Vec<DeviceDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectDeviceRequest {
    /// `null` returns to the system default input.
    pub device_id: Option<String>,
}
