//! The compiled web UI, carried inside the binary.
//!
//! A release build bakes `frontend/dist` into the executable so that shipping the service is one file
//! rather than a file plus a directory that has to travel with it and be pointed at correctly. Debug
//! builds read the same directory from disk instead, so editing the UI during development does not mean
//! recompiling the backend.
//!
//! The directory is allowed to be empty. A binary built without ever running the frontend build simply
//! has no files here, [`router`] returns `None`, and the caller falls back to the build instructions page.

use axum::body::Body;
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use rust_embed::{Embed, EmbeddedFile};
use tower_http::compression::CompressionLayer;

/// The single page app shell, served for every path that is not a file.
const INDEX: &str = "index.html";

#[derive(Embed)]
#[folder = "../frontend/dist"]
struct Assets;

/// A router over the embedded UI, or `None` when no UI was built into this binary.
pub fn router() -> Option<Router> {
    Assets::get(INDEX)?;

    Some(Router::new().fallback(serve).layer(CompressionLayer::new()))
}

async fn serve(uri: Uri, headers: HeaderMap) -> Response {
    // Vite emits plain ASCII filenames, so the path is used as it arrives rather than percent decoded.
    // A name that needed decoding would simply miss and fall through to the shell, which is the same
    // thing that happens for any other unknown path.
    let path = uri.path().trim_start_matches('/');

    match Assets::get(path) {
        Some(file) => respond(path, file, &headers),
        // Not a file, so it is a client side route. The shell is returned and the app decides for itself
        // whether the route exists, which is what keeps a reload on /settings working.
        None => match Assets::get(INDEX) {
            Some(index) => respond(INDEX, index, &headers),
            None => StatusCode::NOT_FOUND.into_response(),
        },
    }
}

fn respond(path: &str, file: EmbeddedFile, request_headers: &HeaderMap) -> Response {
    let etag = format!("\"{}\"", hex(&file.metadata.sha256_hash()));

    // Only an exact match is honoured. Browsers echo back the strong tag we issued verbatim, and the
    // cost of a miss is a full body rather than a wrong answer.
    let known = request_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == etag);

    if known {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response();
    }

    // Everything under assets/ carries a content hash in its filename, so it can never change meaning and
    // is safe to keep forever. The shell keeps its name across builds and must be revalidated every time,
    // or a released upgrade would never reach a browser that had already visited.
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    Response::builder()
        .header(header::CONTENT_TYPE, file.metadata.mimetype())
        .header(header::CACHE_CONTROL, cache)
        .header(header::ETAG, etag)
        .body(Body::from(file.data.into_owned()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Lowercase hex, so a digest can go in a header without pulling in an encoding crate.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_pads_every_byte_to_two_digits() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff, 0xa5]), "000fffa5");
        assert_eq!(hex(&[]), "");
    }

    #[test]
    fn a_binary_without_a_built_ui_has_no_router() {
        // Whichever way this build went, the two must agree: a router exists exactly when the shell does.
        assert_eq!(router().is_some(), Assets::get(INDEX).is_some());
    }
}
