//! Exporting a span of the recording as a WAV file.
//!
//! Two properties make this simpler than it looks. Segments are already the sample format a canonical WAV
//! carries, so nothing is transcoded; and uncompressed audio has an exactly predictable size, so the
//! whole file length is known before a byte is written, which is what the RIFF header demands.
//!
//! Gaps are filled with silence rather than skipped. The export is a record of a span of time, so a file
//! whose duration matched the requested range is worth far more than a shorter one: thirty seconds into
//! the file is thirty seconds after the start of the range, whatever the recorder was doing.

use std::sync::Arc;

use crate::audio::resampler::Resampler;
use crate::audio::wav;
use crate::error::{AppError, AppResult};
use crate::models::TimeRange;
use crate::repositories::SegmentRepository;
use crate::services::{CursorOutput, PlaybackService};

/// Largest export offered.
///
/// Well under the four gigabyte ceiling the RIFF format imposes, and chosen so a download stays something
/// a browser and a filesystem handle comfortably. A longer span is a job for copying the data directory.
pub const MAX_EXPORT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Frame size the export reads in. Larger than the streaming frame because nothing is being paced here,
/// and fewer, bigger reads move the data faster.
const EXPORT_FRAME_MS: u32 = 1_000;

/// Everything decided before the first byte is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportPlan {
    pub range: TimeRange,
    /// Rate of the exported file, which is the lowest rate any segment in the range was recorded at.
    pub sample_rate: u32,
    pub channels: u16,
    /// Audio bytes, excluding the header.
    pub data_bytes: u64,
    /// Header plus audio, which is the `Content-Length` of the download.
    pub total_bytes: u64,
    /// True when some segment in the range needs downsampling to reach `sample_rate`.
    pub mixed_rates: bool,
}

impl ExportPlan {
    pub fn duration_ms(&self) -> i64 {
        self.range.duration_ms()
    }
}

pub struct ExportService {
    segments: Arc<SegmentRepository>,
    playback: Arc<PlaybackService>,
}

impl ExportService {
    pub fn new(segments: Arc<SegmentRepository>, playback: Arc<PlaybackService>) -> Self {
        Self { segments, playback }
    }

    /// Work out what an export of `range` would produce, or why it cannot be done.
    pub fn plan(&self, range: TimeRange) -> AppResult<ExportPlan> {
        if range.duration_ms() <= 0 {
            return Err(AppError::bad_request("toMs must be greater than fromMs"));
        }

        let segments = self.segments.find_in_range(range)?;
        if segments.is_empty() {
            return Err(AppError::not_found(
                "there is no recording in that range to export",
            ));
        }

        // Every segment has to agree on channels: the resampler works on mono, and interleaving two
        // different channel counts into one file would produce something no player could open.
        let channels = segments[0].channels.max(1);
        if segments.iter().any(|item| item.channels.max(1) != channels) {
            return Err(AppError::bad_request(
                "that range mixes mono and multi channel audio, export a narrower range",
            ));
        }

        // The lowest rate present, so mixed rates are reconciled by downsampling. Upsampling the quieter
        // material instead would invent detail it never had and make the file larger for nothing.
        let sample_rate = segments
            .iter()
            .map(|item| item.sample_rate)
            .min()
            .unwrap_or(0);
        if sample_rate == 0 {
            return Err(AppError::internal("segments have no usable sample rate"));
        }

        let mixed_rates = segments.iter().any(|item| item.sample_rate != sample_rate);
        if mixed_rates && channels != 1 {
            return Err(AppError::bad_request(
                "that range mixes sample rates on multi channel audio, export a narrower range",
            ));
        }

        let data_bytes = wav::data_bytes_for(sample_rate, channels, range.duration_ms())
            .ok_or_else(|| {
                AppError::bad_request("that range is too long to fit in a WAV file, export less")
            })?;

        if data_bytes > MAX_EXPORT_BYTES {
            return Err(AppError::bad_request(format!(
                "that export would be {} GB, which is over the {} GB limit, export a shorter range",
                data_bytes / 1_000_000_000,
                MAX_EXPORT_BYTES / 1_000_000_000
            )));
        }

        Ok(ExportPlan {
            range,
            sample_rate,
            channels,
            data_bytes,
            total_bytes: data_bytes + wav::HEADER_BYTES as u64,
            mixed_rates,
        })
    }

    /// Produce the file, handing each chunk to `emit` as it is built.
    ///
    /// Blocking throughout, since every step reads a file. The caller runs it off the async runtime.
    /// Stops early, without error, when `emit` returns false, which is how a cancelled download unwinds.
    pub fn write(&self, plan: &ExportPlan, mut emit: impl FnMut(Vec<u8>) -> bool) -> AppResult<()> {
        if !emit(wav::header(plan.sample_rate, plan.channels, plan.data_bytes as u32).to_vec()) {
            return Ok(());
        }

        let bytes_per_sample_frame = 2 * plan.channels.max(1) as u64;
        let mut written: u64 = 0;
        let mut cursor = self
            .playback
            .cursor_at(plan.range.start_ms, EXPORT_FRAME_MS);
        let mut resampler: Option<(u32, Resampler)> = None;
        let mut scratch: Vec<f32> = Vec::new();

        while written < plan.data_bytes {
            let output = cursor.advance()?;

            let chunk = match output {
                CursorOutput::Frame(frame) => {
                    if frame.timestamp_ms >= plan.range.end_ms {
                        break;
                    }
                    self.encode_frame(&frame, plan, &mut resampler, &mut scratch)
                }
                CursorOutput::Gap { from_ms, to_ms } => {
                    // Silence for exactly as long as the recorder was not running, so the file's clock
                    // keeps matching the timeline's.
                    let silent_ms = (to_ms.min(plan.range.end_ms) - from_ms).max(0);
                    silence(plan.sample_rate, bytes_per_sample_frame, silent_ms)
                }
                CursorOutput::EndOfRecording { at_ms } => {
                    // Nothing further exists, so pad to the end of the requested range and finish.
                    let silent_ms = (plan.range.end_ms - at_ms).max(0);
                    let tail = silence(plan.sample_rate, bytes_per_sample_frame, silent_ms);
                    emit_truncated(&mut emit, tail, plan.data_bytes, &mut written);
                    break;
                }
            };

            if !emit_truncated(&mut emit, chunk, plan.data_bytes, &mut written) {
                return Ok(());
            }
        }

        // The header promised an exact length, so make good on it whatever the recording did.
        if written < plan.data_bytes {
            let missing = (plan.data_bytes - written) as usize;
            emit(vec![0u8; missing]);
        }

        Ok(())
    }

    /// Turn one frame into output bytes, downsampling if it was recorded at a higher rate.
    fn encode_frame(
        &self,
        frame: &crate::models::AudioFrame,
        plan: &ExportPlan,
        resampler: &mut Option<(u32, Resampler)>,
        scratch: &mut Vec<f32>,
    ) -> Vec<u8> {
        if frame.sample_rate == plan.sample_rate {
            return frame.to_le_bytes();
        }

        // A rate change means a new filter: its state belongs to the stream it was tracking.
        if resampler.as_ref().map(|(rate, _)| *rate) != Some(frame.sample_rate) {
            *resampler = Resampler::new(frame.sample_rate, plan.sample_rate)
                .map(|built| (frame.sample_rate, built));
        }

        let Some((_, active)) = resampler.as_mut() else {
            // Only reachable if the target is not actually lower, in which case pass the audio through.
            return frame.to_le_bytes();
        };

        scratch.clear();
        let input: Vec<f32> = frame
            .samples
            .iter()
            .map(|sample| *sample as f32 / i16::MAX as f32)
            .collect();
        active.process(&input, scratch);

        let mut out = Vec::with_capacity(scratch.len() * 2);
        for sample in scratch.iter() {
            let clamped = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            out.extend_from_slice(&clamped.to_le_bytes());
        }
        out
    }
}

/// Emit a chunk, trimming it so the promised length is never exceeded. Returns false to stop.
fn emit_truncated(
    emit: &mut impl FnMut(Vec<u8>) -> bool,
    mut chunk: Vec<u8>,
    limit: u64,
    written: &mut u64,
) -> bool {
    if chunk.is_empty() {
        return true;
    }

    let remaining = limit.saturating_sub(*written);
    if remaining == 0 {
        return false;
    }
    if chunk.len() as u64 > remaining {
        chunk.truncate(remaining as usize);
    }

    *written += chunk.len() as u64;
    emit(chunk)
}

fn silence(sample_rate: u32, bytes_per_sample_frame: u64, duration_ms: i64) -> Vec<u8> {
    if duration_ms <= 0 {
        return Vec::new();
    }

    let samples = (duration_ms as u64 * sample_rate as u64) / 1000;
    vec![0u8; (samples * bytes_per_sample_frame) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{PcmS16Encoder, SegmentLayout, SegmentLocation, SegmentWriter};
    use crate::config::AppConfig;
    use crate::db::Database;
    use crate::models::{AudioFrame, SessionDraft};
    use crate::repositories::SessionRepository;
    use std::path::PathBuf;

    struct Fixture {
        service: ExportService,
        data_dir: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.data_dir).ok();
        }
    }

    /// Build a real data directory holding segments at the given ranges and rates.
    fn fixture(name: &str, blocks: &[(i64, i64, u32)]) -> Fixture {
        let data_dir =
            std::env::temp_dir().join(format!("oar-export-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&data_dir).ok();
        std::fs::create_dir_all(&data_dir).expect("data dir");

        let config = Arc::new(AppConfig {
            data_dir: data_dir.clone(),
            ..AppConfig::default()
        });

        let database = Arc::new(Database::open_in_memory().expect("database"));
        let sessions = SessionRepository::new(database.clone());
        let segments = Arc::new(SegmentRepository::new(database));
        let layout = SegmentLayout::under_data_dir(&data_dir);

        for (index, (start_ms, end_ms, rate)) in blocks.iter().enumerate() {
            let session = sessions
                .create(&SessionDraft {
                    device_id: "mic".to_string(),
                    device_name: "mic".to_string(),
                    sample_rate: *rate,
                    channels: 1,
                    started_at_ms: *start_ms,
                })
                .expect("session");

            let samples_per_frame = (*rate / 10) as usize;
            let first =
                AudioFrame::from_samples(*start_ms, *rate, 1, vec![0; samples_per_frame], true);
            let location =
                SegmentLocation::for_segment(&layout, session.id, index as i64, *start_ms);
            let mut writer = SegmentWriter::create(location, &first).expect("writer");

            let frames = ((end_ms - start_ms) / 100) as usize;
            for frame_index in 0..frames {
                let frame = AudioFrame::from_samples(
                    start_ms + frame_index as i64 * 100,
                    *rate,
                    1,
                    vec![1_000; samples_per_frame],
                    true,
                );
                writer.append(&frame, &PcmS16Encoder).expect("append");
            }

            let draft = writer.finish().expect("finish").expect("indexed");
            segments.insert(&draft).expect("insert");
        }

        let playback = Arc::new(PlaybackService::new(config, segments.clone()));

        Fixture {
            service: ExportService::new(segments, playback),
            data_dir,
        }
    }

    /// Collect a whole export into one buffer.
    fn render(fixture: &Fixture, plan: &ExportPlan) -> Vec<u8> {
        let mut out = Vec::new();
        fixture
            .service
            .write(plan, |chunk| {
                out.extend_from_slice(&chunk);
                true
            })
            .expect("write");
        out
    }

    #[test]
    fn plans_a_single_rate_range() {
        let fixture = fixture("plain", &[(0, 10_000, 48_000)]);
        let plan = fixture
            .service
            .plan(TimeRange::new(0, 10_000))
            .expect("plan");

        assert_eq!(plan.sample_rate, 48_000);
        assert_eq!(plan.channels, 1);
        assert!(!plan.mixed_rates);
        // Ten seconds of 48 kHz mono.
        assert_eq!(plan.data_bytes, 48_000 * 2 * 10);
        assert_eq!(plan.total_bytes, plan.data_bytes + 44);
    }

    #[test]
    fn the_file_is_exactly_the_promised_length() {
        let fixture = fixture("length", &[(0, 10_000, 48_000)]);
        let plan = fixture
            .service
            .plan(TimeRange::new(0, 5_000))
            .expect("plan");
        let file = render(&fixture, &plan);

        assert_eq!(file.len() as u64, plan.total_bytes);
        // And the header agrees with what actually followed.
        let declared = u32::from_le_bytes(file[40..44].try_into().expect("four bytes"));
        assert_eq!(declared as usize, file.len() - 44);
    }

    #[test]
    fn a_gap_becomes_silence_so_the_clock_still_lines_up() {
        // Two seconds recorded, four seconds of nothing, two seconds recorded.
        let fixture = fixture("gap", &[(0, 2_000, 48_000), (6_000, 8_000, 48_000)]);
        let plan = fixture
            .service
            .plan(TimeRange::new(0, 8_000))
            .expect("plan");
        let file = render(&fixture, &plan);

        assert_eq!(file.len() as u64, plan.total_bytes);

        // The middle of the gap must be silent, and the audio after it must not be.
        let sample_at = |second: f64| {
            let offset = 44 + (second * 48_000.0) as usize * 2;
            i16::from_le_bytes(file[offset..offset + 2].try_into().expect("two bytes"))
        };
        assert_ne!(sample_at(1.0), 0, "recorded audio should not be silent");
        assert_eq!(sample_at(4.0), 0, "the gap should be silent");
        assert_ne!(sample_at(7.0), 0, "audio after the gap should return");
    }

    #[test]
    fn a_range_running_past_the_recording_is_padded() {
        let fixture = fixture("tail", &[(0, 2_000, 48_000)]);
        let plan = fixture
            .service
            .plan(TimeRange::new(0, 6_000))
            .expect("plan");
        let file = render(&fixture, &plan);

        assert_eq!(file.len() as u64, plan.total_bytes);
        let last = i16::from_le_bytes(file[file.len() - 2..].try_into().expect("two bytes"));
        assert_eq!(last, 0);
    }

    #[test]
    fn mixed_rates_export_at_the_lowest_one() {
        let fixture = fixture("mixed", &[(0, 4_000, 48_000), (4_000, 8_000, 16_000)]);
        let plan = fixture
            .service
            .plan(TimeRange::new(0, 8_000))
            .expect("plan");

        assert!(plan.mixed_rates);
        assert_eq!(plan.sample_rate, 16_000);

        let file = render(&fixture, &plan);
        assert_eq!(file.len() as u64, plan.total_bytes);
        assert_eq!(
            u32::from_le_bytes(file[24..28].try_into().expect("four bytes")),
            16_000
        );
    }

    #[test]
    fn an_empty_or_backwards_range_is_refused() {
        let fixture = fixture("empty", &[(0, 4_000, 48_000)]);
        assert!(fixture.service.plan(TimeRange::new(1_000, 1_000)).is_err());
        assert!(fixture.service.plan(TimeRange::new(4_000, 1_000)).is_err());
    }

    #[test]
    fn a_range_with_no_recording_is_not_found() {
        let fixture = fixture("nothing", &[(0, 4_000, 48_000)]);
        let outcome = fixture.service.plan(TimeRange::new(900_000, 960_000));
        assert!(matches!(outcome, Err(AppError::NotFound(_))));
    }

    #[test]
    fn an_oversized_range_is_refused_before_anything_is_written() {
        let fixture = fixture("huge", &[(0, 4_000, 48_000)]);
        // Ten hours of 48 kHz mono is comfortably past the limit.
        let outcome = fixture
            .service
            .plan(TimeRange::new(0, 10 * 60 * 60 * 1_000));
        assert!(matches!(outcome, Err(AppError::BadRequest(_))));
    }

    #[test]
    fn a_cancelled_download_stops_without_error() {
        let fixture = fixture("cancel", &[(0, 10_000, 48_000)]);
        let plan = fixture
            .service
            .plan(TimeRange::new(0, 10_000))
            .expect("plan");

        let mut chunks = 0;
        fixture
            .service
            .write(&plan, |_chunk| {
                chunks += 1;
                // Refuse everything after the header, as a client hanging up would.
                chunks < 2
            })
            .expect("write");

        assert_eq!(chunks, 2);
    }
}
