//! What the Prometheus endpoint reports.
//!
//! Gathered once per scrape into [`MetricsSnapshot`], then turned into metric families by a pure function,
//! so every rule about what is reported (and what is left out) is tested without a running service. The
//! spelling of the format is `models::metrics`' business, not this file's.
//!
//! **Metric names are a public contract.** People build dashboards and alerts on them, so renaming one
//! breaks somebody's monitoring silently. `every_metric_name_and_type_is_pinned` lists them all; change it
//! only on purpose, and say so in the changelog.

use serde::Serialize;

use crate::app::AppState;
use crate::error::AppResult;
use crate::models::metrics::MetricFamily;
use crate::models::update::Channel;
use crate::models::{
    CaptureSnapshot, CaptureState, LevelSnapshot, ListenerActivity, ListenerEntry,
};
use crate::repositories::segment_repository::SegmentStorageStats;
use crate::services::recorder_service::RecorderReport;

/// Open streams by what they are doing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListenerCounts {
    pub live: usize,
    pub playback: usize,
    pub paused: usize,
}

impl ListenerCounts {
    pub fn tally(entries: &[ListenerEntry]) -> Self {
        let mut counts = Self::default();
        for entry in entries {
            match entry.activity {
                ListenerActivity::Live => counts.live += 1,
                ListenerActivity::Playback { .. } => counts.playback += 1,
                ListenerActivity::Paused => counts.paused += 1,
            }
        }
        counts
    }
}

/// Everything one scrape reports, read from the running service in one go.
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub version: &'static str,
    pub channel: Channel,
    pub started_at_ms: i64,
    pub capture: CaptureSnapshot,
    pub recorder: RecorderReport,
    pub levels: LevelSnapshot,
    pub listeners: ListenerCounts,
    pub storage: SegmentStorageStats,
    /// `None` when recordings are kept forever.
    pub retention_hours: Option<u32>,
}

/// `GET` and `DELETE /api/metrics/token`: whether a scrape token exists. The token itself is never in it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsTokenResponse {
    /// When the current token was made, `null` when there is none.
    pub created_at_ms: Option<i64>,
}

/// `POST /api/metrics/token`: the new token, the only time it is ever sent.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewMetricsTokenResponse {
    pub token: String,
    pub created_at_ms: i64,
}

impl MetricsSnapshot {
    /// Read every figure from the service that owns it. The only query is the one aggregate row the
    /// storage card also reads, so a scrape every few seconds costs nothing worth measuring.
    pub fn from_state(state: &AppState) -> AppResult<Self> {
        Ok(Self {
            version: env!("CARGO_PKG_VERSION"),
            channel: state.updates.status().channel,
            started_at_ms: state.started_at_ms,
            capture: state.capture.snapshot(),
            recorder: state.capture.recorder_report(),
            levels: state.hub.levels(),
            listeners: ListenerCounts::tally(&state.listeners.snapshot()),
            storage: state.segments.stats()?,
            retention_hours: state.settings.current().retention_hours,
        })
    }

    /// Every family in a fixed order. A reading that does not exist right now, such as a level while
    /// nothing is captured or the oldest recording before anything is recorded, is left without a series
    /// rather than reported as zero, which an alert could not tell from a real zero.
    pub fn families(&self) -> Vec<MetricFamily> {
        let capture = &self.capture;
        let running = capture.state == CaptureState::Recording;
        let active = capture.state.is_active();

        let state = [
            (CaptureState::Idle, "idle"),
            (CaptureState::Starting, "starting"),
            (CaptureState::Recording, "recording"),
            (CaptureState::Error, "error"),
        ]
        .iter()
        .fold(
            MetricFamily::gauge(
                "oar_capture_state",
                "Where the recorder is, one series per state with the current one at 1.",
            ),
            |family, (state, name)| {
                family.labelled(&[("state", name)], flag(capture.state == *state))
            },
        );

        vec![
            MetricFamily::gauge(
                "oar_build_info",
                "The running version and the release channel it follows, always 1.",
            )
            .labelled(
                &[("version", self.version), ("channel", channel_name(self.channel))],
                1.0,
            ),
            MetricFamily::gauge(
                "oar_start_time_seconds",
                "When the service started, in seconds since the Unix epoch.",
            )
            .value(seconds(self.started_at_ms)),
            state,
            when(
                MetricFamily::gauge(
                    "oar_capture_sample_rate_hertz",
                    "The sample rate being recorded, while capture runs.",
                ),
                active && capture.sample_rate > 0,
                f64::from(capture.sample_rate),
            ),
            when(
                MetricFamily::gauge(
                    "oar_capture_device_sample_rate_hertz",
                    "The sample rate the microphone runs at, while capture runs.",
                ),
                active && capture.device_sample_rate > 0,
                f64::from(capture.device_sample_rate),
            ),
            MetricFamily::counter(
                "oar_capture_dropped_frames_total",
                "Frames the microphone delivered that the recorder had no room for, since the service started.",
            )
            .value(capture.dropped_frames as f64),
            MetricFamily::counter(
                "oar_recorder_frames_written_total",
                "Frames written to segment files since capture last started.",
            )
            .value(self.recorder.frames_written as f64),
            MetricFamily::counter(
                "oar_recorder_frames_not_written_total",
                "Frames broadcast live that never reached a segment file since capture last started.",
            )
            .value(self.recorder.frames_not_written as f64),
            when(
                MetricFamily::gauge(
                    "oar_recorder_disk_healthy",
                    "1 while recording to disk works, 0 while it fails and only the live feed continues.",
                ),
                running,
                flag(!self.recorder.problem),
            ),
            when(
                MetricFamily::gauge(
                    "oar_input_level_rms",
                    "Recent loudness of the input as RMS, 0 to 1, while capture runs.",
                ),
                running,
                f64::from(self.levels.rms),
            ),
            when(
                MetricFamily::gauge(
                    "oar_input_level_peak",
                    "Recent peak of the input, 0 to 1, while capture runs.",
                ),
                running,
                f64::from(self.levels.peak),
            ),
            MetricFamily::gauge(
                "oar_listeners",
                "Open audio streams by what they are doing.",
            )
            .labelled(&[("activity", "live")], self.listeners.live as f64)
            .labelled(&[("activity", "playback")], self.listeners.playback as f64)
            .labelled(&[("activity", "paused")], self.listeners.paused as f64),
            MetricFamily::gauge(
                "oar_recordings_bytes",
                "Bytes of recorded audio kept on disk, counting closed segments.",
            )
            .value(self.storage.bytes as f64),
            MetricFamily::gauge(
                "oar_recordings_segments",
                "Closed segment files kept on disk.",
            )
            .value(self.storage.segment_count as f64),
            optional(
                MetricFamily::gauge(
                    "oar_recordings_oldest_timestamp_seconds",
                    "Start of the oldest recording kept, in seconds since the Unix epoch.",
                ),
                self.storage.oldest_ms.map(seconds),
            ),
            optional(
                MetricFamily::gauge(
                    "oar_recordings_newest_timestamp_seconds",
                    "End of the newest closed segment, in seconds since the Unix epoch.",
                ),
                self.storage.newest_ms.map(seconds),
            ),
            optional(
                MetricFamily::gauge(
                    "oar_retention_seconds",
                    "How long recordings are kept, absent when they are kept forever.",
                ),
                self.retention_hours.map(|hours| f64::from(hours) * 3_600.0),
            ),
        ]
    }
}

fn channel_name(channel: Channel) -> &'static str {
    match channel {
        Channel::Stable => "stable",
        Channel::Beta => "beta",
    }
}

fn flag(on: bool) -> f64 {
    if on {
        1.0
    } else {
        0.0
    }
}

fn seconds(ms: i64) -> f64 {
    ms as f64 / 1_000.0
}

fn when(family: MetricFamily, present: bool, value: f64) -> MetricFamily {
    optional(family, present.then_some(value))
}

fn optional(family: MetricFamily, value: Option<f64>) -> MetricFamily {
    match value {
        Some(value) => family.value(value),
        None => family,
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;
    use crate::models::metrics::{render, MetricKind};
    use crate::models::PlayerState;

    fn idle() -> MetricsSnapshot {
        MetricsSnapshot {
            version: "0.9.0-beta.1",
            channel: Channel::Beta,
            started_at_ms: 1_790_000_000_000,
            capture: CaptureSnapshot::default(),
            recorder: RecorderReport::default(),
            levels: LevelSnapshot::default(),
            listeners: ListenerCounts::default(),
            storage: SegmentStorageStats::default(),
            retention_hours: Some(24),
        }
    }

    fn recording() -> MetricsSnapshot {
        MetricsSnapshot {
            capture: CaptureSnapshot {
                state: CaptureState::Recording,
                sample_rate: 16_000,
                device_sample_rate: 48_000,
                channels: 1,
                frame_ms: 100,
                dropped_frames: 2,
                ..CaptureSnapshot::default()
            },
            recorder: RecorderReport {
                frames_written: 600,
                frames_not_written: 5,
                problem: false,
            },
            levels: LevelSnapshot {
                rms: 0.125,
                peak: 0.5,
            },
            ..idle()
        }
    }

    fn body(snapshot: &MetricsSnapshot) -> String {
        render(&snapshot.families())
    }

    /// The value of one series, found by its whole name and label text as it is written.
    fn value_of(body: &str, series: &str) -> Option<String> {
        body.lines()
            .find_map(|line| line.strip_prefix(series)?.strip_prefix(' '))
            .map(str::to_string)
    }

    #[test]
    fn every_metric_name_and_type_is_pinned() {
        let snapshot = MetricsSnapshot {
            storage: SegmentStorageStats {
                segment_count: 1,
                bytes: 1,
                oldest_ms: Some(1),
                newest_ms: Some(2),
            },
            ..recording()
        };
        let pinned: Vec<(&str, MetricKind)> = snapshot
            .families()
            .iter()
            .map(|family| (family.name, family.kind))
            .collect();
        assert_eq!(
            pinned,
            vec![
                ("oar_build_info", MetricKind::Gauge),
                ("oar_start_time_seconds", MetricKind::Gauge),
                ("oar_capture_state", MetricKind::Gauge),
                ("oar_capture_sample_rate_hertz", MetricKind::Gauge),
                ("oar_capture_device_sample_rate_hertz", MetricKind::Gauge),
                ("oar_capture_dropped_frames_total", MetricKind::Counter),
                ("oar_recorder_frames_written_total", MetricKind::Counter),
                ("oar_recorder_frames_not_written_total", MetricKind::Counter),
                ("oar_recorder_disk_healthy", MetricKind::Gauge),
                ("oar_input_level_rms", MetricKind::Gauge),
                ("oar_input_level_peak", MetricKind::Gauge),
                ("oar_listeners", MetricKind::Gauge),
                ("oar_recordings_bytes", MetricKind::Gauge),
                ("oar_recordings_segments", MetricKind::Gauge),
                ("oar_recordings_oldest_timestamp_seconds", MetricKind::Gauge),
                ("oar_recordings_newest_timestamp_seconds", MetricKind::Gauge),
                ("oar_retention_seconds", MetricKind::Gauge),
            ]
        );
    }

    /// Prometheus' own rules for names: counters end in `_total`, units are spelled out as a suffix, and
    /// everything carries the service's prefix so it cannot collide with another exporter's.
    #[test]
    fn names_follow_the_prometheus_conventions() {
        for family in recording().families() {
            assert!(family.name.starts_with("oar_"), "{}", family.name);
            assert!(
                family
                    .name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "{}",
                family.name
            );
            assert_eq!(
                family.kind == MetricKind::Counter,
                family.name.ends_with("_total"),
                "{}",
                family.name
            );
            assert!(family.help.ends_with('.'), "{}", family.help);
            assert!(family.help.is_ascii(), "{}", family.help);
        }
    }

    #[test]
    fn build_info_names_the_version_and_channel() {
        assert_eq!(
            value_of(
                &body(&idle()),
                r#"oar_build_info{version="0.9.0-beta.1",channel="beta"}"#
            ),
            Some("1".to_string())
        );
        let stable = MetricsSnapshot {
            version: "0.9.0",
            channel: Channel::Stable,
            ..idle()
        };
        assert_eq!(
            value_of(
                &body(&stable),
                r#"oar_build_info{version="0.9.0",channel="stable"}"#
            ),
            Some("1".to_string())
        );
    }

    #[test]
    fn the_start_time_is_in_seconds() {
        assert_eq!(
            value_of(&body(&idle()), "oar_start_time_seconds"),
            Some("1790000000".to_string())
        );
    }

    /// One series per state, exactly one of them 1, so `oar_capture_state{state="recording"} == 0` is an
    /// alert that also fires when the recorder is in an error state, and a series never disappears when the
    /// state changes.
    #[test]
    fn the_capture_state_is_one_hot_for_every_state() {
        let states = [
            (CaptureState::Idle, "idle"),
            (CaptureState::Starting, "starting"),
            (CaptureState::Recording, "recording"),
            (CaptureState::Error, "error"),
        ];
        for (state, name) in states {
            let snapshot = MetricsSnapshot {
                capture: CaptureSnapshot {
                    state,
                    ..CaptureSnapshot::default()
                },
                ..idle()
            };
            let text = body(&snapshot);
            for (_, other) in states {
                let expected = if other == name { "1" } else { "0" };
                assert_eq!(
                    value_of(&text, &format!(r#"oar_capture_state{{state="{other}"}}"#)),
                    Some(expected.to_string()),
                    "in {name}, {other}"
                );
            }
        }
    }

    #[test]
    fn a_running_capture_reports_its_rates_and_counters() {
        let text = body(&recording());
        assert_eq!(
            value_of(&text, "oar_capture_sample_rate_hertz"),
            Some("16000".to_string())
        );
        assert_eq!(
            value_of(&text, "oar_capture_device_sample_rate_hertz"),
            Some("48000".to_string())
        );
        assert_eq!(
            value_of(&text, "oar_capture_dropped_frames_total"),
            Some("2".to_string())
        );
        assert_eq!(
            value_of(&text, "oar_recorder_frames_written_total"),
            Some("600".to_string())
        );
        assert_eq!(
            value_of(&text, "oar_recorder_frames_not_written_total"),
            Some("5".to_string())
        );
        assert_eq!(
            value_of(&text, "oar_input_level_rms"),
            Some("0.125".to_string())
        );
        assert_eq!(
            value_of(&text, "oar_input_level_peak"),
            Some("0.5".to_string())
        );
    }

    /// Stopped, there is no rate and no level to report: a zero would read as a broken microphone, and a
    /// level left over from the last run as a working one.
    #[test]
    fn a_stopped_capture_reports_no_rate_and_no_level() {
        let text = body(&idle());
        for name in [
            "oar_capture_sample_rate_hertz",
            "oar_capture_device_sample_rate_hertz",
            "oar_input_level_rms",
            "oar_input_level_peak",
            "oar_recorder_disk_healthy",
        ] {
            assert!(!text.contains(name), "{name} in\n{text}");
        }
        let stale = MetricsSnapshot {
            levels: LevelSnapshot {
                rms: 0.3,
                peak: 0.9,
            },
            ..idle()
        };
        assert!(!body(&stale).contains("oar_input_level"));
    }

    /// Counters are reported whatever the state, so a scrape between runs still sees the last run's totals
    /// rather than the series vanishing.
    #[test]
    fn counters_are_reported_while_stopped_too() {
        let stopped = MetricsSnapshot {
            capture: CaptureSnapshot {
                dropped_frames: 4,
                ..CaptureSnapshot::default()
            },
            recorder: RecorderReport {
                frames_written: 10,
                frames_not_written: 1,
                problem: false,
            },
            ..idle()
        };
        let text = body(&stopped);
        assert_eq!(
            value_of(&text, "oar_capture_dropped_frames_total"),
            Some("4".to_string())
        );
        assert_eq!(
            value_of(&text, "oar_recorder_frames_written_total"),
            Some("10".to_string())
        );
        assert_eq!(
            value_of(&text, "oar_recorder_frames_not_written_total"),
            Some("1".to_string())
        );
    }

    #[test]
    fn disk_health_follows_the_recorders_problem_while_recording() {
        assert_eq!(
            value_of(&body(&recording()), "oar_recorder_disk_healthy"),
            Some("1".to_string())
        );
        let failing = MetricsSnapshot {
            recorder: RecorderReport {
                problem: true,
                ..recording().recorder
            },
            ..recording()
        };
        assert_eq!(
            value_of(&body(&failing), "oar_recorder_disk_healthy"),
            Some("0".to_string())
        );
    }

    #[test]
    fn listeners_are_counted_by_what_they_are_doing() {
        let snapshot = MetricsSnapshot {
            listeners: ListenerCounts {
                live: 3,
                playback: 1,
                paused: 0,
            },
            ..idle()
        };
        let text = body(&snapshot);
        assert_eq!(
            value_of(&text, r#"oar_listeners{activity="live"}"#),
            Some("3".to_string())
        );
        assert_eq!(
            value_of(&text, r#"oar_listeners{activity="playback"}"#),
            Some("1".to_string())
        );
        assert_eq!(
            value_of(&text, r#"oar_listeners{activity="paused"}"#),
            Some("0".to_string())
        );
    }

    fn entry(id: u64, activity: ListenerActivity) -> ListenerEntry {
        ListenerEntry {
            id,
            account: None,
            address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            user_agent: None,
            connected_at_ms: 0,
            activity,
            player: PlayerState::Playing,
        }
    }

    #[test]
    fn a_tally_counts_each_open_stream_once() {
        let counts = ListenerCounts::tally(&[
            entry(1, ListenerActivity::Live),
            entry(2, ListenerActivity::Playback { from_ms: 5 }),
            entry(3, ListenerActivity::Live),
            entry(4, ListenerActivity::Paused),
        ]);
        assert_eq!(
            counts,
            ListenerCounts {
                live: 2,
                playback: 1,
                paused: 1,
            }
        );
        assert_eq!(ListenerCounts::tally(&[]), ListenerCounts::default());
    }

    #[test]
    fn history_reports_its_size_and_both_ends_in_seconds() {
        let snapshot = MetricsSnapshot {
            storage: SegmentStorageStats {
                segment_count: 360,
                bytes: 115_200_000,
                oldest_ms: Some(1_789_999_000_500),
                newest_ms: Some(1_790_000_000_000),
            },
            ..idle()
        };
        let text = body(&snapshot);
        assert_eq!(
            value_of(&text, "oar_recordings_bytes"),
            Some("115200000".to_string())
        );
        assert_eq!(
            value_of(&text, "oar_recordings_segments"),
            Some("360".to_string())
        );
        assert_eq!(
            value_of(&text, "oar_recordings_oldest_timestamp_seconds"),
            Some("1789999000.5".to_string())
        );
        assert_eq!(
            value_of(&text, "oar_recordings_newest_timestamp_seconds"),
            Some("1790000000".to_string())
        );
    }

    /// Nothing recorded yet has no oldest or newest moment. Reporting zero would read as 1970, and an alert
    /// on "no new audio for five minutes" would fire at once on a fresh install for the wrong reason.
    #[test]
    fn an_empty_history_reports_no_ends() {
        let text = body(&idle());
        assert_eq!(
            value_of(&text, "oar_recordings_bytes"),
            Some("0".to_string())
        );
        assert_eq!(
            value_of(&text, "oar_recordings_segments"),
            Some("0".to_string())
        );
        assert!(!text.contains("oar_recordings_oldest_timestamp_seconds"));
        assert!(!text.contains("oar_recordings_newest_timestamp_seconds"));
    }

    #[test]
    fn retention_is_in_seconds_and_absent_when_kept_forever() {
        assert_eq!(
            value_of(&body(&idle()), "oar_retention_seconds"),
            Some("86400".to_string())
        );
        let forever = MetricsSnapshot {
            retention_hours: None,
            ..idle()
        };
        assert!(!body(&forever).contains("oar_retention_seconds"));
    }
}
