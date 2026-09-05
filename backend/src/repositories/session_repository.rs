//! Persistence for capture sessions.

use std::sync::Arc;

use rusqlite::Row;

use crate::db::Database;
use crate::error::AppResult;
use crate::models::{RecordingSession, SessionDraft};

/// A session joined with aggregates over its segments, which is what the sessions endpoint returns.
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub session: RecordingSession,
    pub segment_count: i64,
    pub bytes: i64,
}

pub struct SessionRepository {
    database: Arc<Database>,
}

impl SessionRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub fn create(&self, draft: &SessionDraft) -> AppResult<RecordingSession> {
        self.database.with_connection(|conn| {
            conn.execute(
                "INSERT INTO sessions (device_id, device_name, sample_rate, channels, started_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    draft.device_id,
                    draft.device_name,
                    draft.sample_rate,
                    draft.channels,
                    draft.started_at_ms,
                ],
            )?;

            Ok(RecordingSession {
                id: conn.last_insert_rowid(),
                device_id: draft.device_id.clone(),
                device_name: draft.device_name.clone(),
                sample_rate: draft.sample_rate,
                channels: draft.channels,
                started_at_ms: draft.started_at_ms,
                ended_at_ms: None,
            })
        })
    }

    pub fn close(&self, session_id: i64, ended_at_ms: i64) -> AppResult<()> {
        self.database.with_connection(|conn| {
            conn.execute(
                "UPDATE sessions SET ended_at_ms = ?2 WHERE id = ?1 AND ended_at_ms IS NULL",
                rusqlite::params![session_id, ended_at_ms],
            )?;
            Ok(())
        })
    }

    /// Close every session left open by a previous run.
    ///
    /// A hard kill leaves `ended_at_ms` null forever, which would make the UI show a session that is still
    /// recording. On startup those are closed at the end of their newest segment, which is the last moment
    /// audio is known to have existed.
    pub fn close_dangling(&self) -> AppResult<usize> {
        self.database.with_connection(|conn| {
            let affected = conn.execute(
                "UPDATE sessions
                 SET ended_at_ms = COALESCE(
                     (SELECT MAX(ended_at_ms) FROM segments WHERE segments.session_id = sessions.id),
                     started_at_ms
                 )
                 WHERE ended_at_ms IS NULL",
                [],
            )?;
            Ok(affected)
        })
    }

    pub fn find(&self, session_id: i64) -> AppResult<Option<RecordingSession>> {
        self.database.with_connection(|conn| {
            let mut statement = conn.prepare(
                "SELECT id, device_id, device_name, sample_rate, channels, started_at_ms, ended_at_ms
                 FROM sessions WHERE id = ?1",
            )?;
            let mut rows = statement.query(rusqlite::params![session_id])?;
            match rows.next()? {
                Some(row) => Ok(Some(map_session(row)?)),
                None => Ok(None),
            }
        })
    }

    /// Newest sessions first, with their segment aggregates.
    pub fn list_summaries(&self, limit: i64) -> AppResult<Vec<SessionSummary>> {
        self.database.with_connection(|conn| {
            let mut statement = conn.prepare(
                "SELECT s.id, s.device_id, s.device_name, s.sample_rate, s.channels,
                        s.started_at_ms, s.ended_at_ms,
                        COUNT(seg.id), COALESCE(SUM(seg.byte_len), 0)
                 FROM sessions s
                 LEFT JOIN segments seg ON seg.session_id = s.id
                 GROUP BY s.id
                 ORDER BY s.started_at_ms DESC
                 LIMIT ?1",
            )?;

            let rows = statement.query_map(rusqlite::params![limit], |row| {
                Ok(SessionSummary {
                    session: RecordingSession {
                        id: row.get(0)?,
                        device_id: row.get(1)?,
                        device_name: row.get(2)?,
                        sample_rate: row.get::<_, i64>(3)? as u32,
                        channels: row.get::<_, i64>(4)? as u16,
                        started_at_ms: row.get(5)?,
                        ended_at_ms: row.get(6)?,
                    },
                    segment_count: row.get(7)?,
                    bytes: row.get(8)?,
                })
            })?;

            let mut summaries = Vec::new();
            for row in rows {
                summaries.push(row?);
            }
            Ok(summaries)
        })
    }

    /// Drop sessions that no longer own any segment. Called by the retention janitor after it prunes.
    pub fn delete_empty(&self) -> AppResult<usize> {
        self.database.with_connection(|conn| {
            let affected = conn.execute(
                "DELETE FROM sessions
                 WHERE ended_at_ms IS NOT NULL
                   AND NOT EXISTS (SELECT 1 FROM segments WHERE segments.session_id = sessions.id)",
                [],
            )?;
            Ok(affected)
        })
    }
}

fn map_session(row: &Row<'_>) -> rusqlite::Result<RecordingSession> {
    Ok(RecordingSession {
        id: row.get(0)?,
        device_id: row.get(1)?,
        device_name: row.get(2)?,
        sample_rate: row.get::<_, i64>(3)? as u32,
        channels: row.get::<_, i64>(4)? as u16,
        started_at_ms: row.get(5)?,
        ended_at_ms: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> SessionDraft {
        SessionDraft {
            device_id: "Built-in Microphone".to_string(),
            device_name: "Built-in Microphone".to_string(),
            sample_rate: 48_000,
            channels: 1,
            started_at_ms: 1_700_000_000_000,
        }
    }

    fn repository() -> SessionRepository {
        SessionRepository::new(Arc::new(Database::open_in_memory().expect("database")))
    }

    #[test]
    fn create_assigns_an_id_and_leaves_the_session_open() {
        let repository = repository();
        let session = repository.create(&draft()).expect("create");
        assert!(session.id > 0);
        assert_eq!(session.ended_at_ms, None);
    }

    #[test]
    fn close_sets_the_end_timestamp_once() {
        let repository = repository();
        let session = repository.create(&draft()).expect("create");
        repository.close(session.id, 1_700_000_060_000).expect("close");
        repository.close(session.id, 1_700_000_090_000).expect("close again");

        let stored = repository.find(session.id).expect("find").expect("present");
        assert_eq!(stored.ended_at_ms, Some(1_700_000_060_000));
    }

    #[test]
    fn dangling_sessions_are_closed_at_their_start_when_they_have_no_segments() {
        let repository = repository();
        let session = repository.create(&draft()).expect("create");
        assert_eq!(repository.close_dangling().expect("close dangling"), 1);

        let stored = repository.find(session.id).expect("find").expect("present");
        assert_eq!(stored.ended_at_ms, Some(draft().started_at_ms));
    }

    #[test]
    fn summaries_report_zero_segments_for_a_new_session() {
        let repository = repository();
        repository.create(&draft()).expect("create");
        let summaries = repository.list_summaries(10).expect("list");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].segment_count, 0);
        assert_eq!(summaries[0].bytes, 0);
    }

    #[test]
    fn empty_closed_sessions_are_deleted() {
        let repository = repository();
        let session = repository.create(&draft()).expect("create");
        repository.close(session.id, 1).expect("close");
        assert_eq!(repository.delete_empty().expect("delete"), 1);
        assert!(repository.find(session.id).expect("find").is_none());
    }
}
