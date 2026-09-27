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

    mod props {
        use super::*;
        use proptest::prelude::*;

        /// Up to three stored envelopes in time order, with gaps, near a window starting at `from_ms`.
        fn sources_near(from_ms: i64) -> impl Strategy<Value = Vec<(i64, Vec<u8>)>> {
            proptest::collection::vec(
                (
                    0i64..3_600_000,
                    proptest::collection::vec(any::<u8>(), 0..400),
                ),
                0..4,
            )
            .prop_map(move |runs| {
                let mut next_ms = from_ms - 1_800_000;
                runs.into_iter()
                    .map(|(gap_ms, values)| {
                        let start_ms = next_ms + gap_ms;
                        next_ms = start_ms + values.len() as i64 * PEAK_BUCKET_MS;
                        (start_ms, values)
                    })
                    .collect()
            })
        }

        /// A window anywhere a clock could read, from a tenth of a second (zoomed in past the stored
        /// resolution) to the 32 days the controller allows, with sources around it.
        fn window() -> impl Strategy<Value = (i64, i64, usize, Vec<(i64, Vec<u8>)>)> {
            (
                -1_000_000_000_000i64..4_000_000_000_000,
                prop_oneof![
                    1i64..10_000,
                    10_000i64..7_200_000,
                    7_200_000i64..2_764_800_000
                ],
                1usize..=4_000,
            )
                .prop_flat_map(|(from_ms, span_ms, buckets)| {
                    (
                        Just(from_ms),
                        Just(from_ms + span_ms),
                        Just(buckets),
                        sources_near(from_ms),
                    )
                })
        }

        fn as_sources(runs: &[(i64, Vec<u8>)]) -> Vec<PeakSource<'_>> {
            runs.iter()
                .map(|(start_ms, values)| PeakSource {
                    start_ms: *start_ms,
                    values,
                })
                .collect()
        }

        /// Every stored bucket as `(start_ms, value)`.
        fn buckets_of(runs: &[(i64, Vec<u8>)]) -> impl Iterator<Item = (i64, u8)> + '_ {
            runs.iter().flat_map(|(start_ms, values)| {
                values
                    .iter()
                    .enumerate()
                    .map(move |(index, value)| (start_ms + index as i64 * PEAK_BUCKET_MS, *value))
            })
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

            /// The waveform has exactly the columns asked for, and each column holds the loudest stored
            /// level that overlaps the stretch of time it stands for, or nothing. The canvas draws one bar
            /// per value, so a short answer would squash the waveform against the timeline's scale, and a
            /// level smeared into a column it does not touch would put a sound where the playhead will not
            /// find it. The expected columns are worked out here in exact integer arithmetic, against which
            /// the renderer's floating point placement has to agree to the column.
            #[test]
            fn each_column_holds_the_loudest_level_overlapping_it((from_ms, to_ms, buckets, runs) in window()) {
                let rendered = render(&as_sources(&runs), from_ms, to_ms, buckets);
                prop_assert_eq!(rendered.len(), buckets);

                // Column `c` covers `[from + c * span / n, from + (c + 1) * span / n)`, so a stored bucket
                // `[start, end)` overlaps columns `floor((start - from) * n / span)` up to, not including,
                // `ceil((end - from) * n / span)`.
                let span = i128::from(to_ms - from_ms);
                let columns = buckets as i128;
                let mut expected = vec![0u8; buckets];
                for (start_ms, value) in buckets_of(&runs) {
                    let start = i128::from(start_ms - from_ms) * columns;
                    let end = i128::from(start_ms + PEAK_BUCKET_MS - from_ms) * columns;
                    let first = start.div_euclid(span).max(0);
                    let last = (-(-end).div_euclid(span)).min(columns);
                    for column in first..last {
                        let slot = &mut expected[column as usize];
                        *slot = (*slot).max(value);
                    }
                }
                prop_assert_eq!(rendered, expected);
            }

            /// No stored level inside the window is lost however far the view is zoomed out: some column
            /// is at least as loud. Taking the maximum is the whole reason the renderer exists, so a
            /// transient that vanished at some zoom would make the waveform useless for finding it.
            #[test]
            fn no_level_inside_the_window_is_lost((from_ms, to_ms, buckets, runs) in window()) {
                let rendered = render(&as_sources(&runs), from_ms, to_ms, buckets);
                let loudest_drawn = rendered.iter().copied().max().unwrap_or(0);
                let loudest_stored = buckets_of(&runs)
                    .filter(|(start_ms, _)| *start_ms + PEAK_BUCKET_MS > from_ms && *start_ms < to_ms)
                    .map(|(_, value)| value)
                    .max()
                    .unwrap_or(0);
                prop_assert_eq!(loudest_drawn, loudest_stored);
            }

            /// An empty or inverted window, or no columns, is an empty answer for any sources, never a
            /// panic or a buffer sized from a negative span.
            #[test]
            fn a_degenerate_window_is_empty(
                from_ms in -1_000_000_000_000i64..4_000_000_000_000,
                back_ms in 0i64..1_000_000,
                buckets in 0usize..4_000,
            ) {
                let values = vec![200u8; 10];
                let sources = [PeakSource { start_ms: from_ms - 500, values: &values }];
                prop_assert!(render(&sources, from_ms, from_ms - back_ms, buckets).is_empty());
                prop_assert!(render(&sources, from_ms, from_ms + 1_000, 0).is_empty());
            }

            /// The envelope does not depend on how the samples were delivered: any chunking gives the same
            /// buckets as one long push, and one bucket per 100 ms of audio started. Capture hands the
            /// recorder frames of whatever size the device and the frame setting produce, so an envelope
            /// that shifted with the chunk size would draw the same recording differently from day to day.
            #[test]
            fn chunking_never_changes_the_envelope(
                sample_rate in prop_oneof![Just(8_000u32), Just(11_025), Just(16_000), Just(44_100), Just(48_000), 1u32..200_000],
                seed in any::<u64>(),
                length in 0usize..20_000,
                cuts in proptest::collection::vec(1usize..5_000, 0..20),
            ) {
                // Samples from a seed rather than a generated vector: twenty thousand generated values per
                // case cost more than everything else here, and the property is about where the cuts fall.
                let mut state = seed | 1;
                let samples: Vec<i16> = (0..length)
                    .map(|_| {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        state as i16
                    })
                    .collect();
                let mut whole = PeakEnvelopeBuilder::new(sample_rate);
                whole.push(&samples);
                let whole = whole.finish();

                let mut chunked = PeakEnvelopeBuilder::new(sample_rate);
                let mut rest: &[i16] = &samples;
                for cut in cuts {
                    let (head, tail) = rest.split_at(cut.min(rest.len()));
                    chunked.push(head);
                    rest = tail;
                }
                chunked.push(rest);
                prop_assert_eq!(&chunked.finish(), &whole);

                let bucket_samples = ((i64::from(sample_rate) * PEAK_BUCKET_MS) / 1000).max(1) as usize;
                prop_assert_eq!(whole.len(), samples.len().div_ceil(bucket_samples));
            }

            /// A steady amplitude reads back as that amplitude on the linear scale in every full bucket, and
            /// silence as zero, so the waveform's height means the same thing everywhere.
            #[test]
            fn a_steady_amplitude_reads_back_linearly(amplitude in 0i16..=i16::MAX, buckets in 1usize..20) {
                let mut builder = PeakEnvelopeBuilder::new(48_000);
                let samples: Vec<i16> = (0..buckets * 4_800)
                    .map(|index| if index % 2 == 0 { amplitude } else { -amplitude })
                    .collect();
                builder.push(&samples);
                let expected = (f64::from(amplitude) / f64::from(i16::MAX) * 255.0).round() as u8;
                prop_assert_eq!(builder.finish(), vec![expected; buckets]);
            }
        }
    }
}
