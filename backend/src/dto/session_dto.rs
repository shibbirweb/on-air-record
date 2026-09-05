//! Session and storage payloads.

use serde::Serialize;

use crate::models::Settings;
use crate::repositories::{SegmentStorageStats, SessionSummary};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub id: i64,
    pub device_id: String,
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub segment_count: i64,
    pub bytes: i64,
}

impl From<SessionSummary> for SessionDto {
    fn from(summary: SessionSummary) -> Self {
        Self {
            id: summary.session.id,
            device_id: summary.session.device_id,
            device_name: summary.session.device_name,
            sample_rate: summary.session.sample_rate,
            channels: summary.session.channels,
            started_at_ms: summary.session.started_at_ms,
            ended_at_ms: summary.session.ended_at_ms,
            segment_count: summary.segment_count,
            bytes: summary.bytes,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionListResponse {
    pub sessions: Vec<SessionDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageResponse {
    pub bytes: i64,
    pub segment_count: i64,
    pub oldest_ms: Option<i64>,
    pub newest_ms: Option<i64>,
    pub retention_hours: u32,
    pub data_dir: String,
}

impl StorageResponse {
    pub fn new(stats: SegmentStorageStats, settings: &Settings, data_dir: String) -> Self {
        Self {
            bytes: stats.bytes,
            segment_count: stats.segment_count,
            oldest_ms: stats.oldest_ms,
            newest_ms: stats.newest_ms,
            retention_hours: settings.retention_hours,
            data_dir,
        }
    }
}
