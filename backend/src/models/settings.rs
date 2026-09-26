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
pub const KEY_AUTO_START_DELAY_SECONDS: &str = "auto_start_delay_seconds";
pub const KEY_FRAME_MS: &str = "frame_ms";
pub const KEY_RECORDINGS_DIR: &str = "recordings_dir";
pub const KEY_RECORDING_SAMPLE_RATE: &str = "recording_sample_rate";
pub const KEY_CHECK_FOR_UPDATES: &str = "check_for_updates";

/// Sample rates the recorder will downmix to, highest first.
///
/// Uncompressed PCM means the rate *is* the bit rate: at 16 bit mono, 48 kHz is 768 kbit/s and 8 kHz is
/// 128 kbit/s, so this list is also the storage ladder. Anything above the device's own rate is not
/// offered, because upsampling would cost disk without adding information.
pub const SUPPORTED_SAMPLE_RATES: [u32; 5] = [48_000, 32_000, 24_000, 16_000, 8_000];

/// Bits per second a mono 16 bit stream occupies at `sample_rate`.
pub fn bit_rate_for(sample_rate: u32) -> u32 {
    sample_rate * 16
}

pub const GAIN_RANGE: (f32, f32) = (0.0, 4.0);
pub const SEGMENT_SECONDS_RANGE: (u32, u32) = (5, 300);
/// Ten years is the largest finite window. Anything longer is really "keep forever", which has its own
/// representation rather than being encoded as an implausibly large number.
pub const RETENTION_HOURS_RANGE: (u32, u32) = (1, 87_600);
pub const FRAME_MS_RANGE: (u32, u32) = (20, 500);
/// Ten minutes is far longer than any USB interface takes to enumerate. A delay beyond that is more
/// likely a typo than a plan, and would leave a freshly booted recorder silently idle for too long.
pub const AUTO_START_DELAY_SECONDS_RANGE: (u32, u32) = (0, 600);

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// `None` means follow the system default input device.
    pub input_device_id: Option<String>,
    pub gain: f32,
    pub segment_seconds: u32,
    /// How long recordings are kept. `None` means keep them forever, which disables pruning entirely and
    /// makes the disk the only limit.
    pub retention_hours: Option<u32>,
    pub auto_start: bool,
    /// How long auto start waits after the service boots before opening the device.
    ///
    /// Exists for hosts started by a service manager, where the process can come up before a USB
    /// microphone has enumerated. Opening too early falls back to the default input and records silence
    /// from a device nobody meant to use, so the operator can buy the hardware a few seconds.
    pub auto_start_delay_seconds: u32,
    pub frame_ms: u32,
    /// Rate the recorder downsamples to. `None` keeps the device's own rate, which is the best quality
    /// the hardware offers and the largest files.
    pub recording_sample_rate: Option<u32>,
    /// Where segment files are written. `None` uses `<data dir>/recordings`.
    ///
    /// Always an absolute path when set, because the process working directory is not something the
    /// operator controls once the service runs under a service manager.
    pub recordings_dir: Option<String>,
    /// Whether to ask GitHub every few hours if a newer release exists. It is the only request the
    /// service makes to the internet, so it can be switched off for a host that should make none.
    pub check_for_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            input_device_id: None,
            gain: 1.0,
            segment_seconds: 10,
            retention_hours: Some(24),
            auto_start: true,
            auto_start_delay_seconds: 0,
            frame_ms: 100,
            recording_sample_rate: None,
            recordings_dir: None,
            check_for_updates: true,
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
            retention_hours: parse_retention(
                pairs.get(KEY_RETENTION_HOURS),
                defaults.retention_hours,
            ),
            auto_start: parse_bool(pairs.get(KEY_AUTO_START), defaults.auto_start),
            check_for_updates: parse_bool(
                pairs.get(KEY_CHECK_FOR_UPDATES),
                defaults.check_for_updates,
            ),
            auto_start_delay_seconds: parse_u32(
                pairs.get(KEY_AUTO_START_DELAY_SECONDS),
                defaults.auto_start_delay_seconds,
            ),
            frame_ms: parse_u32(pairs.get(KEY_FRAME_MS), defaults.frame_ms),
            recording_sample_rate: pairs
                .get(KEY_RECORDING_SAMPLE_RATE)
                .and_then(|value| value.trim().parse::<u32>().ok())
                .map(nearest_supported_rate),
            recordings_dir: pairs
                .get(KEY_RECORDINGS_DIR)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
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
                // Forever is stored as an empty value rather than a sentinel number, so nobody has to
                // remember that some particular hour count secretly means something else.
                self.retention_hours
                    .map(|hours| hours.to_string())
                    .unwrap_or_default(),
            ),
            (KEY_AUTO_START.to_string(), self.auto_start.to_string()),
            (
                KEY_CHECK_FOR_UPDATES.to_string(),
                self.check_for_updates.to_string(),
            ),
            (
                KEY_AUTO_START_DELAY_SECONDS.to_string(),
                self.auto_start_delay_seconds.to_string(),
            ),
            (KEY_FRAME_MS.to_string(), self.frame_ms.to_string()),
            (
                KEY_RECORDING_SAMPLE_RATE.to_string(),
                self.recording_sample_rate
                    .map(|rate| rate.to_string())
                    .unwrap_or_default(),
            ),
            (
                KEY_RECORDINGS_DIR.to_string(),
                self.recordings_dir.clone().unwrap_or_default(),
            ),
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
            .map(|hours| hours.clamp(RETENTION_HOURS_RANGE.0, RETENTION_HOURS_RANGE.1));
        self.frame_ms = self.frame_ms.clamp(FRAME_MS_RANGE.0, FRAME_MS_RANGE.1);
        self.auto_start_delay_seconds = self.auto_start_delay_seconds.clamp(
            AUTO_START_DELAY_SECONDS_RANGE.0,
            AUTO_START_DELAY_SECONDS_RANGE.1,
        );
        self.recording_sample_rate = self.recording_sample_rate.map(nearest_supported_rate);
        self
    }

    /// The rate the recorder should produce, given what the device offers.
    ///
    /// Never above the device rate: asking for 48 kHz from a 16 kHz interface would invent detail that
    /// was never captured while doubling the disk it takes to store it.
    pub fn effective_sample_rate(&self, device_rate: u32) -> u32 {
        match self.recording_sample_rate {
            Some(target) if device_rate > 0 => target.min(device_rate),
            Some(target) => target,
            None => device_rate,
        }
    }

    /// How long the janitor keeps material, in milliseconds, or `None` to keep it forever.
    pub fn retention_ms(&self) -> Option<i64> {
        self.retention_hours.map(|hours| hours as i64 * 3_600_000)
    }

    /// True when nothing is ever pruned.
    pub fn keeps_forever(&self) -> bool {
        self.retention_hours.is_none()
    }
}

/// A partial settings update.
///
/// Every field is optional so a caller can change one preference without having to send back values it
/// does not care about, and without risking a lost update when two clients edit different fields.
#[derive(Debug, Clone, Default)]
pub struct SettingsPatch {
    /// `Some(None)` clears the stored device and returns to the system default, while `None` leaves it
    /// untouched. The nesting is deliberate: those are genuinely different requests.
    pub input_device_id: Option<Option<String>>,
    pub gain: Option<f32>,
    pub segment_seconds: Option<u32>,
    /// `Some(None)` switches to keeping forever, `None` leaves the current window alone.
    pub retention_hours: Option<Option<u32>>,
    pub auto_start: Option<bool>,
    pub auto_start_delay_seconds: Option<u32>,
    pub frame_ms: Option<u32>,
    /// `Some(None)` returns to the device's own rate.
    pub recording_sample_rate: Option<Option<u32>>,
    /// `Some(None)` returns to the default location under the data directory.
    pub recordings_dir: Option<Option<String>>,
    pub check_for_updates: Option<bool>,
}

impl SettingsPatch {
    pub fn is_empty(&self) -> bool {
        self.input_device_id.is_none()
            && self.gain.is_none()
            && self.segment_seconds.is_none()
            && self.retention_hours.is_none()
            && self.auto_start.is_none()
            && self.auto_start_delay_seconds.is_none()
            && self.frame_ms.is_none()
            && self.recording_sample_rate.is_none()
            && self.recordings_dir.is_none()
            && self.check_for_updates.is_none()
    }

    /// Apply the patch to `base` and return the clamped result.
    pub fn apply_to(&self, base: &Settings) -> Settings {
        let mut updated = base.clone();

        if let Some(device_id) = &self.input_device_id {
            updated.input_device_id = device_id
                .as_ref()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
        }
        if let Some(gain) = self.gain {
            updated.gain = gain;
        }
        if let Some(segment_seconds) = self.segment_seconds {
            updated.segment_seconds = segment_seconds;
        }
        if let Some(retention_hours) = self.retention_hours {
            updated.retention_hours = retention_hours;
        }
        if let Some(auto_start) = self.auto_start {
            updated.auto_start = auto_start;
        }
        if let Some(check_for_updates) = self.check_for_updates {
            updated.check_for_updates = check_for_updates;
        }
        if let Some(auto_start_delay_seconds) = self.auto_start_delay_seconds {
            updated.auto_start_delay_seconds = auto_start_delay_seconds;
        }
        if let Some(frame_ms) = self.frame_ms {
            updated.frame_ms = frame_ms;
        }
        if let Some(recording_sample_rate) = self.recording_sample_rate {
            updated.recording_sample_rate = recording_sample_rate;
        }
        if let Some(recordings_dir) = &self.recordings_dir {
            updated.recordings_dir = recordings_dir
                .as_ref()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
        }

        updated.clamped()
    }

    /// True when the patch changes something that only takes effect on a fresh capture stream.
    pub fn requires_capture_restart(&self, base: &Settings) -> bool {
        let updated = self.apply_to(base);
        updated.input_device_id != base.input_device_id || updated.frame_ms != base.frame_ms
    }
}

fn parse_f32(raw: Option<&String>, fallback: f32) -> f32 {
    raw.and_then(|value| value.trim().parse::<f32>().ok())
        .unwrap_or(fallback)
}

/// Parse the retention value, where a stored empty string means keep forever.
///
/// A key that is absent entirely is a different case from one stored empty: absent means the setting was
/// never written and should fall back to the default, while empty is a deliberate choice of forever.
/// Snap an arbitrary rate onto the closest supported one, so a hand edited database or an old client
/// cannot put the recorder on a rate the UI has no way to display or undo.
fn nearest_supported_rate(rate: u32) -> u32 {
    SUPPORTED_SAMPLE_RATES
        .into_iter()
        .min_by_key(|supported| supported.abs_diff(rate))
        .unwrap_or(48_000)
}

fn parse_retention(raw: Option<&String>, fallback: Option<u32>) -> Option<u32> {
    match raw {
        None => fallback,
        Some(value) if value.trim().is_empty() => None,
        Some(value) => value.trim().parse::<u32>().ok().or(fallback),
    }
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
            retention_hours: Some(48),
            auto_start: false,
            auto_start_delay_seconds: 15,
            frame_ms: 40,
            recording_sample_rate: Some(16_000),
            recordings_dir: Some("/mnt/audio".to_string()),
            check_for_updates: false,
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
            (
                KEY_AUTO_START_DELAY_SECONDS.to_string(),
                "86400".to_string(),
            ),
        ]);
        let settings = Settings::from_pairs(&pairs);
        assert_eq!(settings.gain, GAIN_RANGE.1);
        assert_eq!(settings.segment_seconds, SEGMENT_SECONDS_RANGE.0);
        assert_eq!(settings.frame_ms, FRAME_MS_RANGE.1);
        assert_eq!(
            settings.auto_start_delay_seconds,
            AUTO_START_DELAY_SECONDS_RANGE.1
        );
    }

    #[test]
    fn an_existing_install_keeps_starting_immediately() {
        // A database written before the delay existed has no row for it, and must behave as it always
        // did rather than quietly gaining a pause at boot.
        let pairs = HashMap::from([(KEY_AUTO_START.to_string(), "true".to_string())]);
        assert_eq!(Settings::from_pairs(&pairs).auto_start_delay_seconds, 0);
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
    fn the_sample_rate_ladder_is_the_bit_rate_ladder() {
        // Uncompressed 16 bit mono, so the two are the same number scaled by sixteen.
        assert_eq!(bit_rate_for(48_000), 768_000);
        assert_eq!(bit_rate_for(16_000), 256_000);
        assert_eq!(bit_rate_for(8_000), 128_000);
    }

    #[test]
    fn an_unsupported_rate_snaps_to_the_nearest_offered_one() {
        let pairs = HashMap::from([(KEY_RECORDING_SAMPLE_RATE.to_string(), "44100".to_string())]);
        assert_eq!(
            Settings::from_pairs(&pairs).recording_sample_rate,
            Some(48_000)
        );

        let low = HashMap::from([(KEY_RECORDING_SAMPLE_RATE.to_string(), "9000".to_string())]);
        assert_eq!(
            Settings::from_pairs(&low).recording_sample_rate,
            Some(8_000)
        );
    }

    #[test]
    fn an_empty_rate_means_follow_the_device() {
        let pairs = HashMap::from([(KEY_RECORDING_SAMPLE_RATE.to_string(), String::new())]);
        assert_eq!(Settings::from_pairs(&pairs).recording_sample_rate, None);
    }

    #[test]
    fn the_effective_rate_never_exceeds_the_device() {
        let native = Settings::default();
        assert_eq!(native.effective_sample_rate(44_100), 44_100);

        let asked_for_more = Settings {
            recording_sample_rate: Some(48_000),
            ..Settings::default()
        };
        // Upsampling would invent detail and double the disk, so the device rate wins.
        assert_eq!(asked_for_more.effective_sample_rate(16_000), 16_000);

        let asked_for_less = Settings {
            recording_sample_rate: Some(16_000),
            ..Settings::default()
        };
        assert_eq!(asked_for_less.effective_sample_rate(48_000), 16_000);
    }

    #[test]
    fn retention_converts_to_milliseconds() {
        let settings = Settings {
            retention_hours: Some(2),
            ..Settings::default()
        };
        assert_eq!(settings.retention_ms(), Some(7_200_000));
        assert!(!settings.keeps_forever());

        let forever = Settings {
            retention_hours: None,
            ..Settings::default()
        };
        assert_eq!(forever.retention_ms(), None);
        assert!(forever.keeps_forever());
    }
    #[test]
    fn patch_only_touches_the_fields_it_sets() {
        let base = Settings::default();
        let patch = SettingsPatch {
            gain: Some(2.0),
            ..SettingsPatch::default()
        };
        let updated = patch.apply_to(&base);
        assert_eq!(updated.gain, 2.0);
        assert_eq!(updated.retention_hours, base.retention_hours);
    }

    #[test]
    fn patch_can_clear_the_device_or_leave_it_alone() {
        let base = Settings {
            input_device_id: Some("mic".to_string()),
            ..Settings::default()
        };

        let untouched = SettingsPatch::default().apply_to(&base);
        assert_eq!(untouched.input_device_id, Some("mic".to_string()));

        let cleared = SettingsPatch {
            input_device_id: Some(None),
            ..SettingsPatch::default()
        }
        .apply_to(&base);
        assert_eq!(cleared.input_device_id, None);
    }

    #[test]
    fn patch_clamps_what_it_applies() {
        let patch = SettingsPatch {
            gain: Some(-3.0),
            ..SettingsPatch::default()
        };
        assert_eq!(patch.apply_to(&Settings::default()).gain, GAIN_RANGE.0);
    }

    #[test]
    fn only_device_and_frame_size_force_a_capture_restart() {
        let base = Settings::default();

        let gain_only = SettingsPatch {
            gain: Some(2.0),
            ..SettingsPatch::default()
        };
        assert!(!gain_only.requires_capture_restart(&base));

        let device_change = SettingsPatch {
            input_device_id: Some(Some("other".to_string())),
            ..SettingsPatch::default()
        };
        assert!(device_change.requires_capture_restart(&base));

        let frame_change = SettingsPatch {
            frame_ms: Some(40),
            ..SettingsPatch::default()
        };
        assert!(frame_change.requires_capture_restart(&base));
    }

    #[test]
    fn an_empty_patch_is_detected() {
        assert!(SettingsPatch::default().is_empty());
        assert!(!SettingsPatch {
            auto_start: Some(false),
            ..SettingsPatch::default()
        }
        .is_empty());
    }
}
