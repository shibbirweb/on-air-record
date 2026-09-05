//! Timeline extent and waveform peaks.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::Json;

use crate::app::AppState;
use crate::dto::{PeaksQuery, PeaksResponse, TimelineRangeResponse};
use crate::error::{AppError, AppResult};

/// Largest window a single peaks request may cover.
///
/// A month of envelope is 26 million buckets to walk, and no timeline shows that at once, so an unbounded
/// request is a mistake rather than a use case.
const MAX_WINDOW_MS: i64 = 32 * 24 * 3_600_000;

/// `GET /api/timeline/range`
pub async fn range(State(state): State<Arc<AppState>>) -> AppResult<Json<TimelineRangeResponse>> {
    Ok(Json(state.timeline.range()?.into()))
}

/// `GET /api/timeline/peaks`
pub async fn peaks(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PeaksQuery>,
) -> AppResult<Json<PeaksResponse>> {
    if query.to_ms <= query.from_ms {
        return Err(AppError::bad_request("toMs must be greater than fromMs"));
    }
    if query.to_ms - query.from_ms > MAX_WINDOW_MS {
        return Err(AppError::bad_request(
            "the requested window is too wide, ask for at most 32 days at a time",
        ));
    }

    let view = state
        .timeline
        .peaks(query.from_ms, query.to_ms, query.buckets)?;

    Ok(Json(view.into()))
}
