//! Service and capture status payloads.

use serde::Serialize;

use crate::app::AppState;
use crate::models::{CaptureSnapshot, CaptureState, LevelSnapshot};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub uptime_ms: i64,
}

impl HealthResponse {
    pub fn from_state(state: &AppState) -> Self {
        Self {
            status: "ok",
            version: env!("CARGO_PKG_VERSION"),
            uptime_ms: state.uptime_ms(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDto {
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

impl From<CaptureSnapshot> for CaptureDto {
    fn from(snapshot: CaptureSnapshot) -> Self {
        Self {
            state: snapshot.state,
            session_id: snapshot.session_id,
            device_id: snapshot.device_id,
            device_name: snapshot.device_name,
            sample_rate: snapshot.sample_rate,
            channels: snapshot.channels,
            frame_ms: snapshot.frame_ms,
            started_at_ms: snapshot.started_at_ms,
            dropped_frames: snapshot.dropped_frames,
            error: snapshot.error,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelsDto {
    pub rms: f32,
    pub peak: f32,
}

impl From<LevelSnapshot> for LevelsDto {
    fn from(levels: LevelSnapshot) -> Self {
        Self {
            rms: levels.rms,
            peak: levels.peak,
        }
    }
}

/// The single call the UI polls to know the state of the world.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub capture: CaptureDto,
    pub levels: LevelsDto,
    pub listeners: usize,
    pub server_time_ms: i64,
    pub live_edge_ms: Option<i64>,
}

impl StatusResponse {
    pub fn from_state(state: &AppState) -> Self {
        Self {
            capture: state.capture.snapshot().into(),
            levels: state.hub.levels().into(),
            listeners: state.listeners.count(),
            server_time_ms: crate::util::time::now_ms(),
            live_edge_ms: state.live_edge_ms(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CaptureState;

    #[test]
    fn a_capture_snapshot_carries_every_field_onto_the_wire() {
        let dto: CaptureDto = CaptureSnapshot {
            state: CaptureState::Recording,
            session_id: Some(7),
            device_id: Some("usb".to_string()),
            device_name: Some("USB microphone".to_string()),
            sample_rate: 48_000,
            channels: 1,
            frame_ms: 100,
            started_at_ms: Some(1_000),
            dropped_frames: 3,
            error: None,
        }
        .into();
        let json = serde_json::to_value(dto).expect("serialise");
        assert_eq!(json["state"], "recording");
        assert_eq!(json["sessionId"], 7);
        assert_eq!(json["deviceId"], "usb");
        assert_eq!(json["deviceName"], "USB microphone");
        assert_eq!(json["sampleRate"], 48_000);
        assert_eq!(json["channels"], 1);
        assert_eq!(json["frameMs"], 100);
        assert_eq!(json["startedAtMs"], 1_000);
        assert_eq!(json["droppedFrames"], 3);
        assert_eq!(json["error"], serde_json::Value::Null);
    }

    #[test]
    fn a_failed_capture_says_why() {
        let dto: CaptureDto = CaptureSnapshot {
            state: CaptureState::Error,
            error: Some("audio device error: unplugged".to_string()),
            ..CaptureSnapshot::default()
        }
        .into();
        let json = serde_json::to_value(dto).expect("serialise");
        assert_eq!(json["state"], "error");
        assert_eq!(json["error"], "audio device error: unplugged");
    }

    #[test]
    fn levels_carry_the_meter_reading() {
        let dto: LevelsDto = LevelSnapshot {
            rms: 0.25,
            peak: 0.5,
        }
        .into();
        let json = serde_json::to_value(dto).expect("serialise");
        assert_eq!(json["rms"], 0.25);
        assert_eq!(json["peak"], 0.5);
    }
}
