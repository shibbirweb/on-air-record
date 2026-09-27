//! A closed chunk of recorded audio and its index entry.

/// Half open time range, `[start_ms, end_ms)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRange {
    pub start_ms: i64,
    pub end_ms: i64,
}

impl TimeRange {
    pub fn new(start_ms: i64, end_ms: i64) -> Self {
        Self { start_ms, end_ms }
    }

    /// Saturating, because an export range comes straight from the query string and extreme ends would
    /// otherwise overflow: a panic in a debug build, a wrapped duration in a release one.
    pub fn duration_ms(&self) -> i64 {
        self.end_ms.saturating_sub(self.start_ms).max(0)
    }

    pub fn contains(&self, timestamp_ms: i64) -> bool {
        timestamp_ms >= self.start_ms && timestamp_ms < self.end_ms
    }

    pub fn overlaps(&self, other: &TimeRange) -> bool {
        self.start_ms < other.end_ms && other.start_ms < self.end_ms
    }
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub id: i64,
    pub session_id: i64,
    pub sequence: i64,
    /// Local calendar day the segment started in, `YYYY-MM-DD`. Denormalised from `started_at_ms` so it
    /// can be indexed, and so it keeps matching the directory the file was written to even if the host
    /// timezone later changes.
    pub day: String,
    /// Path relative to the configured data directory, so the directory can be relocated.
    pub path: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub sample_rate: u32,
    pub channels: u16,
    pub byte_len: i64,
    /// One byte of normalised RMS per envelope bucket, see `audio::peaks`.
    pub peaks: Vec<u8>,
}

impl Segment {
    pub fn range(&self) -> TimeRange {
        TimeRange::new(self.started_at_ms, self.ended_at_ms)
    }

    pub fn duration_ms(&self) -> i64 {
        self.range().duration_ms()
    }

    pub fn bytes_per_sample_frame(&self) -> i64 {
        2 * self.channels.max(1) as i64
    }

    /// Byte offset inside the segment file for a wall clock timestamp, aligned down to a sample boundary.
    ///
    /// This arithmetic is the reason segments are stored as raw PCM: seeking anywhere inside an hour of
    /// audio costs one multiplication and one `seek`, with no index and no decoder warm up.
    ///
    /// Bounded with `max` and `min` rather than `clamp`, and multiplied saturating, because the row may be
    /// damaged: `clamp` panics when a row ends before it starts or claims a negative length, and a panic
    /// here takes a listener's connection down with it.
    pub fn byte_offset_for(&self, timestamp_ms: i64) -> i64 {
        let clamped = timestamp_ms.min(self.ended_at_ms).max(self.started_at_ms);
        let elapsed_ms = clamped - self.started_at_ms;
        let sample_index = elapsed_ms.saturating_mul(self.sample_rate as i64) / 1000;
        let offset = sample_index.saturating_mul(self.bytes_per_sample_frame());
        offset.min(self.byte_len).max(0)
    }

    /// Wall clock timestamp of a byte offset, the inverse of [`Segment::byte_offset_for`].
    pub fn timestamp_for_offset(&self, byte_offset: i64) -> i64 {
        if self.sample_rate == 0 {
            return self.started_at_ms;
        }
        let sample_index = byte_offset / self.bytes_per_sample_frame();
        self.started_at_ms + (sample_index * 1000) / self.sample_rate as i64
    }
}

/// Values needed to index a segment once its file has been closed.
#[derive(Debug, Clone)]
pub struct SegmentDraft {
    pub session_id: i64,
    pub sequence: i64,
    pub day: String,
    pub path: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub sample_rate: u32,
    pub channels: u16,
    pub byte_len: i64,
    pub peaks: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_segment() -> Segment {
        Segment {
            id: 1,
            session_id: 1,
            sequence: 0,
            day: "2026-09-05".to_string(),
            path: "recordings/2026-09-05/1/000000.pcm".to_string(),
            started_at_ms: 1_000_000,
            ended_at_ms: 1_010_000,
            sample_rate: 48_000,
            channels: 1,
            byte_len: 960_000,
            peaks: vec![0; 100],
        }
    }

    #[test]
    fn offset_at_segment_start_is_zero() {
        assert_eq!(sample_segment().byte_offset_for(1_000_000), 0);
    }

    #[test]
    fn offset_scales_with_elapsed_time() {
        // One second into a 48 kHz mono segment is 48000 samples, so 96000 bytes.
        assert_eq!(sample_segment().byte_offset_for(1_001_000), 96_000);
    }

    #[test]
    fn offset_is_clamped_to_the_file() {
        let segment = sample_segment();
        assert_eq!(segment.byte_offset_for(0), 0);
        assert_eq!(segment.byte_offset_for(i64::MAX), segment.byte_len);
    }

    #[test]
    fn offset_and_timestamp_round_trip() {
        let segment = sample_segment();
        let offset = segment.byte_offset_for(1_003_500);
        assert_eq!(segment.timestamp_for_offset(offset), 1_003_500);
    }

    #[test]
    fn ranges_detect_overlap_and_containment() {
        let range = TimeRange::new(100, 200);
        assert!(range.contains(100));
        assert!(!range.contains(200));
        assert!(range.overlaps(&TimeRange::new(150, 250)));
        assert!(!range.overlaps(&TimeRange::new(200, 300)));
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        /// A segment as the recorder closes one: any rate a device offers, a channel count, up to ten
        /// minutes long, starting anywhere a clock could read, and a file holding exactly the whole sample
        /// frames of that duration.
        fn segment() -> impl Strategy<Value = Segment> {
            (
                prop_oneof![
                    Just(8_000u32),
                    Just(11_025),
                    Just(16_000),
                    Just(22_050),
                    Just(24_000),
                    Just(32_000),
                    Just(44_100),
                    Just(48_000),
                    Just(88_200),
                    Just(96_000),
                    Just(192_000),
                ],
                1u16..=8,
                -4_000_000_000_000i64..8_000_000_000_000,
                1i64..600_000,
            )
                .prop_map(|(sample_rate, channels, started_at_ms, duration_ms)| {
                    let frames = duration_ms * i64::from(sample_rate) / 1000;
                    Segment {
                        id: 1,
                        session_id: 1,
                        sequence: 0,
                        day: "2026-09-05".to_string(),
                        path: "recordings/2026-09-05/1/000000.pcm".to_string(),
                        started_at_ms,
                        ended_at_ms: started_at_ms + duration_ms,
                        sample_rate,
                        channels,
                        byte_len: frames * 2 * i64::from(channels),
                        peaks: Vec::new(),
                    }
                })
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

            /// For any moment, before, inside or after the segment, the offset is inside the file and on a
            /// whole sample frame. An offset one byte off a frame swaps the high and low bytes of every
            /// sample after it, which plays as full scale noise; one past the end reads nothing.
            #[test]
            fn any_offset_is_inside_the_file_and_on_a_sample_frame(
                segment in segment(),
                timestamp_ms in any::<i64>(),
            ) {
                let offset = segment.byte_offset_for(timestamp_ms);
                prop_assert!((0..=segment.byte_len).contains(&offset), "{} outside 0..={}", offset, segment.byte_len);
                prop_assert_eq!(offset % segment.bytes_per_sample_frame(), 0);
            }

            /// Later moments never map to earlier bytes, or seeking forward could replay audio already
            /// heard and a scrub across the segment would jitter backwards.
            #[test]
            fn offsets_never_run_backwards(segment in segment(), first in any::<i64>(), second in any::<i64>()) {
                let (earlier, later) = (first.min(second), first.max(second));
                prop_assert!(segment.byte_offset_for(earlier) <= segment.byte_offset_for(later));
            }

            /// Inside the segment, the offset of a moment reads back as that moment to within the one
            /// millisecond the integer division can lose, and never later than it: playback must not
            /// start after the point a listener asked for.
            #[test]
            fn an_offset_reads_back_as_its_moment(segment in segment(), elapsed in 0.0f64..1.0) {
                let duration_ms = segment.duration_ms();
                let timestamp_ms = segment.started_at_ms + (duration_ms as f64 * elapsed) as i64;
                let back = segment.timestamp_for_offset(segment.byte_offset_for(timestamp_ms));
                prop_assert!(back <= timestamp_ms && timestamp_ms - back <= 1, "{} read back as {}", timestamp_ms, back);
            }

            /// The other way round, an aligned offset read as a moment and turned back into bytes lands on
            /// or just before the same byte, never past it, so a cursor that reports its position and is
            /// sent back there resumes without skipping audio.
            #[test]
            fn a_moment_turns_back_into_its_offset(segment in segment(), fraction in 0.0f64..1.0) {
                let frame = segment.bytes_per_sample_frame();
                let offset = ((segment.byte_len as f64 * fraction) as i64 / frame) * frame;
                let again = segment.byte_offset_for(segment.timestamp_for_offset(offset));
                prop_assert!(again <= offset, "{} came back as {}", offset, again);
                let one_ms = i64::from(segment.sample_rate) / 1000 * frame + frame;
                prop_assert!(offset - again <= one_ms, "{} came back as {}", offset, again);
            }
        }
    }
}
