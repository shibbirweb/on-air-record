//! Application services.
//!
//! Services own the behaviour of the product: what happens when capture starts, how a segment rolls over,
//! how a listener seeks. They depend on repositories and on the audio layer, never on HTTP, so the same
//! logic would serve a CLI or a gRPC front end unchanged.

pub mod broadcast_hub;
pub mod capture_service;
pub mod device_service;
pub mod playback_service;
pub mod recorder_service;
pub mod retention_service;
pub mod settings_service;
pub mod timeline_service;

pub use broadcast_hub::BroadcastHub;
pub use capture_service::CaptureService;
pub use device_service::DeviceService;
pub use playback_service::{CursorOutput, PlaybackCursor, PlaybackService};
pub use recorder_service::{RecorderHandle, RecorderService};
pub use retention_service::RetentionService;
pub use settings_service::SettingsService;
pub use timeline_service::{RecordingDay, TimelineRange, TimelineService};
