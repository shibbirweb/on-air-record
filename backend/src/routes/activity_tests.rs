//! The activity log, driven through the real router: each thing a person can do over HTTP, and the one
//! entry it leaves behind.
//!
//! The model, storage and paging are unit tested beside their code. These prove the wiring: that every
//! handler records its event, with the right person, and only once the action has actually happened, so a
//! refused request leaves nothing claiming it worked. Listening is in `stream_tests.rs`, over a real
//! socket, and who may read the log is in `routes/tests.rs`.

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};

use super::test_support::{
    call, code_for, enable_two_factor, get_raw, log_in, seed_recording, send_with, set_up_admin,
    SeedSegment, TestApp,
};
use crate::models::activity::{
    ActivityEntry, ActivityEvent, ActivityQuery, Actor, SettingChange, SignInMethod,
};
use crate::models::Role;

const OWNER: &str = "owner@example.com";
const KITCHEN: &str = "kitchen@example.com";
const KITCHEN_PASSWORD: &str = "listen only please";

fn app(name: &str) -> TestApp {
    super::test_support::app(&format!("activity-{name}"))
}

/// Every entry, oldest first, which reads more naturally in a test than the page's newest first.
fn log(app: &TestApp) -> Vec<ActivityEntry> {
    let mut entries = app
        .state()
        .activity
        .list(ActivityQuery {
            limit: 200,
            ..ActivityQuery::default()
        })
        .expect("list");
    entries.reverse();
    entries
}

fn events(app: &TestApp) -> Vec<ActivityEvent> {
    log(app).into_iter().map(|entry| entry.event).collect()
}

fn last(app: &TestApp) -> ActivityEntry {
    log(app).pop().expect("an entry")
}

fn account(email: &str) -> impl Fn(&Actor) -> bool + '_ {
    move |actor| matches!(actor, Actor::Account { email: logged, .. } if logged == email)
}

/// The admin, signed in, plus a listener account made by them. Returns the admin's cookie and the
/// listener's id.
async fn admin_and_kitchen(app: &TestApp) -> (String, i64) {
    let admin = set_up_admin(app).await;
    let created = call(
        app,
        Method::POST,
        "/api/users",
        Some(&admin),
        Some(json!({ "email": KITCHEN, "password": KITCHEN_PASSWORD, "role": "listener" })),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    (admin, created.body["id"].as_i64().expect("id"))
}

// ---- signing in and out ------------------------------------------------------------------------------

#[tokio::test]
async fn setting_up_accounts_is_logged_as_the_new_admin() {
    let app = app("set-up");
    set_up_admin(&app).await;
    let entry = last(&app);
    assert_eq!(entry.event, ActivityEvent::AccountsSetUp);
    assert!(account(OWNER)(&entry.actor), "{:?}", entry.actor);
}

#[tokio::test]
async fn choosing_to_stay_open_is_logged_as_a_guest() {
    let app = app("stay-open");
    let chosen = call(&app, Method::POST, "/api/auth/open", None, None).await;
    assert_eq!(chosen.status, StatusCode::OK);
    let entry = last(&app);
    assert_eq!(entry.event, ActivityEvent::StayedOpen);
    assert_eq!(entry.actor, Actor::Guest);
}

#[tokio::test]
async fn a_sign_in_is_logged_with_who_it_was_and_their_browser() {
    let app = app("sign-in");
    admin_and_kitchen(&app).await;
    let reply = send_with(
        &app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({ "email": KITCHEN, "password": KITCHEN_PASSWORD })),
        None,
        &[("user-agent", "Kitchen tablet")],
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);

    let entry = last(&app);
    assert_eq!(
        entry.event,
        ActivityEvent::SignedIn {
            method: SignInMethod::Password
        }
    );
    assert!(account(KITCHEN)(&entry.actor), "{:?}", entry.actor);
    assert_eq!(entry.user_agent.as_deref(), Some("Kitchen tablet"));
}

#[tokio::test]
async fn a_wrong_password_is_logged_with_the_email_tried() {
    let app = app("wrong-password");
    set_up_admin(&app).await;
    let reply = log_in(&app, OWNER, "not the password").await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    let entry = last(&app);
    assert_eq!(
        entry.event,
        ActivityEvent::SignInFailed {
            email: Some(OWNER.to_string())
        }
    );
    assert_eq!(entry.actor, Actor::Guest, "nobody is signed in yet");
}

#[tokio::test]
async fn an_unknown_email_is_logged_the_same_way() {
    let app = app("unknown-email");
    set_up_admin(&app).await;
    log_in(&app, "stranger@example.com", "whatever it is").await;
    assert_eq!(
        last(&app).event,
        ActivityEvent::SignInFailed {
            email: Some("stranger@example.com".to_string())
        }
    );
}

/// Somebody who types their password into the email box by mistake must not find it in the log.
#[tokio::test]
async fn a_password_typed_as_the_email_is_not_kept() {
    let app = app("password-as-email");
    set_up_admin(&app).await;
    log_in(&app, "correct horse battery staple", "a long password").await;
    assert_eq!(
        last(&app).event,
        ActivityEvent::SignInFailed { email: None }
    );
}

#[tokio::test]
async fn a_sign_in_refused_for_too_many_failures_is_logged_as_blocked() {
    let app = app("blocked");
    set_up_admin(&app).await;
    let mut blocked = None;
    for _ in 0..10 {
        let reply = log_in(&app, OWNER, "still wrong").await;
        if reply.status == StatusCode::TOO_MANY_REQUESTS {
            blocked = Some(reply);
            break;
        }
    }
    assert!(blocked.is_some(), "the throttle never engaged");
    assert_eq!(
        last(&app).event,
        ActivityEvent::SignInBlocked {
            email: Some(OWNER.to_string())
        }
    );
}

#[tokio::test]
async fn a_sign_in_with_a_code_says_so() {
    let app = app("sign-in-code");
    let admin = set_up_admin(&app).await;
    let (secret, _) = enable_two_factor(&app, &admin).await;

    let before = log(&app).len();
    let password = log_in(&app, OWNER, "a long password").await;
    let challenge = password.challenge.expect("challenge");
    assert_eq!(
        log(&app).len(),
        before,
        "a right password alone is not a sign in"
    );

    let verified = super::test_support::call_with_challenge(
        &app,
        "/api/auth/login/verify",
        &challenge,
        json!({ "code": code_for(&secret, 1) }),
    )
    .await;
    assert_eq!(verified.status, StatusCode::OK, "{}", verified.body);
    let after = log(&app);
    assert_eq!(after.len(), before + 1, "one entry for the whole sign in");
    let entry = after.last().expect("entry");
    assert_eq!(
        entry.event,
        ActivityEvent::SignedIn {
            method: SignInMethod::Code
        }
    );
    assert!(account(OWNER)(&entry.actor));
}

#[tokio::test]
async fn a_sign_in_with_a_recovery_code_says_so() {
    let app = app("sign-in-recovery");
    let admin = set_up_admin(&app).await;
    let (_, codes) = enable_two_factor(&app, &admin).await;
    let challenge = log_in(&app, OWNER, "a long password")
        .await
        .challenge
        .expect("challenge");
    let verified = super::test_support::call_with_challenge(
        &app,
        "/api/auth/login/verify",
        &challenge,
        json!({ "code": codes[0] }),
    )
    .await;
    assert_eq!(verified.status, StatusCode::OK, "{}", verified.body);
    assert_eq!(
        last(&app).event,
        ActivityEvent::SignedIn {
            method: SignInMethod::RecoveryCode
        }
    );
}

#[tokio::test]
async fn a_wrong_code_is_logged_against_the_account() {
    let app = app("wrong-code");
    let admin = set_up_admin(&app).await;
    enable_two_factor(&app, &admin).await;
    let challenge = log_in(&app, OWNER, "a long password")
        .await
        .challenge
        .expect("challenge");
    let reply = super::test_support::call_with_challenge(
        &app,
        "/api/auth/login/verify",
        &challenge,
        json!({ "code": "000000" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    let entry = last(&app);
    assert_eq!(entry.event, ActivityEvent::SecondFactorFailed);
    assert!(account(OWNER)(&entry.actor), "{:?}", entry.actor);
}

/// A code sent with no sign in waiting, or one that expired, is not somebody guessing at an account.
#[tokio::test]
async fn a_code_for_no_pending_sign_in_logs_nothing() {
    let app = app("stale-code");
    set_up_admin(&app).await;
    let before = log(&app).len();
    let reply = super::test_support::call_with_challenge(
        &app,
        "/api/auth/login/verify",
        "no-such-challenge",
        json!({ "code": "000000" }),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(log(&app).len(), before);
}

#[tokio::test]
async fn signing_out_is_logged_for_whoever_was_signed_in() {
    let app = app("sign-out");
    let admin = set_up_admin(&app).await;
    let reply = call(&app, Method::POST, "/api/auth/logout", Some(&admin), None).await;
    assert_eq!(reply.status, StatusCode::OK);
    let entry = last(&app);
    assert_eq!(entry.event, ActivityEvent::SignedOut);
    assert!(account(OWNER)(&entry.actor));

    let before = log(&app).len();
    call(&app, Method::POST, "/api/auth/logout", None, None).await;
    call(&app, Method::POST, "/api/auth/logout", Some(&admin), None).await;
    assert_eq!(log(&app).len(), before, "nobody was signed in either time");
}

// ---- your own account -------------------------------------------------------------------------------

#[tokio::test]
async fn changing_your_own_password_is_logged() {
    let app = app("own-password");
    let admin = set_up_admin(&app).await;
    let reply = call(
        &app,
        Method::POST,
        "/api/auth/password",
        Some(&admin),
        Some(json!({ "currentPassword": "a long password", "newPassword": "an even longer one" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.body);
    let entry = last(&app);
    assert_eq!(entry.event, ActivityEvent::PasswordChanged);
    assert!(account(OWNER)(&entry.actor));
}

#[tokio::test]
async fn a_refused_password_change_logs_nothing() {
    let app = app("own-password-refused");
    let admin = set_up_admin(&app).await;
    let before = log(&app).len();
    let reply = call(
        &app,
        Method::POST,
        "/api/auth/password",
        Some(&admin),
        Some(json!({ "currentPassword": "not it", "newPassword": "an even longer one" })),
    )
    .await;
    assert_ne!(reply.status, StatusCode::NO_CONTENT);
    assert_eq!(log(&app).len(), before);
}

#[tokio::test]
async fn two_factor_on_codes_replaced_and_off_are_each_logged() {
    let app = app("own-two-factor");
    let admin = set_up_admin(&app).await;
    let before = log(&app).len();
    enable_two_factor(&app, &admin).await;
    assert_eq!(
        events(&app)[before..],
        [ActivityEvent::TwoFactorEnabled],
        "starting the setup is not logged, finishing it is"
    );

    let replaced = call(
        &app,
        Method::POST,
        "/api/auth/two-factor/recovery-codes",
        Some(&admin),
        Some(json!({ "password": "a long password" })),
    )
    .await;
    assert_eq!(replaced.status, StatusCode::OK);
    assert_eq!(last(&app).event, ActivityEvent::RecoveryCodesReplaced);

    let disabled = call(
        &app,
        Method::POST,
        "/api/auth/two-factor/disable",
        Some(&admin),
        Some(json!({ "password": "a long password" })),
    )
    .await;
    assert_eq!(disabled.status, StatusCode::NO_CONTENT);
    let entry = last(&app);
    assert_eq!(entry.event, ActivityEvent::TwoFactorDisabled);
    assert!(account(OWNER)(&entry.actor));
}

// ---- an admin's changes to accounts ------------------------------------------------------------------

#[tokio::test]
async fn an_admin_adding_an_account_is_logged_with_its_email_and_role() {
    let app = app("account-created");
    admin_and_kitchen(&app).await;
    let entry = last(&app);
    assert_eq!(
        entry.event,
        ActivityEvent::AccountCreated {
            email: KITCHEN.to_string(),
            role: Role::Listener,
        }
    );
    assert!(account(OWNER)(&entry.actor), "the admin did it");
}

#[tokio::test]
async fn a_role_change_is_logged_from_and_to() {
    let app = app("role-changed");
    let (admin, kitchen) = admin_and_kitchen(&app).await;
    let reply = call(
        &app,
        Method::PATCH,
        &format!("/api/users/{kitchen}"),
        Some(&admin),
        Some(json!({ "role": "admin" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        last(&app).event,
        ActivityEvent::RoleChanged {
            email: KITCHEN.to_string(),
            from: Role::Listener,
            to: Role::Admin,
        }
    );

    let before = log(&app).len();
    call(
        &app,
        Method::PATCH,
        &format!("/api/users/{kitchen}"),
        Some(&admin),
        Some(json!({ "role": "admin" })),
    )
    .await;
    assert_eq!(
        log(&app).len(),
        before,
        "the same role again changes nothing"
    );
}

#[tokio::test]
async fn setting_a_password_removing_two_factor_and_removing_an_account_are_logged() {
    let app = app("admin-actions");
    let (admin, kitchen) = admin_and_kitchen(&app).await;

    let set = call(
        &app,
        Method::POST,
        &format!("/api/users/{kitchen}/password"),
        Some(&admin),
        Some(json!({ "password": "a brand new password" })),
    )
    .await;
    assert_eq!(set.status, StatusCode::NO_CONTENT);
    assert_eq!(
        last(&app).event,
        ActivityEvent::PasswordSet {
            email: KITCHEN.to_string()
        }
    );

    let reset = call(
        &app,
        Method::DELETE,
        &format!("/api/users/{kitchen}/two-factor"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(reset.status, StatusCode::NO_CONTENT);
    assert_eq!(
        last(&app).event,
        ActivityEvent::TwoFactorRemoved {
            email: KITCHEN.to_string()
        }
    );

    let removed = call(
        &app,
        Method::DELETE,
        &format!("/api/users/{kitchen}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
    let entry = last(&app);
    assert_eq!(
        entry.event,
        ActivityEvent::AccountRemoved {
            email: KITCHEN.to_string()
        }
    );
    assert!(account(OWNER)(&entry.actor));
}

/// The log keeps naming somebody after their account is gone, which is when it is most likely needed.
#[tokio::test]
async fn a_removed_accounts_entries_still_name_them() {
    let app = app("removed-still-named");
    let (admin, kitchen) = admin_and_kitchen(&app).await;
    log_in(&app, KITCHEN, KITCHEN_PASSWORD).await;
    call(
        &app,
        Method::DELETE,
        &format!("/api/users/{kitchen}"),
        Some(&admin),
        None,
    )
    .await;
    assert!(log(&app).iter().any(|entry| account(KITCHEN)(&entry.actor)
        && matches!(entry.event, ActivityEvent::SignedIn { .. })));
}

#[tokio::test]
async fn an_account_change_that_is_refused_logs_nothing() {
    let app = app("refused-account-change");
    let admin = set_up_admin(&app).await;
    let before = log(&app).len();
    // The last admin cannot be made a listener, and nobody has id 999.
    let me = call(&app, Method::GET, "/api/users", Some(&admin), None)
        .await
        .body["users"][0]["id"]
        .as_i64()
        .expect("id");
    let demoted = call(
        &app,
        Method::PATCH,
        &format!("/api/users/{me}"),
        Some(&admin),
        Some(json!({ "role": "listener" })),
    )
    .await;
    assert_eq!(demoted.status, StatusCode::CONFLICT);
    let missing = call(&app, Method::DELETE, "/api/users/999", Some(&admin), None).await;
    assert_ne!(missing.status, StatusCode::NO_CONTENT);
    assert_eq!(log(&app).len(), before);
}

// ---- the recorder ------------------------------------------------------------------------------------

#[tokio::test]
async fn stopping_capture_is_logged() {
    let app = app("capture-stop");
    let reply = call(&app, Method::POST, "/api/capture/stop", None, None).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(last(&app).event, ActivityEvent::CaptureStopped);
}

#[tokio::test]
async fn choosing_a_microphone_is_logged() {
    let app = app("device-selected");
    let reply = call(
        &app,
        Method::POST,
        "/api/devices/select",
        None,
        Some(json!({ "deviceId": null })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(
        last(&app).event,
        ActivityEvent::DeviceSelected { device_id: None }
    );
}

#[tokio::test]
async fn saving_settings_logs_what_changed_from_and_to() {
    let app = app("settings-changed");
    let reply = call(
        &app,
        Method::PATCH,
        "/api/settings",
        None,
        Some(json!({ "retentionHours": 48, "soundSensitivity": "high" })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(
        last(&app).event,
        ActivityEvent::SettingsChanged {
            changes: vec![
                SettingChange {
                    key: "retentionHours".to_string(),
                    from: json!(24),
                    to: json!(48),
                },
                SettingChange {
                    key: "soundSensitivity".to_string(),
                    from: json!("medium"),
                    to: json!("high"),
                },
            ]
        }
    );
}

#[tokio::test]
async fn saving_settings_that_change_nothing_logs_nothing() {
    let app = app("settings-unchanged");
    let before = log(&app).len();
    call(
        &app,
        Method::PATCH,
        "/api/settings",
        None,
        Some(json!({ "retentionHours": 24 })),
    )
    .await;
    let refused = call(
        &app,
        Method::PATCH,
        "/api/settings",
        None,
        Some(json!({ "soundSensitivity": "deafening" })),
    )
    .await;
    assert!(refused.status.is_client_error(), "{}", refused.status);
    assert_eq!(log(&app).len(), before);
}

#[tokio::test]
async fn restoring_the_default_settings_is_logged() {
    let app = app("settings-reset");
    let reply = call(&app, Method::POST, "/api/settings/reset", None, None).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(last(&app).event, ActivityEvent::SettingsReset);
}

#[tokio::test]
async fn bookmarks_added_and_removed_are_logged_by_label() {
    let app = app("bookmarks");
    let made = call(
        &app,
        Method::POST,
        "/api/bookmarks",
        None,
        Some(json!({ "timestampMs": 1_790_000_000_000_i64, "label": "Door" })),
    )
    .await;
    assert_eq!(made.status, StatusCode::OK, "{}", made.body);
    assert_eq!(
        last(&app).event,
        ActivityEvent::BookmarkAdded {
            label: "Door".to_string(),
            timestamp_ms: 1_790_000_000_000,
        }
    );
    let id = made.body["id"].as_i64().expect("id");
    let removed = call(
        &app,
        Method::DELETE,
        &format!("/api/bookmarks/{id}"),
        None,
        None,
    )
    .await;
    assert_eq!(removed.status, StatusCode::OK);
    assert_eq!(
        last(&app).event,
        ActivityEvent::BookmarkRemoved {
            label: "Door".to_string()
        }
    );
}

#[tokio::test]
async fn the_scrape_token_made_rotated_and_revoked_is_logged() {
    let app = app("metrics-token");
    call(&app, Method::POST, "/api/metrics/token", None, None).await;
    assert_eq!(
        last(&app).event,
        ActivityEvent::MetricsTokenCreated { replaced: false }
    );
    call(&app, Method::POST, "/api/metrics/token", None, None).await;
    assert_eq!(
        last(&app).event,
        ActivityEvent::MetricsTokenCreated { replaced: true }
    );
    call(&app, Method::DELETE, "/api/metrics/token", None, None).await;
    assert_eq!(last(&app).event, ActivityEvent::MetricsTokenRevoked);
}

#[tokio::test]
async fn a_download_is_logged_with_its_range() {
    let app = app("export");
    let start = 1_790_000_000_000;
    seed_recording(
        &app,
        &[SeedSegment {
            start_ms: start,
            seconds: 10,
            level: 20,
            loud: None,
        }],
    );
    let path = format!("/api/export?fromMs={start}&toMs={}", start + 5_000);
    let reply = get_raw(&app, &path).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        last(&app).event,
        ActivityEvent::Exported {
            from_ms: start,
            to_ms: start + 5_000,
        }
    );

    let before = log(&app).len();
    let empty = get_raw(&app, "/api/export?fromMs=10&toMs=20").await;
    assert_ne!(empty.status, StatusCode::OK, "nothing recorded there");
    assert_eq!(log(&app).len(), before, "a refused download is not logged");
}

/// Everything secret that crosses the API in the course of a day: passwords right and wrong, codes,
/// recovery codes, the scrape token and session cookies. None of it may be anywhere in the log.
#[tokio::test]
async fn no_password_code_token_or_cookie_ever_reaches_the_log() {
    let app = app("no-secrets");
    let (admin, kitchen) = admin_and_kitchen(&app).await;
    let (secret, codes) = enable_two_factor(&app, &admin).await;
    let wrong_password = "wrong-password-8f3a";
    log_in(&app, OWNER, wrong_password).await;
    let challenge = log_in(&app, OWNER, "a long password")
        .await
        .challenge
        .expect("challenge");
    let wrong_code = "912873";
    super::test_support::call_with_challenge(
        &app,
        "/api/auth/login/verify",
        &challenge,
        json!({ "code": wrong_code }),
    )
    .await;
    let right_code = code_for(&secret, 1);
    let signed = super::test_support::call_with_challenge(
        &app,
        "/api/auth/login/verify",
        &challenge,
        json!({ "code": right_code }),
    )
    .await;
    let session = signed.cookie.expect("session");
    let new_password = "new-password-c41d";
    call(
        &app,
        Method::POST,
        &format!("/api/users/{kitchen}/password"),
        Some(&admin),
        Some(json!({ "password": new_password })),
    )
    .await;
    let token = call(&app, Method::POST, "/api/metrics/token", Some(&admin), None)
        .await
        .body["token"]
        .as_str()
        .expect("token")
        .to_string();

    let rows: Vec<String> = app
        .state()
        .activity
        .list(ActivityQuery {
            limit: 200,
            ..ActivityQuery::default()
        })
        .expect("list")
        .iter()
        .map(|entry| format!("{entry:?}"))
        .collect();
    let everything = rows.join("\n");
    assert!(rows.len() >= 8, "{everything}");
    let mut secrets = vec![
        "a long password".to_string(),
        KITCHEN_PASSWORD.to_string(),
        wrong_password.to_string(),
        wrong_code.to_string(),
        right_code,
        new_password.to_string(),
        token,
        admin,
        session,
        challenge,
    ];
    secrets.extend(codes);
    for secret in secrets {
        assert!(
            !everything.contains(&secret),
            "{secret} is in the log:\n{everything}"
        );
    }
}

// ---- reading the log --------------------------------------------------------------------------------

#[tokio::test]
async fn the_log_is_read_newest_first_with_filters() {
    let app = app("read");
    let (admin, _) = admin_and_kitchen(&app).await;
    log_in(&app, KITCHEN, KITCHEN_PASSWORD).await;
    log_in(&app, KITCHEN, "wrong").await;

    let page = call(&app, Method::GET, "/api/activity", Some(&admin), None).await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    let entries = page.body["entries"].as_array().expect("entries");
    let kinds: Vec<&str> = entries
        .iter()
        .filter_map(|entry| entry["event"]["kind"].as_str())
        .collect();
    assert_eq!(
        kinds,
        vec![
            "sign_in_failed",
            "signed_in",
            "account_created",
            "accounts_set_up"
        ]
    );
    let newest = &entries[0];
    assert!(newest["id"].as_i64().is_some());
    assert!(newest["atMs"].as_i64().is_some());
    assert_eq!(newest["actor"], json!({ "kind": "guest" }));
    assert_eq!(newest["event"]["email"], KITCHEN);

    let theirs = call(
        &app,
        Method::GET,
        &format!("/api/activity?email={KITCHEN}"),
        Some(&admin),
        None,
    )
    .await;
    let kinds: Vec<Value> = theirs.body["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| entry["event"]["kind"].clone())
        .collect();
    assert_eq!(kinds, vec![json!("signed_in")]);

    let accounts = call(
        &app,
        Method::GET,
        "/api/activity?group=accounts",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(accounts.body["entries"].as_array().map(Vec::len), Some(2));

    let one = call(
        &app,
        Method::GET,
        "/api/activity?limit=1",
        Some(&admin),
        None,
    )
    .await;
    let first = &one.body["entries"][0];
    let older = call(
        &app,
        Method::GET,
        &format!("/api/activity?limit=1&beforeId={}", first["id"]),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(older.body["entries"][0]["event"]["kind"], "signed_in");
}

#[tokio::test]
async fn an_unknown_group_is_refused() {
    let app = app("bad-group");
    let reply = call(
        &app,
        Method::GET,
        "/api/activity?group=everything",
        None,
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn how_long_the_log_is_kept_is_a_setting_kept_in_range() {
    let app = app("retention-setting");
    let shown = call(&app, Method::GET, "/api/settings", None, None).await;
    assert_eq!(shown.body["activityRetentionDays"], 90);

    let saved = call(
        &app,
        Method::PATCH,
        "/api/settings",
        None,
        Some(json!({ "activityRetentionDays": 30 })),
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert_eq!(saved.body["activityRetentionDays"], 30);
    assert_eq!(
        last(&app).event,
        ActivityEvent::SettingsChanged {
            changes: vec![SettingChange {
                key: "activityRetentionDays".to_string(),
                from: json!(90),
                to: json!(30),
            }]
        },
        "changing how long the log is kept is itself logged"
    );

    let clamped = call(
        &app,
        Method::PATCH,
        "/api/settings",
        None,
        Some(json!({ "activityRetentionDays": 0 })),
    )
    .await;
    assert_eq!(clamped.body["activityRetentionDays"], 1);
}
