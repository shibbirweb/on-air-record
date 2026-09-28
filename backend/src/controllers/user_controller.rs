//! Account management. Every route here is admin only, which the router's guard enforces.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};

use crate::app::AppState;
use crate::controllers::auth_context::{Caller, RequestOrigin};
use crate::controllers::auth_controller::blocking;
use crate::dto::{
    CreateUserRequest, SetPasswordRequest, UpdateUserRequest, UserDto, UserListResponse,
};
use crate::error::AppResult;
use crate::models::activity::ActivityEvent;
use crate::models::User;

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
    Extension(caller): Extension<Caller>,
    RequestOrigin(origin): RequestOrigin,
    Json(request): Json<CreateUserRequest>,
) -> AppResult<(StatusCode, Json<UserDto>)> {
    let role = request.parsed_role()?;
    let auth = state.auth.clone();
    let user = blocking(move || auth.create_user(&request.email, &request.password, role)).await?;
    state.activity.record(
        caller.actor(),
        &origin,
        ActivityEvent::AccountCreated {
            email: user.email.clone(),
            role: user.role,
        },
    );
    Ok((StatusCode::CREATED, Json(user.into())))
}

/// `PATCH /api/users/:id`
pub async fn update(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    RequestOrigin(origin): RequestOrigin,
    Path(user_id): Path<i64>,
    Json(request): Json<UpdateUserRequest>,
) -> AppResult<Json<UserDto>> {
    let role = request.parsed_role()?;
    let before = find(&state, user_id);
    let updated = state.auth.update_role(user_id, role)?;
    if let Some(before) = before.filter(|before| before.role != updated.role) {
        state.activity.record(
            caller.actor(),
            &origin,
            ActivityEvent::RoleChanged {
                email: updated.email.clone(),
                from: before.role,
                to: updated.role,
            },
        );
    }
    Ok(Json(updated.into()))
}

/// `POST /api/users/:id/password`
///
/// For somebody who forgot theirs. Signs that account out everywhere.
pub async fn set_password(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    RequestOrigin(origin): RequestOrigin,
    Path(user_id): Path<i64>,
    Json(request): Json<SetPasswordRequest>,
) -> AppResult<StatusCode> {
    let target = find(&state, user_id);
    let auth = state.auth.clone();
    blocking(move || auth.set_password(user_id, &request.password)).await?;
    if let Some(target) = target {
        state.activity.record(
            caller.actor(),
            &origin,
            ActivityEvent::PasswordSet {
                email: target.email,
            },
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/users/:id`
pub async fn remove(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    RequestOrigin(origin): RequestOrigin,
    Path(user_id): Path<i64>,
) -> AppResult<StatusCode> {
    // Read first: once the account is gone there is no email left to name it by.
    let target = find(&state, user_id);
    state.auth.delete_user(user_id)?;
    if let Some(target) = target {
        state.activity.record(
            caller.actor(),
            &origin,
            ActivityEvent::AccountRemoved {
                email: target.email,
            },
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /api/users/:id/two-factor`
///
/// For somebody who lost both their phone and their recovery codes. They sign in with just their
/// password afterwards and can set up a new app.
pub async fn reset_two_factor(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    RequestOrigin(origin): RequestOrigin,
    Path(user_id): Path<i64>,
) -> AppResult<StatusCode> {
    let target = find(&state, user_id);
    state.auth.reset_two_factor(user_id)?;
    if let Some(target) = target {
        state.activity.record(
            caller.actor(),
            &origin,
            ActivityEvent::TwoFactorRemoved {
                email: target.email,
            },
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

/// An account as it is before a change, for the log to name it and say what it was.
fn find(state: &AppState, user_id: i64) -> Option<User> {
    state
        .auth
        .list_users()
        .ok()?
        .into_iter()
        .find(|user| user.id == user_id)
}
