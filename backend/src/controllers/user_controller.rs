//! Account management. Every route here is admin only, which the router's guard enforces.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use crate::app::AppState;
use crate::controllers::auth_controller::blocking;
use crate::dto::{
    CreateUserRequest, SetPasswordRequest, UpdateUserRequest, UserDto, UserListResponse,
};
use crate::error::AppResult;

/// `GET /api/users`
pub async fn list(State(state): State<Arc<AppState>>) -> AppResult<Json<UserListResponse>> {
    let users = state
        .auth
        .list_users()?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(UserListResponse { users }))
}

/// `POST /api/users`
pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateUserRequest>,
) -> AppResult<(StatusCode, Json<UserDto>)> {
    let role = request.parsed_role()?;
    let auth = state.auth.clone();
    let user = blocking(move || auth.create_user(&request.email, &request.password, role)).await?;
    Ok((StatusCode::CREATED, Json(user.into())))
}

/// `PATCH /api/users/:id`
pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<i64>,
    Json(request): Json<UpdateUserRequest>,
) -> AppResult<Json<UserDto>> {
    let role = request.parsed_role()?;
    Ok(Json(state.auth.update_role(user_id, role)?.into()))
}

/// `POST /api/users/:id/password`
///
/// For somebody who forgot theirs. Signs that account out everywhere.
pub async fn set_password(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<i64>,
    Json(request): Json<SetPasswordRequest>,
) -> AppResult<StatusCode> {
    let auth = state.auth.clone();
    blocking(move || auth.set_password(user_id, &request.password)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/users/:id`
pub async fn remove(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<i64>,
) -> AppResult<StatusCode> {
    state.auth.delete_user(user_id)?;
    Ok(StatusCode::NO_CONTENT)
}
