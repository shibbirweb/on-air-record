//! Frame encoding strategy.
//!
//! The recorder and the streaming layer both need bytes out of an [`AudioFrame`], and both should stay
//! ignorant of how those bytes are produced. Encoding therefore sits behind a trait: today the only
//! implementation writes signed 16 bit PCM, and a compressed implementation can be added later without
//! touching a single caller. The wire header carries [`FrameFormat`] so a client always knows how to
//! interpret the payload it just received.

use std::sync::Arc;

use crate::models::AudioFrame;

/// Payload encodings, numbered because the value goes on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameFormat {
    /// Interleaved signed 16 bit little endian samples.
    PcmS16 = 0,
}

impl FrameFormat {
    pub fn wire_code(self) -> u8 {
        self as u8
    }

    pub fn from_wire_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::PcmS16),
            _ => None,
        }
    }
}

pub trait FrameEncoder: Send + Sync {
    fn format(&self) -> FrameFormat;

    /// Encode the payload of a single frame. The header is added by the transport, not here, because the
    /// same encoded bytes are written to a segment file where no header is wanted.
    fn encode(&self, frame: &AudioFrame) -> Vec<u8>;
}

/// Pass through encoder: the in memory representation already is little endian PCM.
pub struct PcmS16Encoder;

impl FrameEncoder for PcmS16Encoder {
    fn format(&self) -> FrameFormat {
        FrameFormat::PcmS16
    }

    fn encode(&self, frame: &AudioFrame) -> Vec<u8> {
        frame.to_le_bytes()
    }
}

/// Factory for the configured encoder.
pub fn build_encoder(format: FrameFormat) -> Arc<dyn FrameEncoder> {
    match format {
        FrameFormat::PcmS16 => Arc::new(PcmS16Encoder),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_codes_round_trip() {
        assert_eq!(
            FrameFormat::from_wire_code(FrameFormat::PcmS16.wire_code()),
            Some(FrameFormat::PcmS16)
        );
        assert_eq!(FrameFormat::from_wire_code(99), None);
    }

    #[test]
    fn pcm_encoder_emits_two_bytes_per_sample() {
        let encoder = build_encoder(FrameFormat::PcmS16);
        let frame = AudioFrame::from_samples(0, 48_000, 1, vec![256, -256], true);
        assert_eq!(encoder.encode(&frame), vec![0x00, 0x01, 0x00, 0xff]);
    }
}
