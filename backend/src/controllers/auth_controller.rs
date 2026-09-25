//! First run choice, setup, login and logout.

use std::sync::Arc;

use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, StatusCode};
use axum::{Extension, Json};

use crate::app::AppState;
use crate::controllers::auth_context::{
    clear_session_cookie, session_cookie, session_token, Caller, ClientAddr,
};
use crate::dto::{AuthStateResponse, ChangePasswordRequest, CredentialsRequest};
use crate::error::{AppError, AppResult};
use crate::services::SignedIn;

/// `GET /api/auth/state`
pub async fn state(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<Json<AuthStateResponse>> {
    Ok(Json(current_state(
        &state,
        session_token(&headers).as_deref(),
    )?))
}

/// `POST /api/auth/open`
///
/// Answers the first run question with "keep it open". Refused once the question has been answered.
pub async fn choose_open(State(state): State<Arc<AppState>>) -> AppResult<Json<AuthStateResponse>> {
    state.auth.choose_open()?;
    Ok(Json(current_state(&state, None)?))
}

/// `POST /api/auth/setup`
///
/// Creates the first admin and switches accounts on. Works from the first run question and later from
/// the settings page of an install that chose to stay open; refused once accounts are on.
pub async fn set_up(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CredentialsRequest>,
) -> AppResult<(HeaderMap, Json<AuthStateResponse>)> {
    let auth = state.auth.clone();
    let signed_in = blocking(move || auth.set_up(&request.email, &request.password)).await?;
    signed_in_response(&state, &headers, signed_in)
}

/// `POST /api/auth/login`
pub async fn log_in(
    State(state): State<Arc<AppState>>,
    ClientAddr(client): ClientAddr,
    headers: HeaderMap,
    Json(request): Json<CredentialsRequest>,
) -> AppResult<(HeaderMap, Json<AuthStateResponse>)> {
    let auth = state.auth.clone();
    let signed_in =
        blocking(move || auth.log_in(client, &request.email, &request.password)).await?;
    signed_in_response(&state, &headers, signed_in)
}

/// `POST /api/auth/logout`
///
/// Always succeeds and always clears the cookie, so a client can use it to recover from any confused
/// state without first working out whether it is signed in.
pub async fn log_out(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<(HeaderMap, Json<AuthStateResponse>)> {
    if let Some(token) = session_token(&headers) {
        state.auth.log_out(&token)?;
    }

    let mut response_headers = HeaderMap::new();
    response_headers.insert(SET_COOKIE, clear_session_cookie());
    Ok((response_headers, Json(current_state(&state, None)?)))
}

/// `POST /api/auth/password`
pub async fn change_password(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<ChangePasswordRequest>,
) -> AppResult<StatusCode> {
    let (user, token) = caller.signed_in()?;
    let (user_id, token) = (user.id, token.to_string());

    let auth = state.auth.clone();
    blocking(move || {
        auth.change_own_password(
            user_id,
            &request.current_password,
            &request.new_password,
            &token,
        )
    })
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

fn current_state(state: &AppState, token: Option<&str>) -> AppResult<AuthStateResponse> {
    let mode = state.auth.mode()?;
    let user = if mode.requires_login() {
        state.auth.resolve(token)?
    } else {
        None
    };
    Ok(AuthStateResponse {
        mode,
        user: user.map(Into::into),
    })
}

fn signed_in_response(
    state: &AppState,
    request_headers: &HeaderMap,
    signed_in: SignedIn,
) -> AppResult<(HeaderMap, Json<AuthStateResponse>)> {
    let mut headers = HeaderMap::new();
    headers.insert(
        SET_COOKIE,
        session_cookie(&signed_in.token, request_headers),
    );
    Ok((headers, Json(current_state(state, Some(&signed_in.token))?)))
}

/// Run password hashing off the async runtime.
///
/// Argon2 is slow on purpose, tens of milliseconds per call in a release build, and doing that on a
/// runtime thread would stall every WebSocket that thread is serving.
pub async fn blocking<T, F>(operation: F) -> AppResult<T>
where
    F: FnOnce() -> AppResult<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| {
            AppError::internal(format!("the password check did not finish: {error}"))
        })?
}
