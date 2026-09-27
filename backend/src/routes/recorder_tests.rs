//! The recorder's own endpoints over HTTP: health, status, sessions, storage, devices, capture and every
//! settings endpoint, with the JSON the control room and the settings page read.
//!
//! Starting capture is deliberately not driven here. An unknown device falls back to the default input
//! (see `DeviceRegistry::resolve`), so a test that starts capture would record from a developer's own
//! microphone and fail on a CI runner, which has none. What capture does with audio is covered by the
//! recorder and pipeline tests, which feed it frames directly.

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};

use super::test_support::{
    app, call, call_from, seed_recording, SeedSegment, TestApp, SAME_ORIGIN,
};

async fn get(app: &TestApp, path: &str) -> (StatusCode, Value) {
    let reply = call(app, Method::GET, path, None, None).await;
    (reply.status, reply.body)
}

async fn write(
    app: &TestApp,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let reply = call_from(app, method, path, None, body, Some(SAME_ORIGIN)).await;
    (reply.status, reply.body)
}

fn hours_ago(hours: i64) -> i64 {
    (crate::util::time::now_ms() - hours * 3_600_000) / 10_000 * 10_000
}

// ---- health and status ------------------------------------------------------------------------------

#[tokio::test]
async fn health_reports_ok_this_version_and_how_long_it_has_been_up() {
    let app = app("recorder-health");
    let (status, body) = get(&app, "/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert!(body["uptimeMs"].as_i64().expect("uptime") >= 0);
}

#[tokio::test]
async fn status_describes_an_idle_recorder_nobody_is_listening_to() {
    let app = app("recorder-status");
    let (status, body) = get(&app, "/api/status").await;
    assert_eq!(status, StatusCode::OK);
    let capture = &body["capture"];
    assert_eq!(capture["state"], "idle");
    assert_eq!(capture["sessionId"], Value::Null);
    assert_eq!(capture["startedAtMs"], Value::Null);
    assert_eq!(capture["droppedFrames"], 0);
    assert_eq!(capture["error"], Value::Null);
    assert_eq!(body["listeners"], 0);
    assert_eq!(body["liveEdgeMs"], Value::Null);
    assert!(body["levels"]["rms"].as_f64().is_some());
    assert!(body["levels"]["peak"].as_f64().is_some());
    assert!(body["serverTimeMs"].as_i64().is_some());
}

#[tokio::test]
async fn stopping_a_recorder_that_is_not_recording_is_harmless_and_says_it_is_idle() {
    let app = app("recorder-stop-idle");
    let (status, body) = write(&app, Method::POST, "/api/capture/stop", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["capture"]["state"], "idle");
}

// ---- sessions and storage ---------------------------------------------------------------------------

#[tokio::test]
async fn sessions_list_newest_first_with_what_each_recorded() {
    let app = app("recorder-sessions");
    let older = seed_recording(
        &app,
        &[SeedSegment {
            start_ms: hours_ago(5),
            seconds: 10,
            level: 1,
            loud: None,
        }],
    );
    let newer = seed_recording(
        &app,
        &[
            SeedSegment {
                start_ms: hours_ago(2),
                seconds: 10,
                level: 1,
                loud: None,
            },
            SeedSegment {
                start_ms: hours_ago(2) + 10_000,
                seconds: 10,
                level: 1,
                loud: None,
            },
        ],
    );

    let (status, body) = get(&app, "/api/sessions").await;
    assert_eq!(status, StatusCode::OK);
    let sessions = body["sessions"].as_array().expect("sessions");
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0]["id"], newer);
    assert_eq!(sessions[1]["id"], older);
    let newest = &sessions[0];
    assert_eq!(newest["deviceName"], "seeded microphone");
    assert_eq!(newest["sampleRate"], 48_000);
    assert_eq!(newest["channels"], 1);
    assert_eq!(newest["segmentCount"], 2);
    assert_eq!(newest["bytes"], 2 * 10 * 48_000 * 2);
}

#[tokio::test]
async fn storage_adds_up_what_is_on_disk_and_projects_the_retention_window() {
    let app = app("recorder-storage");
    let (_, empty) = get(&app, "/api/storage").await;
    assert_eq!(empty["bytes"], 0);
    assert_eq!(empty["segmentCount"], 0);
    assert_eq!(empty["oldestMs"], Value::Null);

    let first = hours_ago(4);
    seed_recording(
        &app,
        &[
            SeedSegment {
                start_ms: first,
                seconds: 10,
                level: 1,
                loud: None,
            },
            SeedSegment {
                start_ms: first + 10_000,
                seconds: 10,
                level: 1,
                loud: None,
            },
        ],
    );
    let (status, body) = get(&app, "/api/storage").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["bytes"], 2 * 10 * 48_000 * 2);
    assert_eq!(body["segmentCount"], 2);
    assert_eq!(body["oldestMs"], first);
    assert_eq!(body["newestMs"], first + 20_000);
    assert_eq!(body["retentionHours"], 24);
    assert!(body["dataDir"]
        .as_str()
        .expect("data dir")
        .contains("oar-routes-recorder-storage"));
    assert!(body["recordingsDir"]
        .as_str()
        .expect("recordings dir")
        .ends_with("recordings"));
    let per_hour = body["bytesPerHour"].as_i64().expect("bytes per hour");
    assert!(per_hour > 0);
    assert_eq!(body["projectedMaxBytes"], per_hour * 24);
}

#[tokio::test]
async fn keeping_recordings_forever_leaves_nothing_to_project() {
    let app = app("recorder-storage-forever");
    let (status, _) = write(
        &app,
        Method::PATCH,
        "/api/settings",
        Some(json!({ "retentionHours": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = get(&app, "/api/storage").await;
    assert_eq!(body["retentionHours"], Value::Null);
    assert_eq!(body["projectedMaxBytes"], Value::Null);
}

// ---- devices ----------------------------------------------------------------------------------------

#[tokio::test]
async fn devices_come_back_as_a_list_whatever_the_machine_has() {
    // What is in the list depends on the hardware: a CI runner has no microphone. The shape does not.
    let app = app("recorder-devices");
    let (status, body) = get(&app, "/api/devices").await;
    assert_eq!(status, StatusCode::OK);
    for device in body["devices"].as_array().expect("a list") {
        assert!(device["id"].is_string(), "{device}");
        assert!(device["name"].is_string(), "{device}");
    }
}

#[tokio::test]
async fn selecting_a_device_that_is_not_on_this_machine_is_refused() {
    let app = app("recorder-select-unknown");
    let (status, body) = write(
        &app,
        Method::POST,
        "/api/devices/select",
        Some(json!({ "deviceId": "a microphone nobody has" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
    let (_, settings) = get(&app, "/api/settings").await;
    assert_eq!(settings["inputDeviceId"], Value::Null, "nothing was stored");
}

#[tokio::test]
async fn selecting_the_system_default_always_works() {
    let app = app("recorder-select-default");
    let (status, body) = write(
        &app,
        Method::POST,
        "/api/devices/select",
        Some(json!({ "deviceId": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["capture"]["state"], "idle",
        "an idle recorder is not started by choosing"
    );
    let (_, settings) = get(&app, "/api/settings").await;
    assert_eq!(settings["inputDeviceId"], Value::Null);
}

// ---- settings ---------------------------------------------------------------------------------------

#[tokio::test]
async fn settings_start_at_the_defaults_with_every_field_the_settings_page_reads() {
    let app = app("recorder-settings-show");
    let (status, body) = get(&app, "/api/settings").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["inputDeviceId"], Value::Null);
    assert_eq!(body["gain"], 1.0);
    assert_eq!(body["segmentSeconds"], 10);
    assert_eq!(body["retentionHours"], 24);
    assert_eq!(body["autoStart"], true);
    assert_eq!(body["autoStartDelaySeconds"], 0);
    assert_eq!(body["frameMs"], 100);
    assert_eq!(body["recordingSampleRate"], Value::Null);
    assert_eq!(body["recordingsDir"], Value::Null);
    assert!(body["effectiveRecordingsDir"]
        .as_str()
        .expect("path")
        .ends_with("recordings"));
    assert_eq!(body["checkForUpdates"], true);
    assert_eq!(body["soundSensitivity"], "medium");

    let (status, defaults) = get(&app, "/api/settings/defaults").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(defaults, body, "a fresh install is at its defaults");
}

#[tokio::test]
async fn a_change_is_stored_returned_whole_and_kept() {
    let app = app("recorder-settings-patch");
    let (status, body) = write(
        &app,
        Method::PATCH,
        "/api/settings",
        Some(json!({ "gain": 1.5, "segmentSeconds": 30, "autoStart": false, "soundSensitivity": "high" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["gain"], 1.5);
    assert_eq!(body["segmentSeconds"], 30);
    assert_eq!(body["autoStart"], false);
    assert_eq!(body["soundSensitivity"], "high");
    assert_eq!(body["frameMs"], 100, "untouched fields come back too");

    let (_, reread) = get(&app, "/api/settings").await;
    assert_eq!(reread, body);
}

#[tokio::test]
async fn values_out_of_range_are_clamped_rather_than_refused() {
    let app = app("recorder-settings-clamp");
    let (status, body) = write(
        &app,
        Method::PATCH,
        "/api/settings",
        Some(json!({
            "gain": 99.0,
            "segmentSeconds": 1,
            "retentionHours": 0,
            "autoStartDelaySeconds": 100000,
            "frameMs": 5,
            "recordingSampleRate": 44100,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["gain"], 4.0);
    assert_eq!(body["segmentSeconds"], 5);
    assert_eq!(body["retentionHours"], 1);
    assert_eq!(body["autoStartDelaySeconds"], 600);
    assert_eq!(body["frameMs"], 20);
    assert_eq!(
        body["recordingSampleRate"], 48_000,
        "snapped to the nearest supported rate"
    );
}

#[tokio::test]
async fn an_unknown_word_or_a_wrong_type_is_refused_and_changes_nothing() {
    let app = app("recorder-settings-refused");
    for body in [
        json!({ "soundSensitivity": "loud" }),
        json!({ "gain": "loud" }),
        json!({ "autoStart": "sometimes" }),
    ] {
        let (status, _) = write(&app, Method::PATCH, "/api/settings", Some(body.clone())).await;
        assert!(status.is_client_error(), "{body}: {status}");
    }
    let (_, settings) = get(&app, "/api/settings").await;
    assert_eq!(settings["soundSensitivity"], "medium");
    assert_eq!(settings["gain"], 1.0);
}

#[tokio::test]
async fn reset_restores_the_defaults() {
    let app = app("recorder-settings-reset");
    write(
        &app,
        Method::PATCH,
        "/api/settings",
        Some(json!({ "gain": 2.0, "retentionHours": 72, "checkForUpdates": false })),
    )
    .await;
    let (status, body) = write(&app, Method::POST, "/api/settings/reset", None).await;
    assert_eq!(status, StatusCode::OK);
    let (_, defaults) = get(&app, "/api/settings/defaults").await;
    assert_eq!(body, defaults);
}

#[tokio::test]
async fn a_writable_recordings_folder_is_accepted_and_resolved() {
    let app = app("recorder-recordings-dir");
    let folder = app.data_dir.join("elsewhere");
    let (status, probe) = write(
        &app,
        Method::POST,
        "/api/settings/test-recordings-dir",
        Some(json!({ "path": folder.to_string_lossy() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(probe["ok"], true, "{probe}");
    assert_eq!(probe["exists"], false);
    assert_eq!(probe["willCreate"], true);
    assert!(probe["message"].as_str().is_some());

    let (status, saved) = write(
        &app,
        Method::PATCH,
        "/api/settings",
        Some(json!({ "recordingsDir": folder.to_string_lossy() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        saved["effectiveRecordingsDir"],
        folder.to_string_lossy().as_ref()
    );

    let (_, back) = write(
        &app,
        Method::PATCH,
        "/api/settings",
        Some(json!({ "recordingsDir": null })),
    )
    .await;
    assert!(back["effectiveRecordingsDir"]
        .as_str()
        .expect("path")
        .ends_with("recordings"));
}

#[tokio::test]
async fn a_recordings_folder_that_cannot_be_written_is_reported_and_refused() {
    let app = app("recorder-recordings-dir-bad");
    // A folder inside a file cannot be created on any platform.
    let blocker = app.data_dir.join("a-file");
    std::fs::write(&blocker, b"not a folder").expect("file");
    let impossible = blocker.join("recordings");

    let (status, probe) = write(
        &app,
        Method::POST,
        "/api/settings/test-recordings-dir",
        Some(json!({ "path": impossible.to_string_lossy() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(probe["ok"], false, "{probe}");
    assert_eq!(probe["writable"], false);

    let (status, body) = write(
        &app,
        Method::PATCH,
        "/api/settings",
        Some(json!({ "recordingsDir": impossible.to_string_lossy() })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "bad_request");
    let (_, settings) = get(&app, "/api/settings").await;
    assert_eq!(
        settings["recordingsDir"],
        Value::Null,
        "the refused folder was not stored"
    );
}

#[tokio::test]
async fn spaces_around_a_saved_recordings_folder_are_trimmed_but_spaces_inside_it_are_kept() {
    // The settings page stages the folder exactly as typed, so a name like "My Drive" can be entered; the
    // server is what trims it.
    let app = app("recorder-recordings-dir-spaces");
    let folder = app.data_dir.join("My Drive");
    let typed = format!("  {}  ", folder.to_string_lossy());
    let (status, saved) = write(
        &app,
        Method::PATCH,
        "/api/settings",
        Some(json!({ "recordingsDir": typed })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["recordingsDir"], folder.to_string_lossy().as_ref());
    assert_eq!(
        saved["effectiveRecordingsDir"],
        folder.to_string_lossy().as_ref()
    );
}
