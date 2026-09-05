//! Settings payloads.

use serde::{Deserialize, Deserializer, Serialize};

use crate::models::{Settings, SettingsPatch};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub input_device_id: Option<String>,
    pub gain: f32,
    pub segment_seconds: u32,
    /// `null` means recordings are kept forever.
    pub retention_hours: Option<u32>,
    pub auto_start: bool,
    pub frame_ms: u32,
    /// `null` follows the capture device's own rate.
    pub recording_sample_rate: Option<u32>,
    /// `null` means the default location under the data directory.
    pub recordings_dir: Option<String>,
    /// Where segments are actually written right now, resolved from the setting. Always absolute, so the
    /// UI can show the effective path rather than an empty box when the default is in force.
    pub effective_recordings_dir: String,
}

impl SettingsDto {
    /// Project the settings onto the wire, resolving the effective recordings directory against `config`.
    pub fn new(settings: Settings, config: &crate::config::AppConfig) -> Self {
        let effective_recordings_dir = config
            .effective_recordings_dir(settings.recordings_dir.as_deref())
            .to_string_lossy()
            .to_string();

        Self {
            input_device_id: settings.input_device_id,
            gain: settings.gain,
            segment_seconds: settings.segment_seconds,
            retention_hours: settings.retention_hours,
            auto_start: settings.auto_start,
            frame_ms: settings.frame_ms,
            recording_sample_rate: settings.recording_sample_rate,
            recordings_dir: settings.recordings_dir,
            effective_recordings_dir,
        }
    }
}

/// A partial update. Absent fields are left alone, and an explicit `null` device clears the selection,
/// which is why that one field is deserialised into a nested option.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatchRequest {
    #[serde(default, deserialize_with = "deserialize_nested_option")]
    pub input_device_id: Option<Option<String>>,
    #[serde(default)]
    pub gain: Option<f32>,
    #[serde(default)]
    pub segment_seconds: Option<u32>,
    /// Present and null switches to keeping forever; absent leaves the window alone.
    #[serde(default, deserialize_with = "deserialize_nested_option_u32")]
    pub retention_hours: Option<Option<u32>>,
    #[serde(default)]
    pub auto_start: Option<bool>,
    #[serde(default)]
    pub frame_ms: Option<u32>,
    /// Present and null follows the device's own rate.
    #[serde(default, deserialize_with = "deserialize_nested_option_u32")]
    pub recording_sample_rate: Option<Option<u32>>,
    /// Present and null returns to the default location.
    #[serde(default, deserialize_with = "deserialize_nested_option")]
    pub recordings_dir: Option<Option<String>>,
}

impl From<SettingsPatchRequest> for SettingsPatch {
    fn from(request: SettingsPatchRequest) -> Self {
        Self {
            input_device_id: request.input_device_id,
            gain: request.gain,
            segment_seconds: request.segment_seconds,
            retention_hours: request.retention_hours,
            auto_start: request.auto_start,
            frame_ms: request.frame_ms,
            recording_sample_rate: request.recording_sample_rate,
            recordings_dir: request.recordings_dir,
        }
    }
}

/// Turn a present but null field into `Some(None)` rather than `None`, so "clear this" and "do not touch
/// this" stay distinguishable.
fn deserialize_nested_option<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

/// The same trick for the retention window, where a present null means keep forever.
fn deserialize_nested_option_u32<'de, D>(deserializer: D) -> Result<Option<Option<u32>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<u32>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_device_field_leaves_the_selection_alone() {
        let request: SettingsPatchRequest = serde_json::from_str(r#"{"gain":2.0}"#).expect("parse");
        assert_eq!(request.input_device_id, None);
        assert_eq!(request.gain, Some(2.0));
    }

    #[test]
    fn an_explicit_null_device_clears_the_selection() {
        let request: SettingsPatchRequest =
            serde_json::from_str(r#"{"inputDeviceId":null}"#).expect("parse");
        assert_eq!(request.input_device_id, Some(None));
    }

    #[test]
    fn a_named_device_is_carried_through_to_the_patch() {
        let request: SettingsPatchRequest =
            serde_json::from_str(r#"{"inputDeviceId":"Scarlett Solo USB"}"#).expect("parse");
        let patch: SettingsPatch = request.into();
        assert_eq!(
            patch.input_device_id,
            Some(Some("Scarlett Solo USB".to_string()))
        );
    }

    #[test]
    fn settings_serialise_with_camel_case_keys() {
        let json = serde_json::to_value(SettingsDto::new(
            Settings::default(),
            &crate::config::AppConfig::default(),
        ))
        .expect("serialise");
        assert_eq!(json["segmentSeconds"], 10);
        assert_eq!(json["autoStart"], true);
    }
}

/// Request to try a recordings directory without saving it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestDirectoryRequest {
    /// `null` or empty tests the default location.
    pub path: Option<String>,
}

/// The outcome of trying a directory.
///
/// A failed test is a normal answer rather than an API error, so this comes back with a 200 and `ok`
/// set to false. The client always gets the same shape and never has to parse an error envelope to find
/// out why a path will not work.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestDirectoryResponse {
    pub ok: bool,
    pub resolved_path: String,
    pub exists: bool,
    pub will_create: bool,
    pub readable: bool,
    pub writable: bool,
    pub message: String,
}

impl From<crate::services::settings_service::DirectoryProbe> for TestDirectoryResponse {
    fn from(probe: crate::services::settings_service::DirectoryProbe) -> Self {
        Self {
            ok: probe.ok,
            resolved_path: probe.resolved_path,
            exists: probe.exists,
            will_create: probe.will_create,
            readable: probe.readable,
            writable: probe.writable,
            message: probe.message,
        }
    }
}
