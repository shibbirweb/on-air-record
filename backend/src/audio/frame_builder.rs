//! Turns the irregular buffers a device hands us into fixed duration frames with sane timestamps.
//!
//! A capture callback delivers whatever the driver felt like delivering: 256 samples, then 512, then 240.
//! Everything downstream wants a steady heartbeat, so this accumulator buffers samples and emits a frame
//! whenever it has a full one.
//!
//! Timestamps are derived from the accumulated sample count rather than read from the clock once per
//! frame. Reading the clock per frame would inherit the jitter of the callback scheduling, while counting
//! samples alone would slowly drift away from the wall clock because the audio crystal and the system
//! clock never agree exactly. The compromise is to count samples and re anchor to the wall clock only when
//! the two disagree by more than [`MAX_DRIFT_MS`].

use crate::models::AudioFrame;

/// How far the sample derived clock may run from the wall clock before it is re anchored.
///
/// Half a second is large enough that normal scheduling jitter never triggers it and small enough that a
/// re anchor stays inside one timeline pixel at any sensible zoom level.
pub const MAX_DRIFT_MS: i64 = 500;

pub struct FrameBuilder {
    sample_rate: u32,
    channels: u16,
    frame_samples: usize,
    buffer: Vec<i16>,
    /// Wall clock time of the sample at index zero since the last anchor.
    anchor_ms: i64,
    samples_since_anchor: u64,
    anchored: bool,
    drift_resets: u64,
}

impl FrameBuilder {
    pub fn new(sample_rate: u32, channels: u16, frame_ms: u32) -> Self {
        let frame_samples = frame_samples_for(sample_rate, frame_ms);
        Self {
            sample_rate,
            channels,
            frame_samples,
            buffer: Vec::with_capacity(frame_samples),
            anchor_ms: 0,
            samples_since_anchor: 0,
            anchored: false,
            drift_resets: 0,
        }
    }

    pub fn frame_samples(&self) -> usize {
        self.frame_samples
    }

    pub fn drift_resets(&self) -> u64 {
        self.drift_resets
    }

    /// Feed mono samples and emit every complete frame through `emit`.
    ///
    /// `now_ms` is the wall clock at the moment the driver delivered this buffer.
    pub fn push(&mut self, samples: &[i16], now_ms: i64, mut emit: impl FnMut(AudioFrame)) {
        if self.frame_samples == 0 {
            return;
        }

        if !self.anchored {
            // The buffer we were just handed was recorded before now, so back date the anchor by its own
            // duration. Without this every session starts a fraction of a second in the future.
            self.anchor_ms = now_ms - self.samples_to_ms(samples.len() as u64);
            self.samples_since_anchor = 0;
            self.anchored = true;
        }

        for sample in samples {
            self.buffer.push(*sample);
            if self.buffer.len() >= self.frame_samples {
                self.flush_frame(now_ms, &mut emit);
            }
        }
    }

    /// Emit whatever is buffered, even if it is a short frame. Used when capture stops so the tail of the
    /// recording is not lost.
    pub fn flush(&mut self, now_ms: i64, mut emit: impl FnMut(AudioFrame)) {
        if !self.buffer.is_empty() {
            self.flush_frame(now_ms, &mut emit);
        }
    }

    fn flush_frame(&mut self, now_ms: i64, emit: &mut impl FnMut(AudioFrame)) {
        let samples = std::mem::replace(&mut self.buffer, Vec::with_capacity(self.frame_samples));
        let emitted = samples.len() as u64;
        let timestamp_ms = self.anchor_ms + self.samples_to_ms(self.samples_since_anchor);

        emit(AudioFrame::from_samples(
            timestamp_ms,
            self.sample_rate,
            self.channels,
            samples,
            true,
        ));

        self.samples_since_anchor += emitted;
        self.correct_drift(now_ms);
    }

    /// Re anchor when the sample derived clock and the wall clock have separated too far.
    fn correct_drift(&mut self, now_ms: i64) {
        let derived_now_ms = self.anchor_ms + self.samples_to_ms(self.samples_since_anchor);
        if (derived_now_ms - now_ms).abs() > MAX_DRIFT_MS {
            tracing::debug!(
                drift_ms = derived_now_ms - now_ms,
                "re anchoring capture clock to wall clock"
            );
            self.anchor_ms = now_ms;
            self.samples_since_anchor = 0;
            self.drift_resets += 1;
        }
    }

    fn samples_to_ms(&self, samples: u64) -> i64 {
        if self.sample_rate == 0 {
            return 0;
        }
        ((samples * 1000) / self.sample_rate as u64) as i64
    }
}

/// Samples per channel in a frame of `frame_ms` at `sample_rate`, never zero.
pub fn frame_samples_for(sample_rate: u32, frame_ms: u32) -> usize {
    let samples = (sample_rate as u64 * frame_ms as u64) / 1000;
    samples.max(1) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_size_follows_sample_rate_and_duration() {
        assert_eq!(frame_samples_for(48_000, 100), 4800);
        assert_eq!(frame_samples_for(44_100, 20), 882);
        assert_eq!(frame_samples_for(0, 100), 1);
    }

    #[test]
    fn emits_only_complete_frames() {
        let mut builder = FrameBuilder::new(48_000, 1, 100);
        let mut frames = Vec::new();

        builder.push(&vec![0; 2000], 10_000, |frame| frames.push(frame));
        assert!(frames.is_empty());

        builder.push(&vec![0; 3000], 10_020, |frame| frames.push(frame));
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].sample_count(), 4800);
    }

    #[test]
    fn frame_timestamps_advance_by_the_frame_duration() {
        let mut builder = FrameBuilder::new(48_000, 1, 100);
        let mut frames = Vec::new();

        // Feed exactly three frames in one buffer, delivered at a plausible wall clock.
        builder.push(&vec![0; 14_400], 1_000_300, |frame| frames.push(frame));

        assert_eq!(frames.len(), 3);
        assert_eq!(frames[1].timestamp_ms - frames[0].timestamp_ms, 100);
        assert_eq!(frames[2].timestamp_ms - frames[1].timestamp_ms, 100);
    }

    #[test]
    fn first_frame_is_back_dated_by_the_buffer_duration() {
        let mut builder = FrameBuilder::new(48_000, 1, 100);
        let mut frames = Vec::new();

        // A 100 ms buffer delivered at t = 1_000_000 was recorded from t = 999_900.
        builder.push(&vec![0; 4800], 1_000_000, |frame| frames.push(frame));

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].timestamp_ms, 999_900);
    }

    #[test]
    fn large_wall_clock_jump_re_anchors_the_stream() {
        let mut builder = FrameBuilder::new(48_000, 1, 100);
        let mut frames = Vec::new();

        builder.push(&vec![0; 4800], 1_000_000, |frame| frames.push(frame));
        assert_eq!(builder.drift_resets(), 0);

        // The machine slept for a minute. The next buffer arrives far in the future.
        builder.push(&vec![0; 4800], 1_060_000, |frame| frames.push(frame));

        // The frame in flight keeps its sample derived timestamp, the re anchor applies from the next one.
        assert_eq!(builder.drift_resets(), 1);
        assert_eq!(frames[1].timestamp_ms, 1_000_000);

        builder.push(&vec![0; 4800], 1_060_100, |frame| frames.push(frame));
        assert_eq!(frames[2].timestamp_ms, 1_060_000);
        assert_eq!(builder.drift_resets(), 1);
    }

    #[test]
    fn flush_emits_a_partial_frame() {
        let mut builder = FrameBuilder::new(48_000, 1, 100);
        let mut frames = Vec::new();

        builder.push(&vec![0; 1000], 1_000_000, |frame| frames.push(frame));
        builder.flush(1_000_021, |frame| frames.push(frame));

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].sample_count(), 1000);
    }
}
