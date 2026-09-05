//! Read and update runtime preferences.

use std::sync::Arc;

use axum::extract::State;
use axum::Json;

use crate::app::AppState;
use crate::dto::{SettingsDto, SettingsPatchRequest};
use crate::error::AppResult;
use crate::models::SettingsPatch;

/// `GET /api/settings`
pub async fn show(State(state): State<Arc<AppState>>) -> AppResult<Json<SettingsDto>> {
    Ok(Json(state.settings.current().into()))
}

/// `PATCH /api/settings`
///
/// Some preferences only take effect on a fresh stream, so a change to those restarts capture when it is
/// running. Doing it here rather than asking the client to call start and stop keeps the two from drifting
/// out of step.
pub async fn update(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SettingsPatchRequest>,
) -> AppResult<Json<SettingsDto>> {
    let patch: SettingsPatch = request.into();
    let before = state.settings.current();
    let needs_restart = patch.requires_capture_restart(&before);

    let updated = state.settings.update(&patch)?;

    if needs_restart && state.capture.is_active() {
        state.capture.restart().await?;
    }

    Ok(Json(updated.into()))
}
