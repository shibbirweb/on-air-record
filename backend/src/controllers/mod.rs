//! HTTP and WebSocket request handlers.
//!
//! Controllers are deliberately thin: parse the request, call one service, map the result onto a DTO.
//! Anything longer than that belongs in a service, because logic that lives in a handler can only ever be
//! reached over HTTP.

pub mod auth_context;
pub mod auth_controller;
pub mod bookmark_controller;
pub mod capture_controller;
pub mod device_controller;
pub mod export_controller;
pub mod session_controller;
pub mod settings_controller;
pub mod status_controller;
pub mod stream_controller;
pub mod timeline_controller;
pub mod user_controller;
