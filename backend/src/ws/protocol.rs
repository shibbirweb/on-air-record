//! Binary framing for audio messages.
//!
//! A fixed 24 byte little endian header in front of the payload. Little endian because every platform
//! this runs on is little endian and `DataView` in the browser reads it with one flag, and fixed size
//! because a browser parsing a variable header per frame ten times a second is wasted work.
//!
//! The header repeats the sample rate and channel count on every frame rather than stating them once at
//! connect time. That costs eight bytes per frame and buys the ability to change the input device mid
//! stream, which would otherwise silently reinterpret the audio at the wrong rate.

use crate::audio::FrameEncoder;
use crate::models::AudioFrame;

/// The ASCII bytes `OAR1` read as a little endian `u32`.
pub const MAGIC: u32 = 0x3152_414F;
pub const PROTOCOL_VERSION: u8 = 1;
pub const HEADER_LEN: usize = 24;

/// Set when the frame is arriving from the microphone right now, clear when it was replayed from disk.
pub const FLAG_LIVE: u8 = 0b0000_0001;

/// Parsed header, used by the decoder and by the tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub version: u8,
    pub format_code: u8,
    pub channels: u8,
    pub flags: u8,
    pub sample_rate: u32,
    pub sample_count: u32,
    pub timestamp_ms: i64,
}

impl FrameHeader {
    pub fn is_live(&self) -> bool {
        self.flags & FLAG_LIVE != 0
    }
}

/// Serialise a frame into one binary WebSocket message.
pub fn encode_audio_frame(frame: &AudioFrame, encoder: &dyn FrameEncoder) -> Vec<u8> {
    let payload = encoder.encode(frame);
    let mut message = Vec::with_capacity(HEADER_LEN + payload.len());

    message.extend_from_slice(&MAGIC.to_le_bytes());
    message.push(PROTOCOL_VERSION);
    message.push(encoder.format().wire_code());
    message.push(frame.channels.min(u8::MAX as u16) as u8);
    message.push(if frame.live { FLAG_LIVE } else { 0 });
    message.extend_from_slice(&frame.sample_rate.to_le_bytes());
    message.extend_from_slice(&(frame.sample_count() as u32).to_le_bytes());
    message.extend_from_slice(&frame.timestamp_ms.to_le_bytes());
    message.extend_from_slice(&payload);

    message
}

/// Parse a header, returning `None` when the message is too short or is not ours.
pub fn decode_header(message: &[u8]) -> Option<FrameHeader> {
    if message.len() < HEADER_LEN {
        return None;
    }

    let magic = u32::from_le_bytes(message[0..4].try_into().ok()?);
    if magic != MAGIC {
        return None;
    }

    Some(FrameHeader {
        version: message[4],
        format_code: message[5],
        channels: message[6],
        flags: message[7],
        sample_rate: u32::from_le_bytes(message[8..12].try_into().ok()?),
        sample_count: u32::from_le_bytes(message[12..16].try_into().ok()?),
        timestamp_ms: i64::from_le_bytes(message[16..24].try_into().ok()?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::PcmS16Encoder;

    #[test]
    fn magic_spells_oar1() {
        assert_eq!(&MAGIC.to_le_bytes(), b"OAR1");
    }

    #[test]
    fn header_round_trips() {
        let frame = AudioFrame::from_samples(1_757_030_400_000, 48_000, 1, vec![0; 4800], true);
        let message = encode_audio_frame(&frame, &PcmS16Encoder);

        let header = decode_header(&message).expect("header");
        assert_eq!(header.version, PROTOCOL_VERSION);
        assert_eq!(header.format_code, 0);
        assert_eq!(header.channels, 1);
        assert_eq!(header.sample_rate, 48_000);
        assert_eq!(header.sample_count, 4800);
        assert_eq!(header.timestamp_ms, 1_757_030_400_000);
        assert!(header.is_live());
        assert_eq!(message.len(), HEADER_LEN + 4800 * 2);
    }

    #[test]
    fn historic_frames_clear_the_live_flag() {
        let frame = AudioFrame::from_samples(0, 48_000, 1, vec![0; 10], false);
        let header = decode_header(&encode_audio_frame(&frame, &PcmS16Encoder)).expect("header");
        assert!(!header.is_live());
    }

    #[test]
    fn foreign_and_short_messages_are_rejected() {
        assert!(decode_header(&[0u8; 8]).is_none());
        assert!(decode_header(&[0xffu8; HEADER_LEN]).is_none());
    }

    #[test]
    fn payload_follows_the_header_unchanged() {
        let frame = AudioFrame::from_samples(0, 48_000, 1, vec![1, -2], true);
        let message = encode_audio_frame(&frame, &PcmS16Encoder);
        assert_eq!(&message[HEADER_LEN..], &[0x01, 0x00, 0xfe, 0xff]);
    }
}
