//! The Prometheus endpoint and the token a scraper uses to reach it.

use std::sync::Arc;

use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use axum::Json;

use crate::app::AppState;
use crate::dto::{MetricsSnapshot, MetricsTokenResponse, NewMetricsTokenResponse};
use crate::error::AppResult;
use crate::models::metrics;

/// `GET /api/metrics`
pub async fn scrape(State(state): State<Arc<AppState>>) -> AppResult<impl IntoResponse> {
    let body = metrics::render(&MetricsSnapshot::from_state(&state)?.families());
    Ok(([(CONTENT_TYPE, metrics::CONTENT_TYPE)], body))
}

/// `GET /api/metrics/token`
pub async fn token_status(
    State(state): State<Arc<AppState>>,
) -> AppResult<Json<MetricsTokenResponse>> {
    Ok(Json(MetricsTokenResponse {
        created_at_ms: state.auth.metrics_token_created_at()?,
    }))
}

/// `POST /api/metrics/token`, which also rotates: the old token stops working at once.
pub async fn create_token(
    State(state): State<Arc<AppState>>,
) -> AppResult<Json<NewMetricsTokenResponse>> {
    let (token, created_at_ms) = state.auth.create_metrics_token()?;
    Ok(Json(NewMetricsTokenResponse {
        token,
        created_at_ms,
    }))
}

/// `DELETE /api/metrics/token`
pub async fn revoke_token(
    State(state): State<Arc<AppState>>,
) -> AppResult<Json<MetricsTokenResponse>> {
    state.auth.revoke_metrics_token()?;
    Ok(Json(MetricsTokenResponse {
        created_at_ms: None,
    }))
}
