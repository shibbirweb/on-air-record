//! Canonical WAV headers.
//!
//! Segments are already signed 16 bit little endian PCM, which is exactly what a canonical WAV file
//! carries, so an export is the 44 byte header followed by the samples with nothing done to them.
//!
//! The header states the data length up front, which means the length has to be known before a single
//! byte is sent. That is why an export decides its whole plan first, and it is only possible because
//! uncompressed audio has an exactly predictable size.

/// Size of the canonical header, before any audio.
pub const HEADER_BYTES: usize = 44;

/// Largest data chunk a WAV file can describe.
///
/// RIFF sizes are unsigned 32 bit, so four gigabytes is a hard format limit rather than a policy choice.
/// Exports are capped well below it, but the arithmetic here still has to be checked.
pub const MAX_DATA_BYTES: u64 = u32::MAX as u64 - HEADER_BYTES as u64;

/// Build the header for a stream of signed 16 bit little endian samples.
///
/// `data_bytes` is the size of the audio that will follow, which the caller must already know.
pub fn header(sample_rate: u32, channels: u16, data_bytes: u32) -> [u8; HEADER_BYTES] {
    let channels = channels.max(1);
    let bits_per_sample: u16 = 16;
    let block_align = channels * bits_per_sample / 8;
    let byte_rate = sample_rate * block_align as u32;

    let mut out = [0u8; HEADER_BYTES];

    out[0..4].copy_from_slice(b"RIFF");
    // Everything after this field: the remaining 36 bytes of header plus the audio.
    out[4..8].copy_from_slice(&(36u32.saturating_add(data_bytes)).to_le_bytes());
    out[8..12].copy_from_slice(b"WAVE");

    out[12..16].copy_from_slice(b"fmt ");
    out[16..20].copy_from_slice(&16u32.to_le_bytes()); // PCM format chunk length
    out[20..22].copy_from_slice(&1u16.to_le_bytes()); // 1 = uncompressed PCM
    out[22..24].copy_from_slice(&channels.to_le_bytes());
    out[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    out[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    out[32..34].copy_from_slice(&block_align.to_le_bytes());
    out[34..36].copy_from_slice(&bits_per_sample.to_le_bytes());

    out[36..40].copy_from_slice(b"data");
    out[40..44].copy_from_slice(&data_bytes.to_le_bytes());

    out
}

/// Bytes a span of audio occupies, or `None` when it would overflow what WAV can describe.
pub fn data_bytes_for(sample_rate: u32, channels: u16, duration_ms: i64) -> Option<u64> {
    if duration_ms <= 0 {
        return Some(0);
    }

    let samples = (duration_ms as u64).checked_mul(sample_rate as u64)? / 1000;
    let bytes = samples
        .checked_mul(channels.max(1) as u64)?
        .checked_mul(2)?;

    (bytes <= MAX_DATA_BYTES).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_u32(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
    }

    fn read_u16(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes(bytes[at..at + 2].try_into().expect("two bytes"))
    }

    #[test]
    fn writes_the_expected_chunk_layout() {
        let out = header(48_000, 1, 960_000);

        assert_eq!(&out[0..4], b"RIFF");
        assert_eq!(&out[8..12], b"WAVE");
        assert_eq!(&out[12..16], b"fmt ");
        assert_eq!(&out[36..40], b"data");
        assert_eq!(out.len(), HEADER_BYTES);
    }

    #[test]
    fn describes_mono_48k_correctly() {
        let out = header(48_000, 1, 960_000);

        assert_eq!(read_u32(&out, 4), 960_000 + 36);
        assert_eq!(read_u32(&out, 16), 16);
        assert_eq!(read_u16(&out, 20), 1);
        assert_eq!(read_u16(&out, 22), 1);
        assert_eq!(read_u32(&out, 24), 48_000);
        // Byte rate is rate times block align: 48000 * 2 for mono 16 bit.
        assert_eq!(read_u32(&out, 28), 96_000);
        assert_eq!(read_u16(&out, 32), 2);
        assert_eq!(read_u16(&out, 34), 16);
        assert_eq!(read_u32(&out, 40), 960_000);
    }

    #[test]
    fn block_align_and_byte_rate_follow_the_channel_count() {
        let stereo = header(44_100, 2, 0);
        assert_eq!(read_u16(&stereo, 32), 4);
        assert_eq!(read_u32(&stereo, 28), 44_100 * 4);
    }

    #[test]
    fn a_zero_channel_count_is_treated_as_mono() {
        // Never emit a header describing zero channels, which no player can open.
        assert_eq!(read_u16(&header(48_000, 0, 0), 22), 1);
    }

    #[test]
    fn data_size_matches_the_duration() {
        // One second of mono 48 kHz is 96000 bytes.
        assert_eq!(data_bytes_for(48_000, 1, 1_000), Some(96_000));
        assert_eq!(data_bytes_for(16_000, 1, 60_000), Some(16_000 * 2 * 60));
        assert_eq!(data_bytes_for(48_000, 2, 1_000), Some(192_000));
    }

    #[test]
    fn an_empty_or_backwards_span_is_empty() {
        assert_eq!(data_bytes_for(48_000, 1, 0), Some(0));
        assert_eq!(data_bytes_for(48_000, 1, -5_000), Some(0));
    }

    #[test]
    fn a_span_too_large_for_the_format_is_refused() {
        // Roughly thirteen hours of 48 kHz mono exceeds what a RIFF size field can express.
        assert_eq!(data_bytes_for(48_000, 1, 1_000 * 60 * 60 * 13), None);
        assert!(data_bytes_for(48_000, 1, 1_000 * 60 * 60 * 6).is_some());
    }
}
