//! A continuous capture run.
//!
//! A new session starts whenever capture starts or the input device changes, because the sample rate and
//! channel count are fixed for the lifetime of a session. Segments therefore never need per file format
//! negotiation, they inherit it from their session.

#[derive(Debug, Clone)]
pub struct RecordingSession {
    pub id: i64,
    pub device_id: String,
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
}

/// Values needed to open a session, before the database assigns an id.
#[derive(Debug, Clone)]
pub struct SessionDraft {
    pub device_id: String,
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub started_at_ms: i64,
}
