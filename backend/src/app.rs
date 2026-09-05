//! Composition root.
//!
//! Every service is built exactly once here and shared as an `Arc`. Handlers receive [`AppState`] and
//! reach the whole application through it, which is the facade that keeps controller signatures short and
//! makes the dependency graph visible in a single file rather than scattered across constructors.

use std::sync::Arc;

use crate::audio::{build_encoder, FrameEncoder, FrameFormat};
use crate::config::AppConfig;
use crate::db::Database;
use crate::error::AppResult;
use crate::repositories::{
    BookmarkRepository, SegmentRepository, SessionRepository, SettingsRepository,
};
use crate::services::{
    BookmarkService, BroadcastHub, CaptureService, DeviceService, PlaybackService,
    RetentionService, SettingsService, TimelineService,
};
use crate::util::time::now_ms;

pub struct AppState {
    pub config: Arc<AppConfig>,
    pub settings: Arc<SettingsService>,
    pub devices: Arc<DeviceService>,
    pub capture: Arc<CaptureService>,
    pub playback: Arc<PlaybackService>,
    pub timeline: Arc<TimelineService>,
    pub retention: Arc<RetentionService>,
    pub hub: Arc<BroadcastHub>,
    pub bookmarks: Arc<BookmarkService>,
    pub sessions: Arc<SessionRepository>,
    pub segments: Arc<SegmentRepository>,
    pub encoder: Arc<dyn FrameEncoder>,
    /// When the process came up, used for the uptime field of the health endpoint.
    pub started_at_ms: i64,
}

impl AppState {
    /// Open the database, wire every service together, and repair anything a previous crash left behind.
    pub fn bootstrap(config: AppConfig) -> AppResult<Arc<Self>> {
        config.ensure_directories()?;

        let config = Arc::new(config);
        let database = Arc::new(Database::open(&config.database_path())?);

        let settings_repository = Arc::new(SettingsRepository::new(database.clone()));
        let sessions = Arc::new(SessionRepository::new(database.clone()));
        let segments = Arc::new(SegmentRepository::new(database.clone()));
        let bookmark_repository = Arc::new(BookmarkRepository::new(database));

        // A hard kill leaves the last session marked as still recording. Closing it now keeps the
        // sessions list honest and stops the UI from showing two active sessions after a restart.
        match sessions.close_dangling() {
            Ok(0) => {}
            Ok(count) => tracing::info!(count, "closed sessions left open by a previous run"),
            Err(error) => tracing::warn!(%error, "could not close dangling sessions"),
        }

        let settings = Arc::new(SettingsService::load(settings_repository, config.clone())?);
        let hub = Arc::new(BroadcastHub::new());
        let encoder = build_encoder(FrameFormat::PcmS16);

        let capture = Arc::new(CaptureService::new(
            config.clone(),
            settings.clone(),
            sessions.clone(),
            segments.clone(),
            hub.clone(),
            encoder.clone(),
        ));

        let bookmarks = Arc::new(BookmarkService::new(bookmark_repository.clone()));
        let devices = Arc::new(DeviceService::new(settings.clone(), capture.clone()));
        let playback = Arc::new(PlaybackService::new(config.clone(), segments.clone()));
        let timeline = Arc::new(TimelineService::new(segments.clone(), hub.clone()));
        let retention = Arc::new(RetentionService::new(
            config.clone(),
            settings.clone(),
            segments.clone(),
            sessions.clone(),
            bookmark_repository.clone(),
        ));

        Ok(Arc::new(Self {
            config,
            settings,
            devices,
            capture,
            playback,
            timeline,
            retention,
            hub,
            bookmarks,
            sessions,
            segments,
            encoder,
            started_at_ms: now_ms(),
        }))
    }

    pub fn uptime_ms(&self) -> i64 {
        (now_ms() - self.started_at_ms).max(0)
    }

    /// Best estimate of "now" on the recording timeline.
    ///
    /// While capture runs this is the newest captured frame. Once it stops the newest indexed segment is
    /// the end of the timeline, and if nothing was ever recorded there is no live edge at all.
    pub fn live_edge_ms(&self) -> Option<i64> {
        match self.hub.live_edge_ms() {
            Some(edge) => Some(edge),
            None => self.segments.latest_end_ms().ok().flatten(),
        }
    }
}
