//! Timeline payloads.

use serde::{Deserialize, Serialize};

use crate::audio::activity::Sound;
use crate::models::{SoundSensitivity, TimeRange};
use crate::services::timeline_service::{PeaksView, SeekDirection, DEFAULT_BUCKETS};
use crate::services::{RecordingDay, TimelineRange};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageDto {
    pub start_ms: i64,
    pub end_ms: i64,
}

impl From<TimeRange> for CoverageDto {
    fn from(range: TimeRange) -> Self {
        Self {
            start_ms: range.start_ms,
            end_ms: range.end_ms,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineRangeResponse {
    pub earliest_ms: Option<i64>,
    pub latest_ms: Option<i64>,
    pub live_edge_ms: Option<i64>,
    pub server_time_ms: i64,
    pub coverage: Vec<CoverageDto>,
}

impl From<TimelineRange> for TimelineRangeResponse {
    fn from(range: TimelineRange) -> Self {
        Self {
            earliest_ms: range.earliest_ms,
            latest_ms: range.latest_ms,
            live_edge_ms: range.live_edge_ms,
            server_time_ms: crate::util::time::now_ms(),
            coverage: range.coverage.into_iter().map(Into::into).collect(),
        }
    }
}

/// Query string of the peaks endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeaksQuery {
    pub from_ms: i64,
    pub to_ms: i64,
    #[serde(default = "default_buckets")]
    pub buckets: usize,
}

fn default_buckets() -> usize {
    DEFAULT_BUCKETS
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeaksResponse {
    pub from_ms: i64,
    pub to_ms: i64,
    pub bucket_ms: i64,
    pub peaks: Vec<u8>,
}

impl From<PeaksView> for PeaksResponse {
    fn from(view: PeaksView) -> Self {
        Self {
            from_ms: view.from_ms,
            to_ms: view.to_ms,
            bucket_ms: view.bucket_ms,
            peaks: view.values,
        }
    }
}

/// One day that holds recordings, as offered by the day picker.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDayDto {
    /// Local calendar day, `YYYY-MM-DD`.
    pub day: String,
    /// First and last moment recorded on that day.
    pub start_ms: i64,
    pub end_ms: i64,
    /// Local midnight bounds, so the timeline can frame the whole day.
    pub day_start_ms: i64,
    pub day_end_ms: i64,
    pub segment_count: i64,
    pub bytes: i64,
    /// Audio actually captured, which is less than the span whenever the recorder was stopped part way.
    pub recorded_ms: i64,
}

impl From<RecordingDay> for RecordingDayDto {
    fn from(day: RecordingDay) -> Self {
        Self {
            day: day.summary.day,
            start_ms: day.summary.start_ms,
            end_ms: day.summary.end_ms,
            day_start_ms: day.day_start_ms,
            day_end_ms: day.day_end_ms,
            segment_count: day.summary.segment_count,
            bytes: day.summary.bytes,
            recorded_ms: day.summary.recorded_ms,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDaysResponse {
    pub days: Vec<RecordingDayDto>,
}

/// Query string of both export endpoints.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportQuery {
    pub from_ms: i64,
    pub to_ms: i64,
}

/// What an export would produce, so the UI can show it before committing to a download.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPlanResponse {
    pub from_ms: i64,
    pub to_ms: i64,
    pub duration_ms: i64,
    pub sample_rate: u32,
    pub channels: u16,
    pub total_bytes: i64,
    /// True when the range spans more than one recording rate and will be exported at the lowest.
    pub mixed_rates: bool,
}

impl From<crate::services::ExportPlan> for ExportPlanResponse {
    fn from(plan: crate::services::ExportPlan) -> Self {
        Self {
            from_ms: plan.range.start_ms,
            to_ms: plan.range.end_ms,
            duration_ms: plan.duration_ms(),
            sample_rate: plan.sample_rate,
            channels: plan.channels,
            total_bytes: plan.total_bytes as i64,
            mixed_rates: plan.mixed_rates,
        }
    }
}

/// Query string of the sounds endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundsQuery {
    pub from_ms: i64,
    pub to_ms: i64,
}

/// A moment something was heard.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundDto {
    pub start_ms: i64,
    pub end_ms: i64,
    /// Where to start playback to hear it from its beginning: a second early, but never inside a gap.
    pub seek_ms: i64,
    /// Its loudest envelope value, 0 to 255.
    pub peak: u8,
}

impl From<Sound> for SoundDto {
    fn from(sound: Sound) -> Self {
        Self {
            start_ms: sound.start_ms,
            end_ms: sound.end_ms,
            seek_ms: sound.seek_ms,
            peak: sound.peak,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundsResponse {
    pub from_ms: i64,
    pub to_ms: i64,
    /// The setting these were found with, so a client can tell a stale answer after it changes.
    pub sensitivity: SoundSensitivity,
    pub sounds: Vec<SoundDto>,
}

/// Which way the next sound endpoint looks. Forward unless told otherwise.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DirectionParam {
    #[default]
    Forward,
    Backward,
}

impl From<DirectionParam> for SeekDirection {
    fn from(direction: DirectionParam) -> Self {
        match direction {
            DirectionParam::Forward => SeekDirection::Forward,
            DirectionParam::Backward => SeekDirection::Backward,
        }
    }
}

/// Query string of the next sound endpoint: where playback is now, and which way to look.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextSoundQuery {
    pub from_ms: i64,
    #[serde(default)]
    pub direction: DirectionParam,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextSoundResponse {
    /// `null` when there is no sound that way in the recordings.
    pub sound: Option<SoundDto>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::DaySummary;
    use crate::services::ExportPlan;

    #[test]
    fn coverage_and_the_range_travel_with_camel_case_fields() {
        let response: TimelineRangeResponse = TimelineRange {
            earliest_ms: Some(1_000),
            latest_ms: Some(9_000),
            live_edge_ms: None,
            coverage: vec![TimeRange::new(1_000, 4_000), TimeRange::new(6_000, 9_000)],
        }
        .into();
        let json = serde_json::to_value(response).expect("serialise");
        assert_eq!(json["earliestMs"], 1_000);
        assert_eq!(json["latestMs"], 9_000);
        assert_eq!(json["liveEdgeMs"], serde_json::Value::Null);
        assert!(json["serverTimeMs"].as_i64().expect("server time") > 0);
        assert_eq!(
            json["coverage"],
            serde_json::json!([
                { "startMs": 1_000, "endMs": 4_000 },
                { "startMs": 6_000, "endMs": 9_000 },
            ])
        );
    }

    #[test]
    fn a_recording_day_carries_its_extent_its_midnights_and_what_it_holds() {
        let dto: RecordingDayDto = RecordingDay {
            summary: DaySummary {
                day: "2026-09-27".to_string(),
                start_ms: 100,
                end_ms: 900,
                segment_count: 4,
                bytes: 4_000,
                recorded_ms: 700,
            },
            day_start_ms: 0,
            day_end_ms: 86_400_000,
        }
        .into();
        let json = serde_json::to_value(dto).expect("serialise");
        assert_eq!(json["day"], "2026-09-27");
        assert_eq!(json["startMs"], 100);
        assert_eq!(json["endMs"], 900);
        assert_eq!(json["dayStartMs"], 0);
        assert_eq!(json["dayEndMs"], 86_400_000);
        assert_eq!(json["segmentCount"], 4);
        assert_eq!(json["bytes"], 4_000);
        assert_eq!(json["recordedMs"], 700);
    }

    #[test]
    fn peaks_keep_their_window_and_values() {
        let response: PeaksResponse = PeaksView {
            from_ms: 0,
            to_ms: 1_000,
            bucket_ms: 100,
            values: vec![0, 5, 255],
        }
        .into();
        let json = serde_json::to_value(response).expect("serialise");
        assert_eq!(json["fromMs"], 0);
        assert_eq!(json["toMs"], 1_000);
        assert_eq!(json["bucketMs"], 100);
        assert_eq!(json["peaks"], serde_json::json!([0, 5, 255]));
    }

    #[test]
    fn a_peaks_query_defaults_its_columns() {
        let query: PeaksQuery = serde_json::from_str(r#"{"fromMs":1,"toMs":2}"#).expect("parse");
        assert_eq!(query.buckets, DEFAULT_BUCKETS);
        let asked: PeaksQuery =
            serde_json::from_str(r#"{"fromMs":1,"toMs":2,"buckets":50}"#).expect("parse");
        assert_eq!(asked.buckets, 50);
    }

    #[test]
    fn a_sound_carries_where_it_is_where_to_play_it_from_and_how_loud_it_was() {
        let dto: SoundDto = Sound {
            start_ms: 2_000,
            end_ms: 3_000,
            peak: 90,
            seek_ms: 1_000,
        }
        .into();
        let json = serde_json::to_value(dto).expect("serialise");
        assert_eq!(
            json,
            serde_json::json!({ "startMs": 2_000, "endMs": 3_000, "seekMs": 1_000, "peak": 90 })
        );
    }

    #[test]
    fn the_next_sound_query_looks_forward_unless_told_backward_and_refuses_anything_else() {
        let forward: NextSoundQuery = serde_json::from_str(r#"{"fromMs":5}"#).expect("parse");
        assert_eq!(
            SeekDirection::from(forward.direction),
            SeekDirection::Forward
        );
        let backward: NextSoundQuery =
            serde_json::from_str(r#"{"fromMs":5,"direction":"backward"}"#).expect("parse");
        assert_eq!(
            SeekDirection::from(backward.direction),
            SeekDirection::Backward
        );
        assert!(
            serde_json::from_str::<NextSoundQuery>(r#"{"fromMs":5,"direction":"up"}"#).is_err()
        );
    }

    #[test]
    fn an_empty_next_sound_answer_is_null_rather_than_missing() {
        let json = serde_json::to_value(NextSoundResponse { sound: None }).expect("serialise");
        assert_eq!(json, serde_json::json!({ "sound": null }));
    }

    #[test]
    fn an_export_plan_says_what_the_file_will_hold() {
        let response: ExportPlanResponse = ExportPlan {
            range: TimeRange::new(1_000, 3_000),
            sample_rate: 16_000,
            channels: 1,
            data_bytes: 64_000,
            total_bytes: 64_044,
            mixed_rates: true,
        }
        .into();
        let json = serde_json::to_value(response).expect("serialise");
        assert_eq!(json["fromMs"], 1_000);
        assert_eq!(json["toMs"], 3_000);
        assert_eq!(json["durationMs"], 2_000);
        assert_eq!(json["sampleRate"], 16_000);
        assert_eq!(json["channels"], 1);
        assert_eq!(json["totalBytes"], 64_044);
        assert_eq!(json["mixedRates"], true);
    }
}
