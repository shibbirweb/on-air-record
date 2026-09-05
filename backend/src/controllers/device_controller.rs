//! Input device listing and selection.

use std::sync::Arc;

use axum::extract::State;
use axum::Json;

use crate::app::AppState;
use crate::dto::{DeviceDto, DeviceListResponse, SelectDeviceRequest, StatusResponse};
use crate::error::AppResult;

/// `GET /api/devices`
pub async fn list(State(state): State<Arc<AppState>>) -> AppResult<Json<DeviceListResponse>> {
    let selected = state.devices.selected_id();
    let devices = state
        .devices
        .list()
        .await?
        .into_iter()
        .map(|device| DeviceDto::from_model(device, selected.as_deref()))
        .collect();

    Ok(Json(DeviceListResponse { devices }))
}

/// `POST /api/devices/select`
pub async fn select(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SelectDeviceRequest>,
) -> AppResult<Json<StatusResponse>> {
    state.devices.select(request.device_id).await?;
    Ok(Json(StatusResponse::from_state(&state)))
}
