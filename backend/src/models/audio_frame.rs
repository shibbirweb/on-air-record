//! A fixed duration slice of captured audio.
//!
//! Frames are the unit of everything: the broadcast hub publishes them, the recorder appends them, the
//! WebSocket layer serialises them, and the timeline indexes their timestamps. Samples sit behind an `Arc`
//! because a single frame is cloned once per listener on every tick, and copying the payload for each
//! subscriber would dominate the cost of the fan out.

use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AudioFrame {
    /// Start of the frame, milliseconds since the Unix epoch.
    pub timestamp_ms: i64,
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved signed 16 bit samples. Mono in the current pipeline, but the field carries the channel
    /// count so the format is not baked into the protocol.
    pub samples: Arc<Vec<i16>>,
    /// False when the frame was replayed from disk rather than captured just now.
    pub live: bool,
    /// Root mean square amplitude of the frame, normalised to `0.0..1.0`.
    pub rms: f32,
    /// Largest absolute amplitude in the frame, normalised to `0.0..1.0`.
    pub peak: f32,
}

impl AudioFrame {
    /// Build a frame from mono samples and compute its level statistics in the same pass.
    ///
    /// The levels are computed here rather than on demand because the samples are already in cache at this
    /// point, and every consumer (meter, waveform, peak envelope) would otherwise walk the buffer again.
    pub fn from_samples(
        timestamp_ms: i64,
        sample_rate: u32,
        channels: u16,
        samples: Vec<i16>,
        live: bool,
    ) -> Self {
        let (rms, peak) = level_of(&samples);
        Self {
            timestamp_ms,
            sample_rate,
            channels,
            samples: Arc::new(samples),
            live,
            rms,
            peak,
        }
    }

    /// Number of samples per channel.
    pub fn sample_count(&self) -> usize {
        let channels = self.channels.max(1) as usize;
        self.samples.len() / channels
    }

    /// Playing duration of the frame in milliseconds.
    pub fn duration_ms(&self) -> i64 {
        if self.sample_rate == 0 {
            return 0;
        }
        (self.sample_count() as i64 * 1000) / self.sample_rate as i64
    }

    /// Timestamp immediately after the last sample, the start of the next frame.
    pub fn end_timestamp_ms(&self) -> i64 {
        self.timestamp_ms + self.duration_ms()
    }

    /// Little endian byte view of the payload, ready for the wire or for the segment file.
    pub fn to_le_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.samples.len() * 2);
        for sample in self.samples.iter() {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }
}

/// Root mean square and peak amplitude of a buffer, both normalised to `0.0..1.0`.
pub fn level_of(samples: &[i16]) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }

    let mut sum_squares = 0.0f64;
    let mut peak = 0.0f32;
    for sample in samples {
        let normalised = *sample as f32 / i16::MAX as f32;
        sum_squares += (normalised * normalised) as f64;
        let magnitude = normalised.abs();
        if magnitude > peak {
            peak = magnitude;
        }
    }

    let rms = (sum_squares / samples.len() as f64).sqrt() as f32;
    (rms.clamp(0.0, 1.0), peak.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_matches_sample_rate() {
        let frame = AudioFrame::from_samples(1000, 48_000, 1, vec![0; 4800], true);
        assert_eq!(frame.sample_count(), 4800);
        assert_eq!(frame.duration_ms(), 100);
        assert_eq!(frame.end_timestamp_ms(), 1100);
    }

    #[test]
    fn levels_of_silence_are_zero() {
        let (rms, peak) = level_of(&[0, 0, 0, 0]);
        assert_eq!(rms, 0.0);
        assert_eq!(peak, 0.0);
    }

    #[test]
    fn levels_of_full_scale_are_one() {
        let (rms, peak) = level_of(&[i16::MAX, -i16::MAX, i16::MAX, -i16::MAX]);
        assert!((rms - 1.0).abs() < 0.001);
        assert!((peak - 1.0).abs() < 0.001);
    }

    #[test]
    fn payload_is_little_endian() {
        let frame = AudioFrame::from_samples(0, 48_000, 1, vec![1, -1], true);
        assert_eq!(frame.to_le_bytes(), vec![0x01, 0x00, 0xff, 0xff]);
    }

    #[test]
    fn empty_frame_has_no_duration() {
        let frame = AudioFrame::from_samples(0, 48_000, 1, Vec::new(), true);
        assert_eq!(frame.duration_ms(), 0);
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

            /// For any samples, both levels are inside `0..=1`, the peak is the loudest sample and the RMS
            /// never exceeds it. The meter and the stored envelope scale these straight to pixels and bytes,
            /// and `i16::MIN` is one step louder than `i16::MAX`, the case a plain division gets wrong.
            #[test]
            fn levels_are_normalised_and_ordered(samples in proptest::collection::vec(any::<i16>(), 0..2_000)) {
                let (rms, peak) = level_of(&samples);
                prop_assert!((0.0..=1.0).contains(&rms) && (0.0..=1.0).contains(&peak));
                prop_assert!(rms <= peak + 1e-6, "rms {} above peak {}", rms, peak);
                let loudest = samples.iter().map(|sample| f32::from(sample.unsigned_abs())).fold(0.0, f32::max);
                prop_assert!((peak - (loudest / f32::from(i16::MAX)).min(1.0)).abs() < 1e-6);
            }

            /// A frame's end is its start plus the whole milliseconds its samples last, per channel, at any
            /// rate and channel count, so the recorder's discontinuity check and the live edge read the same
            /// clock the frame builder stamped.
            #[test]
            fn duration_follows_samples_per_channel(
                timestamp_ms in -1_000_000_000_000i64..4_000_000_000_000,
                sample_rate in 0u32..400_000,
                channels in 0u16..=8,
                length in 0usize..20_000,
            ) {
                let frame = AudioFrame::from_samples(timestamp_ms, sample_rate, channels, vec![0; length], true);
                let per_channel = length / usize::from(channels.max(1));
                prop_assert_eq!(frame.sample_count(), per_channel);
                let expected = if sample_rate == 0 { 0 } else { per_channel as i64 * 1000 / i64::from(sample_rate) };
                prop_assert_eq!(frame.duration_ms(), expected);
                prop_assert_eq!(frame.end_timestamp_ms(), timestamp_ms + expected);
                prop_assert_eq!(frame.to_le_bytes().len(), 2 * length);
            }
        }
    }
}
