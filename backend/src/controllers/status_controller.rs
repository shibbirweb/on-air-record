//! Health and status endpoints.

use std::sync::Arc;

use axum::extract::State;
use axum::Json;

use crate::app::AppState;
use crate::dto::{HealthResponse, StatusResponse};
use crate::error::AppResult;

/// `GET /api/health`
pub async fn health(State(state): State<Arc<AppState>>) -> AppResult<Json<HealthResponse>> {
    Ok(Json(HealthResponse::from_state(&state)))
}

/// `GET /api/status`
pub async fn status(State(state): State<Arc<AppState>>) -> AppResult<Json<StatusResponse>> {
    Ok(Json(StatusResponse::from_state(&state)))
}
