//! Whether a newer release exists. Admins only: they are the ones who can act on it.

use std::sync::Arc;

use axum::extract::State;
use axum::Json;

use crate::app::AppState;
use crate::dto::UpdateStatusResponse;
use crate::error::AppResult;

/// `GET /api/updates`
///
/// The answer from the last check, which the service makes by itself every few hours. Never waits on
/// the network.
pub async fn status(State(state): State<Arc<AppState>>) -> AppResult<Json<UpdateStatusResponse>> {
    Ok(Json(state.updates.status().into()))
}

/// `POST /api/updates/check`
///
/// Ask GitHub now, for the Check now button. Works with automatic checks switched off, since pressing it
/// is an explicit request. Answers with the new status whether or not GitHub could be reached.
pub async fn check(State(state): State<Arc<AppState>>) -> AppResult<Json<UpdateStatusResponse>> {
    Ok(Json(state.updates.check_now().await.into()))
}
