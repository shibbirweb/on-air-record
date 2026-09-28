//! The HTTP side of being signed in: the session cookie, who is calling, and where from.
//!
//! Lives in the controller layer because handlers need it, and the router's guard reuses it from here
//! rather than the other way round, which keeps dependencies pointing from routes to controllers only.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::header::{AUTHORIZATION, COOKIE, HOST, ORIGIN, USER_AGENT};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue};

use crate::error::AppError;
use crate::models::activity::{Actor, Origin};
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

/// The activity log's name for an account: its id and its email as it is now.
pub fn actor_for(user: &User) -> Actor {
    Actor::Account {
        user_id: user.id,
        email: user.email.clone(),
    }
}

impl Caller {
    /// Who the activity log says did it: the account, or a guest on a recorder without accounts.
    pub fn actor(&self) -> Actor {
        self.user.as_ref().map(actor_for).unwrap_or(Actor::Guest)
    }

    /// The signed in account, for routes that only make sense with one.
    pub fn signed_in(&self) -> Result<(&User, &str), AppError> {
        match (&self.user, &self.token) {
            (Some(user), Some(token)) => Ok((user, token)),
            _ => Err(AppError::conflict(
                "this install has no accounts, so there is no account to change",
            )),
        }
    }
}

/// Read our session cookie out of the `Cookie` header.
pub fn session_token(headers: &HeaderMap) -> Option<String> {
    read_cookie(headers, SESSION_COOKIE)
}

/// What an `Authorization` header holds, for the one route a scraper calls.
#[derive(Debug, PartialEq, Eq)]
pub enum Presented<'a> {
    /// No header: the request stands on its cookie, or on the recorder being open.
    Nothing,
    /// `Bearer <token>`, with the scheme in any case as HTTP allows.
    Bearer(&'a str),
    /// A header that is not a bearer token, or one with nothing after the scheme.
    Malformed,
}

pub fn presented_credentials(headers: &HeaderMap) -> Presented<'_> {
    let Some(value) = headers.get(AUTHORIZATION) else {
        return Presented::Nothing;
    };
    let Ok(value) = value.to_str() else {
        return Presented::Malformed;
    };
    match value.trim().split_once(' ') {
        Some((scheme, token))
            if scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty() =>
        {
            Presented::Bearer(token.trim())
        }
        _ => Presented::Malformed,
    }
}

/// The pending two factor sign in, between a right password and the code.
pub const CHALLENGE_COOKIE: &str = "oar_challenge";

pub fn challenge_token(headers: &HeaderMap) -> Option<String> {
    read_cookie(headers, CHALLENGE_COOKIE)
}

/// The `Set-Cookie` value for a pending two factor sign in.
///
/// Same protections as the session cookie, but it lasts only as long as the code may be typed, and is
/// only sent to the login routes, because nothing else has any use for it.
pub fn challenge_cookie(token: &str, headers: &HeaderMap) -> HeaderValue {
    let secure = if behind_https(headers) {
        "; Secure"
    } else {
        ""
    };
    let value = format!(
        "{CHALLENGE_COOKIE}={token}; Path=/api/auth; HttpOnly; SameSite=Strict; Max-Age=300{secure}"
    );
    HeaderValue::from_str(&value).unwrap_or_else(|_| clear_challenge_cookie())
}

pub fn clear_challenge_cookie() -> HeaderValue {
    HeaderValue::from_static("oar_challenge=; Path=/api/auth; HttpOnly; SameSite=Strict; Max-Age=0")
}

fn read_cookie(headers: &HeaderMap, wanted: &str) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == wanted)
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

/// Longer than any real browser sends, short enough that a hostile client cannot park a large string in
/// every admin's listener list or in every entry of the activity log.
pub const USER_AGENT_LIMIT: usize = 512;

/// The browser's description of itself, trimmed and bounded.
pub fn user_agent(headers: &HeaderMap) -> Option<String> {
    let sent = headers.get(USER_AGENT)?.to_str().ok()?.trim();
    if sent.is_empty() {
        return None;
    }
    Some(sent.chars().take(USER_AGENT_LIMIT).collect())
}

/// Where a request came from, for the activity log: the connection's address and the browser. The address
/// is left out when the server has no connection info, which only happens in tests.
pub struct RequestOrigin(pub Origin);

impl<S: Send + Sync> FromRequestParts<S> for RequestOrigin {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let address = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip().to_string());
        Ok(Self(Origin {
            address,
            user_agent: user_agent(&parts.headers),
        }))
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

    mod props {
        use super::*;
        use proptest::prelude::*;

        /// Host headers as a browser sends them: a name or an address, with or without a port.
        fn host() -> impl Strategy<Value = String> {
            "[a-z0-9]([a-z0-9.-]{0,14}[a-z0-9])?(:[0-9]{1,5})?"
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

            /// A page served from this host is let through whatever the scheme, letter case or trailing
            /// slash its browser put on the origin, and a page from any other host or port is refused.
            /// The origin check is open mode's only protection against another website streaming the
            /// microphone, so it must never let a different authority through, however close it looks.
            #[test]
            fn only_this_host_is_the_same_origin(
                host in host(),
                other in host(),
                scheme in prop_oneof![Just("http"), Just("https")],
                slash in any::<bool>(),
                upper in any::<bool>(),
                trick in prop_oneof![
                    Just(String::new()),
                    Just(".evil.example".to_string()),
                    Just("@evil.example".to_string()),
                    Just(":1".to_string()),
                    Just("/path".to_string()),
                ],
            ) {
                let spelled = if upper { host.to_ascii_uppercase() } else { host.clone() };
                let ours = format!("{scheme}://{spelled}{}", if slash { "/" } else { "" });
                prop_assert!(same_origin(&headers(&[("host", &host), ("origin", &ours)])), "{} refused for {}", ours, host);

                let theirs = format!("{scheme}://{other}{trick}");
                let same_authority = format!("{other}{trick}").trim_end_matches('/').eq_ignore_ascii_case(&host);
                prop_assert_eq!(
                    same_origin(&headers(&[("host", &host), ("origin", &theirs)])),
                    same_authority,
                    "{} for host {}", theirs, host
                );
            }

            /// Among any other cookies, ours is found by its exact name only: a cookie whose name merely
            /// starts or ends like ours is never read as the session, and an empty value is no session.
            #[test]
            fn the_session_cookie_is_read_by_its_exact_name(
                others in proptest::collection::vec(("[a-z_]{1,12}", "[A-Za-z0-9]{0,10}"), 0..5),
                decoys in proptest::collection::vec(prop_oneof![Just("xoar_session"), Just("oar_session_old"), Just("OAR_SESSION")], 0..3),
                token in proptest::option::of("[A-Za-z0-9]{0,16}"),
                position in any::<proptest::sample::Index>(),
            ) {
                let mut pairs: Vec<String> = others
                    .iter()
                    .filter(|(name, _)| name != SESSION_COOKIE)
                    .map(|(name, value)| format!("{name}={value}"))
                    .collect();
                pairs.extend(decoys.iter().map(|name| format!("{name}=decoy")));
                if let Some(token) = &token {
                    let at = position.index(pairs.len() + 1);
                    pairs.insert(at, format!("{SESSION_COOKIE}={token}"));
                }
                let header = pairs.join("; ");
                let found = if header.is_empty() {
                    session_token(&HeaderMap::new())
                } else {
                    session_token(&headers(&[("cookie", &header)]))
                };
                prop_assert_eq!(found, token.filter(|token| !token.is_empty()));
            }
        }
    }
}
