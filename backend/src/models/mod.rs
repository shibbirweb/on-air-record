//! Domain entities.
//!
//! These types describe the vocabulary of the application and know nothing about SQL, HTTP, or the audio
//! host. Repositories map rows onto them and DTOs project them onto the wire.

pub mod audio_frame;
pub mod capture_state;
pub mod device;
pub mod segment;
pub mod session;
pub mod settings;

pub use audio_frame::AudioFrame;
pub use capture_state::{CaptureSnapshot, CaptureState, LevelSnapshot};
pub use device::InputDevice;
pub use segment::{Segment, SegmentDraft, TimeRange};
pub use session::{RecordingSession, SessionDraft};
pub use settings::Settings;
