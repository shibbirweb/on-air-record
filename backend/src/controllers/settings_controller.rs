//! Read and update runtime preferences.

use std::sync::Arc;

use axum::extract::State;
use axum::{Extension, Json};

use crate::app::AppState;
use crate::controllers::auth_context::{Caller, RequestOrigin};
use crate::dto::{SettingsDto, SettingsPatchRequest, TestDirectoryRequest, TestDirectoryResponse};
use crate::error::AppResult;
use crate::models::activity::{setting_changes, ActivityEvent};
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
    Extension(caller): Extension<Caller>,
    RequestOrigin(origin): RequestOrigin,
    Json(request): Json<SettingsPatchRequest>,
) -> AppResult<Json<SettingsDto>> {
    let patch: SettingsPatch = request.into();
    let before = state.settings.current();
    let needs_restart = patch.requires_capture_restart(&before);

    let updated = state.settings.update(&patch)?;

    // Compared as the page names them, so the log reads like the settings page. A save that changed
    // nothing, such as the same value sent again, is not worth an entry.
    let as_seen = |settings: &Settings| {
        serde_json::to_value(SettingsDto::new(settings.clone(), &state.config)).unwrap_or_default()
    };
    let changes = setting_changes(&as_seen(&before), &as_seen(&updated));
    if !changes.is_empty() {
        state.activity.record(
            caller.actor(),
            &origin,
            ActivityEvent::SettingsChanged { changes },
        );
    }

    if needs_restart && state.capture.is_active() {
        state.capture.restart().await?;
    }

    Ok(Json(SettingsDto::new(updated, &state.config)))
}

/// `POST /api/settings/reset`
///
/// Restores the shipped defaults, leaving the chosen input device alone. Like `PATCH`, it restarts
/// capture when a preference that only applies to a fresh stream has actually changed.
pub async fn reset(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    RequestOrigin(origin): RequestOrigin,
) -> AppResult<Json<SettingsDto>> {
    let before = state.settings.current();
    let updated = state.settings.reset()?;
    state
        .activity
        .record(caller.actor(), &origin, ActivityEvent::SettingsReset);

    if updated.frame_ms != before.frame_ms && state.capture.is_active() {
        state.capture.restart().await?;
    }

    Ok(Json(SettingsDto::new(updated, &state.config)))
}

/// `POST /api/settings/test-recordings-dir`
///
/// Try a directory without saving it, so an unusable path is caught while it can still be corrected
/// rather than at the moment of saving. Changes nothing on disk, including for a path that does not
/// exist yet.
pub async fn test_recordings_dir(
    State(state): State<Arc<AppState>>,
    Json(request): Json<TestDirectoryRequest>,
) -> AppResult<Json<TestDirectoryResponse>> {
    let probe = state.settings.probe_recordings_dir(request.path.as_deref());
    Ok(Json(probe.into()))
}
