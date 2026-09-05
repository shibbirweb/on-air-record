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
    pub sample_rate: u32,
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
