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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    fn every_variant() -> Vec<(AppError, StatusCode, &'static str)> {
        vec![
            (
                AppError::bad_request("b"),
                StatusCode::BAD_REQUEST,
                "bad_request",
            ),
            (AppError::not_found("n"), StatusCode::NOT_FOUND, "not_found"),
            (AppError::conflict("c"), StatusCode::CONFLICT, "conflict"),
            (
                AppError::unauthorized("u"),
                StatusCode::UNAUTHORIZED,
                "unauthenticated",
            ),
            (AppError::forbidden("f"), StatusCode::FORBIDDEN, "forbidden"),
            (
                AppError::too_many_requests("t"),
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
            ),
            (
                AppError::audio("a"),
                StatusCode::SERVICE_UNAVAILABLE,
                "audio_error",
            ),
            (
                AppError::internal("i"),
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
            ),
        ]
    }

    #[test]
    fn every_variant_has_its_status_and_its_documented_code() {
        for (error, status, code) in every_variant() {
            assert_eq!(error.status(), status, "{error:?}");
            assert_eq!(error.code(), code, "{error:?}");
        }
    }

    #[test]
    fn storage_failures_are_internal_errors() {
        let io: AppError = std::io::Error::other("disk gone").into();
        assert_eq!(io.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(io.code(), "internal");
        assert!(io.to_string().contains("disk gone"));

        let database: AppError = rusqlite::Error::InvalidQuery.into();
        assert_eq!(database.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(database.code(), "internal");
        assert!(database.to_string().starts_with("database error"));
    }

    #[test]
    fn an_audio_error_says_it_is_one() {
        assert_eq!(
            AppError::audio("device unplugged").to_string(),
            "audio device error: device unplugged"
        );
        // The rest carry their message as it is, because it is written for the person reading it.
        assert_eq!(
            AppError::bad_request("a bookmark needs a label").to_string(),
            "a bookmark needs a label"
        );
    }

    #[tokio::test]
    async fn the_response_is_the_status_and_a_json_body_with_code_and_message() {
        for (error, status, code) in every_variant() {
            let message = error.to_string();
            let response = error.into_response();
            assert_eq!(response.status(), status);
            let bytes = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body");
            let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
            assert_eq!(body["error"]["code"], code);
            assert_eq!(body["error"]["message"], message);
        }
    }
}
