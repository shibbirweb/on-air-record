//! The audio stream over a real WebSocket: the application bound to a port on the loopback interface and
//! a real client talking to it, frame by frame and message by message.
//!
//! The pieces have their own tests (the frame layout, the control messages, the playback cursor on real
//! PCM), but the session loop that ties them together is only proven here: that live frames reach every
//! listener in order, that a seek plays the recording from that moment marked as recorded, that playback
//! is paced to the chosen speed, that it hands over to the live feed or stops when the disk runs out, and
//! that a listener who stops reading holds nobody else up.
//!
//! Live audio is published straight into the broadcast hub, which is exactly what the recorder does, so no
//! microphone is needed. Recorded audio is seeded as real PCM files with matching index rows. The access
//! re-check and the idle timeout run on a 15 second beat, so those tests pause Tokio's clock and let it
//! jump ahead rather than waiting.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use axum::http::{Method, StatusCode};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::test_support::{app, call, seed_recording, set_up_admin, SeedSegment, TestApp};
use crate::models::AudioFrame;
use crate::ws::protocol::{decode_header, FrameHeader, HEADER_LEN};

type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How long any one expected message may take. Generous, because a loaded CI runner is slow, and a test
/// only ever waits this long when it is already failing.
const WAIT: Duration = Duration::from_secs(10);

/// Samples in one 100 ms frame at the rate everything here records at.
const FRAME_SAMPLES: usize = 4_800;

/// The application serving on a real port, stopped when dropped.
struct Server {
    app: TestApp,
    address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl Server {
    /// The Origin the service's own page would send, which the guard insists on for the stream.
    fn origin(&self) -> String {
        format!("http://{}", self.address)
    }

    fn state(&self) -> &std::sync::Arc<crate::app::AppState> {
        self.app.state()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(app: TestApp) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a free port");
    let address = listener.local_addr().expect("the bound address");
    // With the connection's address, as `main` serves it, since the listener list shows where each
    // listener connects from.
    let service = app
        .router
        .clone()
        .into_make_service_with_connect_info::<SocketAddr>();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, service).await;
    });
    Server { app, address, task }
}

/// Open the stream as a browser would, or say why it was refused.
async fn open(
    server: &Server,
    cookie: Option<&str>,
    origin: Option<&str>,
) -> Result<Client, WsError> {
    let mut request = format!("ws://{}/api/ws/stream", server.address)
        .into_client_request()
        .expect("a request");
    let headers = request.headers_mut();
    if let Some(origin) = origin {
        headers.insert("origin", HeaderValue::from_str(origin).expect("origin"));
    }
    if let Some(token) = cookie {
        headers.insert(
            "cookie",
            HeaderValue::from_str(&format!("oar_session={token}")).expect("cookie"),
        );
    }
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(client, _)| client)
}

async fn connect(server: &Server, cookie: Option<&str>) -> Client {
    open(server, cookie, Some(&server.origin()))
        .await
        .expect("the stream opens")
}

/// The status a refused handshake was answered with.
async fn refusal(server: &Server, cookie: Option<&str>, origin: Option<&str>) -> StatusCode {
    match open(server, cookie, origin).await {
        Err(WsError::Http(response)) => response.status(),
        Err(other) => panic!("refused, but not over HTTP: {other}"),
        Ok(_) => panic!("the stream opened"),
    }
}

/// One thing the server said.
#[derive(Debug)]
enum Heard {
    Control(Value),
    Audio(Audio),
    Closed,
}

#[derive(Debug)]
struct Audio {
    header: FrameHeader,
    samples: Vec<i16>,
}

/// The next message, pings and pongs aside, or `Closed` when the server hung up.
async fn next(client: &mut Client, wait: Duration) -> Heard {
    loop {
        let message = tokio::time::timeout(wait, client.next())
            .await
            .expect("the server said something in time");
        match message {
            None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return Heard::Closed,
            Some(Ok(Message::Text(text))) => {
                return Heard::Control(serde_json::from_str(text.as_str()).expect("JSON control"));
            }
            Some(Ok(Message::Binary(bytes))) => {
                let header = decode_header(&bytes).expect("an audio frame header");
                let samples = bytes[HEADER_LEN..]
                    .chunks_exact(2)
                    .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                    .collect();
                return Heard::Audio(Audio { header, samples });
            }
            Some(Ok(_)) => {}
        }
    }
}

/// Read on until a control message of this type, passing over audio and other messages.
async fn until_control(client: &mut Client, kind: &str) -> Value {
    loop {
        match next(client, WAIT).await {
            Heard::Control(message) if message["type"] == kind => return message,
            Heard::Control(_) | Heard::Audio(_) => {}
            Heard::Closed => panic!("the stream closed while waiting for {kind}"),
        }
    }
}

/// Read on until the next audio frame, passing over control messages.
async fn audio(client: &mut Client) -> Audio {
    loop {
        match next(client, WAIT).await {
            Heard::Audio(frame) => return frame,
            Heard::Control(_) => {}
            Heard::Closed => panic!("the stream closed while waiting for audio"),
        }
    }
}

/// Whether no audio arrives for `quiet`, reading (and discarding) any control messages meanwhile.
async fn hears_no_audio_for(client: &mut Client, quiet: Duration) -> bool {
    let waited = tokio::time::timeout(quiet, async {
        loop {
            match next(client, WAIT).await {
                Heard::Audio(_) => return false,
                Heard::Closed => return true,
                Heard::Control(_) => {}
            }
        }
    })
    .await;
    waited.unwrap_or(true)
}

/// Every control message that arrives within `quiet`, audio aside.
async fn controls_within(client: &mut Client, quiet: Duration) -> Vec<Value> {
    let mut seen = Vec::new();
    let _ = tokio::time::timeout(quiet, async {
        loop {
            match next(client, WAIT).await {
                Heard::Control(message) => seen.push(message),
                Heard::Audio(_) => {}
                Heard::Closed => return,
            }
        }
    })
    .await;
    seen
}

async fn send(client: &mut Client, message: Value) {
    client
        .send(Message::Text(message.to_string().into()))
        .await
        .expect("the message is sent");
}

/// Read the greeting a new stream starts with, and return its `stream-info`.
async fn greeting(client: &mut Client) -> Value {
    match next(client, WAIT).await {
        Heard::Control(message) => {
            assert_eq!(message["type"], "stream-info", "the first word: {message}");
            message
        }
        other => panic!("expected the stream info first, heard {other:?}"),
    }
}

/// A live frame of a steady level, as the recorder publishes it.
fn live_frame(timestamp_ms: i64, level: i16) -> AudioFrame {
    AudioFrame::from_samples(timestamp_ms, 48_000, 1, vec![level; FRAME_SAMPLES], true)
}

/// A start time three hours ago on a ten second boundary, well inside the retention window.
fn recent_start() -> i64 {
    (crate::util::time::now_ms() - 3 * 3_600_000) / 10_000 * 10_000
}

/// The magnitude of every sample of a seeded segment at `level`, which is a square wave of that height.
fn seeded_amplitude(level: u8) -> i16 {
    (f64::from(level) / 255.0 * f64::from(i16::MAX)) as i16
}

fn steady(start_ms: i64, seconds: i64, level: u8) -> SeedSegment {
    SeedSegment {
        start_ms,
        seconds,
        level,
        loud: None,
    }
}

// Opening the stream.

#[tokio::test]
async fn a_new_stream_describes_itself_then_lists_who_is_listening_on_an_open_recorder() {
    let server = serve(app("stream-greeting")).await;
    let mut client = connect(&server, None).await;

    let info = greeting(&mut client).await;
    assert_eq!(info["mode"], "live");
    assert_eq!(info["frameMs"], 100);
    assert_eq!(
        info["sampleRate"], 48_000,
        "the format the next capture will most likely use, never zero"
    );
    assert_eq!(info["channels"], 1);
    assert_eq!(info["capturing"], false);
    assert_eq!(info["earliestMs"], Value::Null, "nothing is recorded yet");

    let listeners = until_control(&mut client, "listeners").await;
    let entries = listeners["listeners"].as_array().expect("a list");
    assert_eq!(entries.len(), 1, "only this stream: {listeners}");
    assert_eq!(entries[0]["activity"], "live");
    // Counted as heard until the page says otherwise, so a client that never reports is not hidden.
    assert_eq!(entries[0]["player"], "playing");
    assert_eq!(entries[0]["address"], "127.0.0.1");
    assert_eq!(entries[0]["email"], Value::Null, "a guest");
}

#[tokio::test]
async fn the_stream_says_how_far_back_the_recordings_reach() {
    let server = serve(app("stream-greeting-earliest")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 10, 30)]);
    let mut client = connect(&server, None).await;

    let info = greeting(&mut client).await;
    assert_eq!(info["earliestMs"], t0);
    assert_eq!(
        info["liveEdgeMs"],
        t0 + 10_000,
        "the end of the newest recording"
    );
}

#[tokio::test]
async fn another_website_cannot_open_the_stream_in_any_mode() {
    let server = serve(app("stream-origin")).await;
    assert_eq!(
        refusal(&server, None, Some("http://elsewhere.example")).await,
        StatusCode::FORBIDDEN
    );
    let admin = set_up_admin(&server.app).await;
    assert_eq!(
        refusal(&server, Some(&admin), Some("http://elsewhere.example")).await,
        StatusCode::FORBIDDEN,
        "not even with a valid session, which the other site's page would be sending"
    );
}

#[tokio::test]
async fn with_accounts_the_stream_needs_somebody_signed_in() {
    let server = serve(app("stream-sign-in")).await;
    let admin = set_up_admin(&server.app).await;
    let origin = server.origin();
    assert_eq!(
        refusal(&server, None, Some(&origin)).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        refusal(&server, Some("not-a-session"), Some(&origin)).await,
        StatusCode::UNAUTHORIZED
    );

    let mut client = connect(&server, Some(&admin)).await;
    greeting(&mut client).await;
}

// The live feed.

#[tokio::test]
async fn a_live_frame_reaches_the_listener_as_live_audio_followed_by_its_level() {
    let server = serve(app("stream-live-frame")).await;
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    let frame = live_frame(1_757_034_000_000, 8_000);
    let (rms, peak) = (frame.rms, frame.peak);
    server.state().hub.publish(frame);

    let heard = audio(&mut client).await;
    assert!(heard.header.is_live());
    assert_eq!(heard.header.timestamp_ms, 1_757_034_000_000);
    assert_eq!(heard.header.sample_rate, 48_000);
    assert_eq!(heard.header.channels, 1);
    assert_eq!(heard.header.sample_count as usize, FRAME_SAMPLES);
    assert_eq!(
        heard.samples,
        vec![8_000; FRAME_SAMPLES],
        "the samples as captured"
    );

    let level = until_control(&mut client, "level").await;
    assert!((level["rms"].as_f64().expect("rms") - f64::from(rms)).abs() < 1e-6);
    assert!((level["peak"].as_f64().expect("peak") - f64::from(peak)).abs() < 1e-6);
}

#[tokio::test]
async fn every_listener_hears_every_live_frame_in_order() {
    const LISTENERS: usize = 20;
    const FRAMES: i64 = 40;
    let server = serve(app("stream-many-listeners")).await;
    let mut clients = Vec::new();
    for _ in 0..LISTENERS {
        let mut client = connect(&server, None).await;
        greeting(&mut client).await;
        clients.push(client);
    }

    let t0 = 1_757_034_000_000;
    for index in 0..FRAMES {
        server
            .state()
            .hub
            .publish(live_frame(t0 + index * 100, 1_000));
        // Well inside what a listener drains in real time, so nobody lags and every frame is due.
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    for (number, client) in clients.iter_mut().enumerate() {
        for index in 0..FRAMES {
            let heard = audio(client).await;
            assert_eq!(
                heard.header.timestamp_ms,
                t0 + index * 100,
                "listener {number} frame {index}"
            );
        }
    }
    assert_eq!(server.state().hub.listener_count(), LISTENERS);
}

#[tokio::test]
async fn a_listener_who_stops_reading_holds_up_neither_the_recorder_nor_anybody_else() {
    // One second frames, so the stuck listener's socket buffers fill after a few dozen of them.
    const SAMPLES: usize = 48_000;
    const FRAMES: i64 = 300;
    let server = serve(app("stream-slow-listener")).await;
    let mut stuck = connect(&server, None).await;
    greeting(&mut stuck).await;
    let mut healthy = connect(&server, None).await;
    greeting(&mut healthy).await;

    let t0 = 1_757_034_000_000;
    let started = Instant::now();
    for index in 0..FRAMES {
        let frame =
            AudioFrame::from_samples(t0 + index * 1_000, 48_000, 1, vec![500; SAMPLES], true);
        // Publishing never waits on a listener: this is the recorder's side of the hub.
        server.state().hub.publish(frame);
        // In lock step with the healthy listener, which must get every single frame, in order, while
        // the other one reads nothing at all.
        let heard = audio(&mut healthy).await;
        assert_eq!(heard.header.timestamp_ms, t0 + index * 1_000);
    }
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "the healthy listener was never held up: {:?}",
        started.elapsed()
    );

    // Far more was published than a socket holds, so the stuck listener fell behind and was moved on to
    // the newest audio rather than being handed all of it late.
    let mut caught_up = 0;
    loop {
        let heard = audio(&mut stuck).await;
        caught_up += 1;
        if heard.header.timestamp_ms == t0 + (FRAMES - 1) * 1_000 {
            break;
        }
    }
    assert!(
        caught_up < FRAMES,
        "the stuck listener skipped ahead instead of replaying everything: heard {caught_up}"
    );
}

#[tokio::test]
async fn pausing_the_live_feed_stops_its_audio_and_resuming_rejoins_it() {
    let server = serve(app("stream-live-pause")).await;
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    send(&mut client, json!({ "type": "pause" })).await;
    let paused = until_control(&mut client, "mode").await;
    assert_eq!(paused["mode"], "paused");

    server
        .state()
        .hub
        .publish(live_frame(1_757_034_000_000, 1_000));
    assert!(
        hears_no_audio_for(&mut client, Duration::from_millis(400)).await,
        "no live audio while paused"
    );

    send(&mut client, json!({ "type": "resume" })).await;
    let resumed = until_control(&mut client, "mode").await;
    assert_eq!(resumed["mode"], "live");
    server
        .state()
        .hub
        .publish(live_frame(1_757_034_000_100, 1_000));
    assert_eq!(
        audio(&mut client).await.header.timestamp_ms,
        1_757_034_000_100
    );
}

// Playing back the recording.

#[tokio::test]
async fn a_seek_plays_the_recording_from_that_moment_marked_as_recorded() {
    let server = serve(app("stream-seek")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 60, 40)]);
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    send(
        &mut client,
        json!({ "type": "seek", "timestampMs": t0 + 10_000 }),
    )
    .await;
    let mode = until_control(&mut client, "mode").await;
    assert_eq!(mode["mode"], "playback");
    assert_eq!(mode["positionMs"], t0 + 10_000);

    let amplitude = seeded_amplitude(40);
    for index in 0..6 {
        let heard = audio(&mut client).await;
        assert!(!heard.header.is_live(), "flagged as recorded");
        assert_eq!(heard.header.timestamp_ms, t0 + 10_000 + index * 100);
        assert_eq!(heard.header.sample_rate, 48_000);
        assert_eq!(heard.samples.len(), FRAME_SAMPLES);
        assert!(
            heard.samples.iter().all(|sample| sample.abs() == amplitude),
            "the recorded audio itself, frame {index}"
        );
    }
}

#[tokio::test]
async fn a_seek_shows_up_in_the_listener_list_as_playback_from_that_moment() {
    let server = serve(app("stream-seek-listed")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 60, 40)]);
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;
    until_control(&mut client, "listeners").await;

    send(
        &mut client,
        json!({ "type": "seek", "timestampMs": t0 + 5_000 }),
    )
    .await;
    let listed = loop {
        let listeners = until_control(&mut client, "listeners").await;
        if listeners["listeners"][0]["activity"] == "playback" {
            break listeners;
        }
    };
    // Reported after the three frames sent at once to fill the page's buffer, so up to 300 ms on.
    let from_ms = listed["listeners"][0]["fromMs"].as_i64().expect("fromMs");
    assert!(
        (t0 + 5_000..=t0 + 5_300).contains(&from_ms),
        "where the listener jumped to: {from_ms} against {}",
        t0 + 5_000
    );
}

#[tokio::test]
async fn playback_is_paced_to_real_time_and_a_faster_speed_paces_it_faster() {
    const PACED: i64 = 15;
    let server = serve(app("stream-pacing")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 60, 20)]);

    let time_to_hear = |speed: f64| {
        let server = &server;
        async move {
            let mut client = connect(server, None).await;
            greeting(&mut client).await;
            send(&mut client, json!({ "type": "speed", "value": speed })).await;
            let applied = until_control(&mut client, "speed").await;
            assert_eq!(applied["value"].as_f64(), Some(speed));
            send(&mut client, json!({ "type": "seek", "timestampMs": t0 })).await;
            // The first three arrive at once to fill the browser's jitter buffer; the rest are paced.
            for _ in 0..3 {
                audio(&mut client).await;
            }
            let started = Instant::now();
            for _ in 0..PACED {
                audio(&mut client).await;
            }
            started.elapsed()
        }
    };

    let real_time = time_to_hear(1.0).await;
    assert!(
        real_time >= Duration::from_millis(1_300),
        "15 frames of 100 ms at real time take about 1.5 s, not {real_time:?}"
    );
    let quadruple = time_to_hear(4.0).await;
    assert!(
        quadruple < real_time / 2,
        "four times real time is paced faster: {quadruple:?} against {real_time:?}"
    );
}

#[tokio::test]
async fn a_requested_speed_snaps_to_the_nearest_one_offered() {
    let server = serve(app("stream-speed-snap")).await;
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    for (asked, applied) in [(100.0, 4.0), (0.3, 0.25), (-2.0, 0.25), (1.4, 1.5)] {
        send(&mut client, json!({ "type": "speed", "value": asked })).await;
        let reply = until_control(&mut client, "speed").await;
        assert_eq!(reply["value"].as_f64(), Some(applied), "asked for {asked}");
    }
}

#[tokio::test]
async fn a_gap_in_the_recording_is_announced_then_skipped() {
    let server = serve(app("stream-gap")).await;
    let t0 = recent_start();
    seed_recording(
        &server.app,
        &[steady(t0, 10, 20), steady(t0 + 20_000, 10, 60)],
    );
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    send(&mut client, json!({ "type": "speed", "value": 4.0 })).await;
    until_control(&mut client, "speed").await;
    send(
        &mut client,
        json!({ "type": "seek", "timestampMs": t0 + 9_700 }),
    )
    .await;
    for index in 0..3 {
        assert_eq!(
            audio(&mut client).await.header.timestamp_ms,
            t0 + 9_700 + index * 100
        );
    }

    let gap = until_control(&mut client, "gap").await;
    assert_eq!(gap["fromMs"], t0 + 10_000);
    assert_eq!(gap["toMs"], t0 + 20_000);
    let after = audio(&mut client).await;
    assert_eq!(
        after.header.timestamp_ms,
        t0 + 20_000,
        "straight on to the next recording"
    );
    assert!(after
        .samples
        .iter()
        .all(|sample| sample.abs() == seeded_amplitude(60)));
}

#[tokio::test]
async fn a_seek_to_before_the_first_recording_skips_ahead_to_it() {
    let server = serve(app("stream-seek-early")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 10, 20)]);
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    send(
        &mut client,
        json!({ "type": "seek", "timestampMs": t0 - 5_000 }),
    )
    .await;
    let gap = until_control(&mut client, "gap").await;
    assert_eq!(gap["fromMs"], t0 - 5_000);
    assert_eq!(gap["toMs"], t0);
    assert_eq!(audio(&mut client).await.header.timestamp_ms, t0);
}

#[tokio::test]
async fn pausing_playback_stops_it_and_resuming_carries_on_from_the_same_moment() {
    let server = serve(app("stream-playback-pause")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 60, 20)]);
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    send(&mut client, json!({ "type": "seek", "timestampMs": t0 })).await;
    let mut last = 0;
    for _ in 0..5 {
        last = audio(&mut client).await.header.timestamp_ms;
    }

    send(&mut client, json!({ "type": "pause" })).await;
    let paused = until_control(&mut client, "mode").await;
    assert_eq!(paused["mode"], "paused");
    let position = paused["positionMs"].as_i64().expect("a position");
    assert!(position > last, "the cursor stands just past what was sent");

    // Frames sent before the pause took effect may still be in flight; after that, silence.
    let mut in_flight = Vec::new();
    while let Ok(Heard::Audio(frame)) =
        tokio::time::timeout(Duration::from_millis(200), next(&mut client, WAIT)).await
    {
        in_flight.push(frame.header.timestamp_ms);
    }
    assert!(
        hears_no_audio_for(&mut client, Duration::from_millis(400)).await,
        "no audio while paused"
    );

    send(&mut client, json!({ "type": "resume" })).await;
    let resumed = until_control(&mut client, "mode").await;
    assert_eq!(
        resumed["mode"], "playback",
        "back to the recording, not to live"
    );
    assert_eq!(resumed["positionMs"], position);
    assert_eq!(
        audio(&mut client).await.header.timestamp_ms,
        position,
        "picks up exactly where it stopped"
    );
}

#[tokio::test]
async fn when_playback_runs_out_with_nothing_recording_it_says_so_and_stops() {
    let server = serve(app("stream-exhausted")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 10, 20)]);
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    send(&mut client, json!({ "type": "speed", "value": 4.0 })).await;
    until_control(&mut client, "speed").await;
    send(
        &mut client,
        json!({ "type": "seek", "timestampMs": t0 + 9_500 }),
    )
    .await;
    for index in 0..5 {
        assert_eq!(
            audio(&mut client).await.header.timestamp_ms,
            t0 + 9_500 + index * 100
        );
    }

    let end = until_control(&mut client, "end-of-recording").await;
    assert_eq!(end["timestampMs"], t0 + 10_000);
    let mode = until_control(&mut client, "mode").await;
    assert_eq!(mode["mode"], "paused");
    assert!(hears_no_audio_for(&mut client, Duration::from_millis(400)).await);

    // Stopped, not disconnected: the listener can still seek elsewhere.
    send(&mut client, json!({ "type": "ping", "clientTimeMs": 5 })).await;
    assert_eq!(until_control(&mut client, "pong").await["clientTimeMs"], 5);
}

#[tokio::test]
async fn a_seek_past_everything_recorded_with_nothing_recording_ends_at_once_and_stops() {
    let server = serve(app("stream-seek-past-end")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 10, 20)]);
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    send(
        &mut client,
        json!({ "type": "seek", "timestampMs": t0 + 60_000 }),
    )
    .await;
    let end = until_control(&mut client, "end-of-recording").await;
    assert_eq!(end["timestampMs"], t0 + 60_000);
    let mode = until_control(&mut client, "mode").await;
    assert_eq!(
        mode["mode"], "paused",
        "the page is told the stream stopped"
    );
    assert!(hears_no_audio_for(&mut client, Duration::from_millis(400)).await);
}

#[tokio::test]
async fn when_playback_catches_up_while_recording_it_rejoins_the_live_feed_at_real_time() {
    let server = serve(app("stream-caught-up")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 10, 20)]);
    server.state().capture.pretend_recording(48_000, 100);
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    send(&mut client, json!({ "type": "speed", "value": 2.0 })).await;
    until_control(&mut client, "speed").await;
    send(
        &mut client,
        json!({ "type": "seek", "timestampMs": t0 + 9_500 }),
    )
    .await;
    for _ in 0..5 {
        assert!(!audio(&mut client).await.header.is_live());
    }

    let speed = until_control(&mut client, "speed").await;
    assert_eq!(
        speed["value"].as_f64(),
        Some(1.0),
        "the live feed is real time"
    );
    let switched = until_control(&mut client, "switched-to-live").await;
    assert_eq!(switched["timestampMs"], t0 + 10_000);
    let mode = until_control(&mut client, "mode").await;
    assert_eq!(mode["mode"], "live");

    server.state().hub.publish(live_frame(t0 + 10_000, 1_000));
    let heard = audio(&mut client).await;
    assert!(heard.header.is_live());
    assert_eq!(heard.header.timestamp_ms, t0 + 10_000);
}

#[tokio::test]
async fn a_seek_straight_to_the_end_while_recording_rejoins_the_live_feed_at_real_time() {
    let server = serve(app("stream-seek-to-live")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 10, 20)]);
    server.state().capture.pretend_recording(48_000, 100);
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    send(&mut client, json!({ "type": "speed", "value": 2.0 })).await;
    until_control(&mut client, "speed").await;
    send(
        &mut client,
        json!({ "type": "seek", "timestampMs": t0 + 30_000 }),
    )
    .await;

    // The page sets itself to real time on `switched-to-live`, so the session must too, or the next
    // seek is paced at double speed for a page playing at normal speed.
    let heard = controls_within(&mut client, Duration::from_millis(500)).await;
    let kinds: Vec<&str> = heard
        .iter()
        .filter_map(|message| message["type"].as_str())
        .filter(|kind| *kind != "listeners")
        .collect();
    assert_eq!(
        kinds,
        ["mode", "speed", "switched-to-live", "mode"],
        "{heard:?}"
    );
    let speed = heard
        .iter()
        .find(|message| message["type"] == "speed")
        .expect("speed");
    assert_eq!(speed["value"].as_f64(), Some(1.0));
    let last_mode = heard
        .iter()
        .rev()
        .find(|message| message["type"] == "mode")
        .expect("mode");
    assert_eq!(last_mode["mode"], "live");

    // And the next seek really is paced at real time: two paced frames take about 200 ms.
    send(&mut client, json!({ "type": "seek", "timestampMs": t0 })).await;
    for _ in 0..3 {
        audio(&mut client).await;
    }
    let started = Instant::now();
    for _ in 0..4 {
        audio(&mut client).await;
    }
    assert!(
        started.elapsed() >= Duration::from_millis(330),
        "paced at real time, not double: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn going_live_from_playback_returns_to_the_live_feed_at_real_time() {
    let server = serve(app("stream-go-live")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 60, 20)]);
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    send(&mut client, json!({ "type": "speed", "value": 2.0 })).await;
    until_control(&mut client, "speed").await;
    send(&mut client, json!({ "type": "seek", "timestampMs": t0 })).await;
    audio(&mut client).await;

    send(&mut client, json!({ "type": "live" })).await;
    let speed = until_control(&mut client, "speed").await;
    assert_eq!(speed["value"].as_f64(), Some(1.0));
    let mode = until_control(&mut client, "mode").await;
    assert_eq!(mode["mode"], "live");

    // Recorded frames already on the wire may still land; the first live one is what matters.
    server.state().hub.publish(live_frame(t0 + 120_000, 1_000));
    loop {
        let heard = audio(&mut client).await;
        if heard.header.is_live() {
            assert_eq!(heard.header.timestamp_ms, t0 + 120_000);
            break;
        }
    }
}

#[tokio::test]
async fn pausing_twice_then_resuming_still_returns_to_the_recording() {
    let server = serve(app("stream-double-pause")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 60, 20)]);
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    send(&mut client, json!({ "type": "seek", "timestampMs": t0 })).await;
    audio(&mut client).await;
    send(&mut client, json!({ "type": "pause" })).await;
    until_control(&mut client, "mode").await;
    // A second press, or a second tab of the same page, must not make "paused" the place to resume to.
    send(&mut client, json!({ "type": "pause" })).await;
    until_control(&mut client, "mode").await;

    send(&mut client, json!({ "type": "resume" })).await;
    let resumed = until_control(&mut client, "mode").await;
    assert_eq!(resumed["mode"], "playback");
    loop {
        if !audio(&mut client).await.header.is_live() {
            break;
        }
    }
}

#[tokio::test]
async fn pausing_and_resuming_show_up_in_the_listener_list() {
    let server = serve(app("stream-pause-listed")).await;
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;
    until_control(&mut client, "listeners").await;

    let listed_as =
        |wanted: &'static str| move |message: &Value| message["listeners"][0]["activity"] == wanted;
    send(&mut client, json!({ "type": "pause" })).await;
    let paused = listed_as("paused");
    while !paused(&until_control(&mut client, "listeners").await) {}

    send(&mut client, json!({ "type": "resume" })).await;
    let live = listed_as("live");
    while !live(&until_control(&mut client, "listeners").await) {}
}

#[tokio::test]
async fn while_recording_the_stream_uses_the_format_capture_negotiated() {
    let server = serve(app("stream-capture-format")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 60, 20)]);
    // Not the 48 kHz and 100 ms defaults, so taking the settings instead of the device would show.
    server.state().capture.pretend_recording(44_100, 50);
    let mut client = connect(&server, None).await;

    let info = greeting(&mut client).await;
    assert_eq!(info["sampleRate"], 44_100);
    assert_eq!(info["frameMs"], 50);
    assert_eq!(info["capturing"], true);

    // Playback is cut into frames of the same length, whatever rate the recording itself was made at.
    send(&mut client, json!({ "type": "seek", "timestampMs": t0 })).await;
    let first = audio(&mut client).await;
    let second = audio(&mut client).await;
    assert_eq!(second.header.timestamp_ms - first.header.timestamp_ms, 50);
    assert_eq!(first.samples.len(), 2_400, "50 ms of the 48 kHz recording");
}

#[tokio::test]
async fn a_seek_to_either_end_of_time_is_answered_and_the_stream_survives_it() {
    let server = serve(app("stream-seek-extremes")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 10, 20)]);
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    for timestamp_ms in [i64::MIN, i64::MIN + 1, -1, i64::MAX - 1, i64::MAX] {
        send(
            &mut client,
            json!({ "type": "seek", "timestampMs": timestamp_ms }),
        )
        .await;
        // A panic in the session would drop the socket here instead of answering.
        send(&mut client, json!({ "type": "ping", "clientTimeMs": 9 })).await;
        assert_eq!(
            until_control(&mut client, "pong").await["clientTimeMs"],
            9,
            "after a seek to {timestamp_ms}"
        );
    }
}

#[tokio::test]
async fn the_recorder_meter_keeps_moving_while_a_listener_plays_a_recording() {
    let server = serve(app("stream-meter-in-playback")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 60, 20)]);
    server.state().capture.pretend_recording(48_000, 100);
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    // A click on the timeline: this session now plays the recording, and live frames stop reaching it.
    send(&mut client, json!({ "type": "seek", "timestampMs": t0 })).await;
    assert_eq!(until_control(&mut client, "mode").await["mode"], "playback");

    // The microphone goes on capturing, loudly, which the recorder card's meter must show.
    let loud = live_frame(t0 + 120_000, 16_000);
    let (rms, peak) = (f64::from(loud.rms), f64::from(loud.peak));
    server.state().hub.publish(loud);
    // A deadline of its own, since recorded audio keeps arriving and would otherwise keep the wait alive.
    let level = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let level = until_control(&mut client, "level").await;
            if (level["rms"].as_f64().expect("rms") - rms).abs() < 1e-6 {
                break level;
            }
        }
    })
    .await
    .expect("the microphone's level reached a listener playing a recording");
    assert!((level["peak"].as_f64().expect("peak") - peak).abs() < 1e-6);
    // And the recording still plays: the meter is extra, not instead.
    assert!(!audio(&mut client).await.header.is_live());
}

#[tokio::test]
async fn a_paused_listener_still_sees_the_microphone_level_while_recording() {
    let server = serve(app("stream-meter-paused")).await;
    server.state().capture.pretend_recording(48_000, 100);
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    send(&mut client, json!({ "type": "pause" })).await;
    assert_eq!(until_control(&mut client, "mode").await["mode"], "paused");
    let frame = live_frame(1_757_034_000_000, 9_000);
    let rms = f64::from(frame.rms);
    server.state().hub.publish(frame);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let level = until_control(&mut client, "level").await;
            if (level["rms"].as_f64().expect("rms") - rms).abs() < 1e-6 {
                break;
            }
        }
    })
    .await
    .expect("the microphone's level reached a paused listener");
}

#[tokio::test]
async fn no_level_is_sent_to_a_listener_in_a_recording_when_nothing_is_recording() {
    let server = serve(app("stream-meter-idle")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 60, 20)]);
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    send(&mut client, json!({ "type": "seek", "timestampMs": t0 })).await;
    let heard = controls_within(&mut client, Duration::from_millis(500)).await;
    assert!(
        heard.iter().all(|message| message["type"] != "level"),
        "with capture stopped there is no input to meter: {heard:?}"
    );
}

#[tokio::test]
async fn the_listener_count_includes_people_in_a_recording_or_paused_not_only_live() {
    let server = serve(app("stream-listener-count")).await;
    let t0 = recent_start();
    seed_recording(&server.app, &[steady(t0, 60, 20)]);

    let mut live = connect(&server, None).await;
    greeting(&mut live).await;
    let mut playing = connect(&server, None).await;
    greeting(&mut playing).await;
    send(&mut playing, json!({ "type": "seek", "timestampMs": t0 })).await;
    until_control(&mut playing, "mode").await;
    let mut paused = connect(&server, None).await;
    greeting(&mut paused).await;
    send(&mut paused, json!({ "type": "pause" })).await;
    until_control(&mut paused, "mode").await;

    // The count comes from the listener registry, not the live feed's subscribers, which would miss two.
    let status = call(&server.app, Method::GET, "/api/status", None, None).await;
    assert_eq!(status.body["listeners"], 3, "{}", status.body);
    drop((live, playing, paused));
}

// Control messages.

#[tokio::test]
async fn a_ping_is_answered_with_both_clocks() {
    let server = serve(app("stream-ping")).await;
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    let before = crate::util::time::now_ms();
    send(&mut client, json!({ "type": "ping", "clientTimeMs": 123 })).await;
    let pong = until_control(&mut client, "pong").await;
    assert_eq!(
        pong["clientTimeMs"], 123,
        "echoed, so the page can time the round trip"
    );
    let server_time = pong["serverTimeMs"].as_i64().expect("server time");
    assert!(server_time >= before && server_time <= crate::util::time::now_ms());
}

#[tokio::test]
async fn a_message_that_cannot_be_understood_is_refused_but_the_stream_stays_open() {
    let server = serve(app("stream-bad-message")).await;
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    for nonsense in ["not json", "{\"type\":\"launch\"}", "{\"type\":\"seek\"}"] {
        client
            .send(Message::Text(nonsense.into()))
            .await
            .expect("sent");
        let error = until_control(&mut client, "error").await;
        assert_eq!(error["code"], "bad_request", "{nonsense}");
    }
    // Binary input is not part of the protocol and is ignored rather than fatal.
    client
        .send(Message::Binary(vec![1, 2, 3].into()))
        .await
        .expect("sent");

    send(&mut client, json!({ "type": "ping", "clientTimeMs": 7 })).await;
    assert_eq!(until_control(&mut client, "pong").await["clientTimeMs"], 7);
}

#[tokio::test]
async fn what_the_player_reports_changes_the_listener_list_and_nothing_that_is_streamed() {
    let server = serve(app("stream-player")).await;
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;
    until_control(&mut client, "listeners").await;

    send(&mut client, json!({ "type": "player", "state": "paused" })).await;
    let heard = controls_within(&mut client, Duration::from_millis(500)).await;
    assert!(
        heard.iter().all(|message| message["type"] == "listeners"),
        "no mode or speed change, only the list: {heard:?}"
    );
    let latest = heard.last().expect("the list was sent again");
    assert_eq!(latest["listeners"][0]["player"], "paused");
    assert_eq!(latest["listeners"][0]["activity"], "live");
}

// Who is listening.

#[tokio::test]
async fn the_listener_list_follows_people_arriving_and_leaving() {
    let server = serve(app("stream-presence")).await;
    let mut watcher = connect(&server, None).await;
    greeting(&mut watcher).await;
    until_control(&mut watcher, "listeners").await;

    let mut visitor = connect(&server, None).await;
    greeting(&mut visitor).await;
    let arrived = until_control(&mut watcher, "listeners").await;
    assert_eq!(arrived["listeners"].as_array().map(Vec::len), Some(2));
    assert_eq!(server.state().listeners.count(), 2);

    visitor.close(None).await.expect("closed");
    drop(visitor);
    let left = until_control(&mut watcher, "listeners").await;
    assert_eq!(left["listeners"].as_array().map(Vec::len), Some(1));
    assert_eq!(server.state().listeners.count(), 1);
}

#[tokio::test]
async fn with_accounts_a_listener_never_sees_who_else_is_listening_and_an_admin_does() {
    let server = serve(app("stream-list-access")).await;
    let admin = set_up_admin(&server.app).await;
    let created = call(
        &server.app,
        Method::POST,
        "/api/users",
        Some(&admin),
        Some(json!({ "email": "kitchen@example.com", "password": "listen only", "role": "listener" })),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let login = call(
        &server.app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({ "email": "kitchen@example.com", "password": "listen only" })),
    )
    .await;
    let listener = login.cookie.expect("signed in");

    let mut kitchen = connect(&server, Some(&listener)).await;
    greeting(&mut kitchen).await;
    let mut owner = connect(&server, Some(&admin)).await;
    greeting(&mut owner).await;

    let seen = until_control(&mut owner, "listeners").await;
    let emails: Vec<&str> = seen["listeners"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|entry| entry["email"].as_str())
        .collect();
    assert!(emails.contains(&"kitchen@example.com"), "{seen}");

    let heard = controls_within(&mut kitchen, Duration::from_millis(500)).await;
    assert!(
        heard.iter().all(|message| message["type"] != "listeners"),
        "the list names people and where they connect from: {heard:?}"
    );
}

// The 15 second beat: access re-checks, pings and the idle timeout. Tokio's clock is paused, so it jumps
// straight to each tick once everything else is waiting.

#[tokio::test(start_paused = true)]
async fn signing_out_closes_a_stream_that_is_already_open() {
    let server = serve(app("stream-revoked")).await;
    let admin = set_up_admin(&server.app).await;
    let mut client = connect(&server, Some(&admin)).await;
    greeting(&mut client).await;

    let logout = super::test_support::call_from(
        &server.app,
        Method::POST,
        "/api/auth/logout",
        Some(&admin),
        None,
        None,
    )
    .await;
    assert!(logout.status.is_success(), "{}", logout.body);

    // In one second steps, so the paused clock never leaps past the moment the session ends while its
    // close frame is still crossing the loopback interface.
    let started = tokio::time::Instant::now();
    while server.state().listeners.count() > 0 {
        assert!(
            started.elapsed() <= Duration::from_secs(16),
            "the stream outlived the next access check"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    assert!(
        started.elapsed() >= Duration::from_secs(14),
        "ended by the access check, not by the sign out itself: {:?}",
        started.elapsed()
    );
    loop {
        match next(&mut client, Duration::from_secs(60)).await {
            Heard::Closed => break,
            Heard::Control(_) | Heard::Audio(_) => {}
        }
    }
}

#[tokio::test(start_paused = true)]
async fn an_admin_made_a_listener_loses_the_listener_list_on_the_next_check() {
    let server = serve(app("stream-demoted")).await;
    let owner = set_up_admin(&server.app).await;
    let created = call(
        &server.app,
        Method::POST,
        "/api/users",
        Some(&owner),
        Some(json!({ "email": "deputy@example.com", "password": "deputy password", "role": "admin" })),
    )
    .await;
    let deputy_id = created.body["id"].as_i64().expect("id");
    let login = call(
        &server.app,
        Method::POST,
        "/api/auth/login",
        None,
        Some(json!({ "email": "deputy@example.com", "password": "deputy password" })),
    )
    .await;
    let deputy = login.cookie.expect("signed in");

    let mut client = connect(&server, Some(&deputy)).await;
    greeting(&mut client).await;
    until_control(&mut client, "listeners").await;

    let demoted = call(
        &server.app,
        Method::PATCH,
        &format!("/api/users/{deputy_id}"),
        Some(&owner),
        Some(json!({ "role": "listener" })),
    )
    .await;
    assert!(demoted.status.is_success(), "{}", demoted.body);

    let hidden = loop {
        match next(&mut client, Duration::from_secs(60)).await {
            Heard::Control(message) if message["type"] == "listeners-hidden" => break message,
            Heard::Control(_) | Heard::Audio(_) => {}
            Heard::Closed => panic!("a listener may still listen; only the list goes"),
        }
    };
    assert_eq!(hidden["type"], "listeners-hidden");

    // Still streaming: a ping is answered.
    send(&mut client, json!({ "type": "ping", "clientTimeMs": 1 })).await;
    until_control(&mut client, "pong").await;
}

#[tokio::test(start_paused = true)]
async fn a_client_that_goes_silent_is_dropped_and_leaves_the_list() {
    let server = serve(app("stream-silent")).await;
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;
    assert_eq!(server.state().listeners.count(), 1);

    // Reading nothing means answering none of the server's pings, which is what a vanished phone does.
    tokio::time::sleep(Duration::from_secs(50)).await;
    // Let the session notice its tick and finish.
    for _ in 0..50 {
        if server.state().listeners.count() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        server.state().listeners.count(),
        0,
        "three missed ping rounds and it is gone"
    );
    drop(client);
}

#[tokio::test(start_paused = true)]
async fn a_client_that_answers_pings_stays_connected_past_the_idle_timeout() {
    let server = serve(app("stream-answering")).await;
    let mut client = connect(&server, None).await;
    greeting(&mut client).await;

    // Reading lets the client library answer each ping by itself, as a browser does.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(100);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(5), client.next()).await {
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Ok(Some(Err(_))) => {
                panic!("a client that answers pings was dropped")
            }
            _ => {}
        }
    }
    assert_eq!(server.state().listeners.count(), 1);
}
