//! Downloading a span of the recording as a WAV file.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::app::AppState;
use crate::dto::{ExportPlanResponse, ExportQuery};
use crate::error::{AppError, AppResult};
use crate::models::TimeRange;

/// Chunks buffered between the reader thread and the socket.
///
/// The reader is far faster than any network, so this only needs to be deep enough to keep it from
/// stalling on every write. Bounded so a client that stops reading cannot make the server buffer a
/// gigabyte of audio in memory.
const CHANNEL_DEPTH: usize = 8;

/// `GET /api/export/plan`
///
/// What an export of this range would produce, without producing it. Lets the UI show the size and the
/// format, and surfaces a refusal while the range can still be adjusted.
pub async fn plan(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ExportQuery>,
) -> AppResult<Json<ExportPlanResponse>> {
    let range = TimeRange::new(query.from_ms, query.to_ms);
    Ok(Json(state.export.plan(range)?.into()))
}

/// `GET /api/export`
///
/// Streams the file. The plan runs first so a refusal is a clean error response rather than a truncated
/// download, and so `Content-Length` can be set: without it a browser shows no progress and cannot tell a
/// finished download from a dropped connection.
pub async fn download(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ExportQuery>,
) -> AppResult<Response> {
    let range = TimeRange::new(query.from_ms, query.to_ms);
    let plan = state.export.plan(range)?;

    let (sender, receiver) =
        tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(CHANNEL_DEPTH);
    let export = state.export.clone();
    let writing = plan.clone();

    // Every step of the write reads a file, so it belongs on the blocking pool rather than on a runtime
    // worker where it would stall the recorder and every listener.
    tokio::task::spawn_blocking(move || {
        let outcome = export.write(&writing, |chunk| sender.blocking_send(Ok(chunk)).is_ok());

        if let Err(error) = outcome {
            tracing::error!(%error, "export failed part way through");
            // The header already went out with a length that will now not be met, so the only honest
            // signal left is to fail the body and let the client see a short read.
            let _ = sender.blocking_send(Err(std::io::Error::other(error.to_string())));
        }
    });

    let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|chunk| (chunk, receiver))
    });

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("audio/wav"));
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&plan.total_bytes.to_string())
            .map_err(|error| AppError::internal(format!("bad content length: {error}")))?,
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", filename(&plan)))
            .map_err(|error| AppError::internal(format!("bad filename: {error}")))?,
    );

    Ok((StatusCode::OK, headers, Body::from_stream(stream)).into_response())
}

/// A filename that says what the file holds, so a folder of exports stays readable.
fn filename(plan: &crate::services::ExportPlan) -> String {
    use chrono::{Local, TimeZone};

    let stamp = |timestamp_ms: i64| {
        Local
            .timestamp_millis_opt(timestamp_ms)
            .earliest()
            .map(|value| value.format("%Y-%m-%d_%H-%M-%S").to_string())
            .unwrap_or_else(|| timestamp_ms.to_string())
    };

    format!(
        "on-air-record_{}_to_{}.wav",
        stamp(plan.range.start_ms),
        stamp(plan.range.end_ms)
    )
}
