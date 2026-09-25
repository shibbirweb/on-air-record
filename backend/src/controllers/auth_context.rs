//! The HTTP side of being signed in: the session cookie, who is calling, and where from.
//!
//! Lives in the controller layer because handlers need it, and the router's guard reuses it from here
//! rather than the other way round, which keeps dependencies pointing from routes to controllers only.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::header::{COOKIE, HOST, ORIGIN};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue};

use crate::error::AppError;
use crate::models::User;
use crate::services::auth_service::SESSION_TTL_MS;

pub const SESSION_COOKIE: &str = "oar_session";

/// Who is calling, attached by the guard to every request it lets through.
#[derive(Debug, Clone)]
pub struct Caller {
    /// `None` without accounts, where nobody signs in.
    pub user: Option<User>,
    /// The raw session cookie, needed to sign out or to keep this session when changing a password.
    pub token: Option<String>,
}

impl Caller {
    /// The signed in account, for routes that only make sense with one.
    pub fn signed_in(&self) -> Result<(&User, &str), AppError> {
        match (&self.user, &self.token) {
            (Some(user), Some(token)) => Ok((user, token)),
            _ => Err(AppError::conflict(
                "this install has no accounts, so there is no password to change",
            )),
        }
    }
}

/// Read our session cookie out of the `Cookie` header.
pub fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == SESSION_COOKIE)
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// The `Set-Cookie` value that signs a browser in.
///
/// `HttpOnly` keeps the page's own scripts from reading it, and `SameSite=Strict` keeps other sites from
/// sending it, which is what stops a page elsewhere from acting as a signed in user, WebSocket included.
/// `Secure` is added only when a TLS proxy says the browser is on HTTPS, because on plain HTTP the
/// browser would silently drop the cookie and nobody could sign in.
pub fn session_cookie(token: &str, headers: &HeaderMap) -> HeaderValue {
    let secure = if behind_https(headers) {
        "; Secure"
    } else {
        ""
    };
    let value = format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{secure}",
        SESSION_TTL_MS / 1000
    );
    HeaderValue::from_str(&value).unwrap_or_else(|_| clear_session_cookie())
}

pub fn clear_session_cookie() -> HeaderValue {
    HeaderValue::from_static("oar_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
}

fn behind_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.eq_ignore_ascii_case("https"))
        .unwrap_or(false)
}

/// True when a browser request comes from a page this service served.
///
/// Browsers apply no CORS rules to WebSockets, so without this check any website a listener happens to
/// open could connect to the stream from their browser and hear the microphone, with or without
/// accounts. A request with no `Origin` header at all is not from a browser page, so it is allowed; with
/// accounts on it still needs a session like anything else.
pub fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|value| value.to_str().ok()) else {
        return true;
    };
    let Some(host) = headers.get(HOST).and_then(|value| value.to_str().ok()) else {
        return false;
    };

    let origin_host = origin
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(origin)
        .trim_end_matches('/');
    origin_host.eq_ignore_ascii_case(host)
}

/// The address a request came from, for throttling failed logins.
///
/// Falls back to the unspecified address when the server was not started with connection info, which
/// only happens in tests. Behind a reverse proxy every client shares the proxy's address, so the
/// throttle then applies to everybody at once: stricter than intended, never looser.
pub struct ClientAddr(pub IpAddr);

impl<S: Send + Sync> FromRequestParts<S> for ClientAddr {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let address = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip())
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        Ok(Self(address))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(*name, HeaderValue::from_str(value).expect("header"));
        }
        map
    }

    #[test]
    fn the_session_cookie_is_found_among_others() {
        let found = session_token(&headers(&[(
            "cookie",
            "theme=dark; oar_session=abc123; x=1",
        )]));
        assert_eq!(found.as_deref(), Some("abc123"));
        assert_eq!(session_token(&headers(&[("cookie", "oar_session=")])), None);
        assert_eq!(session_token(&HeaderMap::new()), None);
    }

    #[test]
    fn the_cookie_is_http_only_strict_and_secure_only_behind_https() {
        let plain = session_cookie("t", &HeaderMap::new());
        let plain = plain.to_str().expect("ascii");
        assert!(plain.contains("HttpOnly"));
        assert!(plain.contains("SameSite=Strict"));
        assert!(!plain.contains("Secure"));

        let proxied = session_cookie("t", &headers(&[("x-forwarded-proto", "https")]));
        assert!(proxied.to_str().expect("ascii").ends_with("; Secure"));
    }

    #[test]
    fn only_pages_from_this_host_may_open_the_stream() {
        assert!(same_origin(&headers(&[
            ("host", "192.168.1.5:8080"),
            ("origin", "http://192.168.1.5:8080"),
        ])));
        assert!(!same_origin(&headers(&[
            ("host", "192.168.1.5:8080"),
            ("origin", "https://evil.example"),
        ])));
        assert!(!same_origin(&headers(&[
            ("host", "192.168.1.5:8080"),
            ("origin", "http://192.168.1.5:9999"),
        ])));
        // A script or a command line client sends no Origin at all.
        assert!(same_origin(&headers(&[("host", "192.168.1.5:8080")])));
    }
}
