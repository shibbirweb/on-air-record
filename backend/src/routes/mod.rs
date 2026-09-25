//! Router composition.
//!
//! The only place that maps a URL onto a handler. Reading this file should be enough to know the whole
//! HTTP surface of the service.

use std::path::Path;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::middleware::from_fn_with_state;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

mod embedded_ui;
pub mod guard;
#[cfg(test)]
mod tests;

use crate::app::AppState;
use crate::controllers::{
    auth_controller, bookmark_controller, capture_controller, device_controller, export_controller,
    session_controller, settings_controller, status_controller, stream_controller,
    timeline_controller, user_controller,
};

/// Build the complete application router.
///
/// Who may call each route is decided by [`guard::required_access`], not here, so this file stays a plain
/// map of URLs onto handlers.
pub fn build(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/health", get(status_controller::health))
        .route("/auth/state", get(auth_controller::state))
        .route("/auth/open", post(auth_controller::choose_open))
        .route("/auth/setup", post(auth_controller::set_up))
        .route("/auth/login", post(auth_controller::log_in))
        .route("/auth/login/verify", post(auth_controller::verify_login))
        .route("/auth/logout", post(auth_controller::log_out))
        .route("/auth/password", post(auth_controller::change_password))
        .route("/auth/two-factor", get(auth_controller::two_factor_status))
        .route(
            "/auth/two-factor/setup",
            post(auth_controller::two_factor_setup),
        )
        .route(
            "/auth/two-factor/enable",
            post(auth_controller::two_factor_enable),
        )
        .route(
            "/auth/two-factor/disable",
            post(auth_controller::two_factor_disable),
        )
        .route(
            "/auth/two-factor/recovery-codes",
            post(auth_controller::recovery_codes),
        )
        .route(
            "/users",
            get(user_controller::list).post(user_controller::create),
        )
        .route(
            "/users/{id}",
            axum::routing::patch(user_controller::update).delete(user_controller::remove),
        )
        .route("/users/{id}/password", post(user_controller::set_password))
        .route(
            "/users/{id}/two-factor",
            axum::routing::delete(user_controller::reset_two_factor),
        )
        .route("/status", get(status_controller::status))
        .route("/capture/start", post(capture_controller::start))
        .route("/capture/stop", post(capture_controller::stop))
        .route("/devices", get(device_controller::list))
        .route("/devices/select", post(device_controller::select))
        .route(
            "/settings",
            get(settings_controller::show).patch(settings_controller::update),
        )
        .route("/settings/defaults", get(settings_controller::defaults))
        .route("/settings/reset", post(settings_controller::reset))
        .route(
            "/settings/test-recordings-dir",
            post(settings_controller::test_recordings_dir),
        )
        .route("/timeline/range", get(timeline_controller::range))
        .route("/timeline/days", get(timeline_controller::days))
        .route("/timeline/peaks", get(timeline_controller::peaks))
        .route(
            "/bookmarks",
            get(bookmark_controller::list).post(bookmark_controller::create),
        )
        .route(
            "/bookmarks/{id}",
            axum::routing::patch(bookmark_controller::update).delete(bookmark_controller::remove),
        )
        .route("/sessions", get(session_controller::list))
        .route("/storage", get(session_controller::storage))
        .route("/export", get(export_controller::download))
        .route("/export/plan", get(export_controller::plan))
        .route("/ws/stream", get(stream_controller::stream))
        .route_layer(from_fn_with_state(state.clone(), guard::guard))
        .with_state(state.clone());

    // No CORS layer, on purpose. The UI is served from this same origin and the Vite dev server proxies
    // rather than calling across origins, so nothing legitimate needs it. A permissive policy would let
    // any website a listener opens read recordings and change settings through their browser, which is
    // exactly the network position a LAN only service must not lend out.
    Router::new()
        .nest("/api", api)
        .fallback_service(static_files(&state.config.static_dir))
        .layer(TraceLayer::new_for_http())
}

/// Serve the compiled web UI, falling back to `index.html` so client side routes survive a page reload.
///
/// Disk wins over the copy embedded in the binary. That ordering is what lets `--static-dir` point at a
/// fresh Vite build during development, or at a UI swapped in on a deployed machine, without rebuilding
/// the backend. A downloaded release has no such directory and serves itself.
fn static_files(static_dir: &Path) -> Router {
    let index = static_dir.join("index.html");

    if index.exists() {
        let service = ServeDir::new(static_dir).fallback(ServeFile::new(index));
        return Router::new()
            .fallback_service(service)
            .layer(CompressionLayer::new());
    }

    if let Some(embedded) = embedded_ui::router() {
        return embedded;
    }

    tracing::warn!(
        path = %static_dir.display(),
        "no web ui on disk and none embedded in this binary, serving build instructions instead"
    );
    Router::new().fallback(missing_ui)
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
