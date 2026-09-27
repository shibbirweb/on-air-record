//! Reading recordings back when the files under the index are missing, short, damaged or misplaced.
//!
//! The index and the files can disagree: somebody prunes the data directory by hand, a disk develops a
//! bad sector, a backup restores the database but not every file. Playback, export and the timeline all
//! have to keep going through that, say honestly what is missing, and never read anything that is not a
//! recording.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use tower::ServiceExt;

use super::{seed_segment, SeedAt, Store, FRAME_MS, T0};
use crate::app::AppState;
use crate::audio::wav;
use crate::audio::SegmentLayout;
use crate::config::AppConfig;
use crate::models::{SegmentDraft, SoundSensitivity, TimeRange};
use crate::services::{CursorOutput, ExportService, PlaybackService, TimelineService};

/// More steps than any of these recordings could need, so a cursor that stops making progress fails the
/// test instead of hanging it.
const STEP_LIMIT: usize = 10_000;

fn playback(store: &Store) -> PlaybackService {
    PlaybackService::new(store.config.clone(), store.segments.clone())
}

fn export(store: &Store) -> ExportService {
    ExportService::new(store.segments.clone(), Arc::new(playback(store)))
}

/// What a listener would be told, in order: frames as their start time and first sample, gaps as their
/// bounds, until the end of the recording.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Heard {
    Frame { at_ms: i64, level: i16 },
    Gap { from_ms: i64, to_ms: i64 },
    End { at_ms: i64 },
}

fn listen_from(store: &Store, position_ms: i64) -> Vec<Heard> {
    let mut cursor = playback(store).cursor_at(position_ms, FRAME_MS as u32);
    let mut heard = Vec::new();
    for _ in 0..STEP_LIMIT {
        match cursor.advance().expect("a missing file is not an error") {
            CursorOutput::Frame(frame) => heard.push(Heard::Frame {
                at_ms: frame.timestamp_ms,
                level: frame.samples.first().copied().unwrap_or(0),
            }),
            CursorOutput::Gap { from_ms, to_ms } => {
                assert!(
                    to_ms > from_ms,
                    "a gap always moves forward: {from_ms}..{to_ms}"
                );
                heard.push(Heard::Gap { from_ms, to_ms });
            }
            CursorOutput::EndOfRecording { at_ms } => {
                heard.push(Heard::End { at_ms });
                return heard;
            }
        }
    }
    panic!("the cursor never reached the end of the recording");
}

fn gaps(heard: &[Heard]) -> Vec<(i64, i64)> {
    heard
        .iter()
        .filter_map(|item| match item {
            Heard::Gap { from_ms, to_ms } => Some((*from_ms, *to_ms)),
            _ => None,
        })
        .collect()
}

fn frames(heard: &[Heard]) -> Vec<(i64, i16)> {
    heard
        .iter()
        .filter_map(|item| match item {
            Heard::Frame { at_ms, level } => Some((*at_ms, *level)),
            _ => None,
        })
        .collect()
}

/// Three contiguous seconds, each at its own level, so where any sample came from is readable off it.
fn three_seconds(store: &Store) -> Vec<std::path::PathBuf> {
    vec![
        store.seed(0, T0, T0 + 1_000, 1_000),
        store.seed(1, T0 + 1_000, T0 + 2_000, 2_000),
        store.seed(2, T0 + 2_000, T0 + 3_000, 3_000),
    ]
}

#[test]
fn a_deleted_segment_file_plays_as_a_gap_rather_than_a_silent_jump() {
    let store = Store::new("play-deleted");
    let files = three_seconds(&store);
    std::fs::remove_file(&files[1]).expect("delete");

    let heard = listen_from(&store, T0);

    // The listener's clock would otherwise lurch a second forward with nothing to say why.
    assert_eq!(gaps(&heard), vec![(T0 + 1_000, T0 + 2_000)]);
    let frames = frames(&heard);
    assert_eq!(frames.len(), 20);
    assert!(frames[..10].iter().all(|(_, level)| *level == 1_000));
    assert_eq!(frames[10], (T0 + 2_000, 3_000));
    assert_eq!(heard.last(), Some(&Heard::End { at_ms: T0 + 3_000 }));
}

#[test]
fn a_truncated_segment_plays_what_survives_and_reports_the_rest_as_a_gap() {
    let store = Store::new("play-truncated");
    let files = three_seconds(&store);
    // Keep 450 ms and a stray odd byte, the way a crash mid write leaves a file.
    let bytes = std::fs::read(&files[1]).expect("read");
    std::fs::write(&files[1], &bytes[..(RATE_BYTES_PER_MS * 450) + 1]).expect("truncate");

    let heard = listen_from(&store, T0);

    let second: Vec<(i64, i16)> = frames(&heard)
        .into_iter()
        .filter(|(_, level)| *level == 2_000)
        .collect();
    assert_eq!(second.len(), 5, "the 450 ms that survived are played");
    let [(from_ms, to_ms)] = gaps(&heard)[..] else {
        panic!("one gap for the lost tail, got {:?}", gaps(&heard));
    };
    assert!((T0 + 1_400..=T0 + 1_450).contains(&from_ms));
    assert_eq!(to_ms, T0 + 2_000);
    assert!(frames(&heard).contains(&(T0 + 2_000, 3_000)));
}

/// Bytes per millisecond of the 48 kHz mono recordings these tests write.
const RATE_BYTES_PER_MS: usize = 96;

#[test]
fn a_segment_overwritten_with_garbage_plays_as_noise_and_never_derails_the_cursor() {
    let store = Store::new("play-garbage");
    let files = three_seconds(&store);
    let length = std::fs::metadata(&files[1]).expect("meta").len() as usize;
    let garbage: Vec<u8> = (0..length).map(|index| (index * 31 + 7) as u8).collect();
    std::fs::write(&files[1], &garbage).expect("garbage");
    // And one shorter than its row and of odd length.
    std::fs::write(&files[2], &garbage[..12_345]).expect("short garbage");

    let heard = listen_from(&store, T0);

    // Raw PCM carries no checksum, so garbage of the right length is indistinguishable from audio. What
    // matters is that it plays in its place, time keeps moving forward, and the short file ends in a gap.
    let mut previous_ms = i64::MIN;
    for (at_ms, _) in frames(&heard) {
        assert!(at_ms > previous_ms, "time only moves forward");
        previous_ms = at_ms;
    }
    assert!(frames(&heard).iter().any(|(at_ms, _)| *at_ms == T0 + 1_500));
    assert_eq!(gaps(&heard).last().map(|gap| gap.1), Some(T0 + 3_000));
}

#[test]
fn more_missing_files_in_a_row_than_one_step_consults_are_a_gap_not_the_end_of_the_recording() {
    let store = Store::new("play-many-missing");
    let mut files = Vec::new();
    for index in 0..40 {
        let start_ms = T0 + index * 1_000;
        files.push(store.seed(index, start_ms, start_ms + 1_000, 500 + index as i16));
    }
    // A day restored from a backup that missed a folder: the first 30 files are gone.
    for file in &files[..30] {
        std::fs::remove_file(file).expect("delete");
    }

    let heard = listen_from(&store, T0);

    // Ending here would throw a listener to the live feed, and cut an export's audio off, with ten
    // seconds of recording still on disk after the hole.
    let frames = frames(&heard);
    assert_eq!(frames.first(), Some(&(T0 + 30_000, 530)));
    assert_eq!(frames.len(), 100);
    let gaps = gaps(&heard);
    assert_eq!(gaps.first().map(|gap| gap.0), Some(T0));
    assert_eq!(gaps.last().map(|gap| gap.1), Some(T0 + 30_000));
    assert!(
        heard[..heard.len() - 1]
            .iter()
            .all(|item| !matches!(item, Heard::End { .. })),
        "the end of the recording is only reported once, at the real end"
    );
}

#[test]
fn index_rows_with_impossible_formats_never_panic_or_spin() {
    let store = Store::new("play-impossible");
    let file = store.seed(0, T0, T0 + 1_000, 1_000);
    let path = store.config.relativise_data_path(&file);

    let impossible = [
        // No sample rate, no channels, no bytes, and a row that ends before it starts.
        (1, 1_000, 2_000, 0, 1, 96_000),
        (2, 2_000, 3_000, 48_000, 0, 96_000),
        (3, 3_000, 4_000, 48_000, 1, 0),
        (4, 4_000, 5_000, 48_000, 1, -5),
        (5, 6_000, 5_500, 48_000, 1, 96_000),
    ];
    for (sequence, start, end, rate, channels, byte_len) in impossible {
        store
            .segments
            .insert(&SegmentDraft {
                session_id: store.session_id,
                sequence,
                day: crate::util::day::local_day(T0),
                path: path.clone(),
                started_at_ms: T0 + start,
                ended_at_ms: T0 + end,
                sample_rate: rate,
                channels,
                byte_len,
                peaks: vec![1; 10],
            })
            .expect("a damaged row");
    }

    let heard = listen_from(&store, T0);
    assert!(matches!(heard.last(), Some(Heard::End { .. })));

    let timeline = TimelineService::new(store.segments.clone(), store.hub.clone());
    timeline.peaks(T0, T0 + 10_000, 100).expect("peaks");
    timeline
        .sounds(T0, T0 + 10_000, SoundSensitivity::High)
        .expect("sounds");
}

#[test]
fn an_export_across_a_deleted_segment_keeps_every_later_sound_at_its_true_offset() {
    let store = Store::new("export-deleted");
    let files = three_seconds(&store);
    std::fs::remove_file(&files[1]).expect("delete");

    let bytes = export_bytes(&store, T0, T0 + 3_000);

    // The file is a record of the span: thirty seconds in is thirty seconds after its start, so a missing
    // second is silence where it was, not the next second's audio pulled forward.
    assert_eq!(second_levels(&bytes), vec![1_000, 0, 3_000]);
}

#[test]
fn an_export_across_a_truncated_segment_keeps_every_later_sound_at_its_true_offset() {
    let store = Store::new("export-truncated");
    let files = three_seconds(&store);
    let bytes = std::fs::read(&files[1]).expect("read");
    std::fs::write(&files[1], &bytes[..RATE_BYTES_PER_MS * 500]).expect("truncate");

    let exported = export_bytes(&store, T0, T0 + 3_000);
    let samples = samples_of(&exported);
    let at = |ms: usize| samples[ms * 48];
    assert_eq!(
        at(1_250),
        2_000,
        "what survived plays where it was recorded"
    );
    assert_eq!(at(1_750), 0, "what was lost is silence where it was");
    assert_eq!(at(2_500), 3_000, "the next second is not pulled forward");
}

#[test]
fn an_export_over_more_missing_files_than_one_step_consults_still_holds_the_audio_after_them() {
    let store = Store::new("export-many-missing");
    let mut files = Vec::new();
    for index in 0..25 {
        let start_ms = T0 + index * 1_000;
        files.push(store.seed(index, start_ms, start_ms + 1_000, 1_000));
    }
    for file in &files[..20] {
        std::fs::remove_file(file).expect("delete");
    }

    let levels = second_levels(&export_bytes(&store, T0, T0 + 25_000));
    assert!(levels[..20].iter().all(|level| *level == 0));
    assert_eq!(
        levels[20..].to_vec(),
        vec![1_000; 5],
        "the five seconds after the hole are in the file, not replaced by silence"
    );
}

#[test]
fn an_export_whose_index_fails_part_way_is_an_error_not_a_short_file() {
    let store = Store::new("export-index-fails");
    three_seconds(&store);
    let service = export(&store);
    let plan = service.plan(TimeRange::new(T0, T0 + 3_000)).expect("plan");

    // The index disappears between the plan and the reading, as a damaged database would make it.
    store
        .second_connection()
        .execute_batch("ALTER TABLE segments RENAME TO segments_hidden;")
        .expect("hide");

    let mut written = 0u64;
    let outcome = service.write(&plan, |chunk| {
        written += chunk.len() as u64;
        true
    });

    // The controller turns this error into a failed body, so the client sees a broken download instead
    // of a WAV whose header promises more audio than it holds.
    assert!(outcome.is_err());
    assert!(written < plan.total_bytes);
}

#[tokio::test]
async fn a_download_across_a_missing_file_is_exactly_its_content_length() {
    let scratch = super::Scratch::new("export-download");
    let state = AppState::bootstrap(AppConfig {
        data_dir: scratch.root.join("data"),
        ..AppConfig::default()
    })
    .expect("bootstrap");
    let session_id = state
        .sessions
        .create(&crate::models::SessionDraft {
            device_id: "generated".to_string(),
            device_name: "generated".to_string(),
            sample_rate: 48_000,
            channels: 1,
            started_at_ms: T0,
        })
        .expect("session")
        .id;
    let layout = SegmentLayout::under_data_dir(&state.config.data_dir);
    let mut files = Vec::new();
    for sequence in 0..3 {
        let start_ms = T0 + sequence * 1_000;
        files.push(seed_segment(
            &state.segments,
            &SeedAt {
                layout: layout.clone(),
                session_id,
                sequence,
                start_ms,
                end_ms: start_ms + 1_000,
                level: 1_000 * (sequence as i16 + 1),
            },
        ));
    }
    std::fs::remove_file(&files[1]).expect("delete");

    let router = crate::routes::build(state.clone());
    let request = Request::builder()
        .uri(format!("/api/export?fromMs={T0}&toMs={}", T0 + 3_000))
        .header(header::HOST, "recorder.test")
        .body(Body::empty())
        .expect("request");
    let response = router.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let promised: usize = response
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .expect("content length");
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");

    assert_eq!(body.len(), promised);
    assert_eq!(&body[..4], b"RIFF");
    assert_eq!(second_levels(&body), vec![1_000, 0, 3_000]);
    drop(state);
}

#[test]
fn envelopes_of_the_wrong_length_draw_only_where_audio_was_recorded() {
    let store = Store::new("peaks-wrong-length");
    let day = crate::util::day::local_day(T0);
    let rows = [
        // A second of audio whose envelope is far too long: it would spill into the gap after it.
        (0, 0, 1_000, vec![200u8; 50]),
        // One too short, and one missing altogether.
        (1, 5_000, 6_000, vec![100u8; 3]),
        (2, 6_000, 7_000, Vec::new()),
    ];
    for (sequence, start, end, peaks) in rows {
        store
            .segments
            .insert(&SegmentDraft {
                session_id: store.session_id,
                sequence,
                day: day.clone(),
                path: format!("recordings/{day}/1/{sequence:06}.pcm"),
                started_at_ms: T0 + start,
                ended_at_ms: T0 + end,
                sample_rate: 48_000,
                channels: 1,
                byte_len: 96_000,
                peaks,
            })
            .expect("row");
    }

    let timeline = TimelineService::new(store.segments.clone(), store.hub.clone());
    let view = timeline.peaks(T0, T0 + 10_000, 100).expect("peaks");

    // One column per 100 ms. The recorder was not running from one second to five, so nothing may be
    // drawn there, whatever a damaged envelope claims.
    assert_eq!(view.values.len(), 100);
    assert!(view.values[..10].iter().all(|value| *value == 200));
    assert!(
        view.values[11..50].iter().all(|value| *value == 0),
        "drawn in a gap: {:?}",
        &view.values[10..50]
    );
    assert!(view.values[50..53].iter().all(|value| *value == 100));

    // Nor may a sound be found there for the next and previous buttons to jump into.
    let sounds = timeline
        .sounds(T0, T0 + 10_000, SoundSensitivity::High)
        .expect("sounds");
    for sound in sounds {
        assert!(
            sound.end_ms <= T0 + 1_100 || sound.start_ms >= T0 + 5_000,
            "a sound in the gap: {sound:?}"
        );
    }
}

fn export_bytes(store: &Store, from_ms: i64, to_ms: i64) -> Vec<u8> {
    let service = export(store);
    let plan = service.plan(TimeRange::new(from_ms, to_ms)).expect("plan");
    let mut bytes = Vec::new();
    service
        .write(&plan, |chunk| {
            bytes.extend_from_slice(&chunk);
            true
        })
        .expect("a missing file does not fail the export");
    assert_eq!(
        bytes.len() as u64,
        plan.total_bytes,
        "exactly the length the header promised"
    );
    bytes
}

fn samples_of(wav_bytes: &[u8]) -> Vec<i16> {
    crate::audio::segment_writer::decode_pcm_s16(&wav_bytes[wav::HEADER_BYTES..])
}

/// The level in the middle of each second of an exported file.
fn second_levels(wav_bytes: &[u8]) -> Vec<i16> {
    let samples = samples_of(wav_bytes);
    samples
        .chunks(48_000)
        .map(|second| second[second.len() / 2])
        .collect()
}
