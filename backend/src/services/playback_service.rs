//! DVR playback: reading recorded audio back off disk as if it were arriving live.
//!
//! A cursor is a stateful read head over the segment index. It knows where it is on the wall clock,
//! which segment file that lands in, and how far into that file the read head sits. Each call produces
//! one frame, and the caller decides how fast to call, which is what keeps pacing policy in the streaming
//! layer where it belongs.
//!
//! Gaps are first class. A recorder that was stopped overnight leaves a hole, and a cursor that silently
//! skipped it would make an eight hour gap play as an instant jump with no explanation. Instead the
//! cursor reports the hole and moves to the next material.

use std::sync::Arc;

use crate::audio::frame_builder::frame_samples_for;
use crate::audio::segment_writer::{decode_pcm_s16, read_segment_bytes};
use crate::config::AppConfig;
use crate::error::AppResult;
use crate::models::{AudioFrame, Segment};
use crate::repositories::SegmentRepository;

/// What one step of a cursor produced.
#[derive(Debug)]
pub enum CursorOutput {
    /// Audio, flagged historic so the client can tell it apart from the live feed.
    Frame(AudioFrame),
    /// No recording existed between these two moments, and the cursor has moved past it.
    Gap { from_ms: i64, to_ms: i64 },
    /// Nothing further exists on disk. The caller decides whether to wait, or to join the live feed.
    EndOfRecording { at_ms: i64 },
}

/// Segments the cursor will consult before giving up in one step.
///
/// A run of empty or truncated files could otherwise spin the loop, and a bound turns that into a
/// reported end of recording instead of a stuck connection.
const MAX_SEGMENT_HOPS: usize = 16;

pub struct PlaybackService {
    config: Arc<AppConfig>,
    segments: Arc<SegmentRepository>,
}

impl PlaybackService {
    pub fn new(config: Arc<AppConfig>, segments: Arc<SegmentRepository>) -> Self {
        Self { config, segments }
    }

    /// Open a read head at `position_ms` producing frames of `frame_ms`.
    pub fn cursor_at(&self, position_ms: i64, frame_ms: u32) -> PlaybackCursor {
        PlaybackCursor {
            config: self.config.clone(),
            segments: self.segments.clone(),
            frame_ms,
            position_ms,
            open: None,
        }
    }

    /// Oldest moment that can still be played.
    pub fn earliest_ms(&self) -> AppResult<Option<i64>> {
        Ok(self.segments.stats()?.oldest_ms)
    }

    /// Newest moment on disk, which is where playback catches up with the live feed.
    pub fn latest_ms(&self) -> AppResult<Option<i64>> {
        self.segments.latest_end_ms()
    }
}

struct OpenSegment {
    segment: Segment,
    /// Read head inside the segment file.
    offset: i64,
}

pub struct PlaybackCursor {
    config: Arc<AppConfig>,
    segments: Arc<SegmentRepository>,
    frame_ms: u32,
    position_ms: i64,
    open: Option<OpenSegment>,
}

impl PlaybackCursor {
    pub fn position_ms(&self) -> i64 {
        self.position_ms
    }

    /// Move the read head. The next call reopens whichever segment covers the new position.
    pub fn seek(&mut self, position_ms: i64) {
        self.position_ms = position_ms;
        self.open = None;
    }

    /// Produce the next step of playback.
    pub fn advance(&mut self) -> AppResult<CursorOutput> {
        for _ in 0..MAX_SEGMENT_HOPS {
            if self.open.is_none() {
                match self.locate()? {
                    Located::Ready => {}
                    Located::Jumped { from_ms, to_ms } => {
                        return Ok(CursorOutput::Gap { from_ms, to_ms });
                    }
                    Located::Exhausted => {
                        return Ok(CursorOutput::EndOfRecording {
                            at_ms: self.position_ms,
                        });
                    }
                }
            }

            match self.read_frame()? {
                Some(frame) => return Ok(CursorOutput::Frame(frame)),
                None => continue,
            }
        }

        Ok(CursorOutput::EndOfRecording {
            at_ms: self.position_ms,
        })
    }

    /// Point `open` at the segment covering the current position, jumping a gap if there is one.
    fn locate(&mut self) -> AppResult<Located> {
        if let Some(segment) = self.segments.find_covering(self.position_ms)? {
            let offset = segment.byte_offset_for(self.position_ms);
            self.open = Some(OpenSegment { segment, offset });
            return Ok(Located::Ready);
        }

        let Some(segment) = self.segments.find_next_after(self.position_ms)? else {
            return Ok(Located::Exhausted);
        };

        let from_ms = self.position_ms;
        let to_ms = segment.started_at_ms;
        self.position_ms = to_ms;
        self.open = Some(OpenSegment { segment, offset: 0 });

        Ok(Located::Jumped { from_ms, to_ms })
    }

    /// Read one frame from the open segment, or `None` when it is spent and the caller should retry.
    fn read_frame(&mut self) -> AppResult<Option<AudioFrame>> {
        let Some(open) = self.open.as_mut() else {
            return Ok(None);
        };

        let available = open.segment.byte_len - open.offset;
        if available <= 0 {
            self.advance_past_open_segment();
            return Ok(None);
        }

        let samples_per_frame = frame_samples_for(open.segment.sample_rate, self.frame_ms) as i64;
        let wanted = samples_per_frame * open.segment.bytes_per_sample_frame();
        let read_len = wanted.min(available).max(0) as usize;

        let path = self.config.resolve_segment_path(&open.segment.path);
        let bytes = match read_segment_bytes(&path, open.offset, read_len) {
            Ok(bytes) => bytes,
            Err(error) => {
                // The index outlived the file, which happens if the data directory was pruned by hand.
                // Skipping the segment keeps playback moving instead of failing the whole connection.
                tracing::warn!(%error, segment_id = open.segment.id, "skipping an unreadable segment");
                self.advance_past_open_segment();
                return Ok(None);
            }
        };

        if bytes.len() < 2 {
            self.advance_past_open_segment();
            return Ok(None);
        }

        let timestamp_ms = open.segment.timestamp_for_offset(open.offset);
        let frame = AudioFrame::from_samples(
            timestamp_ms,
            open.segment.sample_rate,
            open.segment.channels,
            decode_pcm_s16(&bytes),
            false,
        );

        open.offset += bytes.len() as i64;
        self.position_ms = frame.end_timestamp_ms();

        Ok(Some(frame))
    }

    fn advance_past_open_segment(&mut self) {
        if let Some(open) = self.open.take() {
            // Never move backwards, or a short segment would replay forever.
            self.position_ms = self.position_ms.max(open.segment.ended_at_ms);
        }
    }
}

enum Located {
    Ready,
    Jumped { from_ms: i64, to_ms: i64 },
    Exhausted,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{PcmS16Encoder, SegmentLocation, SegmentWriter};
    use crate::db::Database;
    use crate::models::SessionDraft;
    use crate::repositories::SessionRepository;
    use std::path::PathBuf;

    struct Fixture {
        service: PlaybackService,
        data_dir: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.data_dir).ok();
        }
    }

    /// Build a real data directory plus index: these tests are about the interaction between the two, so
    /// stubbing either half would test nothing.
    fn fixture(name: &str, ranges: &[(i64, i64)]) -> Fixture {
        let data_dir =
            std::env::temp_dir().join(format!("oar-playback-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&data_dir).ok();
        std::fs::create_dir_all(&data_dir).expect("data dir");

        let config = Arc::new(AppConfig {
            data_dir: data_dir.clone(),
            ..AppConfig::default()
        });

        let database = Arc::new(Database::open_in_memory().expect("database"));
        let sessions = SessionRepository::new(database.clone());
        let segments = Arc::new(SegmentRepository::new(database));

        let session = sessions
            .create(&SessionDraft {
                device_id: "mic".to_string(),
                device_name: "mic".to_string(),
                sample_rate: 48_000,
                channels: 1,
                started_at_ms: ranges.first().map(|item| item.0).unwrap_or(0),
            })
            .expect("session");

        for (sequence, (start_ms, end_ms)) in ranges.iter().enumerate() {
            let sequence = sequence as i64;
            let first = AudioFrame::from_samples(*start_ms, 48_000, 1, vec![0; 4800], true);
            let location = SegmentLocation::for_segment(
                &crate::audio::SegmentLayout::under_data_dir(&config.data_dir),
                session.id,
                sequence,
                *start_ms,
            );
            let mut writer = SegmentWriter::create(location, &first).expect("writer");

            let frames = ((end_ms - start_ms) / 100) as usize;
            for index in 0..frames {
                let frame = AudioFrame::from_samples(
                    start_ms + index as i64 * 100,
                    48_000,
                    1,
                    vec![(index as i16 % 100) * 300; 4800],
                    true,
                );
                writer.append(&frame, &PcmS16Encoder).expect("append");
            }

            let draft = writer.finish().expect("finish").expect("indexed");
            segments.insert(&draft).expect("insert");
        }

        Fixture {
            service: PlaybackService::new(config, segments),
            data_dir,
        }
    }

    #[test]
    fn plays_frames_in_order_from_the_requested_moment() {
        let fixture = fixture("ordered", &[(10_000, 20_000)]);
        let mut cursor = fixture.service.cursor_at(12_000, 100);

        let first = match cursor.advance().expect("next") {
            CursorOutput::Frame(frame) => frame,
            other => panic!("expected a frame, got {other:?}"),
        };
        assert_eq!(first.timestamp_ms, 12_000);
        assert!(!first.live);
        assert_eq!(first.sample_count(), 4800);

        let second = match cursor.advance().expect("next") {
            CursorOutput::Frame(frame) => frame,
            other => panic!("expected a frame, got {other:?}"),
        };
        assert_eq!(second.timestamp_ms, 12_100);
        assert_eq!(cursor.position_ms(), 12_200);
    }

    #[test]
    fn rolls_from_one_segment_into_the_next() {
        let fixture = fixture("rollover", &[(0, 1_000), (1_000, 2_000)]);
        let mut cursor = fixture.service.cursor_at(900, 100);

        let mut timestamps = Vec::new();
        for _ in 0..3 {
            match cursor.advance().expect("next") {
                CursorOutput::Frame(frame) => timestamps.push(frame.timestamp_ms),
                other => panic!("expected a frame, got {other:?}"),
            }
        }

        assert_eq!(timestamps, vec![900, 1_000, 1_100]);
    }

    #[test]
    fn reports_a_gap_and_resumes_after_it() {
        let fixture = fixture("gap", &[(0, 1_000), (60_000, 61_000)]);
        let mut cursor = fixture.service.cursor_at(500, 100);

        // Drain the first segment.
        while let CursorOutput::Frame(_) = cursor.advance().expect("next") {
            if cursor.position_ms() >= 1_000 {
                break;
            }
        }

        match cursor.advance().expect("next") {
            CursorOutput::Gap { from_ms, to_ms } => {
                assert_eq!(from_ms, 1_000);
                assert_eq!(to_ms, 60_000);
            }
            other => panic!("expected a gap, got {other:?}"),
        }

        match cursor.advance().expect("next") {
            CursorOutput::Frame(frame) => assert_eq!(frame.timestamp_ms, 60_000),
            other => panic!("expected a frame, got {other:?}"),
        }
    }

    #[test]
    fn reports_the_end_of_the_recording() {
        let fixture = fixture("end", &[(0, 500)]);
        let mut cursor = fixture.service.cursor_at(0, 100);

        for _ in 0..5 {
            assert!(matches!(
                cursor.advance().expect("next"),
                CursorOutput::Frame(_)
            ));
        }

        match cursor.advance().expect("next") {
            CursorOutput::EndOfRecording { at_ms } => assert_eq!(at_ms, 500),
            other => panic!("expected the end of the recording, got {other:?}"),
        }
    }

    #[test]
    fn seeking_backwards_replays_the_same_audio() {
        let fixture = fixture("seek", &[(0, 2_000)]);
        let mut cursor = fixture.service.cursor_at(1_000, 100);

        let first = match cursor.advance().expect("next") {
            CursorOutput::Frame(frame) => frame,
            other => panic!("expected a frame, got {other:?}"),
        };

        cursor.seek(1_000);
        let again = match cursor.advance().expect("next") {
            CursorOutput::Frame(frame) => frame,
            other => panic!("expected a frame, got {other:?}"),
        };

        assert_eq!(first.timestamp_ms, again.timestamp_ms);
        assert_eq!(first.samples, again.samples);
    }

    #[test]
    fn seeking_before_the_oldest_recording_jumps_forward_to_it() {
        let fixture = fixture("early", &[(100_000, 101_000)]);
        let mut cursor = fixture.service.cursor_at(0, 100);

        match cursor.advance().expect("next") {
            CursorOutput::Gap { from_ms, to_ms } => {
                assert_eq!(from_ms, 0);
                assert_eq!(to_ms, 100_000);
            }
            other => panic!("expected a gap, got {other:?}"),
        }
    }

    #[test]
    fn service_reports_the_playable_bounds() {
        let fixture = fixture("bounds", &[(5_000, 6_000), (7_000, 8_000)]);
        assert_eq!(
            fixture.service.earliest_ms().expect("earliest"),
            Some(5_000)
        );
        assert_eq!(fixture.service.latest_ms().expect("latest"), Some(8_000));
    }
}
