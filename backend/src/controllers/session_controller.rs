//! Recording sessions and storage usage.

use std::sync::Arc;

use axum::extract::State;
use axum::Json;

use crate::app::AppState;
use crate::dto::{SessionListResponse, StorageResponse};
use crate::error::AppResult;

/// Sessions returned by the list endpoint. Deep history is not useful in the UI, and the timeline is the
/// right tool for finding old material.
const SESSION_LIMIT: i64 = 100;

/// `GET /api/sessions`
pub async fn list(State(state): State<Arc<AppState>>) -> AppResult<Json<SessionListResponse>> {
    let sessions = state
        .sessions
        .list_summaries(SESSION_LIMIT)?
        .into_iter()
        .map(Into::into)
        .collect();

    Ok(Json(SessionListResponse { sessions }))
}

/// `GET /api/storage`
pub async fn storage(State(state): State<Arc<AppState>>) -> AppResult<Json<StorageResponse>> {
    let stats = state.segments.stats()?;
    let settings = state.settings.current();
    let data_dir = state.config.data_dir.to_string_lossy().to_string();

    Ok(Json(StorageResponse::new(stats, &settings, data_dir)))
}
