//! The access rules, exercised through the real router.
//!
//! Unit tests cover the pieces; these cover the wiring, which is where an access bug would actually live:
//! a route outside the guard, a cookie not set, a role not checked. Each test builds the full application
//! on a throwaway data directory and talks to it over HTTP without binding a port.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::header::{COOKIE, HOST, ORIGIN, SET_COOKIE};
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::app::AppState;
use crate::config::AppConfig;

const HOST_NAME: &str = "recorder.test:8080";

struct TestApp {
    router: Router,
    data_dir: std::path::PathBuf,
}

impl Drop for TestApp {
    fn drop(&mut self) {
        // Close the database before deleting its folder. Windows will not delete a file that is still
        // open, so the router, which owns the application and its connection, has to go first.
        drop(std::mem::replace(&mut self.router, Router::new()));
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}

fn app(name: &str) -> TestApp {
    let data_dir = std::env::temp_dir().join(format!("oar-routes-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);
    let config = AppConfig {
        data_dir: data_dir.clone(),
        static_dir: data_dir.join("no-ui"),
        ..AppConfig::default()
    };
    let state: Arc<AppState> = AppState::bootstrap(config).expect("bootstrap");
    TestApp {
        router: super::build(state),
        data_dir,
    }
}

struct Reply {
    status: StatusCode,
    /// A session cookie the response set.
    cookie: Option<String>,
    /// A two factor challenge cookie the response set.
    challenge: Option<String>,
    body: Value,
}

async fn call(
    app: &TestApp,
    method: Method,
    path: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> Reply {
    call_from(app, method, path, cookie, body, None).await
}

async fn call_from(
    app: &TestApp,
    method: Method,
    path: &str,
    cookie: Option<&str>,
    body: Option<Value>,
    origin: Option<&str>,
) -> Reply {
    let cookie_header = cookie.map(|token| format!("oar_session={token}"));
    send(app, method, path, cookie_header, body, origin).await
}

/// Call with the two factor challenge cookie, for the code step of a sign in.
async fn call_with_challenge(app: &TestApp, path: &str, challenge: &str, body: Value) -> Reply {
    let cookie_header = Some(format!("oar_challenge={challenge}"));
    send(app, Method::POST, path, cookie_header, Some(body), None).await
}

async fn send(
    app: &TestApp,
    method: Method,
    path: &str,
    cookie_header: Option<String>,
    body: Option<Value>,
    origin: Option<&str>,
) -> Reply {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(HOST, HOST_NAME);
    if let Some(cookie_header) = cookie_header {
        request = request.header(COOKIE, cookie_header);
    }
    if let Some(origin) = origin {
        request = request.header(ORIGIN, origin);
    }
    let request = match body {
        Some(body) => request
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .expect("request");

    let response = app.router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let set_cookie = |name: &str| {
        response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.strip_prefix(&format!("{name}=")))
            .filter_map(|value| value.split(';').next())
            .find(|value| !value.is_empty())
            .map(str::to_string)
    };
    let cookie = set_cookie("oar_session");
    let challenge = set_cookie("oar_challenge");
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);

    Reply {
        status,
        cookie,
        challenge,
        body,
    }
}

async fn set_up_admin(app: &TestApp) -> String {
    let reply = call(
        app,
        Method::POST,
        "/api/auth/setup",
        None,
        Some(json!({ "email": "owner@example.com", "password": "a long password" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    reply.cookie.expect("setup signs the admin in")
}

#[tokio::test]
async fn an_undecided_install_works_like_before_and_asks_the_question() {
    let app = app("undecided");

    let state = call(&app, Method::GET, "/api/auth/state", None, None).await;
    assert_eq!(state.body["mode"], "undecided");
    assert_eq!(state.body["user"], Value::Null);

    let status = call(&app, Method::GET, "/api/status", None, None).await;
    assert_eq!(status.status, StatusCode::OK);
}

#[tokio::test]
async fn choosing_to_stay_open_is_remembered_and_cannot_be_repeated() {
    let app = app("open");

    let chosen = call(&app, Method::POST, "/api/auth/open", None, None).await;
    assert_eq!(chosen.status, StatusCode::OK);
    assert_eq!(chosen.body["mode"], "open");

    let again = call(&app, Method::POST, "/api/auth/open", None, None).await;
    assert_eq!(again.status, StatusCode::CONFLICT);

    let settings = call(&app, Method::GET, "/api/settings", None, None).await;
    assert_eq!(settings.status, StatusCode::OK);
}

#[tokio::test]
async fn with_accounts_on_everything_but_the_login_needs_a_session() {
    let app = app("accounts");
    let admin = set_up_admin(&app).await;

    for path in [
        "/api/status",
        "/api/timeline/range",
        "/api/settings",
        "/api/ws/stream",
    ] {
        let signed_out = call(&app, Method::GET, path, None, None).await;
        assert_eq!(signed_out.status, StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(
            signed_out.body["error"]["code"], "unauthenticated",
            "{path}"
        );
    }

    let health = call(&app, Method::GET, "/api/health", None, None).await;
    assert_eq!(health.status, StatusCode::OK);

    let state = call(&app, Method::GET, "/api/auth/state", Some(&admin), None).await;
    assert_eq!(state.body["mode"], "accounts");
    assert_eq!(state.body["user"]["email"], "owner@example.com");
    assert_eq!(state.body["user"]["role"], "admin");

    let status = call(&app, Method::GET, "/api/status", Some(&admin), None).await;
    assert_eq!(status.status, StatusCode::OK);

    let second_setup = call(
        &app,
        Method::POST,
        "/api/auth/setup",
        None,
        Some(json!({ "email": "intruder@example.com", "password": "a long password" })),
    )
    .await;
    assert_eq!(second_setup.status, StatusCode::CONFLICT);

    let too_late = call(&app, Method::POST, "/api/auth/open", None, None).await;
    assert_eq!(too_late.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_listener_can_listen_but_not_change_anything() {
    let app = app("listener");
    let admin = set_up_admin(&app).await;

    let created = call(
        &app,
        Method::POST,
        "/api/users",
        Some(&admin),
        Some(json!({ "email": "kitchen@example.com", "password": "listen only", "role": "listener" })),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let login = call(
        &app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({ "email": "kitchen@example.com", "password": "listen only" })),
    )
    .await;
    assert_eq!(login.status, StatusCode::OK);
    let listener = login.cookie.expect("login sets the cookie");

    for path in [
        "/api/status",
        "/api/timeline/range",
        "/api/bookmarks",
        "/api/settings",
    ] {
        let reply = call(&app, Method::GET, path, Some(&listener), None).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}");
    }

    let forbidden = [
        (Method::POST, "/api/capture/stop", None),
        (Method::PATCH, "/api/settings", Some(json!({ "gain": 2.0 }))),
        (Method::POST, "/api/settings/reset", None),
        (
            Method::POST,
            "/api/bookmarks",
            Some(json!({ "timestampMs": 1, "label": "x" })),
        ),
        (Method::GET, "/api/users", None),
    ];
    for (method, path, body) in forbidden {
        let reply = call(&app, method.clone(), path, Some(&listener), body).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(reply.body["error"]["code"], "forbidden", "{method} {path}");
    }

    let changed = call(
        &app,
        Method::POST,
        "/api/auth/password",
        Some(&listener),
        Some(json!({ "currentPassword": "listen only", "newPassword": "still listen only" })),
    )
    .await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_wrong_password_is_refused_without_a_cookie() {
    let app = app("wrong-password");
    set_up_admin(&app).await;

    let reply = call(
        &app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({ "email": "owner@example.com", "password": "not the password" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(reply.cookie, None);
}

#[tokio::test]
async fn logging_out_ends_the_session_for_every_later_request() {
    let app = app("logout");
    let admin = set_up_admin(&app).await;

    let out = call(&app, Method::POST, "/api/auth/logout", Some(&admin), None).await;
    assert_eq!(out.status, StatusCode::OK);

    let after = call(&app, Method::GET, "/api/status", Some(&admin), None).await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn removing_an_account_signs_it_out_immediately() {
    let app = app("remove-account");
    let admin = set_up_admin(&app).await;

    let created = call(
        &app,
        Method::POST,
        "/api/users",
        Some(&admin),
        Some(json!({ "email": "guest@example.com", "password": "guest password", "role": "listener" })),
    )
    .await;
    let guest_id = created.body["id"].as_i64().expect("id");
    let guest = call(
        &app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({ "email": "guest@example.com", "password": "guest password" })),
    )
    .await
    .cookie
    .expect("guest cookie");

    let removed = call(
        &app,
        Method::DELETE,
        &format!("/api/users/{guest_id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);

    let after = call(&app, Method::GET, "/api/status", Some(&guest), None).await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn other_websites_cannot_change_anything_even_without_accounts() {
    let app = app("cross-site");

    let from_elsewhere = call_from(
        &app,
        Method::POST,
        "/api/capture/stop",
        None,
        None,
        Some("https://evil.example"),
    )
    .await;
    assert_eq!(from_elsewhere.status, StatusCode::FORBIDDEN);

    let stream_from_elsewhere = call_from(
        &app,
        Method::GET,
        "/api/ws/stream",
        None,
        None,
        Some("https://evil.example"),
    )
    .await;
    assert_eq!(stream_from_elsewhere.status, StatusCode::FORBIDDEN);

    let from_itself = call_from(
        &app,
        Method::POST,
        "/api/auth/open",
        None,
        None,
        Some("http://recorder.test:8080"),
    )
    .await;
    assert_eq!(from_itself.status, StatusCode::OK);

    // Reading is harmless without CORS: the other site's script cannot see the answer.
    let read_from_elsewhere = call_from(
        &app,
        Method::GET,
        "/api/status",
        None,
        None,
        Some("https://evil.example"),
    )
    .await;
    assert_eq!(read_from_elsewhere.status, StatusCode::OK);
}

async fn add_account(app: &TestApp, admin: &str, email: &str, password: &str, role: &str) -> i64 {
    let created = call(
        app,
        Method::POST,
        "/api/users",
        Some(admin),
        Some(json!({ "email": email, "password": password, "role": role })),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    created.body["id"].as_i64().expect("id")
}

async fn log_in(app: &TestApp, email: &str, password: &str) -> Reply {
    call(
        app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({ "email": email, "password": password })),
    )
    .await
}

#[tokio::test]
async fn an_admin_can_promote_a_listener_who_then_administers() {
    let app = app("promote");
    let admin = set_up_admin(&app).await;
    let listener_id = add_account(
        &app,
        &admin,
        "kitchen@example.com",
        "listen only",
        "listener",
    )
    .await;
    let listener = log_in(&app, "kitchen@example.com", "listen only")
        .await
        .cookie
        .expect("cookie");

    let before = call(&app, Method::GET, "/api/users", Some(&listener), None).await;
    assert_eq!(before.status, StatusCode::FORBIDDEN);

    let promoted = call(
        &app,
        Method::PATCH,
        &format!("/api/users/{listener_id}"),
        Some(&admin),
        Some(json!({ "role": "admin" })),
    )
    .await;
    assert_eq!(promoted.status, StatusCode::OK);
    assert_eq!(promoted.body["role"], "admin");

    // The role is read fresh on every request, so the same session gains the new powers at once.
    let after = call(&app, Method::GET, "/api/users", Some(&listener), None).await;
    assert_eq!(after.status, StatusCode::OK);
}

#[tokio::test]
async fn the_only_admin_can_be_neither_demoted_nor_removed_over_http() {
    let app = app("last-admin");
    let admin = set_up_admin(&app).await;
    let state = call(&app, Method::GET, "/api/auth/state", Some(&admin), None).await;
    let admin_id = state.body["user"]["id"].as_i64().expect("id");

    let demoted = call(
        &app,
        Method::PATCH,
        &format!("/api/users/{admin_id}"),
        Some(&admin),
        Some(json!({ "role": "listener" })),
    )
    .await;
    assert_eq!(demoted.status, StatusCode::CONFLICT);

    let removed = call(
        &app,
        Method::DELETE,
        &format!("/api/users/{admin_id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(removed.status, StatusCode::CONFLICT);

    let still = call(&app, Method::GET, "/api/status", Some(&admin), None).await;
    assert_eq!(still.status, StatusCode::OK);
}

#[tokio::test]
async fn an_admin_setting_a_password_replaces_it_and_signs_the_person_out() {
    let app = app("set-password");
    let admin = set_up_admin(&app).await;
    let listener_id = add_account(
        &app,
        &admin,
        "kitchen@example.com",
        "listen only",
        "listener",
    )
    .await;
    let listener = log_in(&app, "kitchen@example.com", "listen only")
        .await
        .cookie
        .expect("cookie");

    let set = call(
        &app,
        Method::POST,
        &format!("/api/users/{listener_id}/password"),
        Some(&admin),
        Some(json!({ "password": "a fresh password" })),
    )
    .await;
    assert_eq!(set.status, StatusCode::NO_CONTENT);

    let old_session = call(&app, Method::GET, "/api/status", Some(&listener), None).await;
    assert_eq!(old_session.status, StatusCode::UNAUTHORIZED);

    let old_password = log_in(&app, "kitchen@example.com", "listen only").await;
    assert_eq!(old_password.status, StatusCode::UNAUTHORIZED);

    let new_password = log_in(&app, "kitchen@example.com", "a fresh password").await;
    assert_eq!(new_password.status, StatusCode::OK);
}

#[tokio::test]
async fn account_requests_are_checked_before_anything_is_stored() {
    let app = app("validation");
    let admin = set_up_admin(&app).await;

    let cases = [
        json!({ "email": "not an email", "password": "long enough", "role": "listener" }),
        json!({ "email": "short@example.com", "password": "short", "role": "listener" }),
        json!({ "email": "root@example.com", "password": "long enough", "role": "root" }),
    ];
    for body in cases {
        let reply = call(
            &app,
            Method::POST,
            "/api/users",
            Some(&admin),
            Some(body.clone()),
        )
        .await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{body}");
    }

    add_account(&app, &admin, "taken@example.com", "long enough", "listener").await;
    let duplicate = call(
        &app,
        Method::POST,
        "/api/users",
        Some(&admin),
        Some(json!({ "email": "TAKEN@example.com", "password": "long enough", "role": "admin" })),
    )
    .await;
    assert_eq!(duplicate.status, StatusCode::CONFLICT);

    let users = call(&app, Method::GET, "/api/users", Some(&admin), None).await;
    assert_eq!(users.body["users"].as_array().map(Vec::len), Some(2));
}

#[tokio::test]
async fn accounts_cannot_be_added_to_an_install_that_stayed_open() {
    let app = app("open-add");
    call(&app, Method::POST, "/api/auth/open", None, None).await;

    let reply = call(
        &app,
        Method::POST,
        "/api/users",
        None,
        Some(json!({ "email": "someone@example.com", "password": "long enough", "role": "admin" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CONFLICT);
}

/// Switch on two factor sign in for the signed in account over HTTP, the way the page does, and
/// return the secret the phone would hold plus the recovery codes shown at the end.
async fn enable_two_factor(app: &TestApp, session: &str) -> (Vec<u8>, Vec<String>) {
    let setup = call(
        app,
        Method::POST,
        "/api/auth/two-factor/setup",
        Some(session),
        None,
    )
    .await;
    assert_eq!(setup.status, StatusCode::OK, "{}", setup.body);
    assert!(setup.body["qrSvg"]
        .as_str()
        .is_some_and(|svg| svg.contains("<svg")));
    let secret =
        crate::services::totp::base32_decode(setup.body["secretKey"].as_str().expect("key"));

    let enabled = call(
        app,
        Method::POST,
        "/api/auth/two-factor/enable",
        Some(session),
        Some(json!({ "code": code_for(&secret, 0) })),
    )
    .await;
    assert_eq!(enabled.status, StatusCode::OK, "{}", enabled.body);
    let codes = enabled.body["recoveryCodes"]
        .as_array()
        .expect("codes")
        .iter()
        .filter_map(|code| code.as_str().map(str::to_string))
        .collect();
    (secret, codes)
}

/// What the phone shows `steps_ahead` steps from now.
fn code_for(secret: &[u8], steps_ahead: i64) -> String {
    use crate::services::totp::{code_at, step_at};
    let now = crate::util::time::now_ms();
    format!("{:06}", code_at(secret, step_at(now) + steps_ahead))
}

#[tokio::test]
async fn with_two_factor_on_a_password_only_earns_the_code_step() {
    let app = app("two-factor-login");
    let admin = set_up_admin(&app).await;
    let (secret, _) = enable_two_factor(&app, &admin).await;

    let password = log_in(&app, "owner@example.com", "a long password").await;
    assert_eq!(password.status, StatusCode::OK);
    assert_eq!(password.body["pendingTwoFactor"], true);
    assert_eq!(password.body["user"], Value::Null);
    assert_eq!(password.cookie, None, "no session before the code");
    let challenge = password.challenge.expect("challenge cookie");

    // A reload during the code step still shows the code step.
    let state = send(
        &app,
        Method::GET,
        "/api/auth/state",
        Some(format!("oar_challenge={challenge}")),
        None,
        None,
    )
    .await;
    assert_eq!(state.body["pendingTwoFactor"], true);

    let wrong = call_with_challenge(
        &app,
        "/api/auth/login/verify",
        &challenge,
        json!({ "code": "000000" }),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);

    let verified = call_with_challenge(
        &app,
        "/api/auth/login/verify",
        &challenge,
        json!({ "code": code_for(&secret, 1) }),
    )
    .await;
    assert_eq!(verified.status, StatusCode::OK, "{}", verified.body);
    assert_eq!(verified.body["user"]["twoFactorEnabled"], true);
    let session = verified.cookie.expect("session after the code");

    let status = call(&app, Method::GET, "/api/status", Some(&session), None).await;
    assert_eq!(status.status, StatusCode::OK);
}

#[tokio::test]
async fn the_code_step_cannot_be_skipped_or_reached_without_a_password() {
    let app = app("two-factor-skip");
    let admin = set_up_admin(&app).await;
    let (secret, _) = enable_two_factor(&app, &admin).await;

    // No challenge cookie at all.
    let without = call(
        &app,
        Method::POST,
        "/api/auth/login/verify",
        None,
        Some(json!({ "code": code_for(&secret, 1) })),
    )
    .await;
    assert_eq!(without.status, StatusCode::UNAUTHORIZED);

    // A challenge cookie is not a session.
    let password = log_in(&app, "owner@example.com", "a long password").await;
    let challenge = password.challenge.expect("challenge");
    let as_session = call(&app, Method::GET, "/api/status", Some(&challenge), None).await;
    assert_eq!(as_session.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_recovery_code_signs_in_when_the_phone_is_gone() {
    let app = app("two-factor-recovery");
    let admin = set_up_admin(&app).await;
    let (_, recovery_codes) = enable_two_factor(&app, &admin).await;
    assert_eq!(recovery_codes.len(), 10);

    let challenge = log_in(&app, "owner@example.com", "a long password")
        .await
        .challenge
        .expect("challenge");
    let verified = call_with_challenge(
        &app,
        "/api/auth/login/verify",
        &challenge,
        json!({ "code": recovery_codes[3] }),
    )
    .await;
    assert_eq!(verified.status, StatusCode::OK);
    let session = verified.cookie.expect("session");

    let status = call(
        &app,
        Method::GET,
        "/api/auth/two-factor",
        Some(&session),
        None,
    )
    .await;
    assert_eq!(status.body["enabled"], true);
    assert_eq!(status.body["recoveryCodesLeft"], 9);
}

#[tokio::test]
async fn a_listener_manages_their_own_second_factor_and_an_admin_can_remove_it() {
    let app = app("two-factor-listener");
    let admin = set_up_admin(&app).await;
    let listener_id = add_account(
        &app,
        &admin,
        "kitchen@example.com",
        "listen only",
        "listener",
    )
    .await;
    let listener = log_in(&app, "kitchen@example.com", "listen only")
        .await
        .cookie
        .expect("cookie");

    enable_two_factor(&app, &listener).await;

    let wrong_password = call(
        &app,
        Method::POST,
        "/api/auth/two-factor/disable",
        Some(&listener),
        Some(json!({ "password": "not it" })),
    )
    .await;
    assert_eq!(wrong_password.status, StatusCode::BAD_REQUEST);

    // A listener cannot remove anybody's second factor, their own included, through the admin route.
    let not_theirs = call(
        &app,
        Method::DELETE,
        &format!("/api/users/{listener_id}/two-factor"),
        Some(&listener),
        None,
    )
    .await;
    assert_eq!(not_theirs.status, StatusCode::FORBIDDEN);

    let listed = call(&app, Method::GET, "/api/users", Some(&admin), None).await;
    let entry = listed.body["users"]
        .as_array()
        .and_then(|users| users.iter().find(|user| user["id"] == listener_id))
        .cloned()
        .expect("listed");
    assert_eq!(entry["twoFactorEnabled"], true);

    let removed = call(
        &app,
        Method::DELETE,
        &format!("/api/users/{listener_id}/two-factor"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);

    let login = log_in(&app, "kitchen@example.com", "listen only").await;
    assert!(login.cookie.is_some(), "password alone works again");
    assert_eq!(login.body["pendingTwoFactor"], false);
}

#[tokio::test]
async fn logging_out_from_the_code_step_forgets_it() {
    let app = app("two-factor-back");
    let admin = set_up_admin(&app).await;
    let (secret, _) = enable_two_factor(&app, &admin).await;

    let challenge = log_in(&app, "owner@example.com", "a long password")
        .await
        .challenge
        .expect("challenge");
    let back = call_with_challenge(&app, "/api/auth/logout", &challenge, json!({})).await;
    assert_eq!(back.status, StatusCode::OK);

    let after = call_with_challenge(
        &app,
        "/api/auth/login/verify",
        &challenge,
        json!({ "code": code_for(&secret, 1) }),
    )
    .await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
}
