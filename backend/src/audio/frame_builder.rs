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

    mod props {
        use super::*;
        use proptest::prelude::*;

        const RATES: [u32; 7] = [8_000, 11_025, 16_000, 22_050, 44_100, 48_000, 96_000];

        /// A capture session: a rate, a frame length inside the settings' range, where the wall clock
        /// stood when it began, and the buffers the driver delivered, each with how late the callback
        /// ran relative to the audio it carried, within ordinary scheduling jitter.
        ///
        /// Buffers are at most 100 ms of audio, which is already generous for a driver. The drift check
        /// compares the clock at the end of a buffer with the end of each frame cut from it, so buffers
        /// approaching `MAX_DRIFT_MS` long would be re anchored mid buffer by design of that comparison.
        fn session() -> impl Strategy<Value = (u32, u32, i64, Vec<(usize, i64)>)> {
            (
                proptest::sample::select(RATES.to_vec()),
                20u32..=500,
                0i64..4_000_000_000_000,
                proptest::collection::vec((1usize..=100, 0i64..100), 0..60),
            )
                .prop_map(|(sample_rate, frame_ms, start_ms, buffers)| {
                    let buffers = buffers
                        .into_iter()
                        .map(|(buffer_ms, late_ms)| {
                            ((buffer_ms * sample_rate as usize / 1000).max(1), late_ms)
                        })
                        .collect();
                    (sample_rate, frame_ms, start_ms, buffers)
                })
        }

        /// Run a session through a builder, returning the frames and the samples fed in, in order.
        fn capture(
            sample_rate: u32,
            frame_ms: u32,
            start_ms: i64,
            buffers: &[(usize, i64)],
        ) -> (FrameBuilder, Vec<AudioFrame>, Vec<i16>) {
            let mut builder = FrameBuilder::new(sample_rate, 1, frame_ms);
            let mut frames = Vec::new();
            let mut fed: Vec<i16> = Vec::new();
            for (length, late_ms) in buffers {
                // Distinct values, so a lost, repeated or reordered sample shows up in the comparison.
                let buffer: Vec<i16> = (fed.len()..fed.len() + length)
                    .map(|index| index as i16)
                    .collect();
                fed.extend_from_slice(&buffer);
                // The wall clock when the driver delivered this buffer: the audio's own time plus lateness.
                let now_ms =
                    start_ms + (fed.len() as i64 * 1000) / i64::from(sample_rate) + late_ms;
                builder.push(&buffer, now_ms, |frame| frames.push(frame));
            }
            (builder, frames, fed)
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

            /// However the driver slices the audio, every frame is exactly the configured size, and laid
            /// end to end the frames and the one short tail `flush` returns are exactly the samples fed
            /// in: none lost, none repeated, none reordered. A dropped sample is a click in the recording
            /// and shifts every byte offset after it in the segment.
            #[test]
            fn frames_are_fixed_size_and_lose_nothing((sample_rate, frame_ms, start_ms, buffers) in session()) {
                let (mut builder, mut frames, fed) = capture(sample_rate, frame_ms, start_ms, &buffers);
                let size = frame_samples_for(sample_rate, frame_ms);
                prop_assert!(frames.iter().all(|frame| frame.samples.len() == size));

                let complete = frames.len();
                builder.flush(i64::MAX / 2, |frame| frames.push(frame));
                prop_assert!(frames.len() - complete <= 1);
                if let Some(tail) = frames.get(complete) {
                    prop_assert!(!tail.samples.is_empty() && tail.samples.len() < size);
                }

                let joined: Vec<i16> = frames.iter().flat_map(|frame| frame.samples.iter().copied()).collect();
                prop_assert_eq!(joined, fed);
            }

            /// With the clock running at the audio's own pace, give or take a tenth of a second of callback
            /// jitter, the stream is never re anchored, and every frame is stamped from the samples that
            /// came before it: the first sample's time plus their count at the sample rate, to the
            /// millisecond. That is what keeps timestamps contiguous across frames rather than drifting
            /// with the callback schedule, and is what the recorder's discontinuity check relies on.
            #[test]
            fn timestamps_follow_the_sample_count((sample_rate, frame_ms, start_ms, buffers) in session()) {
                let (builder, frames, _) = capture(sample_rate, frame_ms, start_ms, &buffers);
                prop_assert_eq!(builder.drift_resets(), 0);

                let Some(first) = frames.first() else {
                    return Ok(());
                };
                let size = frame_samples_for(sample_rate, frame_ms) as i64;
                let anchor_ms = first.timestamp_ms;
                for (index, frame) in frames.iter().enumerate() {
                    let before = index as i64 * size;
                    prop_assert_eq!(frame.timestamp_ms, anchor_ms + before * 1000 / i64::from(sample_rate));
                    prop_assert!(frame.timestamp_ms <= frame.end_timestamp_ms());
                }
                for pair in frames.windows(2) {
                    let step = pair[1].timestamp_ms - pair[0].end_timestamp_ms();
                    prop_assert!((0..=1).contains(&step), "a {} ms seam between frames", step);
                }
            }

            /// The first frame is back dated to when its audio was captured, never stamped after the
            /// buffer that completed it arrived, and never earlier than jitter can explain.
            #[test]
            fn the_first_frame_is_stamped_when_its_audio_began((sample_rate, frame_ms, start_ms, buffers) in session()) {
                let (_, frames, _) = capture(sample_rate, frame_ms, start_ms, &buffers);
                if let Some(first) = frames.first() {
                    let late_ms = buffers.first().map(|(_, late_ms)| *late_ms).unwrap_or(0);
                    // The anchor is the first buffer's arrival less its own duration, which rounds down.
                    prop_assert!(first.timestamp_ms >= start_ms - 1 && first.timestamp_ms <= start_ms + late_ms + 1,
                        "stamped {} for audio that began at {}", first.timestamp_ms, start_ms);
                }
            }
        }
    }
}
