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

use crate::audio::{self, CaptureHandle, CaptureOptions, FrameEncoder, SegmentLayout};
use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::models::{CaptureSnapshot, CaptureState, SessionDraft};
use crate::repositories::{SegmentRepository, SessionRepository};
use crate::services::recorder_service::RecorderContext;
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
            transition: tokio::sync::Mutex::new(None),
            snapshot: RwLock::new(CaptureSnapshot::default()),
        }
    }

    /// Current capture state. Cheap, lock free enough to call per request.
    pub fn snapshot(&self) -> CaptureSnapshot {
        let mut snapshot = match self.snapshot.read() {
            Ok(guard) => guard.clone(),
            Err(_) => CaptureSnapshot::default(),
        };
        snapshot.dropped_frames = self
            .dropped_frames
            .load(std::sync::atomic::Ordering::Relaxed);
        snapshot
    }

    pub fn is_active(&self) -> bool {
        self.snapshot().state.is_active()
    }

    /// Open the configured device and begin recording.
    pub async fn start(&self) -> AppResult<CaptureSnapshot> {
        let mut guard = self.transition.lock().await;
        if guard.is_some() {
            return Err(AppError::conflict("capture is already running"));
        }

        let settings = self.settings.current();
        self.set_state(CaptureState::Starting, None);

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

    /// Start capture if the settings ask for it. Called once during boot.
    pub async fn start_if_configured(&self) -> AppResult<()> {
        if !self.settings.current().auto_start {
            tracing::info!("auto start is disabled, waiting for a manual start");
            return Ok(());
        }

        match self.start().await {
            Ok(_) => Ok(()),
            Err(error) => {
                // A missing microphone at boot must not stop the web UI from coming up, since the UI is
                // where the operator would go to fix it.
                tracing::warn!(%error, "auto start failed, the service is running without capture");
                Ok(())
            }
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
        let mut snapshot = self.snapshot();
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
