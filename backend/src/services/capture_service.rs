//! Owns the capture lifecycle: device open, session bookkeeping, recorder wiring, and teardown.
//!
//! Two things make this service more than a thin wrapper around [`crate::audio::capture`]. First, a
//! capture run and a recording session are the same lifetime, so the database row is opened and closed
//! here rather than in two places that could disagree. Second, changing the input device is a stop plus a
//! start, and doing that under one lock is what stops two concurrent clicks from leaving the service with
//! two live streams.

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};

use crossbeam_channel::bounded;

use crate::audio::device_access;
use crate::audio::{self, CaptureHandle, CaptureOptions, FrameEncoder, SegmentLayout};
use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::models::{CaptureSnapshot, CaptureState, SessionDraft};
use crate::repositories::{SegmentRepository, SessionRepository};
use crate::services::recorder_service::{RecorderContext, RecorderHealth};
use crate::services::{BroadcastHub, RecorderHandle, RecorderService, SettingsService};
use crate::util::time::now_ms;

/// Frames the capture thread may run ahead of the recorder.
///
/// At the default frame size this is about thirteen seconds of slack, which is far more than a disk write
/// ever needs and small enough that a genuinely stuck recorder is reported quickly through the dropped
/// frame counter instead of eating memory.
const FRAME_CHANNEL_CAPACITY: usize = 128;

/// Handles that only exist while capture is running.
struct ActiveCapture {
    capture: CaptureHandle,
    recorder: RecorderHandle,
    session_id: i64,
}

pub struct CaptureService {
    config: Arc<AppConfig>,
    settings: Arc<SettingsService>,
    sessions: Arc<SessionRepository>,
    segments: Arc<SegmentRepository>,
    hub: Arc<BroadcastHub>,
    encoder: Arc<dyn FrameEncoder>,
    dropped_frames: Arc<AtomicU64>,
    /// Trouble the recorder has writing or indexing, which it reports while the live feed carries on.
    recorder_health: Arc<RecorderHealth>,
    /// Guards the start and stop transitions. Async because a device open can take a moment and the HTTP
    /// handlers should queue behind it rather than spin.
    transition: tokio::sync::Mutex<Option<ActiveCapture>>,
    /// Read on every status poll and by every WebSocket handshake, so it is a plain lock that never waits
    /// on a device.
    snapshot: RwLock<CaptureSnapshot>,
}

impl CaptureService {
    pub fn new(
        config: Arc<AppConfig>,
        settings: Arc<SettingsService>,
        sessions: Arc<SessionRepository>,
        segments: Arc<SegmentRepository>,
        hub: Arc<BroadcastHub>,
        encoder: Arc<dyn FrameEncoder>,
    ) -> Self {
        Self {
            config,
            settings,
            sessions,
            segments,
            hub,
            encoder,
            dropped_frames: Arc::new(AtomicU64::new(0)),
            recorder_health: Arc::new(RecorderHealth::default()),
            transition: tokio::sync::Mutex::new(None),
            snapshot: RwLock::new(CaptureSnapshot::default()),
        }
    }

    /// Current capture state. Cheap, lock free enough to call per request.
    pub fn snapshot(&self) -> CaptureSnapshot {
        let mut snapshot = self.stored_snapshot();
        snapshot.dropped_frames = self
            .dropped_frames
            .load(std::sync::atomic::Ordering::Relaxed);
        // Still recording, since the live feed is, but not to disk: say so where the panel shows errors.
        if snapshot.state == CaptureState::Recording && snapshot.error.is_none() {
            snapshot.error = self.recorder_health.problem();
        }
        snapshot
    }

    /// The snapshot as last written, without the live counters and the recorder's health folded in, so a
    /// transition that rewrites it never bakes a passing disk error into the stored state.
    fn stored_snapshot(&self) -> CaptureSnapshot {
        match self.snapshot.read() {
            Ok(guard) => guard.clone(),
            Err(_) => CaptureSnapshot::default(),
        }
    }

    /// The recorder's trouble with the disk and the index, for the fault tests.
    #[cfg(all(test, unix))]
    pub(crate) fn recorder_health(&self) -> &Arc<RecorderHealth> {
        &self.recorder_health
    }

    pub fn is_active(&self) -> bool {
        self.snapshot().state.is_active()
    }

    /// Stand in for a running capture, for tests of what listeners get while recording. Starting a real
    /// one would open whatever input the machine has, a developer's microphone, and fail on a runner.
    #[cfg(test)]
    pub(crate) fn pretend_recording(&self, sample_rate: u32, frame_ms: u32) {
        let mut snapshot = self.stored_snapshot();
        snapshot.sample_rate = sample_rate;
        snapshot.device_sample_rate = sample_rate;
        snapshot.channels = 1;
        snapshot.frame_ms = frame_ms;
        self.write_snapshot(snapshot);
        self.set_state(CaptureState::Recording, None);
    }

    /// Open the configured device and begin recording.
    pub async fn start(&self) -> AppResult<CaptureSnapshot> {
        let mut guard = self.transition.lock().await;
        if guard.is_some() {
            return Err(AppError::conflict("capture is already running"));
        }

        let settings = self.settings.current();
        self.set_state(CaptureState::Starting, None);

        self.recorder_health.reset();
        let (sender, receiver) = bounded(FRAME_CHANNEL_CAPACITY);
        let options = CaptureOptions {
            device_id: settings.input_device_id.clone(),
            target_sample_rate: settings.recording_sample_rate,
            frame_ms: settings.frame_ms,
            gain: self.settings.gain_control(),
            dropped_frames: self.dropped_frames.clone(),
        };

        let capture = match audio::capture::spawn(options, sender) {
            Ok(handle) => handle,
            Err(error) => {
                let error = with_container_advice(error);
                self.set_state(CaptureState::Error, Some(error.to_string()));
                return Err(error);
            }
        };

        let runtime = capture.runtime().clone();
        let started_at_ms = now_ms();

        let session = match self.sessions.create(&SessionDraft {
            device_id: runtime.device_id.clone(),
            device_name: runtime.device_name.clone(),
            sample_rate: runtime.sample_rate,
            channels: runtime.channels,
            started_at_ms,
        }) {
            Ok(session) => session,
            Err(error) => {
                // Without a session id there is nowhere to file the segments, so unwind the stream rather
                // than record audio that can never be found again.
                capture.stop();
                self.set_state(CaptureState::Error, Some(error.to_string()));
                return Err(error);
            }
        };

        let recorder = match RecorderService::spawn(
            RecorderContext {
                config: self.config.clone(),
                segments: self.segments.clone(),
                settings: self.settings.clone(),
                hub: self.hub.clone(),
                encoder: self.encoder.clone(),
                session_id: session.id,
                layout: self.segment_layout(&settings),
                health: self.recorder_health.clone(),
            },
            receiver,
        ) {
            Ok(handle) => handle,
            Err(error) => {
                capture.stop();
                let _ = self.sessions.close(session.id, now_ms());
                self.set_state(CaptureState::Error, Some(error.to_string()));
                return Err(error);
            }
        };

        *guard = Some(ActiveCapture {
            capture,
            recorder,
            session_id: session.id,
        });

        self.write_snapshot(CaptureSnapshot {
            state: CaptureState::Recording,
            session_id: Some(session.id),
            device_id: Some(runtime.device_id),
            device_name: Some(runtime.device_name),
            sample_rate: runtime.sample_rate,
            device_sample_rate: runtime.source_sample_rate,
            channels: runtime.channels,
            frame_ms: runtime.frame_ms,
            started_at_ms: Some(started_at_ms),
            dropped_frames: 0,
            error: None,
        });

        Ok(self.snapshot())
    }

    /// Stop capture and close the session. Idempotent, because a client double clicking stop should not
    /// see an error.
    pub async fn stop(&self) -> AppResult<CaptureSnapshot> {
        let mut guard = self.transition.lock().await;
        self.stop_locked(&mut guard);
        Ok(self.snapshot())
    }

    /// Restart on the currently configured device, used after a device or frame size change.
    pub async fn restart(&self) -> AppResult<CaptureSnapshot> {
        {
            let mut guard = self.transition.lock().await;
            self.stop_locked(&mut guard);
        }
        self.start().await
    }

    /// Start capture if the settings ask for it, after the configured delay. Spawned once during boot.
    ///
    /// Runs as its own task so a long delay never holds up the HTTP listener, since the UI is where an
    /// operator would go to see why nothing is recording yet. The settings are read again once the wait
    /// is over, so switching auto start off, or starting by hand, during the delay is respected rather
    /// than overridden.
    pub async fn start_if_configured(
        self: Arc<Self>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        let settings = self.settings.current();
        if !settings.auto_start {
            // Nothing will try the device until somebody presses Start, so say now what would stop it. With
            // auto start on, the failed start below carries the same advice, and saying it twice is noise.
            if let Some(problem) = device_access::check_container() {
                tracing::warn!(advice = %problem.advice(), "the sound cards cannot be reached from this container");
            }
            tracing::info!("auto start is disabled, waiting for a manual start");
            return;
        }

        let delay_seconds = settings.auto_start_delay_seconds;
        if delay_seconds > 0 {
            tracing::info!(
                delay_seconds,
                "auto start is waiting before opening the device"
            );
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(delay_seconds.into())) => {}
                // Stopping during the delay must not open a device on the way out.
                _ = shutdown.changed() => return,
            }

            if !self.settings.current().auto_start {
                tracing::info!("auto start was switched off during the delay");
                return;
            }
        }

        if *shutdown.borrow() {
            return;
        }

        if self.is_active() {
            tracing::info!("capture was already started by hand, auto start has nothing to do");
            return;
        }

        if let Err(error) = self.start().await {
            // A missing microphone at boot must not stop the web UI from coming up, since the UI is where
            // the operator would go to fix it.
            tracing::warn!(%error, "auto start failed, the service is running without capture");
        }
    }

    /// Shut everything down cleanly, flushing the open segment.
    pub async fn shutdown(&self) {
        let mut guard = self.transition.lock().await;
        self.stop_locked(&mut guard);
    }

    fn stop_locked(&self, guard: &mut Option<ActiveCapture>) {
        let Some(active) = guard.take() else {
            return;
        };

        // Order matters. Dropping the capture closes the frame channel, which is what tells the recorder
        // to flush its open segment and index it, so the capture must go first.
        active.capture.stop();
        active.recorder.stop();

        if let Err(error) = self.sessions.close(active.session_id, now_ms()) {
            tracing::error!(%error, session_id = active.session_id, "could not close the session row");
        }

        self.hub.reset_levels();
        self.set_state(CaptureState::Idle, None);
    }

    /// Decide where this session's segments go.
    ///
    /// Resolved once per session from the settings in force at start, so a directory change never splits
    /// one recording across two roots.
    fn segment_layout(&self, settings: &crate::models::Settings) -> SegmentLayout {
        let recordings_dir = self
            .config
            .effective_recordings_dir(settings.recordings_dir.as_deref());

        if self.config.is_default_recordings_dir(&recordings_dir) {
            SegmentLayout::under_data_dir(&self.config.data_dir)
        } else {
            SegmentLayout::at(recordings_dir)
        }
    }

    fn set_state(&self, state: CaptureState, error: Option<String>) {
        let mut snapshot = self.stored_snapshot();
        snapshot.state = state;
        snapshot.error = error;

        if state == CaptureState::Idle || state == CaptureState::Error {
            snapshot.session_id = None;
            snapshot.started_at_ms = None;
        }

        self.write_snapshot(snapshot);
    }

    fn write_snapshot(&self, snapshot: CaptureSnapshot) {
        match self.snapshot.write() {
            Ok(mut guard) => *guard = snapshot,
            Err(_) => tracing::error!("capture snapshot lock was poisoned"),
        }
    }
}

/// A device error in a container gets the reason and the fix appended, when the container's view of
/// `/dev/snd` shows one. cpal only says the device has "no usable input config", which is true of every
/// access problem alike; this error is what the recorder panel shows, so the fix belongs in it.
fn with_container_advice(error: AppError) -> AppError {
    match error {
        AppError::Audio(message) => match device_access::check_container() {
            // cpal's messages usually end in a full stop already.
            Some(problem) => AppError::Audio(format!(
                "{}. {}",
                message.trim_end_matches('.'),
                problem.advice()
            )),
            None => AppError::Audio(message),
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    //! What the capture service decides without touching audio hardware. Opening a device cannot be
    //! tested on a CI runner, which has none, and on a developer's machine it would open their microphone;
    //! what capture does with audio once open is covered by the recorder and pipeline tests.

    use std::sync::Arc;
    use std::time::Duration;

    use crate::app::AppState;
    use crate::config::AppConfig;
    use crate::models::{CaptureState, SettingsPatch};

    struct Running {
        state: Arc<AppState>,
        data_dir: std::path::PathBuf,
    }

    impl Drop for Running {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.data_dir);
        }
    }

    fn running(name: &str) -> Running {
        let data_dir =
            std::env::temp_dir().join(format!("oar-capture-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data_dir);
        let state = AppState::bootstrap(AppConfig {
            data_dir: data_dir.clone(),
            ..AppConfig::default()
        })
        .expect("bootstrap");
        Running { state, data_dir }
    }

    fn configure(running: &Running, patch: SettingsPatch) {
        running.state.settings.update(&patch).expect("settings");
    }

    #[test]
    fn a_fresh_service_is_idle_with_nothing_open() {
        let running = running("fresh");
        let snapshot = running.state.capture.snapshot();
        assert_eq!(snapshot.state, CaptureState::Idle);
        assert_eq!(snapshot.session_id, None);
        assert!(!running.state.capture.is_active());
    }

    #[tokio::test]
    async fn stopping_or_shutting_down_an_idle_service_is_harmless() {
        let running = running("stop-idle");
        let stopped = running.state.capture.stop().await.expect("stop");
        assert_eq!(stopped.state, CaptureState::Idle);
        running.state.capture.shutdown().await;
        assert_eq!(running.state.capture.snapshot().state, CaptureState::Idle);
    }

    #[tokio::test]
    async fn auto_start_switched_off_opens_nothing() {
        let running = running("auto-off");
        configure(
            &running,
            SettingsPatch {
                auto_start: Some(false),
                ..SettingsPatch::default()
            },
        );
        let (_shutdown, receiver) = tokio::sync::watch::channel(false);
        tokio::time::timeout(
            Duration::from_secs(5),
            running.state.capture.clone().start_if_configured(receiver),
        )
        .await
        .expect("returns straight away");
        assert_eq!(running.state.capture.snapshot().state, CaptureState::Idle);
    }

    #[tokio::test]
    async fn shutting_down_during_the_start_up_delay_opens_nothing() {
        let running = running("auto-shutdown");
        configure(
            &running,
            SettingsPatch {
                auto_start: Some(true),
                auto_start_delay_seconds: Some(600),
                ..SettingsPatch::default()
            },
        );
        let (shutdown, receiver) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(running.state.capture.clone().start_if_configured(receiver));
        tokio::time::sleep(Duration::from_millis(50)).await;
        shutdown.send(true).expect("signal");
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the ten minute wait is cut short")
            .expect("task");
        assert_eq!(running.state.capture.snapshot().state, CaptureState::Idle);
    }

    #[tokio::test]
    async fn switching_auto_start_off_during_the_delay_is_respected() {
        let running = running("auto-changed-mind");
        configure(
            &running,
            SettingsPatch {
                auto_start: Some(true),
                auto_start_delay_seconds: Some(1),
                ..SettingsPatch::default()
            },
        );
        let (_shutdown, receiver) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(running.state.capture.clone().start_if_configured(receiver));
        // Let it read the settings and start waiting first, so the change lands during the delay.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let started = std::time::Instant::now();
        configure(
            &running,
            SettingsPatch {
                auto_start: Some(false),
                ..SettingsPatch::default()
            },
        );
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("returns once the delay is over")
            .expect("task");
        assert!(
            started.elapsed() >= Duration::from_millis(500),
            "it waited out the delay before checking again"
        );
        assert_eq!(running.state.capture.snapshot().state, CaptureState::Idle);
    }
}
