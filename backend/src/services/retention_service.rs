//! Prunes recordings that have aged out of the retention window.
//!
//! Without this the service fills the disk, which for an always on recorder is a question of when rather
//! than whether. Deletion order is deliberate: the file goes first, then the index row. If the process
//! dies between the two, the next pass sees an index row whose file is missing and removes it, whereas
//! the opposite order would leave a file nobody remembers and nothing would ever clean it up.

use std::sync::Arc;
use std::time::Duration;

use crate::config::AppConfig;
use crate::error::AppResult;
use crate::repositories::{SegmentRepository, SessionRepository};
use crate::services::SettingsService;
use crate::util::time::now_ms;

/// How often the janitor wakes up. A minute is far shorter than the shortest retention window of an hour,
/// so material never outlives its window by a noticeable margin, and it is long enough that the pass is
/// invisible in the process load.
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// Segments removed per pass, so a first run against a huge backlog stays responsive.
const BATCH_SIZE: i64 = 500;

/// What one pass did, returned for logging and for the tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepReport {
    pub segments_deleted: usize,
    pub bytes_reclaimed: i64,
    pub sessions_deleted: usize,
}

pub struct RetentionService {
    config: Arc<AppConfig>,
    settings: Arc<SettingsService>,
    segments: Arc<SegmentRepository>,
    sessions: Arc<SessionRepository>,
}

impl RetentionService {
    pub fn new(
        config: Arc<AppConfig>,
        settings: Arc<SettingsService>,
        segments: Arc<SegmentRepository>,
        sessions: Arc<SessionRepository>,
    ) -> Self {
        Self {
            config,
            settings,
            segments,
            sessions,
        }
    }

    /// Run one pass against the current retention setting.
    pub fn sweep(&self) -> AppResult<SweepReport> {
        let retention_ms = self.settings.current().retention_ms();
        self.sweep_before(now_ms() - retention_ms)
    }

    /// Delete everything that ended before `cutoff_ms`.
    pub fn sweep_before(&self, cutoff_ms: i64) -> AppResult<SweepReport> {
        let expired = self.segments.find_expired(cutoff_ms, BATCH_SIZE)?;
        let mut report = SweepReport::default();

        for segment in expired {
            let path = self.config.resolve_data_path(&segment.path);
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    // Already gone. Removing the row is exactly the repair that is wanted.
                }
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "could not delete an expired segment");
                    continue;
                }
            }

            self.segments.delete(segment.id)?;
            report.segments_deleted += 1;
            report.bytes_reclaimed += segment.byte_len;
        }

        if report.segments_deleted > 0 {
            report.sessions_deleted = self.sessions.delete_empty()?;
            self.remove_empty_session_directories();
        }

        Ok(report)
    }

    /// Background loop. Ends when `shutdown` resolves, so the process can exit promptly.
    pub async fn run(self: Arc<Self>, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(SWEEP_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let janitor = self.clone();
                    // File deletion and SQLite writes are blocking, so keep them off the runtime worker.
                    let outcome = tokio::task::spawn_blocking(move || janitor.sweep()).await;

                    match outcome {
                        Ok(Ok(report)) if report.segments_deleted > 0 => {
                            tracing::info!(
                                segments = report.segments_deleted,
                                bytes = report.bytes_reclaimed,
                                sessions = report.sessions_deleted,
                                "retention sweep reclaimed space"
                            );
                        }
                        Ok(Ok(_)) => {}
                        Ok(Err(error)) => tracing::error!(%error, "retention sweep failed"),
                        Err(error) => tracing::error!(%error, "retention sweep task failed"),
                    }
                }
                _ = shutdown.changed() => {
                    tracing::debug!("retention janitor stopping");
                    break;
                }
            }
        }
    }

    /// Remove session directories that no longer hold any segment file.
    fn remove_empty_session_directories(&self) {
        let Ok(entries) = std::fs::read_dir(self.config.recordings_dir()) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let is_empty = std::fs::read_dir(&path)
                .map(|mut items| items.next().is_none())
                .unwrap_or(false);
            if is_empty {
                let _ = std::fs::remove_dir(&path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::models::{SegmentDraft, SessionDraft};
    use crate::repositories::SettingsRepository;
    use std::path::PathBuf;

    struct Fixture {
        service: RetentionService,
        segments: Arc<SegmentRepository>,
        session_id: i64,
        config: Arc<AppConfig>,
        data_dir: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.data_dir).ok();
        }
    }

    fn fixture(name: &str) -> Fixture {
        let data_dir = std::env::temp_dir().join(format!("oar-retention-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&data_dir).ok();
        std::fs::create_dir_all(&data_dir).expect("data dir");

        let mut config = AppConfig::default();
        config.data_dir = data_dir.clone();
        let config = Arc::new(config);

        let database = Arc::new(Database::open_in_memory().expect("database"));
        let settings = Arc::new(
            SettingsService::load(Arc::new(SettingsRepository::new(database.clone())))
                .expect("settings"),
        );
        let segments = Arc::new(SegmentRepository::new(database.clone()));
        let sessions = Arc::new(SessionRepository::new(database));

        let session = sessions
            .create(&SessionDraft {
                device_id: "mic".to_string(),
                device_name: "mic".to_string(),
                sample_rate: 48_000,
                channels: 1,
                started_at_ms: 0,
            })
            .expect("session");
        sessions.close(session.id, 1_000).expect("close");

        Fixture {
            service: RetentionService::new(config.clone(), settings, segments.clone(), sessions),
            segments,
            session_id: session.id,
            config,
            data_dir,
        }
    }

    fn insert_with_file(fixture: &Fixture, sequence: i64, start_ms: i64, end_ms: i64) -> PathBuf {
        let relative = format!("recordings/{}/{sequence:06}.pcm", fixture.session_id);
        let absolute = fixture.config.resolve_data_path(&relative);
        std::fs::create_dir_all(absolute.parent().expect("parent")).expect("dir");
        std::fs::write(&absolute, vec![0u8; 128]).expect("file");

        fixture
            .segments
            .insert(&SegmentDraft {
                session_id: fixture.session_id,
                sequence,
                path: relative,
                started_at_ms: start_ms,
                ended_at_ms: end_ms,
                sample_rate: 48_000,
                channels: 1,
                byte_len: 128,
                peaks: vec![0; 10],
            })
            .expect("insert");

        absolute
    }

    #[test]
    fn deletes_expired_segments_and_their_files() {
        let fixture = fixture("expired");
        let old = insert_with_file(&fixture, 0, 0, 1_000);
        let recent = insert_with_file(&fixture, 1, 100_000, 101_000);

        let report = fixture.service.sweep_before(50_000).expect("sweep");

        assert_eq!(report.segments_deleted, 1);
        assert_eq!(report.bytes_reclaimed, 128);
        assert!(!old.exists());
        assert!(recent.exists());
        assert_eq!(fixture.segments.stats().expect("stats").segment_count, 1);
    }

    #[test]
    fn keeps_everything_inside_the_window() {
        let fixture = fixture("inside");
        insert_with_file(&fixture, 0, 100_000, 101_000);

        assert_eq!(
            fixture.service.sweep_before(50_000).expect("sweep"),
            SweepReport::default()
        );
        assert_eq!(fixture.segments.stats().expect("stats").segment_count, 1);
    }

    #[test]
    fn a_missing_file_still_removes_the_index_row() {
        let fixture = fixture("missing");
        let path = insert_with_file(&fixture, 0, 0, 1_000);
        std::fs::remove_file(&path).expect("remove");

        let report = fixture.service.sweep_before(50_000).expect("sweep");
        assert_eq!(report.segments_deleted, 1);
        assert_eq!(fixture.segments.stats().expect("stats").segment_count, 0);
    }

    #[test]
    fn empty_sessions_are_removed_after_their_segments_go() {
        let fixture = fixture("sessions");
        insert_with_file(&fixture, 0, 0, 1_000);

        let report = fixture.service.sweep_before(50_000).expect("sweep");
        assert_eq!(report.sessions_deleted, 1);
    }
}
