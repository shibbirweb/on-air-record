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

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str) -> InputDevice {
        InputDevice {
            id: id.to_string(),
            name: format!("{id} microphone"),
            is_default: id == "built-in",
            available: true,
            channels: 1,
            sample_rate: 48_000,
        }
    }

    #[test]
    fn the_stored_device_is_marked_selected_and_no_other() {
        assert!(DeviceDto::from_model(device("usb"), Some("usb")).is_selected);
        assert!(!DeviceDto::from_model(device("built-in"), Some("usb")).is_selected);
        assert!(
            !DeviceDto::from_model(device("usb"), None).is_selected,
            "following the system default"
        );
    }

    #[test]
    fn devices_travel_with_camel_case_fields() {
        let json = serde_json::to_value(DeviceDto::from_model(device("built-in"), None))
            .expect("serialise");
        assert_eq!(json["id"], "built-in");
        assert_eq!(json["name"], "built-in microphone");
        assert_eq!(json["isDefault"], true);
        assert_eq!(json["isSelected"], false);
        assert_eq!(json["available"], true);
        assert_eq!(json["channels"], 1);
        assert_eq!(json["sampleRate"], 48_000);
    }

    #[test]
    fn a_selection_is_a_device_id_or_null_for_the_system_default() {
        let named: SelectDeviceRequest =
            serde_json::from_str(r#"{"deviceId":"usb"}"#).expect("parse");
        assert_eq!(named.device_id.as_deref(), Some("usb"));
        let default: SelectDeviceRequest =
            serde_json::from_str(r#"{"deviceId":null}"#).expect("parse");
        assert_eq!(default.device_id, None);
    }
}
