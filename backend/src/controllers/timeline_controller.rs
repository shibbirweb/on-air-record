//! Timeline extent and waveform peaks.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::Json;

use crate::app::AppState;
use crate::dto::{
    NextSoundQuery, NextSoundResponse, PeaksQuery, PeaksResponse, RecordingDaysResponse,
    SoundsQuery, SoundsResponse, TimelineRangeResponse,
};
use crate::error::{AppError, AppResult};

/// Largest window a single peaks request may cover.
///
/// A month of envelope is 26 million buckets to walk, and no timeline shows that at once, so an unbounded
/// request is a mistake rather than a use case.
const MAX_WINDOW_MS: i64 = 32 * 24 * 3_600_000;

/// The width of a requested window. Saturating, because both ends come straight from the query string:
/// a plain subtraction of extreme values would panic in a debug build and, in a release build, wrap to a
/// negative width that slips under [`MAX_WINDOW_MS`] into a scan of every segment.
fn window_ms(from_ms: i64, to_ms: i64) -> i64 {
    to_ms.saturating_sub(from_ms)
}

/// `GET /api/timeline/range`
pub async fn range(State(state): State<Arc<AppState>>) -> AppResult<Json<TimelineRangeResponse>> {
    Ok(Json(state.timeline.range()?.into()))
}

/// `GET /api/timeline/days`
///
/// The list the day picker is built from: which calendar days hold audio, and where in each day it sits.
pub async fn days(State(state): State<Arc<AppState>>) -> AppResult<Json<RecordingDaysResponse>> {
    let days = state.timeline.days()?.into_iter().map(Into::into).collect();

    Ok(Json(RecordingDaysResponse { days }))
}

/// `GET /api/timeline/peaks`
pub async fn peaks(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PeaksQuery>,
) -> AppResult<Json<PeaksResponse>> {
    if query.to_ms <= query.from_ms {
        return Err(AppError::bad_request("toMs must be greater than fromMs"));
    }
    if window_ms(query.from_ms, query.to_ms) > MAX_WINDOW_MS {
        return Err(AppError::bad_request(
            "the requested window is too wide, ask for at most 32 days at a time",
        ));
    }

    let view = state
        .timeline
        .peaks(query.from_ms, query.to_ms, query.buckets)?;

    Ok(Json(view.into()))
}

/// `GET /api/timeline/sounds`
///
/// The moments something was heard in a window, found with the current sensitivity setting.
pub async fn sounds(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SoundsQuery>,
) -> AppResult<Json<SoundsResponse>> {
    if window_ms(query.from_ms, query.to_ms) > MAX_WINDOW_MS {
        return Err(AppError::bad_request(
            "the requested window is too wide, ask for at most 32 days at a time",
        ));
    }
    let sensitivity = state.settings.current().sound_sensitivity;
    let sounds = state
        .timeline
        .sounds(query.from_ms, query.to_ms, sensitivity)?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(SoundsResponse {
        from_ms: query.from_ms,
        to_ms: query.to_ms,
        sensitivity,
        sounds,
    }))
}

/// `GET /api/timeline/sounds/next`
///
/// The sound to jump to from where playback is, forward or back.
pub async fn next_sound(
    State(state): State<Arc<AppState>>,
    Query(query): Query<NextSoundQuery>,
) -> AppResult<Json<NextSoundResponse>> {
    let sensitivity = state.settings.current().sound_sensitivity;
    let sound = state
        .timeline
        .next_sound(query.from_ms, query.direction.into(), sensitivity)?
        .map(Into::into);
    Ok(Json(NextSoundResponse { sound }))
}
