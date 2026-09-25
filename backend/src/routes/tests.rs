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
    cookie: Option<String>,
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
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(HOST, HOST_NAME);
    if let Some(cookie) = cookie {
        request = request.header(COOKIE, format!("oar_session={cookie}"));
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
    let cookie = response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("oar_session="))
        .and_then(|value| value.split(';').next())
        .map(str::to_string)
        .filter(|value| !value.is_empty());
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);

    Reply {
        status,
        cookie,
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
