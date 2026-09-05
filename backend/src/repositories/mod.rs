//! Data access layer.
//!
//! One repository per aggregate. Repositories own every SQL string in the codebase and translate rows into
//! domain models, so no other layer needs to know that the store happens to be SQLite.

pub mod segment_repository;
pub mod session_repository;
pub mod settings_repository;

pub use segment_repository::{DaySummary, SegmentRepository, SegmentStorageStats};
pub use session_repository::{SessionRepository, SessionSummary};
pub use settings_repository::SettingsRepository;
