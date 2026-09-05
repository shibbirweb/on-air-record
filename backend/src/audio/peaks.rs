//! Waveform envelope: computing it while recording and rendering it back for the timeline.
//!
//! Drawing a waveform from the audio itself does not scale. An hour at 48 kHz is 172 million samples, and
//! the browser only has a few thousand pixels to put them in. So the recorder reduces the signal once,
//! while the samples are already in cache, and stores one byte per 100 ms bucket next to the segment.
//! An hour of envelope is then 36 KB, which is cheap enough to send on every timeline pan.

/// Envelope resolution. 100 ms is the smallest bucket that still keeps an hour of envelope under 40 KB,
/// and it matches the default frame size so a frame never straddles more than two buckets.
pub const PEAK_BUCKET_MS: i64 = 100;

/// Accumulates the envelope of a segment as it is written.
pub struct PeakEnvelopeBuilder {
    bucket_samples: usize,
    filled_samples: usize,
    sum_squares: f64,
    values: Vec<u8>,
}

impl PeakEnvelopeBuilder {
    pub fn new(sample_rate: u32) -> Self {
        let bucket_samples = ((sample_rate as i64 * PEAK_BUCKET_MS) / 1000).max(1) as usize;
        Self {
            bucket_samples,
            filled_samples: 0,
            sum_squares: 0.0,
            values: Vec::new(),
        }
    }

    /// Feed the mono samples of one frame.
    pub fn push(&mut self, samples: &[i16]) {
        for sample in samples {
            let normalised = *sample as f64 / i16::MAX as f64;
            self.sum_squares += normalised * normalised;
            self.filled_samples += 1;

            if self.filled_samples >= self.bucket_samples {
                self.close_bucket();
            }
        }
    }

    /// Close the envelope, flushing a partial trailing bucket so a short segment still draws something.
    pub fn finish(mut self) -> Vec<u8> {
        if self.filled_samples > 0 {
            self.close_bucket();
        }
        self.values
    }

    /// Buckets completed so far, useful for asserting alignment against the segment duration.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    fn close_bucket(&mut self) {
        let rms = (self.sum_squares / self.filled_samples.max(1) as f64).sqrt();
        // The envelope is drawn, not measured, so a plain linear scale is what matches what people expect
        // to see. Full scale maps to 255 and anything above clamps.
        let scaled = (rms * 255.0).round().clamp(0.0, 255.0) as u8;
        self.values.push(scaled);
        self.filled_samples = 0;
        self.sum_squares = 0.0;
    }
}

/// One segment's envelope, positioned on the timeline.
pub struct PeakSource<'a> {
    pub start_ms: i64,
    pub values: &'a [u8],
}

/// Resample a set of stored envelopes onto `buckets` evenly spaced columns covering `[from_ms, to_ms)`.
///
/// Overlapping input buckets are combined with a maximum rather than a mean. When an hour is squeezed into
/// a thousand columns each column covers 3.6 seconds, and averaging would flatten a short loud event into
/// nothing. Taking the maximum keeps transients visible, which is what makes the timeline usable for
/// finding the moment something happened.
pub fn render(sources: &[PeakSource<'_>], from_ms: i64, to_ms: i64, buckets: usize) -> Vec<u8> {
    if buckets == 0 || to_ms <= from_ms {
        return Vec::new();
    }

    let mut output = vec![0u8; buckets];
    let span_ms = (to_ms - from_ms) as f64;

    for source in sources {
        for (index, value) in source.values.iter().enumerate() {
            if *value == 0 {
                continue;
            }

            let bucket_start_ms = source.start_ms + index as i64 * PEAK_BUCKET_MS;
            let bucket_end_ms = bucket_start_ms + PEAK_BUCKET_MS;
            if bucket_end_ms <= from_ms || bucket_start_ms >= to_ms {
                continue;
            }

            // A source bucket can be wider than an output column when zoomed in, so fill the whole span
            // it covers instead of only the column its start lands in.
            let first = (((bucket_start_ms - from_ms) as f64 / span_ms) * buckets as f64).floor();
            let last = (((bucket_end_ms - from_ms) as f64 / span_ms) * buckets as f64).ceil();
            let first_index = first.max(0.0) as usize;
            let last_index = (last.max(0.0) as usize).min(buckets);

            for slot in output.iter_mut().take(last_index).skip(first_index) {
                if *value > *slot {
                    *slot = *value;
                }
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_produces_zero_buckets() {
        let mut builder = PeakEnvelopeBuilder::new(48_000);
        builder.push(&vec![0; 4800]);
        assert_eq!(builder.finish(), vec![0]);
    }

    #[test]
    fn one_second_at_48k_produces_ten_buckets() {
        let mut builder = PeakEnvelopeBuilder::new(48_000);
        builder.push(&vec![1000; 48_000]);
        assert_eq!(builder.finish().len(), 10);
    }

    #[test]
    fn full_scale_saturates_the_bucket() {
        let mut builder = PeakEnvelopeBuilder::new(48_000);
        let samples: Vec<i16> = (0..4800)
            .map(|index| if index % 2 == 0 { i16::MAX } else { -i16::MAX })
            .collect();
        builder.push(&samples);
        assert_eq!(builder.finish(), vec![255]);
    }

    #[test]
    fn partial_bucket_is_flushed_on_finish() {
        let mut builder = PeakEnvelopeBuilder::new(48_000);
        builder.push(&vec![1000; 1000]);
        assert_eq!(builder.len(), 0);
        assert_eq!(builder.finish().len(), 1);
    }

    #[test]
    fn render_places_values_in_the_right_columns() {
        let values = vec![10, 20, 30, 40];
        let sources = vec![PeakSource {
            start_ms: 1_000,
            values: &values,
        }];

        // Four 100 ms buckets rendered onto four columns covering exactly the same 400 ms.
        let rendered = render(&sources, 1_000, 1_400, 4);
        assert_eq!(rendered, vec![10, 20, 30, 40]);
    }

    #[test]
    fn render_keeps_the_loudest_value_when_downsampling() {
        let values = vec![10, 200, 15, 20];
        let sources = vec![PeakSource {
            start_ms: 0,
            values: &values,
        }];

        // Four buckets squeezed into one column must not average the transient away.
        assert_eq!(render(&sources, 0, 400, 1), vec![200]);
    }

    #[test]
    fn render_leaves_gaps_empty() {
        let first = vec![100, 100];
        let second = vec![200, 200];
        let sources = vec![
            PeakSource {
                start_ms: 0,
                values: &first,
            },
            PeakSource {
                start_ms: 600,
                values: &second,
            },
        ];

        let rendered = render(&sources, 0, 800, 8);
        assert_eq!(rendered[0], 100);
        assert_eq!(rendered[1], 100);
        assert_eq!(rendered[3], 0);
        assert_eq!(rendered[6], 200);
    }

    #[test]
    fn render_rejects_an_empty_window() {
        assert!(render(&[], 100, 100, 10).is_empty());
        assert!(render(&[], 0, 100, 0).is_empty());
    }
}
