//! Where something was heard: the stretches of a recording that rise clearly above its own background.
//!
//! A recorder left running is mostly quiet, and the point of the timeline is finding the few moments that
//! are not. This works entirely from the stored envelope (one byte of RMS per 100 ms, see `audio::peaks`),
//! so it needs no decoding and no extra storage, and a day of it is under a megabyte to scan.
//!
//! The level that counts as a sound is relative, not fixed. Background noise differs by tens of decibels
//! between a studio microphone and a laptop's, and between a bedroom at night and a kitchen at noon, so a
//! fixed threshold would fire constantly for one person and never for another. Each five minute block gets
//! a noise floor, a low percentile of its own levels, and a moment is a sound when it rises far enough
//! above that floor, how far being the sensitivity setting. A block's own levels, not a blend with its
//! neighbours', because a blend carries a quiet block's floor into a noisy one and flags the whole of it
//! when the heating comes on. A background that changes partway through a block can still read as one long
//! sound until the block ends, which is at most five minutes and does at least mark when it changed.
//!
//! Blocks are aligned to the clock rather than to the requested window, so the same recording yields the
//! same sounds whether the timeline asks about an hour or a day, as long as the caller passes the context
//! either side (see `FLOOR_CONTEXT_MS`).
//!
//! The envelope is linear and eight bit, so one step is about -48 dBFS and anything quieter reads as zero.
//! Very faint sounds on a quiet microphone can therefore not register at all; raising the input gain is the
//! remedy, and the user guide says so.

use std::collections::BTreeMap;

use crate::audio::peaks::PEAK_BUCKET_MS;
use crate::models::SoundSensitivity;

/// Length of the blocks the noise floor is estimated over. Long enough that a conversation, pauses and
/// all, does not raise its own floor; short enough to follow the heating coming on or the traffic dying
/// down at night.
pub const FLOOR_BLOCK_MS: i64 = 300_000;

/// A block holding less recording than this, because the recording started or stopped inside it, borrows
/// its neighbours' levels too, rather than guessing a floor from a few seconds.
const SPARSE_BLOCK_SLOTS: u32 = 600;

/// How much recording either side of a window the detector needs to see to give the same answer as it
/// would for a wider window: a block either side for the floor, and a little more for merging.
pub const FLOOR_CONTEXT_MS: i64 = 2 * FLOOR_BLOCK_MS;

/// Sounds closer together than this are one event, so a sentence with pauses is one sound, not six.
pub const MERGE_GAP_MS: i64 = 2_000;

/// Shorter than this is a click or a knock on the desk, not something anybody wants to jump to.
pub const MIN_SOUND_MS: i64 = 300;

/// A burst of loud slots shorter than this never takes part in merging. A single 100 ms slot over the line
/// is the background flickering, and merging such flickers two seconds apart would turn a noisy room into
/// one endless sound; a syllable, a clap or a door lasts longer.
const MIN_BURST_MS: i64 = 200;

/// Playback starts this long before a sound, so its beginning is heard rather than cut.
pub const LEAD_IN_MS: i64 = 1_000;

/// Recordings separated by less than this are one continuous stretch; the same tolerance the timeline's
/// coverage bands use for the few milliseconds of rounding between segments.
pub const CONTINUITY_TOLERANCE_MS: i64 = 500;

/// Which fraction of a block's levels lies at or below its floor. Low enough that a block which is more
/// than half conversation still finds the quiet between the words, high enough to land on the background's
/// typical level rather than its quietest moments, which a background that wavers would otherwise exceed
/// a quarter of the time.
const FLOOR_PERCENTILE: f64 = 0.3;

/// Nothing quieter counts, however quiet the room: at 1 the dither of a silent input would be a sound.
const MIN_LEVEL: u8 = 2;

/// One segment's envelope, positioned on the timeline. Callers pass them in time order.
#[derive(Debug, Clone, Copy)]
pub struct EnvelopeRun<'a> {
    pub start_ms: i64,
    pub values: &'a [u8],
}

/// A moment something was heard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sound {
    pub start_ms: i64,
    pub end_ms: i64,
    /// The loudest envelope value inside it, 0 to 255.
    pub peak: u8,
    /// Where playback should start to hear it from its beginning: `LEAD_IN_MS` early, but never earlier
    /// than the continuous recording holding it, so a jump never lands in a gap.
    pub seek_ms: i64,
}

/// How far above the floor a level must be: at least `factor` times it and at least `margin` over it.
/// Both, because a multiple alone means nothing when the floor is zero, and a margin alone is swamped by a
/// noisy room.
fn thresholds(sensitivity: SoundSensitivity) -> (f64, u8) {
    match sensitivity {
        SoundSensitivity::High => (2.5, 3),
        SoundSensitivity::Medium => (3.5, 5),
        SoundSensitivity::Low => (5.0, 10),
    }
}

/// The level a 100 ms slot must reach to count as a sound, given the floor around it.
pub fn threshold(floor: u8, sensitivity: SoundSensitivity) -> u8 {
    let (factor, margin) = thresholds(sensitivity);
    let multiple = (f64::from(floor) * factor).ceil().min(255.0) as u8;
    multiple.max(floor.saturating_add(margin)).max(MIN_LEVEL)
}

/// Every sound in `runs`, in time order.
///
/// Two passes in one walk: consecutive loud slots form a burst, bursts shorter than `MIN_BURST_MS` are
/// dropped as flicker, and the rest merge into sounds when less than `MERGE_GAP_MS` apart.
pub fn detect(runs: &[EnvelopeRun<'_>], sensitivity: SoundSensitivity) -> Vec<Sound> {
    let floors = noise_floors(runs);
    let mut sounds = Vec::new();
    let mut merging: Option<Sound> = None;
    let mut burst: Option<Sound> = None;
    let mut stretch_start_ms = i64::MIN;
    let mut previous_end_ms: Option<i64> = None;

    for run in runs {
        let run_end_ms = run.start_ms + run.values.len() as i64 * PEAK_BUCKET_MS;
        // A gap in the recording ends whatever was being heard: nobody knows what happened in between.
        let continuous =
            previous_end_ms.is_some_and(|end| run.start_ms - end <= CONTINUITY_TOLERANCE_MS);
        if !continuous {
            end_burst(&mut burst, &mut merging, &mut sounds);
            close(&mut merging, &mut sounds);
            stretch_start_ms = run.start_ms;
        }
        previous_end_ms = Some(run_end_ms);

        for (index, value) in run.values.iter().enumerate() {
            let slot_start_ms = run.start_ms + index as i64 * PEAK_BUCKET_MS;
            let floor = floors
                .get(&slot_start_ms.div_euclid(FLOOR_BLOCK_MS))
                .copied()
                .unwrap_or(0);
            if *value < threshold(floor, sensitivity) {
                end_burst(&mut burst, &mut merging, &mut sounds);
                continue;
            }
            let slot_end_ms = slot_start_ms + PEAK_BUCKET_MS;
            match burst.as_mut() {
                Some(current) if current.end_ms == slot_start_ms => {
                    current.end_ms = slot_end_ms;
                    current.peak = current.peak.max(*value);
                }
                _ => {
                    end_burst(&mut burst, &mut merging, &mut sounds);
                    burst = Some(Sound {
                        start_ms: slot_start_ms,
                        end_ms: slot_end_ms,
                        peak: *value,
                        seek_ms: (slot_start_ms - LEAD_IN_MS).max(stretch_start_ms),
                    });
                }
            }
        }
    }
    end_burst(&mut burst, &mut merging, &mut sounds);
    close(&mut merging, &mut sounds);
    sounds
}

/// Finish the burst in progress: drop it as flicker when too short, otherwise fold it into the sound
/// being merged, or start a new one when the last ended too long ago.
fn end_burst(burst: &mut Option<Sound>, merging: &mut Option<Sound>, sounds: &mut Vec<Sound>) {
    let Some(finished) = burst.take() else {
        return;
    };
    if finished.end_ms - finished.start_ms < MIN_BURST_MS {
        return;
    }
    match merging.as_mut() {
        Some(sound) if finished.start_ms - sound.end_ms < MERGE_GAP_MS => {
            sound.end_ms = finished.end_ms;
            sound.peak = sound.peak.max(finished.peak);
        }
        _ => {
            close(merging, sounds);
            *merging = Some(finished);
        }
    }
}

fn close(merging: &mut Option<Sound>, sounds: &mut Vec<Sound>) {
    if let Some(sound) = merging.take() {
        if sound.end_ms - sound.start_ms >= MIN_SOUND_MS {
            sounds.push(sound);
        }
    }
}

/// The noise floor of every block that has recording in it: its own levels, or its neighbours' as well
/// when it holds too little to judge by.
fn noise_floors(runs: &[EnvelopeRun<'_>]) -> BTreeMap<i64, u8> {
    let mut histograms: BTreeMap<i64, [u32; 256]> = BTreeMap::new();
    for run in runs {
        for (index, value) in run.values.iter().enumerate() {
            let slot_start_ms = run.start_ms + index as i64 * PEAK_BUCKET_MS;
            let histogram = histograms
                .entry(slot_start_ms.div_euclid(FLOOR_BLOCK_MS))
                .or_insert([0; 256]);
            histogram[usize::from(*value)] += 1;
        }
    }

    histograms
        .keys()
        .map(|block| {
            let own = &histograms[block];
            if own.iter().sum::<u32>() >= SPARSE_BLOCK_SLOTS {
                return (*block, percentile(own, FLOOR_PERCENTILE));
            }
            let mut combined = [0u32; 256];
            for neighbour in [block - 1, *block, block + 1] {
                if let Some(histogram) = histograms.get(&neighbour) {
                    for (total, count) in combined.iter_mut().zip(histogram) {
                        *total += count;
                    }
                }
            }
            (*block, percentile(&combined, FLOOR_PERCENTILE))
        })
        .collect()
}

fn percentile(histogram: &[u32; 256], fraction: f64) -> u8 {
    let total: u64 = histogram.iter().map(|count| u64::from(*count)).sum();
    if total == 0 {
        return 0;
    }
    let target = ((total as f64) * fraction).ceil().max(1.0) as u64;
    let mut seen = 0u64;
    for (level, count) in histogram.iter().enumerate() {
        seen += u64::from(*count);
        if seen >= target {
            return level as u8;
        }
    }
    255
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLOT: i64 = PEAK_BUCKET_MS;

    /// A run of `seconds` at a steady background `level`, starting at `start_ms`.
    fn background(start_ms: i64, seconds: i64, level: u8) -> (i64, Vec<u8>) {
        (start_ms, vec![level; (seconds * 1000 / SLOT) as usize])
    }

    /// Put a sound of `level` from `from_ms` to `to_ms` into a run.
    fn with_sound(run: &mut (i64, Vec<u8>), from_ms: i64, to_ms: i64, level: u8) {
        for slot in ((from_ms - run.0) / SLOT)..((to_ms - run.0) / SLOT) {
            run.1[slot as usize] = level;
        }
    }

    fn detect_in(runs: &[(i64, Vec<u8>)], sensitivity: SoundSensitivity) -> Vec<Sound> {
        let envelopes: Vec<EnvelopeRun<'_>> = runs
            .iter()
            .map(|(start_ms, values)| EnvelopeRun {
                start_ms: *start_ms,
                values,
            })
            .collect();
        detect(&envelopes, sensitivity)
    }

    #[test]
    fn a_quiet_room_holds_no_sounds() {
        let runs = [background(0, 600, 1)];
        assert!(detect_in(&runs, SoundSensitivity::High).is_empty());
    }

    #[test]
    fn a_sound_is_found_where_it_happened_with_its_peak() {
        let mut run = background(0, 600, 1);
        with_sound(&mut run, 100_000, 101_500, 60);
        run.1[1_005] = 90;

        let sounds = detect_in(&[run], SoundSensitivity::Medium);
        assert_eq!(sounds.len(), 1);
        assert_eq!(sounds[0].start_ms, 100_000);
        assert_eq!(sounds[0].end_ms, 101_500);
        assert_eq!(sounds[0].peak, 90);
    }

    #[test]
    fn a_click_is_too_short_to_count() {
        let mut run = background(0, 600, 1);
        with_sound(&mut run, 100_000, 100_200, 120);
        assert!(detect_in(&[run], SoundSensitivity::High).is_empty());
    }

    #[test]
    fn pauses_within_a_sentence_are_one_sound_and_silence_between_sentences_is_two() {
        let mut run = background(0, 600, 1);
        with_sound(&mut run, 100_000, 101_000, 50);
        with_sound(&mut run, 102_500, 103_500, 50); // 1.5 s later: the same sound
        with_sound(&mut run, 110_000, 111_000, 50); // 6.5 s later: a new one

        let sounds = detect_in(&[run], SoundSensitivity::Medium);
        assert_eq!(sounds.len(), 2);
        assert_eq!((sounds[0].start_ms, sounds[0].end_ms), (100_000, 103_500));
        assert_eq!(sounds[1].start_ms, 110_000);
    }

    #[test]
    fn sensitivity_decides_what_a_noisy_room_counts() {
        // A noisy room with a floor of 10, and a moderately raised level of 25.
        let mut run = background(0, 600, 10);
        with_sound(&mut run, 100_000, 102_000, 25);

        assert_eq!(detect_in(&[run.clone()], SoundSensitivity::High).len(), 1);
        assert!(detect_in(&[run.clone()], SoundSensitivity::Medium).is_empty());
        assert!(detect_in(&[run], SoundSensitivity::Low).is_empty());
    }

    #[test]
    fn a_wavering_background_is_not_a_sound_at_any_sensitivity() {
        // Found by running the service: a background that wanders between 1 and 3, as real ones do, was a
        // continuous "sound" at High, because its quiet moments set the floor and its flickers merged.
        let wavering = [1u8, 2, 2, 3, 2, 1, 3, 2, 2, 3, 1, 2];
        let values: Vec<u8> = (0..6_000)
            .map(|slot| wavering[slot % wavering.len()])
            .collect();
        let mut run = (0i64, values);
        for sensitivity in [
            SoundSensitivity::Low,
            SoundSensitivity::Medium,
            SoundSensitivity::High,
        ] {
            assert!(
                detect_in(&[run.clone()], sensitivity).is_empty(),
                "{sensitivity:?}"
            );
        }

        // And speech over it is still found, even at the least sensitive setting.
        with_sound(&mut run, 300_000, 302_000, 30);
        for sensitivity in [
            SoundSensitivity::Low,
            SoundSensitivity::Medium,
            SoundSensitivity::High,
        ] {
            let sounds = detect_in(&[run.clone()], sensitivity);
            assert_eq!(sounds.len(), 1, "{sensitivity:?}");
            assert_eq!(sounds[0].start_ms, 300_000, "{sensitivity:?}");
        }
    }

    #[test]
    fn flickers_do_not_chain_into_a_sound_but_real_bursts_merge() {
        let mut run = background(0, 600, 1);
        // Single slots over the line, a second apart: each is flicker, and none of them merge.
        for second in 100..130 {
            run.1[(second * 10) as usize] = 60;
        }
        assert!(detect_in(&[run.clone()], SoundSensitivity::High).is_empty());
        // Two 300 ms bursts a second apart are one sound.
        with_sound(&mut run, 200_000, 200_300, 60);
        with_sound(&mut run, 201_300, 201_600, 60);
        let sounds = detect_in(&[run], SoundSensitivity::High);
        assert_eq!(sounds.len(), 1);
        assert_eq!((sounds[0].start_ms, sounds[0].end_ms), (200_000, 201_600));
    }

    #[test]
    fn a_digitally_silent_input_still_needs_a_real_rise() {
        assert_eq!(threshold(0, SoundSensitivity::High), 3);
        assert_eq!(threshold(0, SoundSensitivity::Medium), 5);
        assert_eq!(threshold(0, SoundSensitivity::Low), 10);
        // A multiple of the floor wins once the room is noisy.
        assert_eq!(threshold(10, SoundSensitivity::Medium), 35);
        // And nothing overflows at the top of the scale.
        assert_eq!(threshold(200, SoundSensitivity::Low), 255);
    }

    #[test]
    fn the_floor_follows_the_room_as_it_changes() {
        // Half an hour of quiet, then half an hour with the heating roaring at 20.
        let mut quiet = background(0, 1_800, 1);
        let mut noisy = background(1_800_000, 1_800, 20);
        with_sound(&mut quiet, 600_000, 602_000, 15);
        with_sound(&mut noisy, 3_000_000, 3_002_000, 15);

        let sounds = detect_in(&[quiet, noisy], SoundSensitivity::Medium);
        assert_eq!(
            sounds.len(),
            1,
            "15 stands out in the quiet, not over the heating"
        );
        assert_eq!(sounds[0].start_ms, 600_000);
    }

    #[test]
    fn a_gap_in_the_recording_splits_a_sound_and_contiguous_segments_do_not() {
        let mut first = background(0, 10, 1);
        let mut second = background(10_000, 10, 1);
        with_sound(&mut first, 9_000, 10_000, 60);
        with_sound(&mut second, 10_000, 11_000, 60);
        let contiguous = detect_in(&[first.clone(), second], SoundSensitivity::Medium);
        assert_eq!(contiguous.len(), 1);
        assert_eq!(
            (contiguous[0].start_ms, contiguous[0].end_ms),
            (9_000, 11_000)
        );

        let mut later = background(70_000, 10, 1);
        with_sound(&mut later, 70_000, 71_000, 60);
        let split = detect_in(&[first, later], SoundSensitivity::Medium);
        assert_eq!(split.len(), 2);
        assert_eq!(split[1].seek_ms, 70_000, "the lead in stops at the gap");
    }

    #[test]
    fn playback_starts_a_second_early_but_never_inside_a_gap() {
        let mut run = background(50_000, 60, 1);
        with_sound(&mut run, 80_000, 81_000, 60);
        with_sound(&mut run, 50_200, 51_000, 60);

        let sounds = detect_in(&[run], SoundSensitivity::Medium);
        assert_eq!(
            sounds[0].seek_ms, 50_000,
            "the recording began only 200 ms before"
        );
        assert_eq!(sounds[1].seek_ms, 79_000);
    }

    #[test]
    fn the_same_recording_gives_the_same_sounds_whatever_the_window() {
        let mut day: Vec<(i64, Vec<u8>)> = (0..12)
            .map(|hour| background(hour * 3_600_000, 3_600, 1 + (hour % 3) as u8))
            .collect();
        with_sound(&mut day[5], 18_100_000, 18_103_000, 70);

        let whole = detect_in(&day, SoundSensitivity::Medium);
        // Hours 4 to 6 cover the sound with more than the ten minutes of context either side.
        let part = detect_in(&day[4..7], SoundSensitivity::Medium);
        assert_eq!(whole, part);
        assert_eq!(whole.len(), 1);
    }
}
