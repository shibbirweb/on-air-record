//! Lifecycle of the capture engine as seen from outside.

use serde::Serialize;

/// Where the capture engine currently is. `Starting` exists because opening a device can block for a
/// noticeable moment on some hosts, and the UI should show that rather than a stale idle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureState {
    Idle,
    Starting,
    Recording,
    Error,
}

impl CaptureState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Recording)
    }
}

/// Immutable view of the capture engine, taken under the engine lock and then released, so callers never
/// hold a lock while serialising a response.
#[derive(Debug, Clone)]
pub struct CaptureSnapshot {
    pub state: CaptureState,
    pub session_id: Option<i64>,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    /// The rate being recorded, after the recording rate setting: what lands in segments and on the wire.
    pub sample_rate: u32,
    /// The rate the microphone itself runs at, which the choice of recording rates is offered up to. Not
    /// `sample_rate`: recording at 8 kHz from a 48 kHz device must still offer everything up to 48 kHz.
    pub device_sample_rate: u32,
    pub channels: u16,
    pub frame_ms: u32,
    pub started_at_ms: Option<i64>,
    pub dropped_frames: u64,
    pub error: Option<String>,
}

impl Default for CaptureSnapshot {
    fn default() -> Self {
        Self {
            state: CaptureState::Idle,
            session_id: None,
            device_id: None,
            device_name: None,
            sample_rate: 0,
            device_sample_rate: 0,
            channels: 0,
            frame_ms: 0,
            started_at_ms: None,
            dropped_frames: 0,
            error: None,
        }
    }
}

/// Latest input meter reading.
#[derive(Debug, Clone, Copy, Default)]
pub struct LevelSnapshot {
    pub rms: f32,
    pub peak: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_and_recording_are_active_and_idle_and_error_are_not() {
        assert!(CaptureState::Starting.is_active());
        assert!(CaptureState::Recording.is_active());
        assert!(!CaptureState::Idle.is_active());
        assert!(!CaptureState::Error.is_active());
    }

    #[test]
    fn states_travel_as_lowercase_words() {
        for (state, word) in [
            (CaptureState::Idle, "idle"),
            (CaptureState::Starting, "starting"),
            (CaptureState::Recording, "recording"),
            (CaptureState::Error, "error"),
        ] {
            assert_eq!(serde_json::to_value(state).expect("serialise"), word);
        }
    }

    #[test]
    fn a_fresh_snapshot_is_an_idle_recorder_with_nothing_open() {
        let snapshot = CaptureSnapshot::default();
        assert_eq!(snapshot.state, CaptureState::Idle);
        assert_eq!(snapshot.session_id, None);
        assert_eq!(snapshot.device_id, None);
        assert_eq!(snapshot.started_at_ms, None);
        assert_eq!(snapshot.dropped_frames, 0);
        assert_eq!(snapshot.error, None);
        let levels = LevelSnapshot::default();
        assert_eq!((levels.rms, levels.peak), (0.0, 0.0));
    }
}
