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

    pub fn duration_ms(&self) -> i64 {
        (self.end_ms - self.start_ms).max(0)
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
    pub fn byte_offset_for(&self, timestamp_ms: i64) -> i64 {
        let clamped = timestamp_ms.clamp(self.started_at_ms, self.ended_at_ms);
        let elapsed_ms = clamped - self.started_at_ms;
        let sample_index = (elapsed_ms * self.sample_rate as i64) / 1000;
        let offset = sample_index * self.bytes_per_sample_frame();
        offset.clamp(0, self.byte_len)
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
            path: "recordings/1/000000.pcm".to_string(),
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
}
