//! Timeline payloads.

use serde::{Deserialize, Serialize};

use crate::models::TimeRange;
use crate::services::timeline_service::{PeaksView, DEFAULT_BUCKETS};
use crate::services::{RecordingDay, TimelineRange};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageDto {
    pub start_ms: i64,
    pub end_ms: i64,
}

impl From<TimeRange> for CoverageDto {
    fn from(range: TimeRange) -> Self {
        Self {
            start_ms: range.start_ms,
            end_ms: range.end_ms,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineRangeResponse {
    pub earliest_ms: Option<i64>,
    pub latest_ms: Option<i64>,
    pub live_edge_ms: Option<i64>,
    pub server_time_ms: i64,
    pub coverage: Vec<CoverageDto>,
}

impl From<TimelineRange> for TimelineRangeResponse {
    fn from(range: TimelineRange) -> Self {
        Self {
            earliest_ms: range.earliest_ms,
            latest_ms: range.latest_ms,
            live_edge_ms: range.live_edge_ms,
            server_time_ms: crate::util::time::now_ms(),
            coverage: range.coverage.into_iter().map(Into::into).collect(),
        }
    }
}

/// Query string of the peaks endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeaksQuery {
    pub from_ms: i64,
    pub to_ms: i64,
    #[serde(default = "default_buckets")]
    pub buckets: usize,
}

fn default_buckets() -> usize {
    DEFAULT_BUCKETS
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeaksResponse {
    pub from_ms: i64,
    pub to_ms: i64,
    pub bucket_ms: i64,
    pub peaks: Vec<u8>,
}

impl From<PeaksView> for PeaksResponse {
    fn from(view: PeaksView) -> Self {
        Self {
            from_ms: view.from_ms,
            to_ms: view.to_ms,
            bucket_ms: view.bucket_ms,
            peaks: view.values,
        }
    }
}

/// One day that holds recordings, as offered by the day picker.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDayDto {
    /// Local calendar day, `YYYY-MM-DD`.
    pub day: String,
    /// First and last moment recorded on that day.
    pub start_ms: i64,
    pub end_ms: i64,
    /// Local midnight bounds, so the timeline can frame the whole day.
    pub day_start_ms: i64,
    pub day_end_ms: i64,
    pub segment_count: i64,
    pub bytes: i64,
    /// Audio actually captured, which is less than the span whenever the recorder was stopped part way.
    pub recorded_ms: i64,
}

impl From<RecordingDay> for RecordingDayDto {
    fn from(day: RecordingDay) -> Self {
        Self {
            day: day.summary.day,
            start_ms: day.summary.start_ms,
            end_ms: day.summary.end_ms,
            day_start_ms: day.day_start_ms,
            day_end_ms: day.day_end_ms,
            segment_count: day.summary.segment_count,
            bytes: day.summary.bytes,
            recorded_ms: day.summary.recorded_ms,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDaysResponse {
    pub days: Vec<RecordingDayDto>,
}

/// Query string of both export endpoints.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportQuery {
    pub from_ms: i64,
    pub to_ms: i64,
}

/// What an export would produce, so the UI can show it before committing to a download.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPlanResponse {
    pub from_ms: i64,
    pub to_ms: i64,
    pub duration_ms: i64,
    pub sample_rate: u32,
    pub channels: u16,
    pub total_bytes: i64,
    /// True when the range spans more than one recording rate and will be exported at the lowest.
    pub mixed_rates: bool,
}

impl From<crate::services::ExportPlan> for ExportPlanResponse {
    fn from(plan: crate::services::ExportPlan) -> Self {
        Self {
            from_ms: plan.range.start_ms,
            to_ms: plan.range.end_ms,
            duration_ms: plan.duration_ms(),
            sample_rate: plan.sample_rate,
            channels: plan.channels,
            total_bytes: plan.total_bytes as i64,
            mixed_rates: plan.mixed_rates,
        }
    }
}
