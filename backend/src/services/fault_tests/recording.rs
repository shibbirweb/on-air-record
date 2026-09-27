//! The recorder meeting a disk or a database that fails under it.
//!
//! The recorder is the single publisher into the broadcast hub, so the rule that matters most is that a
//! failing disk costs the recording and never the live feed. Beyond that: nothing panics on the recorder
//! thread, the failure shows in the capture status rather than only in a log, and recording picks up
//! again by itself once the fault clears. Each test drives the real recorder thread with synthetic frames
//! through its real channel; no microphone is opened.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{header, Request};
use crossbeam_channel::{bounded, Sender};
use tower::ServiceExt;

use super::{frame_at, wait_until, Store, FRAME_MS, T0};
use crate::app::AppState;
use crate::audio::{build_encoder, FrameFormat, SegmentLayout};
use crate::config::AppConfig;
use crate::models::{AudioFrame, SegmentDraft, SessionDraft};
use crate::repositories::SegmentRepository;
use crate::services::recorder_service::{RecorderContext, RecorderHealth, RecorderService};
use crate::services::{BroadcastHub, RecorderHandle, SettingsService};

/// The pieces a recorder thread is wired to, whichever way the test built them.
struct Wiring {
    config: Arc<AppConfig>,
    segments: Arc<SegmentRepository>,
    settings: Arc<SettingsService>,
    hub: Arc<BroadcastHub>,
    session_id: i64,
    health: Arc<RecorderHealth>,
}

impl Wiring {
    fn from_store(store: &Store) -> Self {
        Self {
            config: store.config.clone(),
            segments: store.segments.clone(),
            settings: store.settings.clone(),
            hub: store.hub.clone(),
            session_id: store.session_id,
            health: Arc::new(RecorderHealth::default()),
        }
    }
}

/// A running recorder thread fed through its channel, the way the capture callback feeds it.
struct Recorder {
    handle: Option<RecorderHandle>,
    sender: Option<Sender<AudioFrame>>,
    hub: Arc<BroadcastHub>,
    health: Arc<RecorderHealth>,
    next_ms: i64,
    sent: u64,
}

impl Recorder {
    fn start(wiring: &Wiring) -> Self {
        let (sender, receiver) = bounded(256);
        let handle = RecorderService::spawn(
            RecorderContext {
                config: wiring.config.clone(),
                segments: wiring.segments.clone(),
                settings: wiring.settings.clone(),
                hub: wiring.hub.clone(),
                encoder: build_encoder(FrameFormat::PcmS16),
                session_id: wiring.session_id,
                layout: SegmentLayout::under_data_dir(&wiring.config.data_dir),
                health: wiring.health.clone(),
            },
            receiver,
        )
        .expect("recorder");
        Self {
            handle: Some(handle),
            sender: Some(sender),
            hub: wiring.hub.clone(),
            health: wiring.health.clone(),
            next_ms: T0,
            sent: 0,
        }
    }

    /// Capture `milliseconds` more audio and wait until the recorder has handled all of it, so a fault
    /// staged next lands between two known frames rather than somewhere in the channel.
    fn capture(&mut self, milliseconds: i64) {
        let sender = self.sender.as_ref().expect("capturing");
        for _ in 0..(milliseconds / FRAME_MS) {
            sender
                .send(frame_at(self.next_ms, level_at(self.next_ms)))
                .expect("the recorder is still taking frames");
            self.next_ms += FRAME_MS;
            self.sent += 1;
        }
        // Published first, written second: waiting on the broadcast alone would let the test stage its
        // next fault while the last frame is still on its way to the disk.
        let health = self.health.clone();
        let sent = self.sent;
        wait_until("the recorder has handled every frame", move || {
            health.frames_written() + health.frames_not_written() == sent
        });
    }

    /// End the session the way stopping capture does, which closes and indexes the open segment.
    fn stop(&mut self) {
        drop(self.sender.take());
        if let Some(handle) = self.handle.take() {
            handle.stop();
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A different level for every frame, so where a sample was played back from says when it was captured.
fn level_at(timestamp_ms: i64) -> i16 {
    1 + ((timestamp_ms - T0) / FRAME_MS % 30_000) as i16
}

/// Every indexed segment, oldest first, as `(start, end)` offsets from `T0`.
fn indexed(segments: &SegmentRepository) -> Vec<(i64, i64)> {
    segments
        .find_in_range(crate::models::TimeRange::new(i64::MIN / 2, i64::MAX / 2))
        .expect("segments")
        .iter()
        .map(|segment| (segment.started_at_ms - T0, segment.ended_at_ms - T0))
        .collect()
}

#[cfg(unix)]
#[tokio::test]
async fn a_read_only_recordings_folder_stops_the_recording_but_not_the_broadcast_and_says_so() {
    let mut scratch = super::Scratch::new("record-read-only");
    let state = AppState::bootstrap(AppConfig {
        data_dir: scratch.root.join("data"),
        ..AppConfig::default()
    })
    .expect("bootstrap");
    let session_id = state
        .sessions
        .create(&SessionDraft {
            device_id: "generated".to_string(),
            device_name: "generated".to_string(),
            sample_rate: 48_000,
            channels: 1,
            started_at_ms: T0,
        })
        .expect("session")
        .id;
    // Stand in for a running capture whose recorder reports into the capture service, as a real start
    // wires it.
    state.capture.pretend_recording(48_000, 100);
    let wiring = Wiring {
        config: state.config.clone(),
        segments: state.segments.clone(),
        settings: state.settings.clone(),
        hub: state.hub.clone(),
        session_id,
        health: state.capture.recorder_health().clone(),
    };
    let recordings = state.config.recordings_dir();
    if !scratch.make_read_only(&recordings) {
        return;
    }
    let mut listener = state.hub.subscribe();
    let mut recorder = Recorder::start(&wiring);

    recorder.capture(3_000);

    // Every frame still reached the broadcast, in order.
    let mut heard = 0;
    while let Ok(frame) = listener.try_recv() {
        assert_eq!(frame.timestamp_ms, T0 + heard * FRAME_MS);
        heard += 1;
    }
    assert_eq!(heard, 30);
    assert_eq!(state.hub.live_edge_ms(), Some(T0 + 3_000));
    // None of it reached the disk, and the panel says so instead of showing a healthy recorder.
    assert_eq!(recorder.health.frames_not_written(), 30);
    let error = state
        .capture
        .snapshot()
        .error
        .expect("the failure is reported");
    assert!(
        error.contains("Recording to disk is failing"),
        "got: {error}"
    );
    assert!(error.contains("live audio continues"), "got: {error}");
    let status = status_json(&state).await;
    assert_eq!(status["capture"]["state"], "recording");
    assert_eq!(status["capture"]["error"], error.as_str());

    // The folder comes back, and so does recording, with no restart.
    scratch.make_writable(&recordings);
    recorder.capture(22_000);
    assert_eq!(state.capture.snapshot().error, None, "the error clears");
    recorder.stop();

    assert_eq!(
        indexed(&state.segments),
        vec![(3_000, 13_000), (13_000, 23_000), (23_000, 25_000)],
        "recording resumed at the first frame after the folder came back"
    );
    drop(recorder);
    drop(state);
}

#[test]
fn a_full_disk_costs_the_recording_not_the_broadcast_and_recording_resumes_when_space_returns() {
    let store = Store::new("record-disk-full");
    let wiring = Wiring::from_store(&store);
    let mut recorder = Recorder::start(&wiring);

    recorder.capture(5_000);
    let full = crate::audio::segment_writer::disk_full::fill(store.data_dir());
    recorder.capture(3_000);

    assert_eq!(
        recorder.hub.frames_published(),
        80,
        "the broadcast never paused"
    );
    assert!(recorder.health.frames_not_written() >= 30);
    let problem = recorder.health.problem().expect("the failure is reported");
    assert!(problem.contains("no space left"), "got: {problem}");

    drop(full);
    recorder.capture(12_000);
    assert_eq!(recorder.health.problem(), None, "the error clears");
    recorder.stop();

    // The segment open when the disk filled is lost whole: its last few frames were still buffered, the
    // flush on closing it failed, and the writer does not yet salvage the part that did reach the disk.
    // That costs at most one segment. What this pins is that recording resumed at the first frame after
    // space came back, and that every segment indexed afterwards is whole and plays at the right time.
    let after: Vec<(i64, i64)> = indexed(&store.segments)
        .into_iter()
        .filter(|(start, _)| *start >= 8_000)
        .collect();
    assert_eq!(after.first().map(|segment| segment.0), Some(8_000));
    assert_eq!(after.last().map(|segment| segment.1), Some(20_000));
    let playback =
        crate::services::PlaybackService::new(store.config.clone(), store.segments.clone());
    let mut cursor = playback.cursor_at(T0 + 8_000, 100);
    for step in 0..120 {
        match cursor.advance().expect("advance") {
            crate::services::CursorOutput::Frame(frame) => {
                assert_eq!(frame.timestamp_ms, T0 + 8_000 + step * FRAME_MS);
            }
            other => panic!("the audio after the disk freed up is whole, got {other:?}"),
        }
    }
}

#[cfg(unix)]
#[test]
fn a_recordings_folder_deleted_mid_segment_is_recreated_and_no_row_points_at_the_lost_file() {
    let store = Store::new("record-folder-deleted");
    let wiring = Wiring::from_store(&store);
    let mut recorder = Recorder::start(&wiring);

    recorder.capture(12_000);
    // Somebody tidies the data directory by hand while the second segment is being written.
    std::fs::remove_dir_all(store.config.recordings_dir()).expect("delete");
    recorder.capture(13_000);
    recorder.stop();

    // The segment that was open went down with the folder: on Unix its writes landed in a file with no
    // name, which is gone. Indexing it anyway would draw ten seconds on the timeline that do not exist.
    // Recording carried on into a recreated folder from the next segment. The first segment, indexed
    // before the folder went, is an ordinary orphan row now, which playback reports as a gap and the
    // janitor removes.
    assert_eq!(
        indexed(&store.segments),
        vec![(0, 10_000), (20_000, 25_000)]
    );
    let latest = store
        .segments
        .find_covering(T0 + 20_000)
        .expect("query")
        .expect("the segment after the deletion");
    let path = store
        .config
        .resolve_segment_path(&latest.path)
        .expect("inside the data directory");
    assert!(path.is_file(), "recorded into a recreated folder");
}

#[test]
fn a_frame_that_fails_to_write_never_shifts_the_audio_after_it() {
    let store = Store::new("record-one-frame-lost");
    let wiring = Wiring::from_store(&store);
    let mut recorder = Recorder::start(&wiring);

    recorder.capture(2_000);
    let full = crate::audio::segment_writer::disk_full::fill(store.data_dir());
    recorder.capture(FRAME_MS);
    drop(full);
    recorder.capture(3_000);
    recorder.stop();

    // Seeking is arithmetic on the byte offset, so a segment must never hold a hole: had the frames
    // after the lost one been appended to the same file, every later moment in it would play the audio
    // captured one frame after it. (The two seconds before the lost frame were still buffered when the
    // segment closed on the full disk, so they are lost with it, the limit the full disk test describes.)
    let playback =
        crate::services::PlaybackService::new(store.config.clone(), store.segments.clone());
    for at_ms in [2_100, 2_200, 3_000, 4_900] {
        let mut cursor = playback.cursor_at(T0 + at_ms, 100);
        match cursor.advance().expect("advance") {
            crate::services::CursorOutput::Frame(frame) => {
                assert_eq!(frame.timestamp_ms, T0 + at_ms);
                assert_eq!(
                    frame.samples[0],
                    level_at(T0 + at_ms),
                    "the audio at {at_ms} ms is the audio captured then"
                );
            }
            other => panic!("expected audio at {at_ms} ms, got {other:?}"),
        }
    }
}

#[test]
fn a_segment_the_locked_database_refused_is_indexed_once_the_lock_clears() {
    let store = Store::new("record-db-locked");
    let wiring = Wiring::from_store(&store);
    let mut recorder = Recorder::start(&wiring);

    // Another program, a backup or somebody in the sqlite3 shell, holds the write lock as the first
    // segment closes.
    let locker = store.second_connection();
    locker.execute_batch("BEGIN EXCLUSIVE;").expect("lock");
    recorder.capture(10_100);
    let health = recorder.health.clone();
    wait_until("the refused insert is reported", move || {
        health.problem().is_some()
    });
    assert!(indexed(&store.segments).is_empty());
    assert_eq!(
        recorder.hub.frames_published(),
        101,
        "the broadcast carried on"
    );

    locker.execute_batch("COMMIT;").expect("unlock");
    recorder.capture(14_900);
    recorder.stop();

    // The first segment's audio was on disk the whole time. Dropping its row would lose it from the
    // DVR for good, so it is indexed at the next chance, and the timeline has no hole.
    assert_eq!(
        indexed(&store.segments),
        vec![(0, 10_000), (10_000, 20_000), (20_000, 25_000)]
    );
    assert_eq!(recorder.health.problem(), None);
}

#[test]
fn a_segment_the_database_can_never_accept_does_not_hold_up_the_ones_after_it() {
    let store = Store::new("record-db-constraint");
    // A row already claims this session's first sequence number, so the recorder's first segment breaks
    // a uniqueness rule however often it is retried.
    store
        .segments
        .insert(&SegmentDraft {
            session_id: store.session_id,
            sequence: 0,
            day: crate::util::day::local_day(T0 - 60_000),
            path: "recordings/elsewhere.pcm".to_string(),
            started_at_ms: T0 - 60_000,
            ended_at_ms: T0 - 50_000,
            sample_rate: 48_000,
            channels: 1,
            byte_len: 0,
            peaks: Vec::new(),
        })
        .expect("the conflicting row");
    let wiring = Wiring::from_store(&store);
    let mut recorder = Recorder::start(&wiring);

    recorder.capture(35_000);
    recorder.stop();

    let after_start: Vec<(i64, i64)> = indexed(&store.segments)
        .into_iter()
        .filter(|(start, _)| *start >= 0)
        .collect();
    assert_eq!(
        after_start,
        vec![(10_000, 20_000), (20_000, 30_000), (30_000, 35_000)],
        "every later segment is indexed"
    );
}

async fn status_json(state: &Arc<AppState>) -> serde_json::Value {
    let router = crate::routes::build(state.clone());
    let request = Request::builder()
        .uri("/api/status")
        .header(header::HOST, "recorder.test")
        .body(Body::empty())
        .expect("request");
    let response = router.oneshot(request).await.expect("response");
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&body).expect("json")
}

/// Give the index's own connection a busy timeout long enough to see a stall, where the other fault tests
/// keep it short so a locked database costs them little time.
fn slow_to_give_up_on_a_lock(store: &Store) {
    store
        .segments
        .database()
        .with_connection(|conn| Ok(conn.busy_timeout(std::time::Duration::from_secs(1))?))
        .expect("busy timeout");
}

#[test]
fn a_database_held_locked_at_a_segment_close_never_pauses_the_live_feed() {
    let store = Store::new("record-db-locked-live");
    slow_to_give_up_on_a_lock(&store);
    let wiring = Wiring::from_store(&store);
    let mut listener = wiring.hub.subscribe();
    let mut recorder = Recorder::start(&wiring);

    let locker = store.second_connection();
    locker.execute_batch("BEGIN EXCLUSIVE;").expect("lock");

    // Twelve seconds of capture, so the first segment closes, and its insert waits out the lock, part way.
    // Each frame must reach a listener promptly all the same.
    let sender = recorder.sender.clone().expect("capturing");
    let mut slowest = std::time::Duration::ZERO;
    for index in 0..120 {
        let sent_at = std::time::Instant::now();
        sender
            .send(frame_at(T0 + index * FRAME_MS, 100))
            .expect("the recorder is taking frames");
        let heard = loop {
            match listener.try_recv() {
                Ok(frame) => break frame,
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                    assert!(
                        sent_at.elapsed() < std::time::Duration::from_secs(5),
                        "frame {index} never reached the listener"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(other) => panic!("the listener fell behind: {other:?}"),
            }
        };
        assert_eq!(heard.timestamp_ms, T0 + index * FRAME_MS);
        slowest = slowest.max(sent_at.elapsed());
    }
    assert!(
        slowest < std::time::Duration::from_millis(600),
        "a live frame waited {slowest:?} behind the locked database"
    );

    locker.execute_batch("COMMIT;").expect("unlock");
    recorder.sent = 120;
    recorder.next_ms = T0 + 120 * FRAME_MS;
    recorder.capture(9_900);
    recorder.stop();
    assert_eq!(
        indexed(&store.segments),
        vec![(0, 10_000), (10_000, 20_000), (20_000, 21_900)],
        "and the recording lost nothing to the wait"
    );
}

#[test]
fn storage_a_whole_queue_behind_costs_the_recording_frames_never_the_broadcast() {
    let store = Store::new("record-queue-full");
    slow_to_give_up_on_a_lock(&store);
    let wiring = Wiring::from_store(&store);
    let recorder = Recorder::start(&wiring);
    let locker = store.second_connection();
    locker.execute_batch("BEGIN EXCLUSIVE;").expect("lock");

    // The first segment's close stalls the disk thread for a second, while 85 seconds of audio arrive at
    // once: far more than the minute the queue holds.
    let sender = recorder.sender.clone().expect("capturing");
    for index in 0..850 {
        sender
            .send(frame_at(T0 + index * FRAME_MS, 100))
            .expect("the recorder is taking frames");
    }
    let hub = recorder.hub.clone();
    wait_until("every frame is broadcast", move || {
        hub.frames_published() == 850
    });
    let health = recorder.health.clone();
    wait_until("the recorder says frames are being left out", move || {
        health
            .problem()
            .is_some_and(|problem| problem.contains("left out of the recording"))
    });

    locker.execute_batch("COMMIT;").expect("unlock");
    let health = recorder.health.clone();
    wait_until("the queue has drained to disk", move || {
        health.frames_written() + health.frames_not_written() == 850
    });
    let dropped = recorder.health.frames_not_written();
    assert!(
        (100..=300).contains(&dropped),
        "about the frames past the minute's queue were dropped, not {dropped}"
    );

    // Storage is back. Capture carries on at its true time, after the frames that never reached the disk.
    let resume_ms = T0 + 850 * FRAME_MS;
    for index in 0..30 {
        sender
            .send(frame_at(resume_ms + index * FRAME_MS, 100))
            .expect("the recorder is taking frames");
    }
    drop(sender);
    let mut recorder = recorder;
    recorder.sent = 880;
    recorder.stop();

    let segments = indexed(&store.segments);
    let recorded_ms: i64 = segments.iter().map(|(start, end)| end - start).sum();
    assert_eq!(
        recorded_ms + dropped as i64 * FRAME_MS,
        88_000,
        "every frame is either recorded or counted as left out: {segments:?}"
    );
    for pair in segments.windows(2) {
        assert!(
            pair[0].1 <= pair[1].0,
            "segments never overlap: {segments:?}"
        );
    }
    let last = segments.last().expect("a segment after storage came back");
    assert_eq!(
        *last,
        (85_000, 88_000),
        "recording resumed in a new segment at its true time, not spliced onto the one before the hole"
    );
}
