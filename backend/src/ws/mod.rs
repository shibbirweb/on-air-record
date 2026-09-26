//! WebSocket streaming.
//!
//! One socket carries the live broadcast and DVR playback, because a listener moving between the two is
//! the normal case and a second connection would mean two audio pipelines to keep in sync.

pub mod messages;
pub mod protocol;
pub mod session;

pub use messages::{ClientMessage, ServerMessage, StreamMode};
pub use protocol::{encode_audio_frame, FrameHeader, HEADER_LEN, MAGIC, PROTOCOL_VERSION};
pub use session::{ListenerIdentity, StreamSession};
