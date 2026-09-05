//! Read and update runtime preferences.

use std::sync::Arc;

use axum::extract::State;
use axum::Json;

use crate::app::AppState;
use crate::dto::{SettingsDto, SettingsPatchRequest};
use crate::error::AppResult;
use crate::models::{Settings, SettingsPatch};

/// `GET /api/settings`
pub async fn show(State(state): State<Arc<AppState>>) -> AppResult<Json<SettingsDto>> {
    Ok(Json(SettingsDto::new(
        state.settings.current(),
        &state.config,
    )))
}

/// `GET /api/settings/defaults`
///
/// The values a reset restores. Exposed so the UI can say precisely what a reset will change instead of
/// hardcoding a second copy of the defaults that drifts the first time one of them is retuned.
pub async fn defaults(State(state): State<Arc<AppState>>) -> AppResult<Json<SettingsDto>> {
    Ok(Json(SettingsDto::new(Settings::default(), &state.config)))
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

    Ok(Json(SettingsDto::new(updated, &state.config)))
}

/// `POST /api/settings/reset`
///
/// Restores the shipped defaults, leaving the chosen input device alone. Like `PATCH`, it restarts
/// capture when a preference that only applies to a fresh stream has actually changed.
pub async fn reset(State(state): State<Arc<AppState>>) -> AppResult<Json<SettingsDto>> {
    let before = state.settings.current();
    let updated = state.settings.reset()?;

    if updated.frame_ms != before.frame_ms && state.capture.is_active() {
        state.capture.restart().await?;
    }

    Ok(Json(SettingsDto::new(updated, &state.config)))
}
