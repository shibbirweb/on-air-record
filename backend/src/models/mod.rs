//! Domain entities.
//!
//! These types describe the vocabulary of the application and know nothing about SQL, HTTP, or the audio
//! host. Repositories map rows onto them and DTOs project them onto the wire.

pub mod audio_frame;
pub mod bookmark;
pub mod capture_state;
pub mod device;
pub mod listener;
pub mod segment;
pub mod session;
pub mod settings;
pub mod update;
pub mod user;

pub use audio_frame::AudioFrame;
pub use bookmark::{Bookmark, BookmarkDraft, BookmarkPatch};
pub use capture_state::{CaptureSnapshot, CaptureState, LevelSnapshot};
pub use device::InputDevice;
pub use listener::{ListenerAccount, ListenerActivity, ListenerEntry, PlayerState};
pub use segment::{Segment, SegmentDraft, TimeRange};
pub use session::{RecordingSession, SessionDraft};
pub use settings::{Settings, SettingsPatch};
pub use user::{authorize, Access, AuthMode, Denied, Role, User};
