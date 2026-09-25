//! Application wide error type.
//!
//! Every fallible operation below the controller layer returns [`AppResult`]. The controller layer never
//! matches on the concrete variant, it simply returns the error and lets [`IntoResponse`] map it onto the
//! documented JSON envelope. Keeping that mapping in one place is what guarantees the API error contract
//! stays consistent as new endpoints are added.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),

    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Conflict(String),

    /// Nobody is signed in, or the credentials were wrong. The client shows the login page.
    #[error("{0}")]
    Unauthorized(String),

    /// Somebody is signed in, but their role does not allow this.
    #[error("{0}")]
    Forbidden(String),

    #[error("{0}")]
    TooManyRequests(String),

    /// The host audio system refused an operation. Usually a device that vanished or is held exclusively
    /// by another application, so it is reported as a temporary condition rather than a server bug.
    #[error("audio device error: {0}")]
    Audio(String),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(message.into())
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }

    pub fn too_many_requests(message: impl Into<String>) -> Self {
        Self::TooManyRequests(message.into())
    }

    pub fn audio(message: impl Into<String>) -> Self {
        Self::Audio(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    /// Stable machine readable code, documented in `docs/API.md`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Unauthorized(_) => "unauthenticated",
            Self::Forbidden(_) => "forbidden",
            Self::TooManyRequests(_) => "rate_limited",
            Self::Audio(_) => "audio_error",
            Self::Database(_) | Self::Io(_) | Self::Internal(_) => "internal",
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
            Self::Audio(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Database(_) | Self::Io(_) | Self::Internal(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "request failed");
        } else {
            tracing::debug!(error = %self, "request rejected");
        }

        let body = Json(json!({
            "error": {
                "code": self.code(),
                "message": self.to_string(),
            }
        }));

        (status, body).into_response()
    }
}
