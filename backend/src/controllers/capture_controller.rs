//! Start and stop the recorder.

use std::sync::Arc;

use axum::extract::State;
use axum::Json;

use crate::app::AppState;
use crate::dto::StatusResponse;
use crate::error::AppResult;

/// `POST /api/capture/start`
pub async fn start(State(state): State<Arc<AppState>>) -> AppResult<Json<StatusResponse>> {
    state.capture.start().await?;
    Ok(Json(StatusResponse::from_state(&state)))
}

/// `POST /api/capture/stop`
pub async fn stop(State(state): State<Arc<AppState>>) -> AppResult<Json<StatusResponse>> {
    state.capture.stop().await?;
    Ok(Json(StatusResponse::from_state(&state)))
}
