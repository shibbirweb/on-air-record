//! Router composition.
//!
//! The only place that maps a URL onto a handler. Reading this file should be enough to know the whole
//! HTTP surface of the service.

use std::path::Path;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::app::AppState;
use crate::controllers::{
    capture_controller, device_controller, session_controller, settings_controller,
    status_controller, stream_controller, timeline_controller,
};

/// Build the complete application router.
pub fn build(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/health", get(status_controller::health))
        .route("/status", get(status_controller::status))
        .route("/capture/start", post(capture_controller::start))
        .route("/capture/stop", post(capture_controller::stop))
        .route("/devices", get(device_controller::list))
        .route("/devices/select", post(device_controller::select))
        .route(
            "/settings",
            get(settings_controller::show).patch(settings_controller::update),
        )
        .route("/timeline/range", get(timeline_controller::range))
        .route("/timeline/peaks", get(timeline_controller::peaks))
        .route("/sessions", get(session_controller::list))
        .route("/storage", get(session_controller::storage))
        .route("/ws/stream", get(stream_controller::stream))
        .with_state(state.clone());

    Router::new()
        .nest("/api", api)
        .fallback_service(static_files(&state.config.static_dir))
        // The UI is served from the same origin, so CORS is only needed for the Vite dev server and for
        // anyone scripting the API from another page on the LAN. There is no authentication and no cookie
        // to protect, so a permissive policy costs nothing here.
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

/// Serve the compiled web UI, falling back to `index.html` so client side routes survive a page reload.
fn static_files(static_dir: &Path) -> Router {
    let index = static_dir.join("index.html");

    if !index.exists() {
        tracing::warn!(
            path = %static_dir.display(),
            "the web ui was not found, serving build instructions instead"
        );
        return Router::new().fallback(missing_ui);
    }

    let service = ServeDir::new(static_dir).fallback(ServeFile::new(index));
    Router::new()
        .fallback_service(service)
        .layer(CompressionLayer::new())
}

/// Explain how to build the UI rather than returning a bare 404, because a missing `dist` directory is by
/// far the most common first run problem and the answer is one command.
async fn missing_ui() -> impl IntoResponse {
    let body = Html(
        r#"<!doctype html>
<html lang="en">
  <head><meta charset="utf-8"><title>On Air Record</title></head>
  <body style="font-family: ui-sans-serif, system-ui, sans-serif; max-width: 42rem; margin: 4rem auto; line-height: 1.6;">
    <h1>The web UI has not been built</h1>
    <p>The API is running, but there is no compiled frontend to serve. Build it once:</p>
    <pre style="background:#f4f4f5;padding:1rem;border-radius:.5rem;">cd frontend
npm install
npm run build</pre>
    <p>Then reload this page. The API itself is available under <code>/api</code>, for example
    <a href="/api/health">/api/health</a>.</p>
  </body>
</html>"#,
    );

    (StatusCode::SERVICE_UNAVAILABLE, body)
}
