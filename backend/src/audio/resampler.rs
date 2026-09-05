//! Downsampling for the capture path.
//!
//! Segments are uncompressed PCM, so the bytes on disk are exactly
//! `sample_rate * channels * 2` per second. With mono fixed and the 16 bit depth load bearing for the
//! seek arithmetic, the sample rate is the only lever on storage, and halving it halves the disk.
//!
//! Two stages, both allocation free so they can run on the audio callback thread:
//!
//! 1. A low pass filter at just under the target's Nyquist frequency. Skipping it would fold every
//!    frequency above the new Nyquist back down into the audible band as aliasing, which sounds far worse
//!    than the honest loss of treble that filtering costs.
//! 2. Linear interpolation at the rate ratio.
//!
//! Linear interpolation rather than a windowed sinc: this is a speech and monitoring recorder, the anti
//! alias filter has already removed the content that interpolation error would be most audible on, and a
//! polyphase resampler would need buffering that does not fit a callback delivering variable chunk sizes.
//! For archival grade resampling of music this would be the wrong choice, and that is a fair trade for a
//! setting whose entire purpose is to spend quality on disk space.

use std::f32::consts::PI;

/// Cutoff as a fraction of the output sample rate.
///
/// Nyquist is 0.5, so 0.45 leaves a little transition band for the filter to roll off in rather than
/// trying to be brick wall at exactly the limit.
const CUTOFF_RATIO: f32 = 0.45;

/// One biquad section in direct form 1.
#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    /// Low pass from the audio EQ cookbook, normalised so `a0` is one.
    fn low_pass(sample_rate: f32, cutoff_hz: f32, q: f32) -> Self {
        let w0 = 2.0 * PI * (cutoff_hz / sample_rate).clamp(0.0001, 0.4999);
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q);

        let a0 = 1.0 + alpha;
        Self {
            b0: ((1.0 - cos_w0) / 2.0) / a0,
            b1: (1.0 - cos_w0) / a0,
            b2: ((1.0 - cos_w0) / 2.0) / a0,
            a1: (-2.0 * cos_w0) / a0,
            a2: (1.0 - alpha) / a0,
            ..Self::default()
        }
    }

    fn process(&mut self, x0: f32) -> f32 {
        let y0 = self.b0 * x0 + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;

        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        self.y1 = y0;

        y0
    }
}

/// Converts a mono stream from one sample rate to a lower one.
pub struct Resampler {
    input_rate: u32,
    output_rate: u32,
    /// Two cascaded biquads, giving a fourth order rolloff. One section alone leaks enough above the
    /// cutoff to alias audibly when the ratio is large, as it is going from 48 kHz to 8 kHz.
    stages: [Biquad; 2],
    /// Position of the next output sample in input samples, carried across calls so chunk boundaries
    /// do not click.
    position: f32,
    /// Filtered value of the previous input sample, the left hand side of the interpolation.
    previous: f32,
    primed: bool,
}

impl Resampler {
    /// Build a resampler, or `None` when the rates match and the audio should pass through untouched.
    pub fn new(input_rate: u32, output_rate: u32) -> Option<Self> {
        if input_rate == 0 || output_rate == 0 || output_rate >= input_rate {
            return None;
        }

        let cutoff_hz = output_rate as f32 * CUTOFF_RATIO;
        // Butterworth Q values for a fourth order response built from two sections.
        let stages = [
            Biquad::low_pass(input_rate as f32, cutoff_hz, 0.541),
            Biquad::low_pass(input_rate as f32, cutoff_hz, 1.307),
        ];

        Some(Self {
            input_rate,
            output_rate,
            stages,
            position: 0.0,
            previous: 0.0,
            primed: false,
        })
    }

    pub fn output_rate(&self) -> u32 {
        self.output_rate
    }

    /// How many input samples advance one output sample.
    fn step(&self) -> f32 {
        self.input_rate as f32 / self.output_rate as f32
    }

    /// Filter and resample `input`, appending to `output`.
    ///
    /// `output` is reused by the caller and is not cleared here, so the caller controls allocation.
    pub fn process(&mut self, input: &[f32], output: &mut Vec<f32>) {
        let step = self.step();

        for &sample in input {
            let filtered = self
                .stages
                .iter_mut()
                .fold(sample, |value, stage| stage.process(value));

            if !self.primed {
                self.previous = filtered;
                self.primed = true;
                self.position = 0.0;
                continue;
            }

            // Emit every output sample that falls between the previous input sample and this one. A
            // while loop rather than an if, because a large ratio can still leave the position behind
            // after one output, and a small one can leave it ahead for several inputs in a row.
            while self.position <= 1.0 {
                let blend = self.position.clamp(0.0, 1.0);
                output.push(self.previous + (filtered - self.previous) * blend);
                self.position += step;
            }

            self.position -= 1.0;
            self.previous = filtered;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Root mean square of a signal, which is what "how much of the tone survived" means here.
    fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f32 = samples.iter().map(|value| value * value).sum();
        (sum / samples.len() as f32).sqrt()
    }

    fn tone(frequency: f32, sample_rate: u32, samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|index| (2.0 * PI * frequency * index as f32 / sample_rate as f32).sin())
            .collect()
    }

    #[test]
    fn matching_or_higher_rates_need_no_resampler() {
        assert!(Resampler::new(48_000, 48_000).is_none());
        assert!(Resampler::new(16_000, 48_000).is_none());
        assert!(Resampler::new(0, 16_000).is_none());
    }

    #[test]
    fn output_length_follows_the_rate_ratio() {
        let mut resampler = Resampler::new(48_000, 16_000).expect("resampler");
        let mut output = Vec::new();
        resampler.process(&tone(440.0, 48_000, 48_000), &mut output);

        // One second in, one second out, within a sample or two of priming.
        assert!(
            (output.len() as i64 - 16_000).abs() <= 4,
            "expected about 16000 samples, got {}",
            output.len()
        );
    }

    #[test]
    fn a_low_tone_survives_downsampling() {
        let mut resampler = Resampler::new(48_000, 16_000).expect("resampler");
        let mut output = Vec::new();
        resampler.process(&tone(300.0, 48_000, 48_000), &mut output);

        // Well below the 8 kHz output Nyquist, so it should come through essentially untouched.
        let ratio = rms(&output) / rms(&tone(300.0, 48_000, 48_000));
        assert!(ratio > 0.9, "a 300 Hz tone lost too much: ratio {ratio}");
    }

    #[test]
    fn a_tone_above_the_new_nyquist_is_filtered_out_rather_than_aliased() {
        let mut resampler = Resampler::new(48_000, 16_000).expect("resampler");
        let mut output = Vec::new();
        // 12 kHz is above the 8 kHz Nyquist of a 16 kHz stream. Without the filter it would fold back
        // to 4 kHz at full volume, which is the aliasing this exists to prevent.
        resampler.process(&tone(12_000.0, 48_000, 48_000), &mut output);

        assert!(
            rms(&output) < 0.05,
            "a 12 kHz tone should be filtered away, got rms {}",
            rms(&output)
        );
    }

    #[test]
    fn chunked_input_matches_one_long_call() {
        let input = tone(500.0, 48_000, 12_000);

        let mut whole = Vec::new();
        Resampler::new(48_000, 16_000)
            .expect("resampler")
            .process(&input, &mut whole);

        let mut chunked = Vec::new();
        let mut resampler = Resampler::new(48_000, 16_000).expect("resampler");
        for chunk in input.chunks(317) {
            resampler.process(chunk, &mut chunked);
        }

        // Filter and interpolation state carries across calls, so a callback boundary is not a click.
        assert_eq!(whole.len(), chunked.len());
        for (index, (left, right)) in whole.iter().zip(chunked.iter()).enumerate() {
            assert!(
                (left - right).abs() < 1e-6,
                "sample {index} diverged: {left} vs {right}"
            );
        }
    }

    #[test]
    fn handles_awkward_ratios() {
        // 44.1 kHz to 16 kHz is not an integer ratio, which is exactly where naive decimation breaks.
        let mut resampler = Resampler::new(44_100, 16_000).expect("resampler");
        let mut output = Vec::new();
        resampler.process(&tone(440.0, 44_100, 44_100), &mut output);

        assert!(
            (output.len() as i64 - 16_000).abs() <= 4,
            "expected about 16000 samples, got {}",
            output.len()
        );
        assert!(rms(&output) > 0.5, "the tone should have survived");
    }

    #[test]
    fn silence_stays_silent() {
        let mut resampler = Resampler::new(48_000, 8_000).expect("resampler");
        let mut output = Vec::new();
        resampler.process(&vec![0.0; 4_800], &mut output);

        assert!(output.iter().all(|value| value.abs() < 1e-6));
    }
}
