//! WebSocket upgrade for the audio stream.

use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::State;
use axum::response::Response;

use crate::app::AppState;
use crate::ws::StreamSession;

/// `GET /api/ws/stream`
///
/// The handshake itself does no work beyond handing the socket to a session task, so a burst of listeners
/// connecting at once never queues behind each other.
pub async fn stream(State(state): State<Arc<AppState>>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| StreamSession::new(state).run(socket))
}
