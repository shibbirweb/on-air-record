//! The Prometheus endpoint and its scrape token, driven through the real router.
//!
//! The rules about what is reported are unit tested in `dto::metrics_dto`; these prove the wiring: that
//! each figure is read from the service that owns it, and that the body is what a scraper can parse. Who may
//! read it is in `routes/tests.rs` with the other access rules.

use std::net::{IpAddr, Ipv4Addr};

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};

use super::test_support::{call, get_raw, seed_recording, RawReply, SeedSegment, TestApp};
use crate::models::metrics::tests::parse_sample;
use crate::models::ListenerActivity;

fn app(name: &str) -> TestApp {
    super::test_support::app(&format!("metrics-{name}"))
}

async fn scrape(app: &TestApp) -> String {
    let reply: RawReply = get_raw(app, "/api/metrics").await;
    assert_eq!(reply.status, StatusCode::OK);
    String::from_utf8(reply.bytes).expect("utf-8")
}

/// The value of one series, by its whole name and label text as written.
fn value_of(body: &str, series: &str) -> Option<String> {
    body.lines()
        .find_map(|line| line.strip_prefix(series)?.strip_prefix(' '))
        .map(str::to_string)
}

#[tokio::test]
async fn the_endpoint_answers_in_the_prometheus_text_format() {
    let app = app("format");
    let reply = get_raw(&app, "/api/metrics").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.headers["content-type"],
        "text/plain; version=0.0.4; charset=utf-8"
    );
    let body = String::from_utf8(reply.bytes).expect("utf-8");
    assert!(body.ends_with('\n'), "the last line is terminated");

    let mut samples = 0;
    for line in body.lines() {
        if line.starts_with("# HELP ") || line.starts_with("# TYPE ") {
            continue;
        }
        let parsed = parse_sample(line);
        assert!(parsed.is_some(), "not a sample line: {line}");
        let (_, _, value) = parsed.unwrap_or_default();
        assert!(value.parse::<f64>().is_ok(), "not a number: {line}");
        samples += 1;
    }
    assert!(samples > 10, "{body}");

    assert_eq!(
        value_of(
            &body,
            &format!(
                r#"oar_build_info{{version="{}",channel="{}"}}"#,
                env!("CARGO_PKG_VERSION"),
                if env!("CARGO_PKG_VERSION").contains("-beta.") {
                    "beta"
                } else {
                    "stable"
                }
            )
        ),
        Some("1".to_string()),
        "{body}"
    );
}

#[tokio::test]
async fn a_fresh_install_reports_an_idle_recorder_and_no_history() {
    let app = app("fresh");
    let body = scrape(&app).await;
    assert_eq!(
        value_of(&body, r#"oar_capture_state{state="idle"}"#),
        Some("1".to_string())
    );
    assert_eq!(
        value_of(&body, r#"oar_capture_state{state="recording"}"#),
        Some("0".to_string())
    );
    assert_eq!(
        value_of(&body, "oar_recordings_segments"),
        Some("0".to_string())
    );
    assert!(!body.contains("oar_recordings_oldest_timestamp_seconds"));
    assert!(!body.contains("oar_input_level_rms"));
    assert_eq!(
        value_of(&body, "oar_retention_seconds"),
        Some("86400".to_string())
    );
}

#[tokio::test]
async fn the_start_time_is_when_the_service_came_up() {
    let app = app("start-time");
    let body = scrape(&app).await;
    let reported: f64 = value_of(&body, "oar_start_time_seconds")
        .expect("start time")
        .parse()
        .expect("number");
    assert_eq!(reported, app.state().started_at_ms as f64 / 1_000.0);
}

#[tokio::test]
async fn seeded_recordings_show_up_as_history() {
    let app = app("history");
    let start = 1_790_000_000_000;
    seed_recording(
        &app,
        &[
            SeedSegment {
                start_ms: start,
                seconds: 10,
                level: 20,
                loud: None,
            },
            SeedSegment {
                start_ms: start + 10_000,
                seconds: 10,
                level: 20,
                loud: None,
            },
        ],
    );
    let stats = app.state().segments.stats().expect("stats");
    assert!(stats.bytes > 0);

    let body = scrape(&app).await;
    assert_eq!(
        value_of(&body, "oar_recordings_segments"),
        Some("2".to_string())
    );
    assert_eq!(
        value_of(&body, "oar_recordings_bytes"),
        Some(stats.bytes.to_string())
    );
    assert_eq!(
        value_of(&body, "oar_recordings_oldest_timestamp_seconds"),
        Some("1790000000".to_string())
    );
    assert_eq!(
        value_of(&body, "oar_recordings_newest_timestamp_seconds"),
        Some("1790000020".to_string())
    );
}

#[tokio::test]
async fn open_streams_are_counted_by_what_they_are_doing() {
    let app = app("listeners");
    let registry = &app.state().listeners;
    let here = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let live = registry.register(None, here, None);
    let back = registry.register(None, here, None);
    back.set_activity(ListenerActivity::Playback { from_ms: 1_000 });

    let body = scrape(&app).await;
    assert_eq!(
        value_of(&body, r#"oar_listeners{activity="live"}"#),
        Some("1".to_string())
    );
    assert_eq!(
        value_of(&body, r#"oar_listeners{activity="playback"}"#),
        Some("1".to_string())
    );

    drop(live);
    drop(back);
    let body = scrape(&app).await;
    assert_eq!(
        value_of(&body, r#"oar_listeners{activity="live"}"#),
        Some("0".to_string())
    );
    assert_eq!(
        value_of(&body, r#"oar_listeners{activity="playback"}"#),
        Some("0".to_string())
    );
}

#[tokio::test]
async fn a_running_capture_is_reported_with_its_rate_and_healthy_disk() {
    let app = app("recording");
    app.state().capture.pretend_recording(16_000, 100);
    let body = scrape(&app).await;
    assert_eq!(
        value_of(&body, r#"oar_capture_state{state="recording"}"#),
        Some("1".to_string())
    );
    assert_eq!(
        value_of(&body, "oar_capture_sample_rate_hertz"),
        Some("16000".to_string())
    );
    assert_eq!(
        value_of(&body, "oar_recorder_disk_healthy"),
        Some("1".to_string())
    );
    assert!(value_of(&body, "oar_input_level_rms").is_some());
}

#[tokio::test]
async fn retention_follows_the_setting() {
    let app = app("retention");
    let changed = call(
        &app,
        Method::PATCH,
        "/api/settings",
        None,
        Some(json!({ "retentionHours": 48 })),
    )
    .await;
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.body);
    assert_eq!(
        value_of(&scrape(&app).await, "oar_retention_seconds"),
        Some("172800".to_string())
    );

    call(
        &app,
        Method::PATCH,
        "/api/settings",
        None,
        Some(json!({ "retentionHours": null })),
    )
    .await;
    assert!(!scrape(&app).await.contains("oar_retention_seconds"));
}

#[tokio::test]
async fn a_scrape_token_is_shown_once_then_only_its_date() {
    let app = app("token-lifecycle");

    let none = call(&app, Method::GET, "/api/metrics/token", None, None).await;
    assert_eq!(none.status, StatusCode::OK);
    assert_eq!(none.body, json!({ "createdAtMs": null }));

    let made = call(&app, Method::POST, "/api/metrics/token", None, None).await;
    assert_eq!(made.status, StatusCode::OK);
    let token = made.body["token"].as_str().expect("token").to_string();
    let created_at_ms = made.body["createdAtMs"].as_i64().expect("date");
    assert_eq!(token.len(), 64);
    assert_eq!(
        made.body.as_object().map(|body| body.len()),
        Some(2),
        "{}",
        made.body
    );

    let status = call(&app, Method::GET, "/api/metrics/token", None, None).await;
    assert_eq!(status.body, json!({ "createdAtMs": created_at_ms }));
    assert!(
        !status.body.to_string().contains(&token),
        "never shown again"
    );

    let rotated = call(&app, Method::POST, "/api/metrics/token", None, None).await;
    assert_ne!(rotated.body["token"], Value::String(token));

    let revoked = call(&app, Method::DELETE, "/api/metrics/token", None, None).await;
    assert_eq!(revoked.status, StatusCode::OK);
    assert_eq!(revoked.body, json!({ "createdAtMs": null }));
}
