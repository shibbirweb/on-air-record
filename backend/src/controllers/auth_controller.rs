//! First run choice, setup, login, the second factor, and logout.

use std::sync::Arc;

use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, StatusCode};
use axum::{Extension, Json};

use crate::app::AppState;
use crate::controllers::auth_context::{
    challenge_cookie, challenge_token, clear_challenge_cookie, clear_session_cookie,
    session_cookie, session_token, Caller, ClientAddr,
};
use crate::dto::{
    AuthStateResponse, ChangePasswordRequest, CodeRequest, ConfirmPasswordRequest,
    CredentialsRequest, RecoveryCodesResponse, TwoFactorSetupResponse, TwoFactorStatusResponse,
};
use crate::error::{AppError, AppResult};
use crate::services::{LoginOutcome, SignedIn};

/// `GET /api/auth/state`
pub async fn state(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<Json<AuthStateResponse>> {
    Ok(Json(current_state(
        &state,
        session_token(&headers).as_deref(),
        challenge_token(&headers).as_deref(),
    )?))
}

/// `POST /api/auth/open`
///
/// Answers the first run question with "keep it open". Refused once the question has been answered.
pub async fn choose_open(State(state): State<Arc<AppState>>) -> AppResult<Json<AuthStateResponse>> {
    state.auth.choose_open()?;
    Ok(Json(current_state(&state, None, None)?))
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
///
/// With two factor sign in on the account, a right password answers `pendingTwoFactor: true` and sets
/// only the short lived challenge cookie; `POST /api/auth/login/verify` finishes the sign in.
pub async fn log_in(
    State(state): State<Arc<AppState>>,
    ClientAddr(client): ClientAddr,
    headers: HeaderMap,
    Json(request): Json<CredentialsRequest>,
) -> AppResult<(HeaderMap, Json<AuthStateResponse>)> {
    let auth = state.auth.clone();
    let outcome = blocking(move || auth.log_in(client, &request.email, &request.password)).await?;

    match outcome {
        LoginOutcome::SignedIn(signed_in) => signed_in_response(&state, &headers, signed_in),
        LoginOutcome::SecondFactorRequired { challenge } => {
            let mut response_headers = HeaderMap::new();
            response_headers.insert(SET_COOKIE, challenge_cookie(&challenge, &headers));
            Ok((
                response_headers,
                Json(current_state(&state, None, Some(&challenge))?),
            ))
        }
    }
}

/// `POST /api/auth/login/verify`
///
/// The second step: a code from the authenticator app, or a recovery code, against the pending sign in
/// in the challenge cookie.
pub async fn verify_login(
    State(state): State<Arc<AppState>>,
    ClientAddr(client): ClientAddr,
    headers: HeaderMap,
    Json(request): Json<CodeRequest>,
) -> AppResult<(HeaderMap, Json<AuthStateResponse>)> {
    let challenge = challenge_token(&headers).ok_or_else(|| {
        AppError::unauthorized("that sign in has expired; enter your password again")
    })?;
    let signed_in = state
        .auth
        .verify_second_factor(client, &challenge, &request.code)?;
    signed_in_response(&state, &headers, signed_in)
}

/// `POST /api/auth/logout`
///
/// Always succeeds and always clears both cookies, so a client can use it to recover from any confused
/// state, including backing out of the code step, without first working out where it is.
pub async fn log_out(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<(HeaderMap, Json<AuthStateResponse>)> {
    if let Some(token) = session_token(&headers) {
        state.auth.log_out(&token)?;
    }
    if let Some(challenge) = challenge_token(&headers) {
        state.auth.abandon_challenge(&challenge);
    }

    let mut response_headers = HeaderMap::new();
    response_headers.append(SET_COOKIE, clear_session_cookie());
    response_headers.append(SET_COOKIE, clear_challenge_cookie());
    Ok((response_headers, Json(current_state(&state, None, None)?)))
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

/// `GET /api/auth/two-factor`
pub async fn two_factor_status(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
) -> AppResult<Json<TwoFactorStatusResponse>> {
    let (user, _) = caller.signed_in()?;
    let (enabled, recovery_codes_left) = state.auth.two_factor_status(user.id)?;
    Ok(Json(TwoFactorStatusResponse {
        enabled,
        recovery_codes_left,
    }))
}

/// `POST /api/auth/two-factor/setup`
///
/// A new secret, as a QR code and as a key to type. Nothing changes for signing in until
/// `POST /api/auth/two-factor/enable` sees a code from the app.
pub async fn two_factor_setup(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
) -> AppResult<Json<TwoFactorSetupResponse>> {
    let (user, _) = caller.signed_in()?;
    let setup = state.auth.begin_two_factor_setup(user)?;
    Ok(Json(TwoFactorSetupResponse {
        secret_key: setup.secret_key,
        otpauth_uri: setup.otpauth_uri,
        qr_svg: setup.qr_svg,
    }))
}

/// `POST /api/auth/two-factor/enable`
pub async fn two_factor_enable(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<CodeRequest>,
) -> AppResult<Json<RecoveryCodesResponse>> {
    let (user, _) = caller.signed_in()?;
    let recovery_codes = state.auth.enable_two_factor(user.id, &request.code)?;
    Ok(Json(RecoveryCodesResponse { recovery_codes }))
}

/// `POST /api/auth/two-factor/disable`
pub async fn two_factor_disable(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<ConfirmPasswordRequest>,
) -> AppResult<StatusCode> {
    let (user, _) = caller.signed_in()?;
    let user_id = user.id;
    let auth = state.auth.clone();
    blocking(move || auth.disable_two_factor(user_id, &request.password)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/auth/two-factor/recovery-codes`
pub async fn recovery_codes(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<ConfirmPasswordRequest>,
) -> AppResult<Json<RecoveryCodesResponse>> {
    let (user, _) = caller.signed_in()?;
    let user_id = user.id;
    let auth = state.auth.clone();
    let recovery_codes =
        blocking(move || auth.regenerate_recovery_codes(user_id, &request.password)).await?;
    Ok(Json(RecoveryCodesResponse { recovery_codes }))
}

fn current_state(
    state: &AppState,
    token: Option<&str>,
    challenge: Option<&str>,
) -> AppResult<AuthStateResponse> {
    let mode = state.auth.mode()?;
    let user = if mode.requires_login() {
        state.auth.resolve(token)?
    } else {
        None
    };
    let pending_two_factor =
        mode.requires_login() && user.is_none() && state.auth.has_pending_challenge(challenge);

    Ok(AuthStateResponse {
        mode,
        user: user.map(Into::into),
        pending_two_factor,
    })
}

/// Set the session cookie, and clear any challenge cookie, since the sign in it was waiting on is done.
fn signed_in_response(
    state: &AppState,
    request_headers: &HeaderMap,
    signed_in: SignedIn,
) -> AppResult<(HeaderMap, Json<AuthStateResponse>)> {
    let mut headers = HeaderMap::new();
    headers.append(
        SET_COOKIE,
        session_cookie(&signed_in.token, request_headers),
    );
    headers.append(SET_COOKIE, clear_challenge_cookie());
    Ok((
        headers,
        Json(current_state(state, Some(&signed_in.token), None)?),
    ))
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
