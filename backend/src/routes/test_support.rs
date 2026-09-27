//! The harness the router tests share: the whole application on a throwaway data directory, driven over
//! HTTP with `tower::ServiceExt::oneshot`, so no port is bound and tests run side by side.
//!
//! The application state is kept alongside the router so a test can seed recordings, bookmarks or settings
//! directly, the way the recorder would have, and then check what the HTTP API makes of them.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::header::{COOKIE, HOST, ORIGIN, SET_COOKIE};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::app::AppState;
use crate::config::AppConfig;
use crate::models::{SegmentDraft, SessionDraft};

pub(super) const HOST_NAME: &str = "recorder.test:8080";
/// An Origin that matches `HOST_NAME`, as the service's own page sends.
pub(super) const SAME_ORIGIN: &str = "http://recorder.test:8080";

pub(super) struct TestApp {
    pub router: Router,
    state: Option<Arc<AppState>>,
    pub data_dir: std::path::PathBuf,
}

impl TestApp {
    pub fn state(&self) -> &Arc<AppState> {
        self.state.as_ref().expect("the application is running")
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        // Close the database before deleting its folder. Windows will not delete a file that is still
        // open, so the router and the state, which own the application and its connection, go first.
        drop(std::mem::replace(&mut self.router, Router::new()));
        drop(self.state.take());
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}

pub(super) fn app(name: &str) -> TestApp {
    build_app(name, |_| {})
}

/// The application serving a small web interface of its own from disk, so what the page routes answer
/// does not depend on whether `frontend/dist` happens to be built on this machine.
pub(super) fn app_serving_ui(name: &str) -> TestApp {
    build_app(name, |static_dir| {
        std::fs::create_dir_all(static_dir.join("assets")).expect("ui folder");
        std::fs::write(
            static_dir.join("index.html"),
            "<!doctype html><html><body>the test interface</body></html>",
        )
        .expect("index");
        std::fs::write(
            static_dir.join("assets").join("app.js"),
            "console.log('app');",
        )
        .expect("asset");
    })
}

fn build_app(name: &str, prepare_ui: impl FnOnce(&std::path::Path)) -> TestApp {
    let data_dir = std::env::temp_dir().join(format!("oar-routes-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data_dir);
    let static_dir = data_dir.join("ui");
    prepare_ui(&static_dir);
    let config = AppConfig {
        data_dir: data_dir.clone(),
        static_dir,
        ..AppConfig::default()
    };
    let state: Arc<AppState> = AppState::bootstrap(config).expect("bootstrap");
    TestApp {
        router: super::build(state.clone()),
        state: Some(state),
        data_dir,
    }
}

pub(super) struct Reply {
    pub status: StatusCode,
    /// A session cookie the response set.
    pub cookie: Option<String>,
    /// A two factor challenge cookie the response set.
    pub challenge: Option<String>,
    pub body: Value,
}

/// A response kept as bytes, for the few endpoints that do not answer in JSON.
pub(super) struct RawReply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
}

pub(super) async fn call(
    app: &TestApp,
    method: Method,
    path: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> Reply {
    call_from(app, method, path, cookie, body, None).await
}

pub(super) async fn call_from(
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
pub(super) async fn call_with_challenge(
    app: &TestApp,
    path: &str,
    challenge: &str,
    body: Value,
) -> Reply {
    let cookie_header = Some(format!("oar_challenge={challenge}"));
    send(app, Method::POST, path, cookie_header, Some(body), None).await
}

/// A GET, with no session, answered as bytes.
pub(super) async fn get_raw(app: &TestApp, path: &str) -> RawReply {
    let request = Request::builder()
        .method(Method::GET)
        .uri(path)
        .header(HOST, HOST_NAME)
        .body(Body::empty())
        .expect("request");
    let response = app.router.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body")
        .to_vec();
    RawReply {
        status,
        headers,
        bytes,
    }
}

pub(super) async fn send(
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

pub(super) async fn set_up_admin(app: &TestApp) -> String {
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

/// One segment of seeded recording: where it sits on the timeline and the level it holds throughout, with
/// an optional loud stretch inside it.
pub(super) struct SeedSegment {
    pub start_ms: i64,
    pub seconds: i64,
    /// The steady level of the envelope, 0 to 255, and of the PCM (as a square wave of that RMS).
    pub level: u8,
    /// A loud stretch: `(offset_ms, length_ms, level)` into the segment.
    pub loud: Option<(i64, i64, u8)>,
}

/// Write seeded segments as the recorder would: PCM files on disk at 48 kHz mono, and a segment row with
/// a matching envelope, all in one session. Returns the session id.
pub(super) fn seed_recording(app: &TestApp, segments: &[SeedSegment]) -> i64 {
    const RATE: i64 = 48_000;
    let state = app.state();
    let first_ms = segments
        .first()
        .map(|segment| segment.start_ms)
        .unwrap_or(0);
    let session = state
        .sessions
        .create(&SessionDraft {
            device_id: "seeded".to_string(),
            device_name: "seeded microphone".to_string(),
            sample_rate: RATE as u32,
            channels: 1,
            started_at_ms: first_ms,
        })
        .expect("session");

    for (sequence, segment) in segments.iter().enumerate() {
        let slots = (segment.seconds * 10) as usize;
        let mut peaks = vec![segment.level; slots];
        if let Some((offset_ms, length_ms, level)) = segment.loud {
            for slot in (offset_ms / 100)..((offset_ms + length_ms) / 100) {
                peaks[slot as usize] = level;
            }
        }
        // A square wave whose RMS is the envelope level, so the audio and the index agree.
        let mut pcm = Vec::with_capacity((segment.seconds * RATE * 2) as usize);
        for sample in 0..(segment.seconds * RATE) {
            let slot = (sample * 10 / RATE) as usize;
            let amplitude = (f64::from(peaks[slot]) / 255.0 * f64::from(i16::MAX)) as i16;
            let value = if sample % 2 == 0 {
                amplitude
            } else {
                -amplitude
            };
            pcm.extend_from_slice(&value.to_le_bytes());
        }

        let day = crate::util::day::local_day(segment.start_ms);
        let relative = format!("recordings/{day}/{}/{sequence:06}.pcm", session.id);
        let path = app.data_dir.join(&relative);
        std::fs::create_dir_all(path.parent().expect("segment folder")).expect("folder");
        std::fs::write(&path, &pcm).expect("pcm");

        state
            .segments
            .insert(&SegmentDraft {
                session_id: session.id,
                sequence: sequence as i64,
                day,
                path: relative,
                started_at_ms: segment.start_ms,
                ended_at_ms: segment.start_ms + segment.seconds * 1000,
                sample_rate: RATE as u32,
                channels: 1,
                byte_len: pcm.len() as i64,
                peaks,
            })
            .expect("segment");
    }
    session.id
}
