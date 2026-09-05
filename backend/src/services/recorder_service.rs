//! The always on recorder.
//!
//! Runs on its own OS thread rather than on the tokio runtime, because every step it takes is blocking
//! work: a channel receive, a file write, and a SQLite insert. Keeping it off the runtime means a slow
//! disk delays recording and nothing else, and it lets the audio callback hand frames over through a
//! plain channel with no async machinery in the hot path.
//!
//! The thread is also the single publisher into [`BroadcastHub`]. Live listeners therefore hear exactly
//! what is being written to disk, in the same order, which is what makes the handoff from playback back
//! to live seamless.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError};

use crate::audio::{FrameEncoder, SegmentWriter};
use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::models::AudioFrame;
use crate::repositories::SegmentRepository;
use crate::services::{BroadcastHub, SettingsService};

/// How long the loop waits for a frame before re checking the stop flag.
const RECEIVE_TIMEOUT: Duration = Duration::from_millis(200);

/// Everything the recorder thread needs, gathered into one struct so the spawn signature stays readable.
pub struct RecorderContext {
    pub config: Arc<AppConfig>,
    pub segments: Arc<SegmentRepository>,
    pub settings: Arc<SettingsService>,
    pub hub: Arc<BroadcastHub>,
    pub encoder: Arc<dyn FrameEncoder>,
    pub session_id: i64,
}

/// Live handle on the recorder thread.
pub struct RecorderHandle {
    stop: Arc<AtomicBool>,
    segments_written: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl RecorderHandle {
    pub fn segments_written(&self) -> u64 {
        self.segments_written.load(Ordering::Relaxed)
    }

    /// Ask the recorder to finish the open segment and stop, then wait for it.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                tracing::error!("recorder thread panicked while shutting down");
            }
        }
    }
}

impl Drop for RecorderHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub struct RecorderService;

impl RecorderService {
    pub fn spawn(
        context: RecorderContext,
        receiver: Receiver<AudioFrame>,
    ) -> AppResult<RecorderHandle> {
        let stop = Arc::new(AtomicBool::new(false));
        let segments_written = Arc::new(AtomicU64::new(0));

        let thread_stop = stop.clone();
        let thread_counter = segments_written.clone();

        let thread = std::thread::Builder::new()
            .name("oar-recorder".to_string())
            .spawn(move || {
                run(context, receiver, thread_stop, thread_counter);
            })
            .map_err(|error| {
                AppError::internal(format!("could not start recorder thread: {error}"))
            })?;

        Ok(RecorderHandle {
            stop,
            segments_written,
            thread: Some(thread),
        })
    }
}

fn run(
    context: RecorderContext,
    receiver: Receiver<AudioFrame>,
    stop: Arc<AtomicBool>,
    segments_written: Arc<AtomicU64>,
) {
    let mut writer: Option<SegmentWriter> = None;
    let mut sequence: i64 = 0;

    tracing::info!(session_id = context.session_id, "recorder started");

    loop {
        match receiver.recv_timeout(RECEIVE_TIMEOUT) {
            Ok(frame) => {
                // Listeners first: a disk hiccup should not add latency to the live broadcast.
                context.hub.publish(frame.clone());

                let segment_ms = context.settings.current().segment_seconds as i64 * 1000;
                if should_roll_over(writer.as_ref(), &frame, segment_ms) {
                    close_segment(&context, writer.take(), &segments_written);
                }

                if writer.is_none() {
                    match open_segment(&context, sequence, &frame) {
                        Ok(opened) => {
                            writer = Some(opened);
                            sequence += 1;
                        }
                        Err(error) => {
                            // Losing the disk should not stop the broadcast, so keep publishing and retry
                            // on the next frame rather than tearing the recorder down.
                            tracing::error!(%error, "could not open a segment file");
                            continue;
                        }
                    }
                }

                if let Some(active) = writer.as_mut() {
                    if let Err(error) = active.append(&frame, context.encoder.as_ref()) {
                        tracing::error!(%error, "could not append to the segment file");
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if stop.load(Ordering::Acquire) {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                // The capture stream was dropped, which is the normal way a session ends.
                break;
            }
        }
    }

    close_segment(&context, writer.take(), &segments_written);
    context.hub.reset_levels();
    tracing::info!(
        session_id = context.session_id,
        segments = segments_written.load(Ordering::Relaxed),
        "recorder stopped"
    );
}

/// A segment rolls over when it is long enough, or when the incoming frame does not continue it.
///
/// The discontinuity check matters after a device glitch or a clock re anchor: splicing a frame with a
/// distant timestamp into the current file would make its byte offsets lie, and every later seek inside
/// that segment would land in the wrong place.
fn should_roll_over(writer: Option<&SegmentWriter>, frame: &AudioFrame, segment_ms: i64) -> bool {
    let Some(writer) = writer else {
        return false;
    };

    if writer.duration_ms() >= segment_ms {
        return true;
    }

    let expected_ms = writer.started_at_ms() + writer.duration_ms();
    (frame.timestamp_ms - expected_ms).abs() > discontinuity_tolerance_ms(frame)
}

/// How far a frame may sit from where it was expected before it counts as a new recording.
///
/// One frame of slack absorbs the integer rounding in the timestamp arithmetic without treating ordinary
/// jitter as a break in the recording.
fn discontinuity_tolerance_ms(frame: &AudioFrame) -> i64 {
    frame.duration_ms().max(20)
}

fn open_segment(
    context: &RecorderContext,
    sequence: i64,
    first_frame: &AudioFrame,
) -> AppResult<SegmentWriter> {
    let relative_path = SegmentWriter::relative_path_for(context.session_id, sequence);
    let absolute_path = context.config.resolve_data_path(&relative_path);
    SegmentWriter::create(
        context.session_id,
        sequence,
        absolute_path,
        relative_path,
        first_frame,
    )
}

fn close_segment(
    context: &RecorderContext,
    writer: Option<SegmentWriter>,
    segments_written: &Arc<AtomicU64>,
) {
    let Some(writer) = writer else {
        return;
    };

    match writer.finish() {
        Ok(Some(draft)) => match context.segments.insert(&draft) {
            Ok(segment_id) => {
                segments_written.fetch_add(1, Ordering::Relaxed);
                tracing::debug!(
                    segment_id,
                    sequence = draft.sequence,
                    duration_ms = draft.ended_at_ms - draft.started_at_ms,
                    bytes = draft.byte_len,
                    "segment indexed"
                );
            }
            Err(error) => {
                // The audio is on disk but unreachable through the index. Say so loudly, because it is
                // the one failure that silently shrinks the DVR window.
                tracing::error!(%error, path = draft.path, "could not index a written segment");
            }
        },
        Ok(None) => {}
        Err(error) => tracing::error!(%error, "could not close a segment file"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_at(timestamp_ms: i64) -> AudioFrame {
        AudioFrame::from_samples(timestamp_ms, 48_000, 1, vec![0; 4800], true)
    }

    fn writer_for(started_at_ms: i64, frames: usize) -> SegmentWriter {
        let dir = std::env::temp_dir().join(format!(
            "oar-recorder-{}-{}",
            std::process::id(),
            started_at_ms
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");

        let first = frame_at(started_at_ms);
        let mut writer = SegmentWriter::create(
            1,
            0,
            dir.join("000000.pcm"),
            "recordings/1/000000.pcm".to_string(),
            &first,
        )
        .expect("create");

        for index in 0..frames {
            let frame = frame_at(started_at_ms + index as i64 * 100);
            writer
                .append(&frame, &crate::audio::PcmS16Encoder)
                .expect("append");
        }

        writer
    }

    #[test]
    fn no_writer_means_no_rollover() {
        assert!(!should_roll_over(None, &frame_at(0), 10_000));
    }

    #[test]
    fn rolls_over_once_the_segment_is_long_enough() {
        let writer = writer_for(1_000, 20);
        assert_eq!(writer.duration_ms(), 2_000);

        assert!(!should_roll_over(Some(&writer), &frame_at(3_000), 10_000));
        assert!(should_roll_over(Some(&writer), &frame_at(3_000), 2_000));
    }

    #[test]
    fn rolls_over_when_the_timeline_jumps() {
        let writer = writer_for(1_000, 10);
        let expected_next_ms = 1_000 + writer.duration_ms();

        assert!(!should_roll_over(
            Some(&writer),
            &frame_at(expected_next_ms),
            60_000
        ));
        assert!(should_roll_over(
            Some(&writer),
            &frame_at(expected_next_ms + 5_000),
            60_000
        ));
    }

    #[test]
    fn small_jitter_does_not_split_a_segment() {
        let writer = writer_for(1_000, 10);
        let expected_next_ms = 1_000 + writer.duration_ms();
        assert!(!should_roll_over(
            Some(&writer),
            &frame_at(expected_next_ms + 10),
            60_000
        ));
    }
}
