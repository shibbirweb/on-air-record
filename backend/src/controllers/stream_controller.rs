//! WebSocket upgrade for the audio stream.

use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::State;
use axum::http::{header, HeaderMap};
use axum::response::Response;
use axum::Extension;

use crate::app::AppState;
use crate::controllers::auth_context::{Caller, ClientAddr};
use crate::models::ListenerAccount;
use crate::ws::{ListenerIdentity, StreamSession};

/// Longer than any real browser sends, short enough that a hostile client cannot park a large string in
/// every admin's listener list.
const USER_AGENT_LIMIT: usize = 512;

/// `GET /api/ws/stream`
///
/// The handshake itself does no work beyond handing the socket to a session task, so a burst of listeners
/// connecting at once never queues behind each other. The router's guard has already checked the page's
/// origin and the listener's session; the session keeps the cookie so it can check again while it runs.
pub async fn stream(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    ClientAddr(address): ClientAddr,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let identity = ListenerIdentity {
        account: caller.user.map(|user| ListenerAccount {
            email: user.email,
            role: user.role,
        }),
        address,
        user_agent: user_agent(&headers),
    };
    upgrade.on_upgrade(move |socket| StreamSession::new(state, caller.token, identity).run(socket))
}

fn user_agent(headers: &HeaderMap) -> Option<String> {
    let sent = headers.get(header::USER_AGENT)?.to_str().ok()?.trim();
    if sent.is_empty() {
        return None;
    }
    Some(sent.chars().take(USER_AGENT_LIMIT).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn with_agent(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::USER_AGENT,
            HeaderValue::from_str(value).expect("header"),
        );
        headers
    }

    #[test]
    fn the_user_agent_is_kept_trimmed_and_bounded() {
        assert_eq!(
            user_agent(&with_agent(" Firefox ")).as_deref(),
            Some("Firefox")
        );
        assert_eq!(user_agent(&with_agent("")), None);
        assert_eq!(user_agent(&HeaderMap::new()), None);
        assert_eq!(
            user_agent(&with_agent(&"x".repeat(2_000))).map(|agent| agent.len()),
            Some(USER_AGENT_LIMIT)
        );
    }
}
