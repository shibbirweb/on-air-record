//! The always on recorder.
//!
//! Two OS threads rather than tokio tasks, because every step is blocking work: a channel receive, a
//! file write, a SQLite insert. That also lets the audio callback hand frames over through a plain
//! channel with no async machinery in the hot path.
//!
//! The recorder thread is the single publisher into [`BroadcastHub`], so live listeners hear frames in
//! capture order, the same order they are written to disk. It publishes and hands each frame to the disk
//! thread without waiting on it. The disk thread owns the segment files and the index. Once they were one
//! thread, and a database another program held locked, or a disk that hung, paused live audio for every
//! listener at each segment close: the broadcast must never wait on storage. The queue between them holds
//! [`DISK_QUEUE`] of audio, so a hiccup shorter than that costs nothing at all; a longer one drops frames
//! from the recording only, counted and shown in the capture status, and the gap in their timestamps
//! makes the disk thread start a new segment rather than splice across the hole.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender, TrySendError};

use crate::audio::{FrameEncoder, SegmentLayout, SegmentLocation, SegmentWriter};
use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::models::{AudioFrame, SegmentDraft};
use crate::repositories::SegmentRepository;
use crate::services::{BroadcastHub, SettingsService};

/// How long the loop waits for a frame before re checking the stop flag.
const RECEIVE_TIMEOUT: Duration = Duration::from_millis(200);

/// Audio the disk thread may fall behind by before frames are dropped from the recording: long enough to
/// ride out a backup holding the database or a network share catching its breath, and at 48 kHz mono a
/// few megabytes of memory.
const DISK_QUEUE: Duration = Duration::from_secs(60);

/// Finished segments the index refused, held to try again at the next close.
///
/// An hour at the default segment length. Past that the oldest is given up on, because memory must not
/// grow without bound while the database stays unavailable; its file stays on disk and the log says so.
const MAX_WAITING_SEGMENTS: usize = 360;

/// Everything the recorder thread needs, gathered into one struct so the spawn signature stays readable.
pub struct RecorderContext {
    pub config: Arc<AppConfig>,
    pub segments: Arc<SegmentRepository>,
    pub settings: Arc<SettingsService>,
    pub hub: Arc<BroadcastHub>,
    pub encoder: Arc<dyn FrameEncoder>,
    pub session_id: i64,
    /// Fixed for the whole session. Changing the directory mid session would scatter one recording
    /// across two roots, so a new location takes effect on the next capture start.
    pub layout: SegmentLayout,
    /// Where the thread reports trouble with the disk or the index, for the capture status to show.
    pub health: Arc<RecorderHealth>,
}

/// How recording to disk is going, shared between the recorder thread and the status the UI polls.
///
/// The recorder keeps the live feed going whatever the disk does, which is right, but it means nothing
/// else notices when recording stops working: without this, a recorder that has not written a byte for
/// hours still reports `recording` with no error, and the operator finds out when they go looking for the
/// audio. Writing, indexing and the disk queue overflowing are tracked apart because they recover apart: a
/// successful write must not clear an index that is still refusing rows, nor frames still being left out
/// because the queue is full, or the error would flicker on and off while the problem went on.
#[derive(Debug, Default)]
pub struct RecorderHealth {
    frames_written: AtomicU64,
    frames_not_written: AtomicU64,
    writing: Mutex<Option<String>>,
    indexing: Mutex<Option<String>>,
    /// Owned by the recorder thread: set when the disk queue is full, cleared only when frames fit again.
    overflow: Mutex<Option<String>>,
}

impl RecorderHealth {
    /// What is wrong right now, worded for the recorder panel, or `None` while all is well.
    pub fn problem(&self) -> Option<String> {
        let current = read_slot(&self.overflow)
            .or_else(|| read_slot(&self.writing))
            .or_else(|| read_slot(&self.indexing))?;
        Some(format!(
            "Recording to disk is failing, live audio continues: {current}"
        ))
    }

    /// Captured frames appended to a segment file.
    pub fn frames_written(&self) -> u64 {
        self.frames_written.load(Ordering::Relaxed)
    }

    /// Captured frames that were broadcast but never reached a segment file.
    pub fn frames_not_written(&self) -> u64 {
        self.frames_not_written.load(Ordering::Relaxed)
    }

    /// Forget the previous session's trouble. Called when capture starts.
    pub fn reset(&self) {
        self.frames_written.store(0, Ordering::Relaxed);
        self.frames_not_written.store(0, Ordering::Relaxed);
        replace_slot(&self.writing, None);
        replace_slot(&self.indexing, None);
        replace_slot(&self.overflow, None);
    }

    fn frame_written(&self) {
        self.frames_written.fetch_add(1, Ordering::Relaxed);
    }

    fn frame_not_written(&self) {
        self.frames_not_written.fetch_add(1, Ordering::Relaxed);
    }
}

fn read_slot(slot: &Mutex<Option<String>>) -> Option<String> {
    // A poisoned lock only means a panic elsewhere mid assignment of a string; the value is still usable,
    // and the recorder thread must never panic over its own bookkeeping.
    slot.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Swap in a new value, returning what was there.
fn replace_slot(slot: &Mutex<Option<String>>, value: Option<String>) -> Option<String> {
    std::mem::replace(
        &mut *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
        value,
    )
}

/// Record a failure in `slot`. Only the first of a run is logged as an error: a disk that stays broken
/// fails on every frame, ten times a second, and an error line each time would fill the log, and on a
/// shared disk the very space the recordings need, within days of an unattended failure.
fn report_failure(slot: &Mutex<Option<String>>, message: String) {
    if replace_slot(slot, Some(message.clone())).is_none() {
        tracing::error!(
            problem = message,
            "recording to disk is failing, live audio continues"
        );
    } else {
        tracing::debug!(problem = message, "recording to disk is still failing");
    }
}

/// Clear `slot` after a success, saying so once if it had been failing.
fn report_recovery(slot: &Mutex<Option<String>>, what: &str) {
    if let Some(previous) = replace_slot(slot, None) {
        tracing::info!(previous_problem = previous, "{what} is working again");
    }
}

/// Live handle on the recorder and disk threads.
pub struct RecorderHandle {
    stop: Arc<AtomicBool>,
    segments_written: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
    disk: Option<JoinHandle<()>>,
}

impl RecorderHandle {
    pub fn segments_written(&self) -> u64 {
        self.segments_written.load(Ordering::Relaxed)
    }

    /// Ask the recorder to finish the open segment and stop, then wait for it.
    pub fn stop(mut self) {
        self.shutdown();
    }

    /// The recorder thread first; when it ends it drops its side of the disk queue, and the disk thread
    /// writes what is still queued, closes and indexes the open segment, then ends too. That is what the
    /// stop scripts wait for, so a stop never loses the last seconds.
    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                tracing::error!("recorder thread panicked while shutting down");
            }
        }
        if let Some(disk) = self.disk.take() {
            if disk.join().is_err() {
                tracing::error!("recorder disk thread panicked while shutting down");
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
        let context = Arc::new(context);

        let frame_ms = u128::from(context.settings.current().frame_ms.max(1));
        let queue_frames = (DISK_QUEUE.as_millis() / frame_ms).max(16) as usize;
        let (to_disk, from_recorder) = bounded(queue_frames);

        let disk_context = context.clone();
        let disk_counter = segments_written.clone();
        let disk = std::thread::Builder::new()
            .name("oar-recorder-disk".to_string())
            .spawn(move || {
                write_to_disk(&disk_context, from_recorder, &disk_counter);
            })
            .map_err(|error| {
                AppError::internal(format!("could not start the recorder disk thread: {error}"))
            })?;

        let thread_stop = stop.clone();
        let thread = std::thread::Builder::new()
            .name("oar-recorder".to_string())
            .spawn(move || {
                run(&context, receiver, to_disk, &thread_stop);
            })
            .map_err(|error| {
                AppError::internal(format!("could not start recorder thread: {error}"))
            })?;

        Ok(RecorderHandle {
            stop,
            segments_written,
            thread: Some(thread),
            disk: Some(disk),
        })
    }
}

/// The recorder thread: publish every frame, then hand it to the disk thread without waiting.
fn run(
    context: &RecorderContext,
    receiver: Receiver<AudioFrame>,
    to_disk: Sender<AudioFrame>,
    stop: &AtomicBool,
) {
    tracing::info!(session_id = context.session_id, "recorder started");
    let mut behind = false;

    loop {
        match receiver.recv_timeout(RECEIVE_TIMEOUT) {
            Ok(frame) => {
                // Listeners first, and nothing below may wait: this thread must keep pace with capture.
                context.hub.publish(frame.clone());

                match to_disk.try_send(frame) {
                    Ok(()) => {
                        if behind {
                            behind = false;
                            report_recovery(
                                &context.health.overflow,
                                "keeping up with the recording",
                            );
                        }
                    }
                    Err(TrySendError::Full(_)) => {
                        // Storage has been stuck for the whole queue. This frame goes to listeners but
                        // not the recording; its missing timestamp makes the disk thread start a new
                        // segment when frames flow again, so no file holds a hole.
                        context.health.frame_not_written();
                        if !behind {
                            behind = true;
                            report_failure(
                                &context.health.overflow,
                                "storage has not kept up for a minute, so frames are being left out of the recording".to_string(),
                            );
                        }
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        // The disk thread is gone, which only a panic there can cause. Keep the broadcast.
                        context.health.frame_not_written();
                        report_failure(
                            &context.health.writing,
                            "the recorder's disk thread stopped".to_string(),
                        );
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

    context.hub.reset_levels();
    tracing::info!(session_id = context.session_id, "recorder stopped");
}

/// The disk thread: segment files and the index, at whatever pace storage allows, until the recorder
/// thread hangs up; then whatever is still queued, and the open segment closed and indexed.
fn write_to_disk(
    context: &RecorderContext,
    from_recorder: Receiver<AudioFrame>,
    segments_written: &Arc<AtomicU64>,
) {
    let mut writer: Option<SegmentWriter> = None;
    let mut sequence: i64 = 0;
    let mut waiting: VecDeque<SegmentDraft> = VecDeque::new();

    for frame in from_recorder.iter() {
        let segment_ms = context.settings.current().segment_seconds as i64 * 1000;
        if should_roll_over(writer.as_ref(), &frame, segment_ms) {
            close_segment(context, writer.take(), segments_written, &mut waiting);
        }

        if writer.is_none() {
            match open_segment(context, sequence, &frame) {
                Ok(opened) => {
                    writer = Some(opened);
                    sequence += 1;
                }
                Err(error) => {
                    // Losing the disk must not stop anything else, so retry on the next frame rather than
                    // tearing the recorder down.
                    context.health.frame_not_written();
                    report_failure(
                        &context.health.writing,
                        format!("could not open a segment file: {error}"),
                    );
                    continue;
                }
            }
        }

        if let Some(active) = writer.as_mut() {
            match active.append(&frame, context.encoder.as_ref()) {
                Ok(()) => {
                    context.health.frame_written();
                    report_recovery(&context.health.writing, "writing segments");
                }
                Err(error) => {
                    context.health.frame_not_written();
                    report_failure(
                        &context.health.writing,
                        format!("could not write to the segment file: {error}"),
                    );
                    // A segment must never hold a hole. Seeking is arithmetic on the byte offset, so
                    // every frame appended after a lost one would play one frame early. Close the segment
                    // here; the next frame starts a new one at its own timestamp.
                    close_segment(context, writer.take(), segments_written, &mut waiting);
                }
            }
        }
    }

    close_segment(context, writer.take(), segments_written, &mut waiting);
    if !waiting.is_empty() {
        tracing::error!(
            segments = waiting.len(),
            "finished segments could not be indexed before the recorder stopped, their files remain on disk"
        );
    }
    tracing::info!(
        session_id = context.session_id,
        segments = segments_written.load(Ordering::Relaxed),
        "recorder disk thread stopped"
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
    let location = SegmentLocation::for_segment(
        &context.layout,
        context.session_id,
        sequence,
        first_frame.timestamp_ms,
    );
    SegmentWriter::create(location, first_frame)
}

/// Finish the open segment, if any, and index it along with anything still waiting from before.
fn close_segment(
    context: &RecorderContext,
    writer: Option<SegmentWriter>,
    segments_written: &Arc<AtomicU64>,
    waiting: &mut VecDeque<SegmentDraft>,
) {
    if let Some(writer) = writer {
        match writer.finish() {
            Ok(Some(draft)) => {
                if waiting.len() >= MAX_WAITING_SEGMENTS {
                    if let Some(abandoned) = waiting.pop_front() {
                        tracing::error!(
                            path = abandoned.path,
                            "the index has refused segments for too long, giving up on the oldest, its file remains on disk"
                        );
                    }
                }
                waiting.push_back(draft);
            }
            Ok(None) => {}
            Err(error) => report_failure(
                &context.health.writing,
                format!("could not close a segment file: {error}"),
            ),
        }
    }

    index_waiting(context, waiting, segments_written);
}

/// Index the waiting segments in the order they were recorded.
///
/// A refused insert leaves audio on disk that the DVR cannot reach, and a backup or a sqlite3 shell
/// holding the write lock for a few seconds is enough to cause one, so the row waits and goes in at the
/// next close rather than being dropped. The first refusal that may pass stops the round, so a database
/// that is still locked costs one busy timeout per close, not one per waiting segment. A refusal that can
/// never pass, a broken uniqueness rule, drops that one row, or it would hold up every segment behind it.
fn index_waiting(
    context: &RecorderContext,
    waiting: &mut VecDeque<SegmentDraft>,
    segments_written: &Arc<AtomicU64>,
) {
    while let Some(draft) = waiting.front() {
        match context.segments.insert(draft) {
            Ok(segment_id) => {
                segments_written.fetch_add(1, Ordering::Relaxed);
                tracing::debug!(
                    segment_id,
                    sequence = draft.sequence,
                    duration_ms = draft.ended_at_ms - draft.started_at_ms,
                    bytes = draft.byte_len,
                    "segment indexed"
                );
                waiting.pop_front();
            }
            Err(error) if will_never_be_accepted(&error) => {
                tracing::error!(%error, path = draft.path, "the index will never accept this segment, its file remains on disk");
                waiting.pop_front();
            }
            Err(error) => {
                // The audio is on disk but unreachable through the index until this clears. Say so,
                // because it is the one failure that silently shrinks the DVR window.
                report_failure(
                    &context.health.indexing,
                    format!("could not index a finished segment: {error}"),
                );
                return;
            }
        }
    }

    report_recovery(&context.health.indexing, "indexing segments");
}

/// True for an insert that fails on the row itself, which no amount of waiting fixes.
fn will_never_be_accepted(error: &AppError) -> bool {
    matches!(
        error,
        AppError::Database(rusqlite::Error::SqliteFailure(failure, _))
            if failure.code == rusqlite::ErrorCode::ConstraintViolation
    )
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
        let location =
            SegmentLocation::for_segment(&SegmentLayout::at(dir.clone()), 1, 0, first.timestamp_ms);
        let mut writer = SegmentWriter::create(location, &first).expect("create");

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
