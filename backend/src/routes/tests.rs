//! The access rules, exercised through the real router.
//!
//! Unit tests cover the pieces; these cover the wiring, which is where an access bug would actually live:
//! a route outside the guard, a cookie not set, a role not checked. Each test builds the full application
//! on a throwaway data directory and talks to it over HTTP without binding a port.

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};

use super::test_support::{
    app, call, call_from, call_with_challenge, get_raw_with, send, set_up_admin, Reply, TestApp,
};

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
        "/api/timeline/sounds?fromMs=0&toMs=60000",
        "/api/timeline/sounds/next?fromMs=0",
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
        "/api/timeline/sounds?fromMs=0&toMs=60000",
        "/api/timeline/sounds/next?fromMs=0&direction=backward",
        "/api/bookmarks",
        "/api/settings",
    ] {
        let reply = call(&app, Method::GET, path, Some(&listener), None).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}");
    }

    let forbidden = [
        (Method::POST, "/api/capture/stop", None),
        (Method::PATCH, "/api/settings", Some(json!({ "gain": 2.0 }))),
        // Finding sounds is for everyone; what counts as one is the admins' call.
        (
            Method::PATCH,
            "/api/settings",
            Some(json!({ "soundSensitivity": "high" })),
        ),
        (Method::POST, "/api/settings/reset", None),
        (
            Method::POST,
            "/api/bookmarks",
            Some(json!({ "timestampMs": 1, "label": "x" })),
        ),
        (Method::GET, "/api/users", None),
        // The update notice is for the people who can act on it, and names the install folder.
        (Method::GET, "/api/updates", None),
        (Method::POST, "/api/updates/check", None),
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
async fn admins_read_the_update_notice_without_waiting_on_the_network() {
    let app = app("updates");

    // Open: everybody has admin powers, so everybody may read it.
    let open = call(&app, Method::GET, "/api/updates", None, None).await;
    assert_eq!(open.status, StatusCode::OK, "{}", open.body);
    assert_eq!(open.body["currentVersion"], env!("CARGO_PKG_VERSION"));
    // Nothing has been checked yet: the first check waits a minute after start, and reading the status
    // never asks GitHub itself.
    assert_eq!(open.body["checkedAtMs"], Value::Null);
    assert_eq!(open.body["available"], Value::Null);
    assert_eq!(open.body["automatic"], true);

    let admin = set_up_admin(&app).await;
    let signed_out = call(&app, Method::GET, "/api/updates", None, None).await;
    assert_eq!(signed_out.status, StatusCode::UNAUTHORIZED);
    let as_admin = call(&app, Method::GET, "/api/updates", Some(&admin), None).await;
    assert_eq!(as_admin.status, StatusCode::OK);
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

#[tokio::test]
async fn five_wrong_passwords_lock_the_address_out_with_a_429_even_for_the_right_one() {
    let app = app("throttled");
    set_up_admin(&app).await;
    for _ in 0..5 {
        let refused = call(
            &app,
            Method::POST,
            "/api/auth/login",
            None,
            Some(json!({ "email": "owner@example.com", "password": "not the password" })),
        )
        .await;
        assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    }

    let locked = call(
        &app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({ "email": "owner@example.com", "password": "a long password" })),
    )
    .await;
    assert_eq!(locked.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(locked.body["error"]["code"], "rate_limited");
    assert!(
        locked.body["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("try again in"),
        "the page can say how long to wait: {}",
        locked.body
    );
    assert_eq!(locked.cookie, None, "no session for a locked out address");
}

/// CLAUDE.md: never add a CORS layer. The Origin check is the open mode's only protection against other
/// websites, and a CORS header would invite their pages to read the answers.
#[tokio::test]
async fn no_answer_ever_invites_another_website_to_read_it() {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    let app = app("no-cors");
    let requests = [
        Request::builder()
            .method(Method::OPTIONS)
            .uri("/api/status")
            .header("host", "recorder.test:8080")
            .header("origin", "http://elsewhere.example")
            .header("access-control-request-method", "POST")
            .body(Body::empty()),
        Request::builder()
            .method(Method::GET)
            .uri("/api/status")
            .header("host", "recorder.test:8080")
            .header("origin", "http://elsewhere.example")
            .body(Body::empty()),
    ];
    for request in requests {
        let request = request.expect("request");
        let described = format!("{} {}", request.method(), request.uri());
        let response = app.router.clone().oneshot(request).await.expect("response");
        let invited: Vec<_> = response
            .headers()
            .keys()
            .filter(|name| name.as_str().starts_with("access-control-"))
            .map(|name| name.as_str().to_string())
            .collect();
        assert!(invited.is_empty(), "{described} answered with {invited:?}");
    }
}

/// Every route and method the router serves, with the access it was decided to need. CLAUDE.md: a new
/// route needs no auth code, only a method that matches its intent, and a GET that reveals admin only data
/// must be named in the guard's exceptions. This table makes that a decision: a route added to
/// `routes/mod.rs` without a line here fails the test until somebody says who may call it.
const ROUTE_ACCESS: &[(&str, &str, &str, Option<crate::models::Access>)] = {
    use crate::models::Access::{Administer as ADMIN, Listen as LISTEN};
    &[
        ("GET", "/health", "/health", None),
        ("GET", "/auth/state", "/auth/state", None),
        ("POST", "/auth/open", "/auth/open", None),
        ("POST", "/auth/setup", "/auth/setup", None),
        ("POST", "/auth/login", "/auth/login", None),
        ("POST", "/auth/login/verify", "/auth/login/verify", None),
        ("POST", "/auth/logout", "/auth/logout", None),
        ("POST", "/auth/password", "/auth/password", Some(LISTEN)),
        ("GET", "/auth/two-factor", "/auth/two-factor", Some(LISTEN)),
        (
            "POST",
            "/auth/two-factor/setup",
            "/auth/two-factor/setup",
            Some(LISTEN),
        ),
        (
            "POST",
            "/auth/two-factor/enable",
            "/auth/two-factor/enable",
            Some(LISTEN),
        ),
        (
            "POST",
            "/auth/two-factor/disable",
            "/auth/two-factor/disable",
            Some(LISTEN),
        ),
        (
            "POST",
            "/auth/two-factor/recovery-codes",
            "/auth/two-factor/recovery-codes",
            Some(LISTEN),
        ),
        ("GET", "/users", "/users", Some(ADMIN)),
        ("POST", "/users", "/users", Some(ADMIN)),
        ("PATCH", "/users/{id}", "/users/7", Some(ADMIN)),
        ("DELETE", "/users/{id}", "/users/7", Some(ADMIN)),
        (
            "POST",
            "/users/{id}/password",
            "/users/7/password",
            Some(ADMIN),
        ),
        (
            "DELETE",
            "/users/{id}/two-factor",
            "/users/7/two-factor",
            Some(ADMIN),
        ),
        ("GET", "/status", "/status", Some(LISTEN)),
        ("POST", "/capture/start", "/capture/start", Some(ADMIN)),
        ("POST", "/capture/stop", "/capture/stop", Some(ADMIN)),
        ("GET", "/devices", "/devices", Some(LISTEN)),
        ("POST", "/devices/select", "/devices/select", Some(ADMIN)),
        ("GET", "/settings", "/settings", Some(LISTEN)),
        ("PATCH", "/settings", "/settings", Some(ADMIN)),
        (
            "GET",
            "/settings/defaults",
            "/settings/defaults",
            Some(LISTEN),
        ),
        ("POST", "/settings/reset", "/settings/reset", Some(ADMIN)),
        ("GET", "/updates", "/updates", Some(ADMIN)),
        ("POST", "/updates/check", "/updates/check", Some(ADMIN)),
        (
            "POST",
            "/settings/test-recordings-dir",
            "/settings/test-recordings-dir",
            Some(ADMIN),
        ),
        ("GET", "/timeline/range", "/timeline/range", Some(LISTEN)),
        ("GET", "/timeline/days", "/timeline/days", Some(LISTEN)),
        ("GET", "/timeline/peaks", "/timeline/peaks", Some(LISTEN)),
        ("GET", "/timeline/sounds", "/timeline/sounds", Some(LISTEN)),
        (
            "GET",
            "/timeline/sounds/next",
            "/timeline/sounds/next",
            Some(LISTEN),
        ),
        ("GET", "/bookmarks", "/bookmarks", Some(LISTEN)),
        ("POST", "/bookmarks", "/bookmarks", Some(ADMIN)),
        ("PATCH", "/bookmarks/{id}", "/bookmarks/7", Some(ADMIN)),
        ("DELETE", "/bookmarks/{id}", "/bookmarks/7", Some(ADMIN)),
        ("GET", "/sessions", "/sessions", Some(LISTEN)),
        ("GET", "/storage", "/storage", Some(LISTEN)),
        ("GET", "/export", "/export", Some(LISTEN)),
        ("GET", "/export/plan", "/export/plan", Some(LISTEN)),
        ("GET", "/ws/stream", "/ws/stream", Some(LISTEN)),
        ("GET", "/metrics", "/metrics", Some(LISTEN)),
        ("GET", "/metrics/token", "/metrics/token", Some(ADMIN)),
        ("POST", "/metrics/token", "/metrics/token", Some(ADMIN)),
        ("DELETE", "/metrics/token", "/metrics/token", Some(ADMIN)),
    ]
};

/// Every (method, route) pair `routes::build` registers, read from its source.
fn registered_routes() -> Vec<(String, String)> {
    let source = include_str!("mod.rs");
    let start = source.find("pub fn build(").expect("the build function");
    let end = source[start..]
        .find(".route_layer(")
        .map(|offset| start + offset)
        .expect("the guard layer after the routes");
    let body = &source[start..end];
    let mut pairs = Vec::new();
    for call in body.split(".route(").skip(1) {
        let path = call.split('"').nth(1).expect("a route path").to_string();
        for (needle, method) in [
            ("get(", "GET"),
            ("post(", "POST"),
            ("patch(", "PATCH"),
            ("delete(", "DELETE"),
            ("put(", "PUT"),
        ] {
            if call.contains(needle) {
                pairs.push((method.to_string(), path.clone()));
            }
        }
    }
    pairs.sort();
    pairs
}

#[test]
fn every_route_has_its_access_decided_in_the_table() {
    let mut decided: Vec<(String, String)> = ROUTE_ACCESS
        .iter()
        .map(|(method, route, _, _)| (method.to_string(), route.to_string()))
        .collect();
    decided.sort();
    let registered = registered_routes();
    let undecided: Vec<_> = registered
        .iter()
        .filter(|pair| !decided.contains(pair))
        .collect();
    let gone: Vec<_> = decided
        .iter()
        .filter(|pair| !registered.contains(pair))
        .collect();
    assert!(
        undecided.is_empty(),
        "routes with no access decided in ROUTE_ACCESS: {undecided:?}"
    );
    assert!(
        gone.is_empty(),
        "ROUTE_ACCESS lists routes that no longer exist: {gone:?}"
    );
}

#[test]
fn the_guard_asks_each_route_for_the_access_decided_for_it() {
    for (method, route, example, expected) in ROUTE_ACCESS {
        let method = Method::from_bytes(method.as_bytes()).expect("method");
        assert_eq!(
            super::guard::required_access(&method, example),
            *expected,
            "{method} {route}"
        );
    }
}

/// A listener account signed in, returning its session cookie.
async fn sign_in_listener(app: &TestApp, admin: &str) -> String {
    let created = call(
        app,
        Method::POST,
        "/api/users",
        Some(admin),
        Some(json!({ "email": "scraper@example.com", "password": "listen only", "role": "listener" })),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    call(
        app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({ "email": "scraper@example.com", "password": "listen only" })),
    )
    .await
    .cookie
    .expect("listener cookie")
}

async fn make_scrape_token(app: &TestApp, admin: Option<&str>) -> String {
    let made = call(app, Method::POST, "/api/metrics/token", admin, None).await;
    assert_eq!(made.status, StatusCode::OK, "{}", made.body);
    made.body["token"].as_str().expect("token").to_string()
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

#[tokio::test]
async fn metrics_are_open_on_an_open_recorder() {
    let app = app("metrics-open");
    let undecided = get_raw_with(&app, "/api/metrics", &[]).await;
    assert_eq!(undecided.status, StatusCode::OK);

    call(&app, Method::POST, "/api/auth/open", None, None).await;
    let open = get_raw_with(&app, "/api/metrics", &[]).await;
    assert_eq!(open.status, StatusCode::OK);
}

#[tokio::test]
async fn with_accounts_metrics_need_a_session_or_the_scrape_token() {
    let app = app("metrics-accounts");
    let admin = set_up_admin(&app).await;
    let listener = sign_in_listener(&app, &admin).await;

    let signed_out = get_raw_with(&app, "/api/metrics", &[]).await;
    assert_eq!(signed_out.status, StatusCode::UNAUTHORIZED);

    for cookie in [&admin, &listener] {
        let session = format!("oar_session={cookie}");
        let reply = get_raw_with(&app, "/api/metrics", &[("cookie", &session)]).await;
        assert_eq!(reply.status, StatusCode::OK);
    }

    let token = make_scrape_token(&app, Some(&admin)).await;
    let scraped = get_raw_with(&app, "/api/metrics", &[("authorization", &bearer(&token))]).await;
    assert_eq!(scraped.status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&scraped.bytes).contains("oar_build_info"));

    // The scheme name is case insensitive in HTTP, and Prometheus writes it as `Bearer`.
    let lower = format!("bearer {token}");
    let reply = get_raw_with(&app, "/api/metrics", &[("authorization", &lower)]).await;
    assert_eq!(reply.status, StatusCode::OK);
}

#[tokio::test]
async fn a_wrong_or_revoked_scrape_token_is_refused() {
    let app = app("metrics-wrong-token");
    let admin = set_up_admin(&app).await;
    let token = make_scrape_token(&app, Some(&admin)).await;

    for header in [
        bearer("0000"),
        bearer(""),
        "Bearer".to_string(),
        format!("Basic {token}"),
        token.clone(),
    ] {
        let reply = get_raw_with(&app, "/api/metrics", &[("authorization", &header)]).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{header}");
    }

    let revoked = call(
        &app,
        Method::DELETE,
        "/api/metrics/token",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(revoked.status, StatusCode::OK);
    let reply = get_raw_with(&app, "/api/metrics", &[("authorization", &bearer(&token))]).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
}

/// A wrong token is a misconfigured scraper, and it is better found now than on the day accounts go on,
/// so it is refused on an open recorder too, where no token is needed at all.
#[tokio::test]
async fn a_wrong_scrape_token_is_refused_even_on_an_open_recorder() {
    let app = app("metrics-open-wrong-token");
    call(&app, Method::POST, "/api/auth/open", None, None).await;
    let reply = get_raw_with(&app, "/api/metrics", &[("authorization", &bearer("nope"))]).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
}

/// The token opens the metrics and nothing else: not what they summarise, not the audio, and not a single
/// change. A leaked Prometheus config must never be a key to the recorder.
#[tokio::test]
async fn the_scrape_token_opens_nothing_but_the_metrics() {
    let app = app("metrics-token-scope");
    let admin = set_up_admin(&app).await;
    let token = make_scrape_token(&app, Some(&admin)).await;
    let authorization = bearer(&token);

    for path in [
        "/api/status",
        "/api/storage",
        "/api/settings",
        "/api/sessions",
        "/api/timeline/range",
        "/api/users",
        "/api/updates",
        "/api/metrics/token",
        "/api/ws/stream",
        "/api/export?fromMs=0&toMs=1000",
    ] {
        let reply = get_raw_with(&app, path, &[("authorization", &authorization)]).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{path}");
    }

    let changed =
        raw_with_authorization(&app, Method::POST, "/api/capture/stop", &authorization).await;
    assert_eq!(changed, StatusCode::UNAUTHORIZED);
    let rotated =
        raw_with_authorization(&app, Method::POST, "/api/metrics/token", &authorization).await;
    assert_eq!(rotated, StatusCode::UNAUTHORIZED);
    let revoked =
        raw_with_authorization(&app, Method::DELETE, "/api/metrics/token", &authorization).await;
    assert_eq!(revoked, StatusCode::UNAUTHORIZED);

    let still = get_raw_with(&app, "/api/metrics", &[("authorization", &authorization)]).await;
    assert_eq!(
        still.status,
        StatusCode::OK,
        "the token itself is untouched"
    );
}

async fn raw_with_authorization(
    app: &TestApp,
    method: Method,
    path: &str,
    authorization: &str,
) -> StatusCode {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", super::test_support::HOST_NAME)
        .header("authorization", authorization)
        .body(Body::empty())
        .expect("request");
    app.router
        .clone()
        .oneshot(request)
        .await
        .expect("response")
        .status()
}

#[tokio::test]
async fn only_an_admin_manages_the_scrape_token() {
    let app = app("metrics-token-admin");
    let admin = set_up_admin(&app).await;
    let listener = sign_in_listener(&app, &admin).await;

    for method in [Method::GET, Method::POST, Method::DELETE] {
        let signed_out = call(&app, method.clone(), "/api/metrics/token", None, None).await;
        assert_eq!(signed_out.status, StatusCode::UNAUTHORIZED, "{method}");
        let listening = call(
            &app,
            method.clone(),
            "/api/metrics/token",
            Some(&listener),
            None,
        )
        .await;
        assert_eq!(listening.status, StatusCode::FORBIDDEN, "{method}");
        let administering = call(
            &app,
            method.clone(),
            "/api/metrics/token",
            Some(&admin),
            None,
        )
        .await;
        assert_eq!(administering.status, StatusCode::OK, "{method}");
    }
}

#[tokio::test]
async fn another_website_cannot_make_or_revoke_the_scrape_token() {
    let app = app("metrics-token-origin");
    for method in [Method::POST, Method::DELETE] {
        let reply = call_from(
            &app,
            method.clone(),
            "/api/metrics/token",
            None,
            None,
            Some("http://elsewhere.example"),
        )
        .await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{method}");
    }
    let status = call(&app, Method::GET, "/api/metrics/token", None, None).await;
    assert_eq!(status.body["createdAtMs"], Value::Null, "nothing was made");
}

/// A scraper set up while the recorder was open keeps working when accounts are turned on, so securing
/// the recorder does not quietly blind its monitoring.
#[tokio::test]
async fn a_scrape_token_made_before_accounts_keeps_working_after() {
    let app = app("metrics-token-before-accounts");
    let token = make_scrape_token(&app, None).await;
    set_up_admin(&app).await;
    let reply = get_raw_with(&app, "/api/metrics", &[("authorization", &bearer(&token))]).await;
    assert_eq!(reply.status, StatusCode::OK);
}
