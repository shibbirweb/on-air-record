//! What the service does when the things around it fail.
//!
//! The service is meant to run unattended for months, so the interesting question is rarely whether a
//! feature works but what happens to it when the disk fills, a folder loses its permissions, somebody
//! prunes the data directory by hand, or another program holds the database. Each test here stages one
//! such fault for real wherever the platform allows it: permissions changed with `chmod`, files deleted,
//! truncated or overwritten, a second SQLite connection holding a lock, garbage written over the database.
//! The one fault staged through a seam is a full disk, see `audio::segment_writer::disk_full`.
//!
//! Permission faults only exist on Unix, and root ignores permissions entirely, so those tests probe
//! whether the fault took hold and skip themselves when it did not rather than pass by accident.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::audio::{PcmS16Encoder, SegmentLayout, SegmentLocation, SegmentWriter};
use crate::config::AppConfig;
use crate::db::Database;
use crate::models::{AudioFrame, SessionDraft};
use crate::repositories::{SegmentRepository, SessionRepository, SettingsRepository};
use crate::services::{BroadcastHub, SettingsService};

mod database;
mod reading;
mod recording;
mod retention;

pub(super) const RATE: u32 = 48_000;
pub(super) const FRAME_MS: i64 = 100;
pub(super) const FRAME_SAMPLES: usize = (RATE as usize * FRAME_MS as usize) / 1000;
/// A fixed start, so a failure reproduces exactly. Recent enough that retention treats it as old.
pub(super) const T0: i64 = 1_757_000_000_000;

/// Short enough that a test of a locked database does not spend its time waiting, long enough that an
/// uncontended statement never trips it.
pub(super) const TEST_BUSY_TIMEOUT: Duration = Duration::from_millis(25);

/// A throwaway directory named after the test and the process, removed on drop.
///
/// Anything a test made read only is made writable again first, or the clean up itself would fail and
/// leave the fault behind for the next run.
pub(super) struct Scratch {
    pub root: PathBuf,
    /// Only Unix tests take permissions away; elsewhere there is nothing to give back.
    #[cfg(unix)]
    locked: Vec<PathBuf>,
}

impl Scratch {
    pub fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("oar-fault-{name}-{}", std::process::id()));
        let scratch = Self {
            root,
            #[cfg(unix)]
            locked: Vec::new(),
        };
        let _ = std::fs::remove_dir_all(&scratch.root);
        std::fs::create_dir_all(&scratch.root).expect("scratch dir");
        scratch
    }

    /// Take write permission away from a directory, remembering to give it back.
    ///
    /// Returns false when the fault did not take hold, which is what running as root looks like: root
    /// writes into a read only directory as if nothing had happened, and a test that went on would be
    /// testing nothing.
    #[cfg(unix)]
    pub fn make_read_only(&mut self, dir: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).expect("chmod");
        self.locked.push(dir.to_path_buf());

        let probe = dir.join(".oar-permission-probe");
        match std::fs::write(&probe, b"probe") {
            Ok(()) => {
                let _ = std::fs::remove_file(&probe);
                eprintln!("skipping: permissions are not enforced here, probably running as root");
                false
            }
            Err(_) => true,
        }
    }

    #[cfg(unix)]
    pub fn make_writable(&mut self, dir: &Path) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        self.locked.retain(|locked| locked != dir);
    }

    fn unlock_all(&self) {
        #[cfg(unix)]
        for dir in &self.locked {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755));
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        self.unlock_all();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A data directory with a real database file and the services a fault test needs, but no HTTP.
///
/// A file rather than an in memory database because several faults need a second connection to the same
/// database, and because that is what the service really runs on. Field order matters: the database
/// handles drop before the scratch directory is deleted, since Windows will not delete an open file.
pub(super) struct Store {
    pub config: Arc<AppConfig>,
    pub database: Arc<Database>,
    pub segments: Arc<SegmentRepository>,
    pub sessions: Arc<SessionRepository>,
    pub settings: Arc<SettingsService>,
    pub hub: Arc<BroadcastHub>,
    pub session_id: i64,
    pub scratch: Scratch,
}

impl Store {
    pub fn new(name: &str) -> Self {
        let scratch = Scratch::new(name);
        let data_dir = scratch.root.join("data");
        let config = Arc::new(AppConfig {
            data_dir: data_dir.clone(),
            ..AppConfig::default()
        });
        std::fs::create_dir_all(config.recordings_dir()).expect("recordings dir");

        let database = Arc::new(Database::open(&config.database_path()).expect("database"));
        database
            .with_connection(|conn| Ok(conn.busy_timeout(TEST_BUSY_TIMEOUT)?))
            .expect("busy timeout");

        let settings = Arc::new(
            SettingsService::load(
                Arc::new(SettingsRepository::new(database.clone())),
                config.clone(),
            )
            .expect("settings"),
        );
        let segments = Arc::new(SegmentRepository::new(database.clone()));
        let sessions = Arc::new(SessionRepository::new(database.clone()));
        let session_id = sessions
            .create(&SessionDraft {
                device_id: "generated".to_string(),
                device_name: "generated".to_string(),
                sample_rate: RATE,
                channels: 1,
                started_at_ms: T0,
            })
            .expect("session")
            .id;

        Self {
            config,
            database,
            segments,
            sessions,
            settings,
            hub: Arc::new(BroadcastHub::new()),
            session_id,
            scratch,
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.config.data_dir
    }

    pub fn layout(&self) -> SegmentLayout {
        SegmentLayout::under_data_dir(&self.config.data_dir)
    }

    /// A second connection to the same database file, the way another program would open it.
    pub fn second_connection(&self) -> rusqlite::Connection {
        let connection =
            rusqlite::Connection::open(self.config.database_path()).expect("second connection");
        connection
            .busy_timeout(TEST_BUSY_TIMEOUT)
            .expect("busy timeout");
        connection
    }

    /// Record `start_ms..end_ms` as the recorder would: real PCM on disk, every sample of it `level`, and
    /// the matching index row. Returns the file.
    pub fn seed(&self, sequence: i64, start_ms: i64, end_ms: i64, level: i16) -> PathBuf {
        seed_segment(
            &self.segments,
            &SeedAt {
                layout: self.layout(),
                session_id: self.session_id,
                sequence,
                start_ms,
                end_ms,
                level,
            },
        )
    }
}

/// Where and what [`seed_segment`] writes.
pub(super) struct SeedAt {
    pub layout: SegmentLayout,
    pub session_id: i64,
    pub sequence: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub level: i16,
}

/// Write a segment through the real writer and index it, so the file and the row agree exactly the way
/// the recorder leaves them. Returns the absolute path of the file.
pub(super) fn seed_segment(segments: &SegmentRepository, at: &SeedAt) -> PathBuf {
    let first = frame_at(at.start_ms, at.level);
    let location =
        SegmentLocation::for_segment(&at.layout, at.session_id, at.sequence, at.start_ms);
    let path = location.absolute_path.clone();
    let mut writer = SegmentWriter::create(location, &first).expect("writer");
    let mut timestamp_ms = at.start_ms;
    while timestamp_ms < at.end_ms {
        writer
            .append(&frame_at(timestamp_ms, at.level), &PcmS16Encoder)
            .expect("append");
        timestamp_ms += FRAME_MS;
    }
    let draft = writer.finish().expect("finish").expect("indexed");
    segments.insert(&draft).expect("insert");
    path
}

/// One captured frame of a constant level, as the frame builder would hand it to the recorder.
pub(super) fn frame_at(timestamp_ms: i64, level: i16) -> AudioFrame {
    AudioFrame::from_samples(timestamp_ms, RATE, 1, vec![level; FRAME_SAMPLES], true)
}

/// Wait for a condition another thread will make true, failing the test rather than hanging it.
pub(super) fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}
