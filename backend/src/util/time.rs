//! Wall clock helpers.
//!
//! The whole DVR is addressed by milliseconds since the Unix epoch, so this is the single place that reads
//! the system clock. Centralising it keeps the timestamp semantics identical in the recorder, the segment
//! index, and the streaming layer.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch.
///
/// A clock set before 1970 is not a case worth handling, so a reversed duration collapses to zero rather
/// than panicking inside an audio callback.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// Format a millisecond duration as `HH:MM:SS`, used in log lines.
pub fn format_duration_ms(duration_ms: i64) -> String {
    let total_seconds = duration_ms.max(0) / 1000;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_duration_with_hours() {
        assert_eq!(format_duration_ms(3_723_000), "01:02:03");
    }

    #[test]
    fn clamps_negative_duration() {
        assert_eq!(format_duration_ms(-5), "00:00:00");
    }

    #[test]
    fn now_is_after_2020() {
        assert!(now_ms() > 1_577_836_800_000);
    }
}
