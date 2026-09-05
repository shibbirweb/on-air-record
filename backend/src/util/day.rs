//! Calendar days on the recording timeline.
//!
//! A DVR is addressed by absolute milliseconds, but people ask for material by day: "play yesterday
//! afternoon". This module is the single place that converts between the two, so the directory a segment
//! is written to, the `day` column it is indexed under, and the day the UI offers can never disagree.
//!
//! Days are **local** to the host machine, not UTC. The recorder and the person reading the timeline are
//! on the same network and almost always in the same timezone, and a folder named `2026-09-05` holding
//! audio from the evening of the fourth would be actively misleading when browsing the data directory.
//!
//! The day of a segment is decided by where it *starts*. A segment straddling midnight therefore belongs
//! to the day it began in. Nothing downstream cares, because playback is driven by timestamps rather than
//! by day boundaries, so audio still crosses midnight seamlessly.

use chrono::{DateTime, Local, NaiveDate, TimeZone};

/// Length of the `YYYY-MM-DD` form, used to validate input before it reaches a path or a query.
const DAY_LEN: usize = 10;

/// Local calendar day of an instant, as `YYYY-MM-DD`.
pub fn local_day(timestamp_ms: i64) -> String {
    local_date(timestamp_ms).format("%Y-%m-%d").to_string()
}

/// Local midnight to midnight bounds of a day, as `[start_ms, end_ms)`.
///
/// The end is derived by adding one calendar day rather than 24 hours, so the bounds stay correct across
/// a daylight saving transition where a local day is 23 or 25 hours long.
pub fn day_bounds_ms(day: &str) -> Option<(i64, i64)> {
    let date = parse_day(day)?;
    let next = date.succ_opt()?;
    Some((start_of_local_date_ms(date)?, start_of_local_date_ms(next)?))
}

/// Reject anything that is not a real `YYYY-MM-DD` date.
///
/// The day reaches us from a query string and is used to build a filesystem path, so it is validated
/// rather than trusted. A strict parse also rules out `..` and separators by construction.
pub fn parse_day(day: &str) -> Option<NaiveDate> {
    if day.len() != DAY_LEN {
        return None;
    }
    NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()
}

/// True when `day` is a well formed calendar day.
pub fn is_valid_day(day: &str) -> bool {
    parse_day(day).is_some()
}

fn local_date(timestamp_ms: i64) -> DateTime<Local> {
    match Local.timestamp_millis_opt(timestamp_ms) {
        chrono::LocalResult::Single(value) => value,
        // Ambiguous only at a DST fall back, where either answer is the same calendar day.
        chrono::LocalResult::Ambiguous(earliest, _) => earliest,
        // Out of the representable range. Falling back to the epoch keeps the recorder writing to a valid
        // path instead of failing on a nonsensical clock reading.
        chrono::LocalResult::None => Local.timestamp_millis_opt(0).earliest().unwrap_or_default(),
    }
}

fn start_of_local_date_ms(date: NaiveDate) -> Option<i64> {
    let midnight = date.and_hms_opt(0, 0, 0)?;
    // On a spring forward night local midnight can be skipped entirely in some zones, so take the first
    // instant that does exist on that date.
    midnight
        .and_local_timezone(Local)
        .earliest()
        .map(|value| value.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_round_trips_through_its_own_bounds() {
        let day = local_day(1_757_030_400_000);
        let (start_ms, end_ms) = day_bounds_ms(&day).expect("bounds");

        assert_eq!(local_day(start_ms), day);
        // The end is exclusive, so it is already the next day.
        assert_ne!(local_day(end_ms), day);
        assert_eq!(local_day(end_ms - 1), day);
    }

    #[test]
    fn bounds_span_a_whole_day() {
        let day = local_day(1_757_030_400_000);
        let (start_ms, end_ms) = day_bounds_ms(&day).expect("bounds");
        let hours = (end_ms - start_ms) / 3_600_000;

        // 24 normally, 23 or 25 on a daylight saving transition. Anything else is a bug.
        assert!(
            (23..=25).contains(&hours),
            "a local day lasted {hours} hours"
        );
    }

    #[test]
    fn every_instant_in_a_day_maps_to_the_same_day() {
        let day = local_day(1_757_030_400_000);
        let (start_ms, end_ms) = day_bounds_ms(&day).expect("bounds");

        for offset in [
            0,
            1_000,
            3_600_000,
            (end_ms - start_ms) / 2,
            end_ms - start_ms - 1,
        ] {
            assert_eq!(local_day(start_ms + offset), day);
        }
    }

    #[test]
    fn consecutive_days_are_contiguous() {
        let (_, first_end) = day_bounds_ms("2026-09-05").expect("bounds");
        let (second_start, _) = day_bounds_ms("2026-09-06").expect("bounds");
        assert_eq!(first_end, second_start);
    }

    #[test]
    fn the_day_format_is_sortable() {
        let day = local_day(1_757_030_400_000);
        assert_eq!(day.len(), DAY_LEN);
        assert!(day.chars().all(|item| item.is_ascii_digit() || item == '-'));
        // Lexicographic order matches chronological order, which is what lets SQLite order by the column.
        assert!("2026-09-05" < "2026-09-06");
        assert!("2026-09-09" < "2026-09-10");
    }

    #[test]
    fn rejects_anything_that_is_not_a_calendar_day() {
        assert!(is_valid_day("2026-09-05"));

        for invalid in [
            "",
            "2026-9-5",
            "2026-13-01",
            "2026-02-30",
            "not-a-date",
            "2026-09-05T10:00:00",
            "../../etc/passwd",
            "2026-09-05/..",
        ] {
            assert!(!is_valid_day(invalid), "{invalid} should be rejected");
            assert!(
                day_bounds_ms(invalid).is_none(),
                "{invalid} should have no bounds"
            );
        }
    }

    #[test]
    fn a_nonsensical_clock_still_yields_a_usable_path() {
        // The recorder must never fail to open a file because the system clock is absurd.
        assert!(is_valid_day(&local_day(i64::MAX)));
        assert!(is_valid_day(&local_day(i64::MIN)));
    }
}
