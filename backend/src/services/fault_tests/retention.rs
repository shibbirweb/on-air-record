//! The janitor meeting files it cannot delete, files already gone, and rows it should not trust, and the
//! service starting against the leftovers of a crash.

use std::sync::Arc;

use super::{seed_segment, SeedAt, Store, T0};
use crate::app::AppState;
use crate::audio::SegmentLayout;
use crate::config::AppConfig;
use crate::models::{SegmentDraft, SessionDraft};
use crate::repositories::BookmarkRepository;
use crate::services::retention_service::BATCH_SIZE;
use crate::services::{CursorOutput, RetentionService};

/// Everything seeded here ends long before this, so a sweep to it expires all of it.
const CUTOFF_MS: i64 = T0 + 86_400_000;

fn janitor(store: &Store) -> RetentionService {
    RetentionService::new(
        store.config.clone(),
        store.settings.clone(),
        store.segments.clone(),
        store.sessions.clone(),
        Arc::new(BookmarkRepository::new(store.database.clone())),
    )
}

/// Index a row, with a small file, without going through the writer: several hundred of these have to
/// be cheap. The file sits where the recorder would put it.
fn row_with_file(
    store: &Store,
    session_id: i64,
    sequence: i64,
    start_ms: i64,
) -> std::path::PathBuf {
    let day = crate::util::day::local_day(start_ms);
    let relative = format!("recordings/{day}/{session_id}/{sequence:06}.pcm");
    let absolute = store.data_dir().join(&relative);
    std::fs::create_dir_all(absolute.parent().expect("parent")).expect("dir");
    std::fs::write(&absolute, [0u8; 64]).expect("file");
    store
        .segments
        .insert(&SegmentDraft {
            session_id,
            sequence,
            day,
            path: relative,
            started_at_ms: start_ms,
            ended_at_ms: start_ms + 1_000,
            sample_rate: 48_000,
            channels: 1,
            byte_len: 64,
            peaks: vec![0; 10],
        })
        .expect("row");
    absolute
}

#[cfg(unix)]
#[test]
fn files_the_janitor_cannot_delete_do_not_stop_it_deleting_everything_after_them() {
    let mut store = Store::new("retention-stuck");

    // More undeletable segments than one pass takes, all older than a deletable one: a day folder that
    // lost its write permission, say, ahead of a week of audio that should age out behind it.
    let stuck = BATCH_SIZE + 20;
    let mut stuck_files = Vec::new();
    for sequence in 0..stuck {
        stuck_files.push(row_with_file(
            &store,
            store.session_id,
            sequence,
            T0 + sequence * 1_000,
        ));
    }
    let later_session = store
        .sessions
        .create(&SessionDraft {
            device_id: "generated".to_string(),
            device_name: "generated".to_string(),
            sample_rate: 48_000,
            channels: 1,
            started_at_ms: T0 + 3_600_000,
        })
        .expect("session")
        .id;
    let later = row_with_file(&store, later_session, 0, T0 + 3_600_000);
    let stuck_dir = stuck_files[0].parent().expect("session dir").to_path_buf();
    assert_ne!(later.parent(), Some(stuck_dir.as_path()));
    if !store.scratch.make_read_only(&stuck_dir) {
        return;
    }

    let report = janitor(&store).sweep_before(CUTOFF_MS).expect("sweep");

    // Otherwise every pass fetches the same oldest rows, fails on each, and never reaches the rest, and
    // the disk fills while the janitor reports nothing wrong.
    assert!(!later.exists(), "the deletable segment was never reached");
    assert_eq!(report.segments_deleted, 1);
    // Nothing is forgotten: a file that could not be deleted keeps the row that points at it, because a
    // file with no row is one nothing will ever clean up.
    assert!(stuck_files.iter().all(|file| file.exists()));
    assert_eq!(store.segments.stats().expect("stats").segment_count, stuck);

    store.scratch.make_writable(&stuck_dir);
    let repaired = janitor(&store).sweep_before(CUTOFF_MS).expect("sweep");
    let second = janitor(&store).sweep_before(CUTOFF_MS).expect("sweep");
    assert_eq!(
        repaired.segments_deleted + second.segments_deleted,
        stuck as usize,
        "once the folder is writable again, the backlog clears"
    );
    assert_eq!(store.segments.stats().expect("stats").segment_count, 0);
}

#[test]
fn the_janitor_never_deletes_a_file_outside_the_data_directory() {
    let store = Store::new("retention-escape");
    let victim = store.scratch.root.join("precious.txt");
    std::fs::write(&victim, b"not a recording").expect("victim");
    store
        .segments
        .insert(&SegmentDraft {
            session_id: store.session_id,
            sequence: 0,
            day: crate::util::day::local_day(T0),
            path: "../precious.txt".to_string(),
            started_at_ms: T0,
            ended_at_ms: T0 + 1_000,
            sample_rate: 48_000,
            channels: 1,
            byte_len: 15,
            peaks: vec![0; 10],
        })
        .expect("a damaged row");

    let report = janitor(&store).sweep_before(CUTOFF_MS).expect("sweep");

    assert!(
        victim.exists(),
        "retention deleted a file that is not a recording"
    );
    // The row itself is the damage, and dropping it is the repair.
    assert_eq!(report.segments_deleted, 1);
    assert_eq!(report.bytes_reclaimed, 0);
    assert_eq!(store.segments.stats().expect("stats").segment_count, 0);
}

#[cfg(unix)]
#[test]
fn a_folder_the_janitor_cannot_tidy_does_not_fail_the_sweep() {
    let mut store = Store::new("retention-tidy");
    let file = store.seed(0, T0, T0 + 1_000, 100);
    let session_dir = file.parent().expect("session").to_path_buf();
    let day_dir = session_dir.parent().expect("day").to_path_buf();
    // Deleting the file needs the session folder writable; removing the emptied folder needs the day
    // folder writable, and that is the permission taken away.
    if !store.scratch.make_read_only(&day_dir) {
        return;
    }

    let report = janitor(&store).sweep_before(CUTOFF_MS).expect("sweep");
    assert_eq!(report.segments_deleted, 1);
    assert!(!file.exists());
    assert!(session_dir.is_dir(), "left for a later pass, not an error");

    store.scratch.make_writable(&day_dir);
}

#[test]
fn startup_after_a_crash_between_file_and_row_serves_the_rest_and_the_janitor_repairs_it() {
    let scratch = super::Scratch::new("retention-crash");
    let config = AppConfig {
        data_dir: scratch.root.join("data"),
        ..AppConfig::default()
    };

    let orphan_file = {
        let state = AppState::bootstrap(config.clone()).expect("first run");
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
        let layout = SegmentLayout::under_data_dir(&config.data_dir);
        let seed = |sequence: i64, start_ms: i64| {
            seed_segment(
                &state.segments,
                &SeedAt {
                    layout: layout.clone(),
                    session_id,
                    sequence,
                    start_ms,
                    end_ms: start_ms + 1_000,
                    level: 700,
                },
            )
        };
        let orphan = seed(0, T0);
        seed(1, T0 + 1_000);
        orphan
    };

    // The janitor deleted the first file and the process died before the row went. Then, while the
    // service was down, somebody moved the whole recordings folder away.
    std::fs::remove_file(&orphan_file).expect("crash");
    std::fs::rename(config.recordings_dir(), scratch.root.join("moved")).expect("move");

    let state = AppState::bootstrap(config.clone()).expect("starts regardless");
    assert!(
        config.recordings_dir().is_dir(),
        "the recordings folder is recreated"
    );

    // Both rows now point at nothing. Playback reports them as a gap and reaches the end, rather than
    // failing or spinning.
    let mut cursor = state.playback.cursor_at(T0, 100);
    let mut steps = Vec::new();
    for _ in 0..10 {
        let step = cursor.advance().expect("advance");
        let end = matches!(step, CursorOutput::EndOfRecording { .. });
        steps.push(step);
        if end {
            break;
        }
    }
    assert!(
        matches!(steps[0], CursorOutput::Gap { from_ms, to_ms } if from_ms == T0 && to_ms == T0 + 2_000),
        "got {steps:?}"
    );
    assert!(matches!(
        steps.last(),
        Some(CursorOutput::EndOfRecording { .. })
    ));

    // Once they age out the janitor removes the rows, which is the repair the deletion order promises.
    let report = state.retention.sweep_before(CUTOFF_MS).expect("sweep");
    assert_eq!(report.segments_deleted, 2);
    assert_eq!(state.segments.stats().expect("stats").segment_count, 0);
    drop(state);
}

#[test]
fn a_row_the_locked_database_would_not_delete_is_repaired_on_the_next_pass() {
    let store = Store::new("retention-locked");
    let first = store.seed(0, T0, T0 + 1_000, 100);
    let second = store.seed(1, T0 + 1_000, T0 + 2_000, 100);

    // The crash the deletion order is designed around, staged for real: the file goes, and then the
    // database will not let the row go, because another program holds the write lock.
    let locker = store.second_connection();
    locker.execute_batch("BEGIN EXCLUSIVE;").expect("lock");
    assert!(
        janitor(&store).sweep_before(CUTOFF_MS).is_err(),
        "the failure is reported to the janitor's loop, which logs it and tries again next minute"
    );
    assert!(!first.exists());
    assert!(second.exists(), "the pass stopped at the first refusal");
    assert_eq!(store.segments.stats().expect("stats").segment_count, 2);

    locker.execute_batch("COMMIT;").expect("unlock");
    let report = janitor(&store).sweep_before(CUTOFF_MS).expect("sweep");
    assert_eq!(report.segments_deleted, 2);
    assert!(!second.exists());
    assert_eq!(store.segments.stats().expect("stats").segment_count, 0);
}
