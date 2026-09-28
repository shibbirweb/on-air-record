//! Start and stop the recorder.

use std::sync::Arc;

use axum::extract::State;
use axum::{Extension, Json};

use crate::app::AppState;
use crate::controllers::auth_context::{Caller, RequestOrigin};
use crate::dto::StatusResponse;
use crate::error::AppResult;
use crate::models::activity::ActivityEvent;

/// `POST /api/capture/start`
pub async fn start(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    RequestOrigin(origin): RequestOrigin,
) -> AppResult<Json<StatusResponse>> {
    state.capture.start().await?;
    state
        .activity
        .record(caller.actor(), &origin, ActivityEvent::CaptureStarted);
    Ok(Json(StatusResponse::from_state(&state)))
}

/// `POST /api/capture/stop`
pub async fn stop(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    RequestOrigin(origin): RequestOrigin,
) -> AppResult<Json<StatusResponse>> {
    state.capture.stop().await?;
    state
        .activity
        .record(caller.actor(), &origin, ActivityEvent::CaptureStopped);
    Ok(Json(StatusResponse::from_state(&state)))
}
