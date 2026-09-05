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
use crate::repositories::{BookmarkRepository, SegmentRepository, SessionRepository};
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
    pub bookmarks_deleted: usize,
}

pub struct RetentionService {
    config: Arc<AppConfig>,
    settings: Arc<SettingsService>,
    segments: Arc<SegmentRepository>,
    sessions: Arc<SessionRepository>,
    bookmarks: Arc<BookmarkRepository>,
}

impl RetentionService {
    pub fn new(
        config: Arc<AppConfig>,
        settings: Arc<SettingsService>,
        segments: Arc<SegmentRepository>,
        sessions: Arc<SessionRepository>,
        bookmarks: Arc<BookmarkRepository>,
    ) -> Self {
        Self {
            config,
            settings,
            segments,
            sessions,
            bookmarks,
        }
    }

    /// Run one pass against the current retention setting.
    ///
    /// Keeping forever is not "a very long window", it is no window at all: the janitor does nothing and
    /// the disk becomes the only limit. Expressing that as an early return rather than an enormous cutoff
    /// means there is no date far enough in the future to accidentally delete something.
    pub fn sweep(&self) -> AppResult<SweepReport> {
        let Some(retention_ms) = self.settings.current().retention_ms() else {
            return Ok(SweepReport::default());
        };

        self.sweep_before(now_ms() - retention_ms)
    }

    /// Delete everything that ended before `cutoff_ms`.
    pub fn sweep_before(&self, cutoff_ms: i64) -> AppResult<SweepReport> {
        let expired = self.segments.find_expired(cutoff_ms, BATCH_SIZE)?;
        let mut report = SweepReport::default();

        for segment in expired {
            let path = self.config.resolve_segment_path(&segment.path);
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
            self.remove_empty_directories();
        }

        // Bookmarks go with the audio they point at. Pruned independently of whether any segment was
        // deleted this pass, because a bookmark can be left behind by a window that shrank while the
        // recorder was stopped and there was nothing on disk to delete.
        report.bookmarks_deleted = self.bookmarks.delete_before(cutoff_ms)?;

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
                        Ok(Ok(report))
                            if report.segments_deleted > 0 || report.bookmarks_deleted > 0 =>
                        {
                            tracing::info!(
                                segments = report.segments_deleted,
                                bytes = report.bytes_reclaimed,
                                sessions = report.sessions_deleted,
                                bookmarks = report.bookmarks_deleted,
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

    /// Remove directories the pruning emptied.
    ///
    /// The layout is `recordings/<day>/<session>/`, so this walks both levels: a session directory goes
    /// once its segments have aged out, and the day directory goes once its last session does. Without
    /// the second level a long running install would accumulate one empty directory per day forever.
    fn remove_empty_directories(&self) {
        let Ok(days) = std::fs::read_dir(self.config.recordings_dir()) else {
            return;
        };

        for day in days.flatten() {
            let day_path = day.path();
            if !day_path.is_dir() {
                continue;
            }

            if let Ok(sessions) = std::fs::read_dir(&day_path) {
                for session in sessions.flatten() {
                    let session_path = session.path();
                    if session_path.is_dir() && is_empty_dir(&session_path) {
                        let _ = std::fs::remove_dir(&session_path);
                    }
                }
            }

            if is_empty_dir(&day_path) {
                let _ = std::fs::remove_dir(&day_path);
            }
        }
    }
}

/// True when the directory exists and holds nothing.
///
/// An unreadable directory reports false, so a permissions problem never turns into a delete attempt.
fn is_empty_dir(path: &std::path::Path) -> bool {
    std::fs::read_dir(path)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false)
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
        bookmarks: Arc<crate::repositories::BookmarkRepository>,
        settings: Arc<SettingsService>,
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
        let data_dir =
            std::env::temp_dir().join(format!("oar-retention-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&data_dir).ok();
        std::fs::create_dir_all(&data_dir).expect("data dir");

        let config = Arc::new(AppConfig {
            data_dir: data_dir.clone(),
            ..AppConfig::default()
        });

        let database = Arc::new(Database::open_in_memory().expect("database"));
        let settings = Arc::new(
            SettingsService::load(
                Arc::new(SettingsRepository::new(database.clone())),
                config.clone(),
            )
            .expect("settings"),
        );
        let segments = Arc::new(SegmentRepository::new(database.clone()));
        let sessions = Arc::new(SessionRepository::new(database.clone()));
        let bookmarks = Arc::new(crate::repositories::BookmarkRepository::new(database));

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
            service: RetentionService::new(
                config.clone(),
                settings.clone(),
                segments.clone(),
                sessions,
                bookmarks.clone(),
            ),
            bookmarks,
            settings,
            segments,
            session_id: session.id,
            config,
            data_dir,
        }
    }

    /// Writes a real file under the day based layout and indexes it, the way the recorder would.
    fn insert_with_file(fixture: &Fixture, sequence: i64, start_ms: i64, end_ms: i64) -> PathBuf {
        let day = crate::util::day::local_day(start_ms);
        let relative = format!("recordings/{day}/{}/{sequence:06}.pcm", fixture.session_id);
        let absolute = fixture.config.resolve_data_path(&relative);
        std::fs::create_dir_all(absolute.parent().expect("parent")).expect("dir");
        std::fs::write(&absolute, vec![0u8; 128]).expect("file");

        fixture
            .segments
            .insert(&SegmentDraft {
                session_id: fixture.session_id,
                sequence,
                day,
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
    fn keeping_forever_prunes_nothing() {
        let fixture = fixture("forever");
        let file = insert_with_file(&fixture, 0, 0, 1_000);

        fixture
            .settings
            .update(&crate::models::SettingsPatch {
                retention_hours: Some(None),
                ..crate::models::SettingsPatch::default()
            })
            .expect("keep forever");

        // Even material from 1970 survives, because there is no cutoff at all.
        assert_eq!(
            fixture.service.sweep().expect("sweep"),
            SweepReport::default()
        );
        assert!(file.exists());
        assert_eq!(fixture.segments.stats().expect("stats").segment_count, 1);
    }

    #[test]
    fn a_finite_window_still_prunes_after_switching_back() {
        let fixture = fixture("switchback");
        let file = insert_with_file(&fixture, 0, 0, 1_000);

        fixture
            .settings
            .update(&crate::models::SettingsPatch {
                retention_hours: Some(None),
                ..crate::models::SettingsPatch::default()
            })
            .expect("keep forever");
        assert_eq!(fixture.service.sweep().expect("sweep").segments_deleted, 0);

        fixture
            .settings
            .update(&crate::models::SettingsPatch {
                retention_hours: Some(Some(1)),
                ..crate::models::SettingsPatch::default()
            })
            .expect("one hour");

        assert_eq!(fixture.service.sweep().expect("sweep").segments_deleted, 1);
        assert!(!file.exists());
    }

    #[test]
    fn bookmarks_go_with_the_audio_they_point_at() {
        let fixture = fixture("bookmarks");
        insert_with_file(&fixture, 0, 0, 1_000);

        let stale = crate::models::BookmarkDraft {
            timestamp_ms: 500,
            label: "inside the pruned audio".to_string(),
            note: None,
        };
        let kept = crate::models::BookmarkDraft {
            timestamp_ms: 100_000,
            label: "still within the window".to_string(),
            note: None,
        };
        fixture.bookmarks.create(&stale).expect("create");
        fixture.bookmarks.create(&kept).expect("create");

        let report = fixture.service.sweep_before(50_000).expect("sweep");

        assert_eq!(report.bookmarks_deleted, 1);
        let remaining = fixture.bookmarks.list().expect("list");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].label, "still within the window");
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

    #[test]
    fn the_day_directory_goes_once_its_last_session_does() {
        let fixture = fixture("daydirs");
        let file = insert_with_file(&fixture, 0, 0, 1_000);

        let session_dir = file.parent().expect("session dir").to_path_buf();
        let day_dir = session_dir.parent().expect("day dir").to_path_buf();
        assert!(day_dir.is_dir());

        fixture.service.sweep_before(50_000).expect("sweep");

        assert!(
            !session_dir.exists(),
            "the session directory should be gone"
        );
        assert!(
            !day_dir.exists(),
            "the day directory should be gone with it"
        );
    }

    #[test]
    fn a_day_still_holding_audio_is_left_alone() {
        let fixture = fixture("keepday");
        let old = insert_with_file(&fixture, 0, 0, 1_000);
        let kept = insert_with_file(&fixture, 1, 100_000, 101_000);

        fixture.service.sweep_before(50_000).expect("sweep");

        assert!(!old.exists());
        assert!(kept.exists());
        assert!(kept.parent().expect("session dir").is_dir());
    }
}
