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
    /// `null` when recordings are kept forever.
    pub retention_hours: Option<u32>,
    pub data_dir: String,
    /// Where segments are being written right now.
    pub recordings_dir: String,
    /// Bytes one hour of audio occupies at the format currently in use. Raw PCM, so this is exact rather
    /// than an average over a compressor.
    pub bytes_per_hour: i64,
    /// What the disk would hold once the retention window is full, or `null` when keeping forever, in
    /// which case there is no ceiling to project.
    pub projected_max_bytes: Option<i64>,
}

/// Everything the storage endpoint needs that does not come from the segment index.
pub struct StorageContext {
    pub data_dir: String,
    pub recordings_dir: String,
    pub sample_rate: u32,
    pub channels: u16,
}

impl StorageResponse {
    pub fn new(stats: SegmentStorageStats, settings: &Settings, context: StorageContext) -> Self {
        let bytes_per_hour = bytes_per_hour(context.sample_rate, context.channels);

        Self {
            bytes: stats.bytes,
            segment_count: stats.segment_count,
            oldest_ms: stats.oldest_ms,
            newest_ms: stats.newest_ms,
            retention_hours: settings.retention_hours,
            data_dir: context.data_dir,
            recordings_dir: context.recordings_dir,
            bytes_per_hour,
            projected_max_bytes: settings
                .retention_hours
                .map(|hours| bytes_per_hour * hours as i64),
        }
    }
}

/// Bytes an hour of raw PCM occupies at the given format.
///
/// Two bytes per sample per channel, and no compression, so the figure is exact rather than an estimate.
/// A zero sample rate means capture has never run, in which case the shipping default is the honest guess.
pub fn bytes_per_hour(sample_rate: u32, channels: u16) -> i64 {
    let rate = if sample_rate == 0 {
        48_000
    } else {
        sample_rate
    };
    let channels = channels.max(1) as i64;
    rate as i64 * channels * 2 * 3_600
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_hour_of_mono_48k_is_about_330_mib() {
        let bytes = bytes_per_hour(48_000, 1);
        assert_eq!(bytes, 345_600_000);
        assert!((bytes as f64 / 1024.0 / 1024.0 - 329.6).abs() < 0.5);
    }

    #[test]
    fn stereo_and_higher_rates_scale_the_figure() {
        assert_eq!(bytes_per_hour(48_000, 2), bytes_per_hour(48_000, 1) * 2);
        assert_eq!(bytes_per_hour(96_000, 1), bytes_per_hour(48_000, 1) * 2);
    }

    #[test]
    fn an_unstarted_capture_falls_back_to_the_shipping_default() {
        assert_eq!(bytes_per_hour(0, 0), bytes_per_hour(48_000, 1));
    }
}
