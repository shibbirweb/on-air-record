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
    let (start_ms, end_ms) = (start_of_local_date_ms(date)?, start_of_local_date_ms(next)?);
    // A day the local calendar skipped, as Samoa skipped 30 December 2011 crossing the date line, holds
    // no instant at all, so it has no bounds rather than an empty window.
    (start_ms < end_ms).then_some((start_ms, end_ms))
}

/// Reject anything that is not a real `YYYY-MM-DD` date.
///
/// The day reaches us from a query string and is used to build a filesystem path, so it is validated
/// rather than trusted. A strict parse also rules out `..` and separators by construction.
pub fn parse_day(day: &str) -> Option<NaiveDate> {
    // The shape is checked by hand first because chrono's parser is lenient: it skips spaces and takes a
    // sign, so ` +00-1- 01` is ten characters and parses as year zero. Only digits and the two dashes, in
    // their places, can name the directory a day's audio is in.
    let shaped = day.len() == DAY_LEN
        && day.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 => byte == b'-',
            _ => byte.is_ascii_digit(),
        });
    if !shaped {
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

/// How far local midnight, read as if it were UTC, can sit from the instant it really is: no zone has been
/// more than 14 hours from UTC, and a day the local calendar skipped outright adds 24 more.
const MIDNIGHT_SEARCH_MS: i64 = 40 * 3_600_000;

/// The first instant whose local day is `date`, found on the instant to day direction that `local_day`
/// uses, so the bounds of a day and the day of an instant cannot disagree.
///
/// Asking chrono for local midnight directly is wrong where the clocks change at midnight, which Cuba,
/// the Azores, Chile and others do: on those nights it can report midnight as existing an hour away from
/// where the calendar actually turns, or in the wrong order when midnight happens twice, and the day before
/// then claims an hour of this one or loses one. Since 1900 no zone's local day has run backwards as time
/// goes forwards, so the first instant of a day is a plain binary search, about thirty lookups.
fn start_of_local_date_ms(date: NaiveDate) -> Option<i64> {
    let naive_midnight_ms = date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis();
    let before = |instant_ms: i64| local_date(instant_ms).date_naive() < date;

    let mut low = naive_midnight_ms.checked_sub(MIDNIGHT_SEARCH_MS)?;
    let mut high = naive_midnight_ms.checked_add(MIDNIGHT_SEARCH_MS)?;
    if !before(low) || before(high) {
        return None;
    }
    // `low` is always before the day and `high` never is, so when they meet `high` is its first instant.
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        if before(middle) {
            low = middle;
        } else {
            high = middle;
        }
    }
    Some(high)
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
            // Found by `any_text_is_a_canonical_day_or_refused`: chrono alone reads this as 0000-01-01.
            " +00-1- 01",
            "2026-09- 5",
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

    /// These run in the host's own timezone, because `Local` is what production uses and changing `TZ`
    /// inside a multi threaded test binary would race every other test that reads a day. CI runners are
    /// on UTC, so the daylight saving cases are exercised on a developer machine in a zone that has them.
    mod props {
        use super::*;
        use proptest::prelude::*;

        /// From 1900 to 2200: well past any clock a recorder will read, and across every daylight saving
        /// rule change the timezone database knows of for the host's zone.
        const FROM_MS: i64 = -2_208_988_800_000;
        const TO_MS: i64 = 7_258_118_400_000;

        proptest! {
            #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

            /// An instant lies inside the bounds of the day it is filed under, and those bounds name the
            /// same day back. This is what keeps a segment's directory, its `day` column and the day the
            /// UI jumps to in agreement: if the bounds of a day missed one of its own instants, audio
            /// recorded then would be on disk under a day whose window does not show it.
            #[test]
            fn an_instant_lies_inside_its_own_day(timestamp_ms in FROM_MS..TO_MS) {
                let day = local_day(timestamp_ms);
                prop_assert!(is_valid_day(&day));
                let (start_ms, end_ms) = day_bounds_ms(&day).expect("bounds of a real day");

                prop_assert!(start_ms <= timestamp_ms && timestamp_ms < end_ms, "{} not in {} {}..{}", timestamp_ms, day, start_ms, end_ms);
                prop_assert_eq!(local_day(start_ms), day.clone());
                prop_assert_eq!(local_day(end_ms - 1), day.clone());
                prop_assert_ne!(local_day(end_ms), day);
            }

            /// Consecutive days tile the timeline with no hole and no overlap, and each is a plausible
            /// length. A hole would be an hour of audio no day claims; an overlap, an hour shown twice.
            /// 22 to 26 hours rather than 23 to 25 because a handful of zones have shifted by more than an
            /// hour in one night when they changed their standard offset. A day the calendar skipped
            /// crossing the date line (Kiritimati's 31 December 1994) has no bounds, and the tiling
            /// continues with the day after it.
            #[test]
            fn consecutive_days_tile_the_timeline(timestamp_ms in FROM_MS..TO_MS) {
                let date = parse_day(&local_day(timestamp_ms)).expect("a day");
                let (start_ms, end_ms) = day_bounds_ms(&date.format("%Y-%m-%d").to_string()).expect("bounds");
                let next_start_ms = date
                    .iter_days()
                    .skip(1)
                    .take(2)
                    .find_map(|next| day_bounds_ms(&next.format("%Y-%m-%d").to_string()))
                    .map(|(next_start_ms, _)| next_start_ms)
                    .expect("one of the next two days exists");

                prop_assert_eq!(end_ms, next_start_ms);
                let hours = (end_ms - start_ms) as f64 / 3_600_000.0;
                prop_assert!((22.0..=26.0).contains(&hours), "{} lasted {} hours", date, hours);
            }

            /// Days sort as text in the same order as the instants they hold, which is what lets SQLite
            /// order and range over the `day` column without parsing it.
            #[test]
            fn day_names_sort_like_the_instants(first_ms in FROM_MS..TO_MS, second_ms in FROM_MS..TO_MS) {
                let (earlier, later) = (first_ms.min(second_ms), first_ms.max(second_ms));
                prop_assert!(local_day(earlier) <= local_day(later));
            }

            /// A day reaches the service from a query string and becomes part of a path, so any text at all
            /// must be either refused or a date whose canonical spelling is exactly what was sent. A parse
            /// that accepted `2026-9-05` or a trailing separator could name a directory that is not there.
            #[test]
            fn any_text_is_a_canonical_day_or_refused(text in prop_oneof![
                ".{0,12}",
                "[0-9+ -]{10}",
                (0i32..10_000, 0u32..14, 0u32..33).prop_map(|(y, m, d)| format!("{y:04}-{m:02}-{d:02}")),
            ]) {
                match parse_day(&text) {
                    Some(date) => {
                        prop_assert_eq!(date.format("%Y-%m-%d").to_string(), text.clone());
                        prop_assert!(day_bounds_ms(&text).is_some());
                    }
                    None => prop_assert!(day_bounds_ms(&text).is_none()),
                }
            }
        }
    }
}
