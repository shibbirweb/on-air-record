//! Timeline payloads.

use serde::{Deserialize, Serialize};

use crate::models::TimeRange;
use crate::services::timeline_service::{PeaksView, DEFAULT_BUCKETS};
use crate::services::TimelineRange;

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
