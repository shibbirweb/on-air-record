//! Persistence for the segment index.
//!
//! This is the hot table of the DVR. Every seek, every timeline paint, and every retention pass goes
//! through it, so the queries here are all served by `idx_segments_range`.

use std::sync::Arc;

use rusqlite::Row;

use crate::db::Database;
use crate::error::AppResult;
use crate::models::{Segment, SegmentDraft, TimeRange};

/// One calendar day that holds recordings, summarised for the day picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaySummary {
    /// Local calendar day, `YYYY-MM-DD`.
    pub day: String,
    /// First and last moment actually recorded on that day, which is usually a slice of it rather than
    /// the whole twenty four hours.
    pub start_ms: i64,
    pub end_ms: i64,
    pub segment_count: i64,
    pub bytes: i64,
    /// Audio actually on disk for the day, which is less than `end_ms - start_ms` whenever the recorder
    /// was stopped part way through.
    pub recorded_ms: i64,
}

/// Aggregate view used by the storage endpoint.
#[derive(Debug, Clone, Default)]
pub struct SegmentStorageStats {
    pub segment_count: i64,
    pub bytes: i64,
    pub oldest_ms: Option<i64>,
    pub newest_ms: Option<i64>,
}

const SELECT_COLUMNS: &str = "id, session_id, sequence, day, path, started_at_ms, ended_at_ms, \
                              sample_rate, channels, byte_len, peaks";

pub struct SegmentRepository {
    database: Arc<Database>,
}

impl SegmentRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub fn insert(&self, draft: &SegmentDraft) -> AppResult<i64> {
        self.database.with_connection(|conn| {
            conn.execute(
                "INSERT INTO segments
                    (session_id, sequence, day, path, started_at_ms, ended_at_ms, sample_rate, channels, byte_len, peaks)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    draft.session_id,
                    draft.sequence,
                    draft.day,
                    draft.path,
                    draft.started_at_ms,
                    draft.ended_at_ms,
                    draft.sample_rate,
                    draft.channels,
                    draft.byte_len,
                    draft.peaks,
                ],
            )?;
            Ok(conn.last_insert_rowid())
        })
    }

    /// The segment that contains `timestamp_ms`, if the recorder was running then.
    pub fn find_covering(&self, timestamp_ms: i64) -> AppResult<Option<Segment>> {
        self.database.with_connection(|conn| {
            let sql = format!(
                "SELECT {SELECT_COLUMNS} FROM segments
                 WHERE started_at_ms <= ?1 AND ended_at_ms > ?1
                 ORDER BY started_at_ms DESC LIMIT 1"
            );
            let mut statement = conn.prepare(&sql)?;
            let mut rows = statement.query(rusqlite::params![timestamp_ms])?;
            match rows.next()? {
                Some(row) => Ok(Some(map_segment(row)?)),
                None => Ok(None),
            }
        })
    }

    /// The first segment that starts at or after `timestamp_ms`.
    ///
    /// Used to jump across a recording gap: when nothing covers the requested moment, playback resumes at
    /// the next material rather than failing.
    pub fn find_next_after(&self, timestamp_ms: i64) -> AppResult<Option<Segment>> {
        self.database.with_connection(|conn| {
            let sql = format!(
                "SELECT {SELECT_COLUMNS} FROM segments
                 WHERE started_at_ms >= ?1
                 ORDER BY started_at_ms ASC LIMIT 1"
            );
            let mut statement = conn.prepare(&sql)?;
            let mut rows = statement.query(rusqlite::params![timestamp_ms])?;
            match rows.next()? {
                Some(row) => Ok(Some(map_segment(row)?)),
                None => Ok(None),
            }
        })
    }

    /// Every segment overlapping `range`, oldest first.
    pub fn find_in_range(&self, range: TimeRange) -> AppResult<Vec<Segment>> {
        self.database.with_connection(|conn| {
            let sql = format!(
                "SELECT {SELECT_COLUMNS} FROM segments
                 WHERE started_at_ms < ?2 AND ended_at_ms > ?1
                 ORDER BY started_at_ms ASC"
            );
            let mut statement = conn.prepare(&sql)?;
            let rows = statement
                .query_map(rusqlite::params![range.start_ms, range.end_ms], |row| {
                    map_segment(row)
                })?;

            let mut segments = Vec::new();
            for row in rows {
                segments.push(row?);
            }
            Ok(segments)
        })
    }

    /// Contiguous coverage bands, so the timeline can shade what actually exists.
    ///
    /// Segments closer together than `gap_tolerance_ms` are merged, because the millisecond rounding
    /// between one segment ending and the next starting should not draw as a hole.
    pub fn coverage(&self, gap_tolerance_ms: i64) -> AppResult<Vec<TimeRange>> {
        let ranges: Vec<TimeRange> = self.database.with_connection(|conn| {
            let mut statement = conn.prepare(
                "SELECT started_at_ms, ended_at_ms FROM segments ORDER BY started_at_ms ASC",
            )?;
            let rows =
                statement.query_map([], |row| Ok(TimeRange::new(row.get(0)?, row.get(1)?)))?;

            let mut ranges = Vec::new();
            for row in rows {
                ranges.push(row?);
            }
            Ok(ranges)
        })?;

        Ok(merge_ranges(ranges, gap_tolerance_ms))
    }

    /// Every day that holds recordings, newest first.
    ///
    /// Served straight from `idx_segments_day`, so opening the day picker costs one grouped scan of the
    /// index rather than a walk over the segments themselves.
    pub fn days(&self) -> AppResult<Vec<DaySummary>> {
        self.database.with_connection(|conn| {
            let mut statement = conn.prepare(
                "SELECT day,
                        MIN(started_at_ms),
                        MAX(ended_at_ms),
                        COUNT(*),
                        COALESCE(SUM(byte_len), 0),
                        COALESCE(SUM(ended_at_ms - started_at_ms), 0)
                 FROM segments
                 GROUP BY day
                 ORDER BY day DESC",
            )?;

            let rows = statement.query_map([], |row| {
                Ok(DaySummary {
                    day: row.get(0)?,
                    start_ms: row.get(1)?,
                    end_ms: row.get(2)?,
                    segment_count: row.get(3)?,
                    bytes: row.get(4)?,
                    recorded_ms: row.get(5)?,
                })
            })?;

            let mut days = Vec::new();
            for row in rows {
                days.push(row?);
            }
            Ok(days)
        })
    }

    pub fn stats(&self) -> AppResult<SegmentStorageStats> {
        self.database.with_connection(|conn| {
            conn.query_row(
                "SELECT COUNT(*), COALESCE(SUM(byte_len), 0), MIN(started_at_ms), MAX(ended_at_ms)
                 FROM segments",
                [],
                |row| {
                    Ok(SegmentStorageStats {
                        segment_count: row.get(0)?,
                        bytes: row.get(1)?,
                        oldest_ms: row.get(2)?,
                        newest_ms: row.get(3)?,
                    })
                },
            )
            .map_err(Into::into)
        })
    }

    /// Newest indexed moment, which is the live edge while capture is stopped.
    pub fn latest_end_ms(&self) -> AppResult<Option<i64>> {
        self.database.with_connection(|conn| {
            conn.query_row("SELECT MAX(ended_at_ms) FROM segments", [], |row| {
                row.get(0)
            })
            .map_err(Into::into)
        })
    }

    /// Segments that ended before `cutoff_ms`, which the janitor is about to delete.
    pub fn find_expired(&self, cutoff_ms: i64, limit: i64) -> AppResult<Vec<Segment>> {
        self.database.with_connection(|conn| {
            let sql = format!(
                "SELECT {SELECT_COLUMNS} FROM segments
                 WHERE ended_at_ms < ?1
                 ORDER BY ended_at_ms ASC LIMIT ?2"
            );
            let mut statement = conn.prepare(&sql)?;
            let rows = statement.query_map(rusqlite::params![cutoff_ms, limit], map_segment)?;

            let mut segments = Vec::new();
            for row in rows {
                segments.push(row?);
            }
            Ok(segments)
        })
    }

    pub fn delete(&self, segment_id: i64) -> AppResult<()> {
        self.database.with_connection(|conn| {
            conn.execute(
                "DELETE FROM segments WHERE id = ?1",
                rusqlite::params![segment_id],
            )?;
            Ok(())
        })
    }
}

fn map_segment(row: &Row<'_>) -> rusqlite::Result<Segment> {
    Ok(Segment {
        id: row.get(0)?,
        session_id: row.get(1)?,
        sequence: row.get(2)?,
        day: row.get(3)?,
        path: row.get(4)?,
        started_at_ms: row.get(5)?,
        ended_at_ms: row.get(6)?,
        sample_rate: row.get::<_, i64>(7)? as u32,
        channels: row.get::<_, i64>(8)? as u16,
        byte_len: row.get(9)?,
        peaks: row.get(10)?,
    })
}

/// Merge already sorted ranges that are adjacent within `gap_tolerance_ms`.
fn merge_ranges(ranges: Vec<TimeRange>, gap_tolerance_ms: i64) -> Vec<TimeRange> {
    let mut merged: Vec<TimeRange> = Vec::new();

    for range in ranges {
        match merged.last_mut() {
            Some(previous) if range.start_ms - previous.end_ms <= gap_tolerance_ms => {
                if range.end_ms > previous.end_ms {
                    previous.end_ms = range.end_ms;
                }
            }
            _ => merged.push(range),
        }
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SessionDraft;
    use crate::repositories::SessionRepository;

    struct Fixture {
        segments: SegmentRepository,
        session_id: i64,
    }

    fn fixture() -> Fixture {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let sessions = SessionRepository::new(database.clone());
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
            segments: SegmentRepository::new(database),
            session_id: session.id,
        }
    }

    fn draft(fixture: &Fixture, sequence: i64, start_ms: i64, end_ms: i64) -> SegmentDraft {
        let day = crate::util::day::local_day(start_ms);
        SegmentDraft {
            session_id: fixture.session_id,
            sequence,
            path: format!("recordings/{day}/1/{sequence:06}.pcm"),
            day,
            started_at_ms: start_ms,
            ended_at_ms: end_ms,
            sample_rate: 48_000,
            channels: 1,
            byte_len: (end_ms - start_ms) * 96,
            peaks: vec![7; ((end_ms - start_ms) / 100) as usize],
        }
    }

    #[test]
    fn find_covering_returns_the_owning_segment() {
        let fixture = fixture();
        fixture
            .segments
            .insert(&draft(&fixture, 0, 0, 10_000))
            .expect("insert");
        fixture
            .segments
            .insert(&draft(&fixture, 1, 10_000, 20_000))
            .expect("insert");

        let found = fixture
            .segments
            .find_covering(15_000)
            .expect("find")
            .expect("present");
        assert_eq!(found.sequence, 1);
        assert!(fixture
            .segments
            .find_covering(25_000)
            .expect("find")
            .is_none());
    }

    #[test]
    fn find_next_after_skips_a_gap() {
        let fixture = fixture();
        fixture
            .segments
            .insert(&draft(&fixture, 0, 0, 10_000))
            .expect("insert");
        fixture
            .segments
            .insert(&draft(&fixture, 1, 60_000, 70_000))
            .expect("insert");

        let next = fixture
            .segments
            .find_next_after(20_000)
            .expect("find")
            .expect("present");
        assert_eq!(next.started_at_ms, 60_000);
    }

    #[test]
    fn find_in_range_returns_only_overlapping_segments() {
        let fixture = fixture();
        fixture
            .segments
            .insert(&draft(&fixture, 0, 0, 10_000))
            .expect("insert");
        fixture
            .segments
            .insert(&draft(&fixture, 1, 10_000, 20_000))
            .expect("insert");
        fixture
            .segments
            .insert(&draft(&fixture, 2, 20_000, 30_000))
            .expect("insert");

        let found = fixture
            .segments
            .find_in_range(TimeRange::new(9_500, 20_500))
            .expect("range");
        assert_eq!(found.len(), 3);

        let narrow = fixture
            .segments
            .find_in_range(TimeRange::new(10_000, 20_000))
            .expect("range");
        assert_eq!(narrow.len(), 1);
        assert_eq!(narrow[0].sequence, 1);
    }

    #[test]
    fn coverage_merges_adjacent_segments_and_keeps_gaps() {
        let fixture = fixture();
        fixture
            .segments
            .insert(&draft(&fixture, 0, 0, 10_000))
            .expect("insert");
        fixture
            .segments
            .insert(&draft(&fixture, 1, 10_000, 20_000))
            .expect("insert");
        fixture
            .segments
            .insert(&draft(&fixture, 2, 60_000, 70_000))
            .expect("insert");

        let coverage = fixture.segments.coverage(200).expect("coverage");
        assert_eq!(coverage.len(), 2);
        assert_eq!(coverage[0], TimeRange::new(0, 20_000));
        assert_eq!(coverage[1], TimeRange::new(60_000, 70_000));
    }

    #[test]
    fn expired_segments_are_selected_by_cutoff() {
        let fixture = fixture();
        fixture
            .segments
            .insert(&draft(&fixture, 0, 0, 10_000))
            .expect("insert");
        fixture
            .segments
            .insert(&draft(&fixture, 1, 10_000, 20_000))
            .expect("insert");

        let expired = fixture.segments.find_expired(15_000, 100).expect("expired");
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].sequence, 0);

        fixture.segments.delete(expired[0].id).expect("delete");
        assert_eq!(fixture.segments.stats().expect("stats").segment_count, 1);
    }

    #[test]
    fn stats_aggregate_bytes_and_bounds() {
        let fixture = fixture();
        fixture
            .segments
            .insert(&draft(&fixture, 0, 0, 10_000))
            .expect("insert");
        fixture
            .segments
            .insert(&draft(&fixture, 1, 10_000, 20_000))
            .expect("insert");

        let stats = fixture.segments.stats().expect("stats");
        assert_eq!(stats.segment_count, 2);
        assert_eq!(stats.bytes, 10_000 * 96 * 2);
        assert_eq!(stats.oldest_ms, Some(0));
        assert_eq!(stats.newest_ms, Some(20_000));
    }

    #[test]
    fn days_group_segments_by_calendar_day() {
        use crate::util::day::{day_bounds_ms, local_day};

        let fixture = fixture();
        let today = local_day(crate::util::time::now_ms());
        let (today_start, _) = day_bounds_ms(&today).expect("bounds");

        // Two segments in the morning of one day, one the following day.
        fixture
            .segments
            .insert(&draft(
                &fixture,
                0,
                today_start + 3_600_000,
                today_start + 3_610_000,
            ))
            .expect("insert");
        fixture
            .segments
            .insert(&draft(
                &fixture,
                1,
                today_start + 3_610_000,
                today_start + 3_620_000,
            ))
            .expect("insert");
        fixture
            .segments
            .insert(&draft(
                &fixture,
                2,
                today_start + 90_000_000,
                today_start + 90_010_000,
            ))
            .expect("insert");

        let days = fixture.segments.days().expect("days");
        assert_eq!(days.len(), 2);

        // Newest first, so the picker opens on the most recent material.
        assert!(days[0].day > days[1].day);

        let earlier = &days[1];
        assert_eq!(earlier.day, today);
        assert_eq!(earlier.segment_count, 2);
        assert_eq!(earlier.start_ms, today_start + 3_600_000);
        assert_eq!(earlier.end_ms, today_start + 3_620_000);
        assert_eq!(earlier.recorded_ms, 20_000);
    }

    #[test]
    fn days_are_empty_before_anything_is_recorded() {
        assert!(fixture().segments.days().expect("days").is_empty());
    }

    #[test]
    fn the_stored_day_matches_the_path_the_file_was_written_to() {
        let fixture = fixture();
        let draft = draft(&fixture, 0, 1_757_030_400_000, 1_757_030_410_000);
        fixture.segments.insert(&draft).expect("insert");

        let stored = fixture
            .segments
            .find_covering(1_757_030_405_000)
            .expect("find")
            .expect("present");

        assert_eq!(stored.day, draft.day);
        assert!(
            stored.path.contains(&stored.day),
            "path {} should sit under its day directory",
            stored.path
        );
    }

    #[test]
    fn peaks_survive_the_blob_round_trip() {
        let fixture = fixture();
        fixture
            .segments
            .insert(&draft(&fixture, 0, 0, 1_000))
            .expect("insert");
        let segment = fixture
            .segments
            .find_covering(500)
            .expect("find")
            .expect("present");
        assert_eq!(segment.peaks, vec![7; 10]);
    }
}
