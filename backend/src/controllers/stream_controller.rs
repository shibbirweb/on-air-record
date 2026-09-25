//! WebSocket upgrade for the audio stream.

use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::State;
use axum::response::Response;
use axum::Extension;

use crate::app::AppState;
use crate::controllers::auth_context::Caller;
use crate::ws::StreamSession;

/// `GET /api/ws/stream`
///
/// The handshake itself does no work beyond handing the socket to a session task, so a burst of listeners
/// connecting at once never queues behind each other. The router's guard has already checked the page's
/// origin and the listener's session; the session keeps the cookie so it can check again while it runs.
pub async fn stream(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    upgrade: WebSocketUpgrade,
) -> Response {
    upgrade.on_upgrade(move |socket| StreamSession::new(state, caller.token).run(socket))
}
