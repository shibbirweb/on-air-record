//! Exporting audio over HTTP: the plan the export dialog shows, and the WAV file the download produces,
//! read back byte by byte from seeded recordings on disk.
//!
//! The export service's own tests cover the arithmetic. These cover what only the router shows: the
//! query parsing, the headers a browser needs to save the file with a sensible name and a progress bar,
//! and a streamed body that is exactly as long as `Content-Length` promised.

use axum::http::header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{Method, StatusCode};
use serde_json::Value;

use super::test_support::{app, call, get_raw, seed_recording, SeedSegment, TestApp};

const RATE: i64 = 48_000;

/// A minute of recording at a steady level, a minute's gap, then another minute louder.
fn seeded(name: &str) -> (TestApp, i64) {
    let app = app(name);
    let t0 = (crate::util::time::now_ms() - 3 * 3_600_000) / 10_000 * 10_000;
    seed_recording(
        &app,
        &[
            SeedSegment {
                start_ms: t0,
                seconds: 60,
                level: 10,
                loud: None,
            },
            SeedSegment {
                start_ms: t0 + 120_000,
                seconds: 60,
                level: 40,
                loud: None,
            },
        ],
    );
    (app, t0)
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// The magnitude of the sample `seconds` into the exported file.
fn sample_at(wav: &[u8], seconds: f64) -> i16 {
    let at = 44 + (seconds * RATE as f64) as usize * 2;
    i16::from_le_bytes([wav[at], wav[at + 1]]).abs()
}

#[tokio::test]
async fn the_plan_says_what_the_file_will_hold() {
    let (app, t0) = seeded("export-plan");
    let reply = call(
        &app,
        Method::GET,
        &format!(
            "/api/export/plan?fromMs={}&toMs={}",
            t0 + 10_000,
            t0 + 30_000
        ),
        None,
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    let plan = reply.body;
    assert_eq!(plan["fromMs"], t0 + 10_000);
    assert_eq!(plan["toMs"], t0 + 30_000);
    assert_eq!(plan["durationMs"], 20_000);
    assert_eq!(plan["sampleRate"], RATE);
    assert_eq!(plan["channels"], 1);
    assert_eq!(plan["totalBytes"], 44 + 20 * RATE * 2);
    assert_eq!(plan["mixedRates"], false);
}

#[tokio::test]
async fn the_plan_covers_a_gap_in_full_because_a_gap_is_exported_as_silence() {
    let (app, t0) = seeded("export-plan-gap");
    let reply = call(
        &app,
        Method::GET,
        &format!("/api/export/plan?fromMs={t0}&toMs={}", t0 + 180_000),
        None,
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body["durationMs"], 180_000);
    assert_eq!(reply.body["totalBytes"], 44 + 180 * RATE * 2);
}

#[tokio::test]
async fn the_plan_refuses_an_inverted_range_and_one_with_no_recording_in_it() {
    let (app, t0) = seeded("export-plan-refusals");
    let refused = |path: String| {
        let app = &app;
        async move { call(app, Method::GET, &path, None, None).await }
    };

    let inverted = refused(format!("/api/export/plan?fromMs={}&toMs={t0}", t0 + 1_000)).await;
    assert_eq!(inverted.status, StatusCode::BAD_REQUEST);
    assert_eq!(inverted.body["error"]["code"], "bad_request");

    let empty = refused(format!(
        "/api/export/plan?fromMs={}&toMs={}",
        t0 + 61_000,
        t0 + 119_000
    ))
    .await;
    assert_eq!(
        empty.status,
        StatusCode::NOT_FOUND,
        "the gap alone holds nothing to export"
    );
    assert_eq!(empty.body["error"]["code"], "not_found");

    let missing = refused("/api/export/plan".to_string()).await;
    assert_eq!(missing.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_download_is_a_wav_file_with_a_name_and_a_length_a_browser_can_use() {
    let (app, t0) = seeded("export-download-headers");
    let reply = get_raw(
        &app,
        &format!("/api/export?fromMs={}&toMs={}", t0 + 10_000, t0 + 12_000),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.headers[CONTENT_TYPE], "audio/wav");
    let expected = 44 + 2 * RATE as usize * 2;
    assert_eq!(reply.headers[CONTENT_LENGTH], expected.to_string().as_str());
    assert_eq!(
        reply.bytes.len(),
        expected,
        "the body is exactly as long as promised"
    );
    let disposition = reply.headers[CONTENT_DISPOSITION].to_str().expect("ascii");
    assert!(
        disposition.starts_with("attachment; filename=\""),
        "{disposition}"
    );
    assert!(disposition.ends_with(".wav\""), "{disposition}");
}

#[tokio::test]
async fn the_file_is_a_valid_16_bit_mono_wav_at_the_recording_rate() {
    let (app, t0) = seeded("export-download-header");
    let wav = get_raw(
        &app,
        &format!("/api/export?fromMs={t0}&toMs={}", t0 + 1_000),
    )
    .await
    .bytes;
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(u32_at(&wav, 4) as usize, wav.len() - 8);
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(&wav[12..16], b"fmt ");
    assert_eq!(u16_at(&wav, 20), 1, "PCM");
    assert_eq!(u16_at(&wav, 22), 1, "mono");
    assert_eq!(u32_at(&wav, 24), RATE as u32);
    assert_eq!(u32_at(&wav, 28), RATE as u32 * 2, "byte rate");
    assert_eq!(u16_at(&wav, 34), 16, "bits per sample");
    assert_eq!(&wav[36..40], b"data");
    assert_eq!(u32_at(&wav, 40) as usize, wav.len() - 44);
}

#[tokio::test]
async fn the_audio_is_the_recording_with_the_gap_as_silence_and_in_the_right_place() {
    let (app, t0) = seeded("export-download-audio");
    let wav = get_raw(
        &app,
        &format!("/api/export?fromMs={}&toMs={}", t0 + 50_000, t0 + 130_000),
    )
    .await
    .bytes;
    assert_eq!(wav.len(), 44 + 80 * RATE as usize * 2);
    // The seeded square waves have amplitudes of level / 255 of full scale.
    let quiet = (10.0 / 255.0 * f64::from(i16::MAX)) as i16;
    let loud = (40.0 / 255.0 * f64::from(i16::MAX)) as i16;
    assert_eq!(sample_at(&wav, 5.0), quiet, "ten seconds before the gap");
    assert_eq!(sample_at(&wav, 40.0), 0, "the middle of the gap");
    assert_eq!(
        sample_at(&wav, 75.0),
        loud,
        "after the gap, where the second part is"
    );
}

#[tokio::test]
async fn the_download_refuses_what_the_plan_refuses_with_an_error_rather_than_a_broken_file() {
    let (app, t0) = seeded("export-download-refusals");
    let empty = get_raw(
        &app,
        &format!("/api/export?fromMs={}&toMs={}", t0 + 61_000, t0 + 119_000),
    )
    .await;
    assert_eq!(empty.status, StatusCode::NOT_FOUND);
    let body: Value = serde_json::from_slice(&empty.bytes).expect("a JSON error");
    assert_eq!(body["error"]["code"], "not_found");

    let inverted = get_raw(
        &app,
        &format!("/api/export?fromMs={}&toMs={t0}", t0 + 1_000),
    )
    .await;
    assert_eq!(inverted.status, StatusCode::BAD_REQUEST);
}
