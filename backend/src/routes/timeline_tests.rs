//! The timeline endpoints over HTTP, on seeded recordings: what the UI asks to draw the timeline, the day
//! picker, the waveform and the sounds, and what it gets back, field by field.
//!
//! The services behind these have their own tests; these prove the wiring (query parsing, validation,
//! the JSON field names the frontend reads) that only shows when the whole router answers a request.

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};

use super::test_support::{
    app, call, call_from, seed_recording, SeedSegment, TestApp, SAME_ORIGIN,
};

/// A few hours back, on a ten second boundary, so every test sits well inside the retention window.
fn start_ms() -> i64 {
    let now = crate::util::time::now_ms();
    (now - 3 * 3_600_000) / 10_000 * 10_000
}

/// Two minutes of recording, a two minute gap, then another minute, with a sound in each part.
fn seeded(name: &str) -> (TestApp, i64) {
    let app = app(name);
    let t0 = start_ms();
    seed_recording(
        &app,
        &[
            SeedSegment {
                start_ms: t0,
                seconds: 60,
                level: 1,
                loud: Some((20_000, 3_000, 90)),
            },
            SeedSegment {
                start_ms: t0 + 60_000,
                seconds: 60,
                level: 1,
                loud: None,
            },
            SeedSegment {
                start_ms: t0 + 240_000,
                seconds: 60,
                level: 1,
                loud: Some((30_000, 2_000, 120)),
            },
        ],
    );
    (app, t0)
}

async fn get(app: &TestApp, path: &str) -> (StatusCode, Value) {
    let reply = call(app, Method::GET, path, None, None).await;
    (reply.status, reply.body)
}

#[tokio::test]
async fn the_range_is_empty_before_anything_is_recorded() {
    let app = app("timeline-empty-range");
    let (status, body) = get(&app, "/api/timeline/range").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["earliestMs"], Value::Null);
    assert_eq!(body["latestMs"], Value::Null);
    assert_eq!(body["coverage"], json!([]));
    assert!(body["serverTimeMs"].as_i64().is_some());
}

#[tokio::test]
async fn the_range_spans_the_recordings_with_the_gap_left_as_a_hole() {
    let (app, t0) = seeded("timeline-range");
    let (status, body) = get(&app, "/api/timeline/range").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["earliestMs"], t0);
    assert_eq!(body["latestMs"], t0 + 300_000);
    assert_eq!(
        body["coverage"],
        json!([
            { "startMs": t0, "endMs": t0 + 120_000 },
            { "startMs": t0 + 240_000, "endMs": t0 + 300_000 },
        ]),
    );
}

#[tokio::test]
async fn the_days_list_names_each_day_with_its_recorded_extent_and_midnights() {
    let (app, t0) = seeded("timeline-days");
    let (status, body) = get(&app, "/api/timeline/days").await;
    assert_eq!(status, StatusCode::OK);
    let days = body["days"].as_array().expect("days");
    let day = crate::util::day::local_day(t0);
    let (midnight, next_midnight) = crate::util::day::day_bounds_ms(&day).expect("bounds");
    // The recording may straddle midnight in some time zones; the first day always holds its start.
    let first = days
        .iter()
        .find(|entry| entry["day"] == day.as_str())
        .expect("the day it started on");
    assert_eq!(first["startMs"], t0);
    assert_eq!(first["dayStartMs"], midnight);
    assert_eq!(first["dayEndMs"], next_midnight);
    assert!(first["endMs"].as_i64().expect("end") <= t0 + 300_000);
}

#[tokio::test]
async fn peaks_come_back_on_the_requested_columns_zero_where_nothing_was_recorded() {
    let (app, t0) = seeded("timeline-peaks");
    let (status, body) = get(
        &app,
        &format!(
            "/api/timeline/peaks?fromMs={t0}&toMs={}&buckets=30",
            t0 + 300_000
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["fromMs"], t0);
    assert_eq!(body["toMs"], t0 + 300_000);
    assert_eq!(body["bucketMs"], 10_000);
    let peaks: Vec<i64> = body["peaks"]
        .as_array()
        .expect("peaks")
        .iter()
        .map(|value| value.as_i64().expect("number"))
        .collect();
    assert_eq!(peaks.len(), 30);
    assert_eq!(peaks[2], 90, "the loud stretch at 20 to 23 s");
    assert!(
        peaks[12..24].iter().all(|value| *value == 0),
        "the gap: {peaks:?}"
    );
    assert_eq!(peaks[27], 120, "the loud stretch after the gap");
}

#[tokio::test]
async fn peaks_default_to_a_thousand_columns_and_clamp_what_is_asked_for() {
    let (app, t0) = seeded("timeline-peaks-columns");
    let window = format!("fromMs={t0}&toMs={}", t0 + 300_000);
    let count = |body: &Value| body["peaks"].as_array().map(Vec::len);

    let (_, default) = get(&app, &format!("/api/timeline/peaks?{window}")).await;
    assert_eq!(count(&default), Some(1000));
    let (_, few) = get(&app, &format!("/api/timeline/peaks?{window}&buckets=1")).await;
    assert_eq!(count(&few), Some(16));
    let (_, many) = get(
        &app,
        &format!("/api/timeline/peaks?{window}&buckets=100000"),
    )
    .await;
    assert_eq!(count(&many), Some(4000));
}

#[tokio::test]
async fn peaks_refuse_an_inverted_a_too_wide_or_a_malformed_window() {
    let app = app("timeline-peaks-refusals");
    let (inverted, body) = get(&app, "/api/timeline/peaks?fromMs=2000&toMs=1000").await;
    assert_eq!(inverted, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "bad_request");

    let too_wide = 33_i64 * 24 * 3_600_000;
    let (wide, body) = get(
        &app,
        &format!("/api/timeline/peaks?fromMs=0&toMs={too_wide}"),
    )
    .await;
    assert_eq!(wide, StatusCode::BAD_REQUEST);
    assert!(body["error"]["message"]
        .as_str()
        .unwrap_or("")
        .contains("32 days"));

    let (malformed, _) = get(&app, "/api/timeline/peaks?fromMs=soon&toMs=later").await;
    assert_eq!(malformed, StatusCode::BAD_REQUEST);
    let (missing, _) = get(&app, "/api/timeline/peaks").await;
    assert_eq!(missing, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn sounds_come_back_with_the_fields_the_timeline_draws_and_seeks_by() {
    let (app, t0) = seeded("timeline-sounds");
    let (status, body) = get(
        &app,
        &format!("/api/timeline/sounds?fromMs={t0}&toMs={}", t0 + 300_000),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["fromMs"], t0);
    assert_eq!(body["toMs"], t0 + 300_000);
    assert_eq!(body["sensitivity"], "medium");
    assert_eq!(
        body["sounds"],
        json!([
            { "startMs": t0 + 20_000, "endMs": t0 + 23_000, "seekMs": t0 + 19_000, "peak": 90 },
            { "startMs": t0 + 270_000, "endMs": t0 + 272_000, "seekMs": t0 + 269_000, "peak": 120 },
        ]),
    );
}

#[tokio::test]
async fn sounds_follow_the_sensitivity_setting_and_say_which_they_used() {
    let (app, t0) = seeded("timeline-sounds-setting");
    let changed = call_from(
        &app,
        Method::PATCH,
        "/api/settings",
        None,
        Some(json!({ "soundSensitivity": "low" })),
        Some(SAME_ORIGIN),
    )
    .await;
    assert_eq!(changed.status, StatusCode::OK);
    let (_, body) = get(
        &app,
        &format!("/api/timeline/sounds?fromMs={t0}&toMs={}", t0 + 300_000),
    )
    .await;
    assert_eq!(body["sensitivity"], "low");
    // Both are loud enough for even the least sensitive setting.
    assert_eq!(body["sounds"].as_array().map(Vec::len), Some(2));
}

#[tokio::test]
async fn sounds_refuse_an_inverted_or_a_too_wide_window() {
    let app = app("timeline-sounds-refusals");
    let (inverted, _) = get(&app, "/api/timeline/sounds?fromMs=2000&toMs=1000").await;
    assert_eq!(inverted, StatusCode::BAD_REQUEST);
    let too_wide = 33_i64 * 24 * 3_600_000;
    let (wide, _) = get(
        &app,
        &format!("/api/timeline/sounds?fromMs=0&toMs={too_wide}"),
    )
    .await;
    assert_eq!(wide, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn next_sound_walks_forward_and_back_across_the_gap_and_says_when_there_is_none() {
    let (app, t0) = seeded("timeline-next");
    let next = |from: i64, direction: &str| {
        let path = format!("/api/timeline/sounds/next?fromMs={from}&direction={direction}");
        let app = &app;
        async move { get(app, &path).await }
    };

    let (status, first) = next(t0, "forward").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["sound"]["startMs"], t0 + 20_000);
    let (_, second) = next(t0 + 19_300, "forward").await;
    assert_eq!(second["sound"]["startMs"], t0 + 270_000, "across the gap");
    let (_, none) = next(t0 + 269_300, "forward").await;
    assert_eq!(none["sound"], Value::Null);

    let (_, back) = next(t0 + 269_500, "backward").await;
    assert_eq!(back["sound"]["startMs"], t0 + 20_000);
    let (_, before_first) = next(t0 + 19_500, "backward").await;
    assert_eq!(before_first["sound"], Value::Null);
}

#[tokio::test]
async fn next_sound_looks_forward_unless_told_otherwise_and_refuses_an_unknown_direction() {
    let (app, t0) = seeded("timeline-next-direction");
    let (status, body) = get(&app, &format!("/api/timeline/sounds/next?fromMs={t0}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sound"]["startMs"], t0 + 20_000);

    let (sideways, _) = get(
        &app,
        &format!("/api/timeline/sounds/next?fromMs={t0}&direction=sideways"),
    )
    .await;
    assert_eq!(sideways, StatusCode::BAD_REQUEST);
    let (missing, _) = get(&app, "/api/timeline/sounds/next").await;
    assert_eq!(missing, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn next_sound_on_an_empty_recorder_finds_nothing_either_way() {
    let app = app("timeline-next-empty");
    for direction in ["forward", "backward"] {
        let (status, body) = get(
            &app,
            &format!("/api/timeline/sounds/next?fromMs=0&direction={direction}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{direction}");
        assert_eq!(body["sound"], Value::Null, "{direction}");
    }
}

#[tokio::test]
async fn extreme_moments_are_refused_or_answered_never_a_crash() {
    let (app, _) = seeded("timeline-extremes");
    let (min, max) = (i64::MIN, i64::MAX);
    // Refused: windows whose width overflows a subtraction, which in a release build would wrap to a
    // negative width and slip under the 32 day limit into a scan of every segment.
    for path in [
        format!("/api/timeline/peaks?fromMs={min}&toMs={max}"),
        "/api/timeline/peaks?fromMs=-9000000000000000000&toMs=9000000000000000000".to_string(),
        format!("/api/timeline/sounds?fromMs={min}&toMs={max}"),
        "/api/timeline/sounds?fromMs=-9000000000000000000&toMs=9000000000000000000".to_string(),
    ] {
        let (status, body) = get(&app, &path).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {body}");
    }
    // Answered: a single moment at either end of time is just somewhere with nothing recorded.
    for path in [
        format!("/api/timeline/sounds/next?fromMs={min}"),
        format!("/api/timeline/sounds/next?fromMs={max}"),
        format!("/api/timeline/sounds/next?fromMs={min}&direction=previous"),
        format!("/api/timeline/sounds/next?fromMs={max}&direction=previous"),
    ] {
        let (status, body) = get(&app, &path).await;
        assert!(
            status.is_success() || status.is_client_error(),
            "{path}: {status} {body}"
        );
    }
    for path in [
        format!("/api/export/plan?fromMs={min}&toMs={max}"),
        format!("/api/export/plan?fromMs={}&toMs={max}", max - 1),
    ] {
        let (status, body) = get(&app, &path).await;
        assert!(status.is_client_error(), "{path}: {status} {body}");
    }
}
