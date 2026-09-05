//! User editable runtime preferences.
//!
//! Settings live in a key value table rather than a single row table so adding a preference is a code
//! change only, with no schema migration. The typed struct here is the contract: [`Settings::from_pairs`]
//! is the single place that parses the stored strings, and every value is clamped rather than rejected so
//! a hand edited database can never stop the service from booting.

use std::collections::HashMap;

pub const KEY_INPUT_DEVICE_ID: &str = "input_device_id";
pub const KEY_GAIN: &str = "gain";
pub const KEY_SEGMENT_SECONDS: &str = "segment_seconds";
pub const KEY_RETENTION_HOURS: &str = "retention_hours";
pub const KEY_AUTO_START: &str = "auto_start";
pub const KEY_FRAME_MS: &str = "frame_ms";

pub const GAIN_RANGE: (f32, f32) = (0.0, 4.0);
pub const SEGMENT_SECONDS_RANGE: (u32, u32) = (5, 300);
pub const RETENTION_HOURS_RANGE: (u32, u32) = (1, 8760);
pub const FRAME_MS_RANGE: (u32, u32) = (20, 500);

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// `None` means follow the system default input device.
    pub input_device_id: Option<String>,
    pub gain: f32,
    pub segment_seconds: u32,
    pub retention_hours: u32,
    pub auto_start: bool,
    pub frame_ms: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            input_device_id: None,
            gain: 1.0,
            segment_seconds: 10,
            retention_hours: 24,
            auto_start: true,
            frame_ms: 100,
        }
    }
}

impl Settings {
    /// Rebuild the typed settings from the raw key value rows, falling back to the default for anything
    /// missing or unparseable.
    pub fn from_pairs(pairs: &HashMap<String, String>) -> Self {
        let defaults = Self::default();

        let input_device_id = pairs
            .get(KEY_INPUT_DEVICE_ID)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        Self {
            input_device_id,
            gain: parse_f32(pairs.get(KEY_GAIN), defaults.gain),
            segment_seconds: parse_u32(pairs.get(KEY_SEGMENT_SECONDS), defaults.segment_seconds),
            retention_hours: parse_u32(pairs.get(KEY_RETENTION_HOURS), defaults.retention_hours),
            auto_start: parse_bool(pairs.get(KEY_AUTO_START), defaults.auto_start),
            frame_ms: parse_u32(pairs.get(KEY_FRAME_MS), defaults.frame_ms),
        }
        .clamped()
    }

    /// Project the settings back onto the key value rows the repository stores.
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        vec![
            (
                KEY_INPUT_DEVICE_ID.to_string(),
                self.input_device_id.clone().unwrap_or_default(),
            ),
            (KEY_GAIN.to_string(), self.gain.to_string()),
            (
                KEY_SEGMENT_SECONDS.to_string(),
                self.segment_seconds.to_string(),
            ),
            (
                KEY_RETENTION_HOURS.to_string(),
                self.retention_hours.to_string(),
            ),
            (KEY_AUTO_START.to_string(), self.auto_start.to_string()),
            (KEY_FRAME_MS.to_string(), self.frame_ms.to_string()),
        ]
    }

    /// Force every value into its documented range.
    pub fn clamped(mut self) -> Self {
        self.gain = if self.gain.is_finite() {
            self.gain.clamp(GAIN_RANGE.0, GAIN_RANGE.1)
        } else {
            1.0
        };
        self.segment_seconds = self
            .segment_seconds
            .clamp(SEGMENT_SECONDS_RANGE.0, SEGMENT_SECONDS_RANGE.1);
        self.retention_hours = self
            .retention_hours
            .clamp(RETENTION_HOURS_RANGE.0, RETENTION_HOURS_RANGE.1);
        self.frame_ms = self.frame_ms.clamp(FRAME_MS_RANGE.0, FRAME_MS_RANGE.1);
        self
    }

    /// How long the retention janitor keeps material, in milliseconds.
    pub fn retention_ms(&self) -> i64 {
        self.retention_hours as i64 * 3_600_000
    }
}

fn parse_f32(raw: Option<&String>, fallback: f32) -> f32 {
    raw.and_then(|value| value.trim().parse::<f32>().ok())
        .unwrap_or(fallback)
}

fn parse_u32(raw: Option<&String>, fallback: u32) -> u32 {
    raw.and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(fallback)
}

fn parse_bool(raw: Option<&String>, fallback: bool) -> bool {
    match raw.map(|value| value.trim().to_ascii_lowercase()) {
        Some(value) if value == "true" || value == "1" || value == "yes" => true,
        Some(value) if value == "false" || value == "0" || value == "no" => false,
        _ => fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_pairs() {
        let settings = Settings {
            input_device_id: Some("Scarlett Solo USB".to_string()),
            gain: 1.5,
            segment_seconds: 20,
            retention_hours: 48,
            auto_start: false,
            frame_ms: 40,
        };
        let pairs: HashMap<String, String> = settings.to_pairs().into_iter().collect();
        assert_eq!(Settings::from_pairs(&pairs), settings);
    }

    #[test]
    fn empty_device_id_becomes_none() {
        let pairs = HashMap::from([(KEY_INPUT_DEVICE_ID.to_string(), "  ".to_string())]);
        assert_eq!(Settings::from_pairs(&pairs).input_device_id, None);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let pairs = HashMap::from([
            (KEY_GAIN.to_string(), "99".to_string()),
            (KEY_SEGMENT_SECONDS.to_string(), "1".to_string()),
            (KEY_FRAME_MS.to_string(), "5000".to_string()),
        ]);
        let settings = Settings::from_pairs(&pairs);
        assert_eq!(settings.gain, GAIN_RANGE.1);
        assert_eq!(settings.segment_seconds, SEGMENT_SECONDS_RANGE.0);
        assert_eq!(settings.frame_ms, FRAME_MS_RANGE.1);
    }

    #[test]
    fn garbage_falls_back_to_defaults() {
        let pairs = HashMap::from([
            (KEY_GAIN.to_string(), "loud".to_string()),
            (KEY_AUTO_START.to_string(), "maybe".to_string()),
        ]);
        let settings = Settings::from_pairs(&pairs);
        assert_eq!(settings.gain, Settings::default().gain);
        assert_eq!(settings.auto_start, Settings::default().auto_start);
    }

    #[test]
    fn retention_converts_to_milliseconds() {
        let settings = Settings {
            retention_hours: 2,
            ..Settings::default()
        };
        assert_eq!(settings.retention_ms(), 7_200_000);
    }
}
