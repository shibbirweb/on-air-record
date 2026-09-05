//! Answers the two questions the timeline widget asks: what exists, and what does it look like.

use std::sync::Arc;

use crate::audio::peaks::{self, PeakSource, PEAK_BUCKET_MS};
use crate::error::{AppError, AppResult};
use crate::models::TimeRange;
use crate::repositories::{DaySummary, SegmentRepository};
use crate::services::BroadcastHub;
use crate::util::day::{day_bounds_ms, is_valid_day};

/// Segments separated by less than this are drawn as one continuous band.
///
/// Rounding between one segment ending and the next beginning is a few milliseconds, and half a second
/// is comfortably above that while still being invisible at any zoom level a person would use.
const COVERAGE_GAP_TOLERANCE_MS: i64 = 500;

/// Bounds on how many columns a client may request, so one request cannot ask the server to allocate an
/// unreasonable buffer.
pub const MIN_BUCKETS: usize = 16;
pub const MAX_BUCKETS: usize = 4000;
pub const DEFAULT_BUCKETS: usize = 1000;

/// Extent of the recorded material.
#[derive(Debug, Clone)]
pub struct TimelineRange {
    pub earliest_ms: Option<i64>,
    pub latest_ms: Option<i64>,
    pub live_edge_ms: Option<i64>,
    pub coverage: Vec<TimeRange>,
}

/// A day that holds recordings, with both the extent of the audio and the day it sits in.
///
/// Both are returned because the UI wants each for a different job: the recorded extent decides where to
/// put the playhead when a day is picked, while the midnight bounds decide the window to draw, so that a
/// twenty minute recording is visibly twenty minutes out of a day rather than filling the screen.
#[derive(Debug, Clone)]
pub struct RecordingDay {
    pub summary: DaySummary,
    pub day_start_ms: i64,
    pub day_end_ms: i64,
}

/// A rendered waveform window.
#[derive(Debug, Clone)]
pub struct PeaksView {
    pub from_ms: i64,
    pub to_ms: i64,
    pub bucket_ms: i64,
    pub values: Vec<u8>,
}

pub struct TimelineService {
    segments: Arc<SegmentRepository>,
    hub: Arc<BroadcastHub>,
}

impl TimelineService {
    pub fn new(segments: Arc<SegmentRepository>, hub: Arc<BroadcastHub>) -> Self {
        Self { segments, hub }
    }

    /// What the timeline can scroll over.
    ///
    /// The live edge comes from the hub rather than the index, because the segment being recorded right
    /// now is not indexed yet and the timeline should still show the playhead advancing.
    pub fn range(&self) -> AppResult<TimelineRange> {
        let stats = self.segments.stats()?;
        let coverage = self.segments.coverage(COVERAGE_GAP_TOLERANCE_MS)?;
        let live_edge_ms = self.hub.live_edge_ms();

        let latest_ms = match (stats.newest_ms, live_edge_ms) {
            (Some(indexed), Some(live)) => Some(indexed.max(live)),
            (Some(indexed), None) => Some(indexed),
            (None, live) => live,
        };

        Ok(TimelineRange {
            earliest_ms: stats.oldest_ms,
            latest_ms,
            live_edge_ms,
            coverage,
        })
    }

    /// Every day that holds recordings, newest first.
    ///
    /// A day whose bounds cannot be resolved is dropped rather than reported with nonsense bounds. That
    /// only happens for a corrupted `day` value, and a missing entry is easier to understand than a day
    /// that scrolls the timeline somewhere impossible.
    pub fn days(&self) -> AppResult<Vec<RecordingDay>> {
        let summaries = self.segments.days()?;
        let mut days = Vec::with_capacity(summaries.len());

        for summary in summaries {
            let Some((day_start_ms, day_end_ms)) = day_bounds_ms(&summary.day) else {
                tracing::warn!(day = summary.day, "skipping a day with an unparseable date");
                continue;
            };

            days.push(RecordingDay {
                summary,
                day_start_ms,
                day_end_ms,
            });
        }

        Ok(days)
    }

    /// Midnight to midnight bounds of one day, for jumping the timeline straight to it.
    pub fn day_window(&self, day: &str) -> AppResult<(i64, i64)> {
        if !is_valid_day(day) {
            return Err(AppError::bad_request(
                "day must be a calendar date in YYYY-MM-DD form",
            ));
        }

        day_bounds_ms(day)
            .ok_or_else(|| AppError::bad_request(format!("'{day}' is not a date on the calendar")))
    }

    /// Render the stored envelopes of a window onto `buckets` columns.
    pub fn peaks(&self, from_ms: i64, to_ms: i64, buckets: usize) -> AppResult<PeaksView> {
        if to_ms <= from_ms {
            return Err(AppError::bad_request("toMs must be greater than fromMs"));
        }

        let buckets = buckets.clamp(MIN_BUCKETS, MAX_BUCKETS);
        let segments = self
            .segments
            .find_in_range(TimeRange::new(from_ms, to_ms))?;

        let sources: Vec<PeakSource<'_>> = segments
            .iter()
            .map(|segment| PeakSource {
                start_ms: segment.started_at_ms,
                values: &segment.peaks,
            })
            .collect();

        let values = peaks::render(&sources, from_ms, to_ms, buckets);
        let bucket_ms = ((to_ms - from_ms) / buckets as i64).max(1);

        Ok(PeaksView {
            from_ms,
            to_ms,
            bucket_ms,
            values,
        })
    }

    /// Resolution of a stored envelope bucket, exposed so the UI can decide when to stop zooming in.
    pub fn source_bucket_ms(&self) -> i64 {
        PEAK_BUCKET_MS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::models::{SegmentDraft, SessionDraft};
    use crate::repositories::SessionRepository;

    struct Fixture {
        service: TimelineService,
        segments: Arc<SegmentRepository>,
        session_id: i64,
    }

    fn fixture() -> Fixture {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let sessions = SessionRepository::new(database.clone());
        let segments = Arc::new(SegmentRepository::new(database));
        let hub = Arc::new(BroadcastHub::new());

        let session = sessions
            .create(&SessionDraft {
                device_id: "mic".to_string(),
                device_name: "mic".to_string(),
                sample_rate: 48_000,
                channels: 1,
                started_at_ms: 0,
            })
            .expect("session");

        Fixture {
            service: TimelineService::new(segments.clone(), hub),
            segments,
            session_id: session.id,
        }
    }

    fn insert(fixture: &Fixture, sequence: i64, start_ms: i64, end_ms: i64, level: u8) {
        let buckets = ((end_ms - start_ms) / PEAK_BUCKET_MS).max(1) as usize;
        fixture
            .segments
            .insert(&SegmentDraft {
                session_id: fixture.session_id,
                sequence,
                day: crate::util::day::local_day(start_ms),
                path: format!("recordings/1/{sequence:06}.pcm"),
                started_at_ms: start_ms,
                ended_at_ms: end_ms,
                sample_rate: 48_000,
                channels: 1,
                byte_len: (end_ms - start_ms) * 96,
                peaks: vec![level; buckets],
            })
            .expect("insert");
    }

    #[test]
    fn range_is_empty_before_anything_is_recorded() {
        let range = fixture().service.range().expect("range");
        assert_eq!(range.earliest_ms, None);
        assert_eq!(range.latest_ms, None);
        assert!(range.coverage.is_empty());
    }

    #[test]
    fn range_merges_contiguous_coverage_and_keeps_holes() {
        let fixture = fixture();
        insert(&fixture, 0, 0, 10_000, 50);
        insert(&fixture, 1, 10_000, 20_000, 50);
        insert(&fixture, 2, 90_000, 100_000, 50);

        let range = fixture.service.range().expect("range");
        assert_eq!(range.earliest_ms, Some(0));
        assert_eq!(range.latest_ms, Some(100_000));
        assert_eq!(range.coverage.len(), 2);
        assert_eq!(range.coverage[0], TimeRange::new(0, 20_000));
    }

    #[test]
    fn days_carry_both_the_recorded_extent_and_the_whole_day() {
        use crate::util::day::{day_bounds_ms, local_day};

        let fixture = fixture();
        let today = local_day(crate::util::time::now_ms());
        let (today_start, today_end) = day_bounds_ms(&today).expect("bounds");

        insert(
            &fixture,
            0,
            today_start + 3_600_000,
            today_start + 3_660_000,
            90,
        );

        let days = fixture.service.days().expect("days");
        assert_eq!(days.len(), 1);

        let day = &days[0];
        assert_eq!(day.summary.day, today);
        assert_eq!(day.summary.start_ms, today_start + 3_600_000);
        assert_eq!(day.day_start_ms, today_start);
        assert_eq!(day.day_end_ms, today_end);
        // The recording is a slice of the day, not the whole thing.
        assert!(day.summary.start_ms > day.day_start_ms);
        assert!(day.summary.end_ms < day.day_end_ms);
    }

    #[test]
    fn day_window_rejects_anything_that_is_not_a_date() {
        let fixture = fixture();
        assert!(fixture.service.day_window("2026-09-05").is_ok());

        for invalid in ["", "yesterday", "2026-13-01", "../../etc"] {
            assert!(fixture.service.day_window(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn peaks_cover_the_requested_window() {
        let fixture = fixture();
        insert(&fixture, 0, 0, 10_000, 200);

        let view = fixture.service.peaks(0, 10_000, 100).expect("peaks");
        assert_eq!(view.values.len(), 100);
        assert!(view.values.iter().all(|value| *value == 200));
        assert_eq!(view.bucket_ms, 100);
    }

    #[test]
    fn peaks_are_zero_where_nothing_was_recorded() {
        let fixture = fixture();
        insert(&fixture, 0, 0, 1_000, 180);

        let view = fixture.service.peaks(0, 2_000, 20).expect("peaks");
        assert!(view.values[0..10].iter().all(|value| *value == 180));
        assert!(view.values[10..20].iter().all(|value| *value == 0));
    }

    #[test]
    fn bucket_count_is_clamped_to_the_supported_range() {
        let fixture = fixture();
        insert(&fixture, 0, 0, 10_000, 100);

        assert_eq!(
            fixture
                .service
                .peaks(0, 10_000, 1)
                .expect("peaks")
                .values
                .len(),
            MIN_BUCKETS
        );
        assert_eq!(
            fixture
                .service
                .peaks(0, 10_000, 100_000)
                .expect("peaks")
                .values
                .len(),
            MAX_BUCKETS
        );
    }

    #[test]
    fn an_inverted_window_is_rejected() {
        assert!(fixture().service.peaks(10_000, 10_000, 100).is_err());
        assert!(fixture().service.peaks(10_000, 5_000, 100).is_err());
    }
}
