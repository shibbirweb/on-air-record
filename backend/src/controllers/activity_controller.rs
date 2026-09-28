//! The activity log, for admins.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::Json;

use crate::app::AppState;
use crate::dto::activity_dto::{ActivityListQuery, ActivityListResponse};
use crate::error::AppResult;

/// `GET /api/activity`
pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ActivityListQuery>,
) -> AppResult<Json<ActivityListResponse>> {
    let entries = state
        .activity
        .list(query.parsed()?)?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(ActivityListResponse { entries }))
}
