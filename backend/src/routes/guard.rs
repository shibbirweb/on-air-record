//! Who may call what, enforced once for the whole API.
//!
//! One middleware in front of every `/api` route rather than a check in each handler, so a route added
//! later is protected by default: anything that reads needs a listener, anything that changes state needs
//! an admin, and the few routes that must work signed out are listed by name. Forgetting to think about a
//! new route therefore fails closed, never open.
//!
//! It also refuses other websites. A page somebody opens elsewhere can make their browser send requests
//! here, and two kinds get through a browser's own defences: a bodiless `POST`, which needs no CORS
//! permission to be sent, and a WebSocket, which CORS does not cover at all. With accounts on, the
//! `SameSite=Strict` cookie already keeps those requests signed out; without accounts, this check is the
//! only thing between a stranger's page and the recorder's controls and microphone.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::app::AppState;
use crate::controllers::auth_context::{same_origin, session_token, Caller};
use crate::error::AppError;
use crate::models::{authorize, Access, Denied};

/// Routes that answer whether or not anybody is signed in. Paths are as seen inside the `/api` router.
fn is_public(path: &str) -> bool {
    matches!(
        path,
        "/health" | "/auth/state" | "/auth/open" | "/auth/setup" | "/auth/login" | "/auth/logout"
    )
}

/// What a route needs, given how it is called.
///
/// Reading is listening and changing is administering, with named exceptions: a listener may change
/// their own password, and only an admin may see the list of accounts.
pub fn required_access(method: &Method, path: &str) -> Option<Access> {
    if is_public(path) {
        return None;
    }
    if path == "/auth/password" {
        return Some(Access::Listen);
    }
    if path == "/users" || path.starts_with("/users/") {
        return Some(Access::Administer);
    }
    if method == Method::GET || method == Method::HEAD {
        return Some(Access::Listen);
    }
    Some(Access::Administer)
}

/// Whether a request from another site must be refused: anything that changes state, and the stream.
pub fn needs_same_origin(method: &Method, path: &str) -> bool {
    let reads = method == Method::GET || method == Method::HEAD || method == Method::OPTIONS;
    !reads || path == "/ws/stream"
}

pub async fn guard(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Response {
    if needs_same_origin(request.method(), request.uri().path()) && !same_origin(request.headers())
    {
        return AppError::forbidden("requests from other websites are not accepted")
            .into_response();
    }

    let Some(needed) = required_access(request.method(), request.uri().path()) else {
        return next.run(request).await;
    };

    let mode = match state.auth.mode() {
        Ok(mode) => mode,
        Err(error) => return error.into_response(),
    };

    let token = session_token(request.headers());
    // Without accounts there is nobody to look up, so the session table is not touched at all.
    let user = if mode.requires_login() {
        match state.auth.resolve(token.as_deref()) {
            Ok(user) => user,
            Err(error) => return error.into_response(),
        }
    } else {
        None
    };

    match authorize(mode, user.as_ref(), needed) {
        Ok(()) => {
            request.extensions_mut().insert(Caller { user, token });
            next.run(request).await
        }
        Err(Denied::SignInRequired) => {
            AppError::unauthorized("sign in to continue").into_response()
        }
        Err(Denied::NotAllowed) => {
            AppError::forbidden("your account can listen, but only an admin can change this")
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_need_a_listener_and_changes_need_an_admin() {
        assert_eq!(required_access(&Method::GET, "/health"), None);
        assert_eq!(required_access(&Method::POST, "/auth/login"), None);
        assert_eq!(
            required_access(&Method::GET, "/status"),
            Some(Access::Listen)
        );
        assert_eq!(
            required_access(&Method::GET, "/ws/stream"),
            Some(Access::Listen)
        );
        assert_eq!(
            required_access(&Method::GET, "/export"),
            Some(Access::Listen)
        );
        assert_eq!(
            required_access(&Method::POST, "/capture/start"),
            Some(Access::Administer)
        );
        assert_eq!(
            required_access(&Method::PATCH, "/settings"),
            Some(Access::Administer)
        );
        assert_eq!(
            required_access(&Method::DELETE, "/bookmarks/4"),
            Some(Access::Administer)
        );
    }

    #[test]
    fn the_named_exceptions_hold() {
        assert_eq!(
            required_access(&Method::POST, "/auth/password"),
            Some(Access::Listen)
        );
        assert_eq!(
            required_access(&Method::GET, "/users"),
            Some(Access::Administer)
        );
        assert_eq!(
            required_access(&Method::GET, "/users/2"),
            Some(Access::Administer)
        );
    }

    #[test]
    fn other_sites_are_refused_for_changes_and_the_stream_only() {
        assert!(needs_same_origin(&Method::POST, "/capture/stop"));
        assert!(needs_same_origin(&Method::DELETE, "/bookmarks/1"));
        assert!(needs_same_origin(&Method::GET, "/ws/stream"));
        assert!(!needs_same_origin(&Method::GET, "/status"));
    }

    #[test]
    fn a_route_nobody_thought_about_fails_closed() {
        assert_eq!(
            required_access(&Method::POST, "/something/new"),
            Some(Access::Administer)
        );
        assert_eq!(
            required_access(&Method::GET, "/something/new"),
            Some(Access::Listen)
        );
    }
}
