//! Persistence for the activity log.
//!
//! Append only from the application's side: entries are written and, past the retention window, pruned,
//! but nothing edits or deletes a single entry, so the page cannot be used to tidy the record.

use std::sync::Arc;

use rusqlite::types::ToSql;
use rusqlite::Row;

use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::models::activity::{ActivityDraft, ActivityEntry, ActivityEvent, ActivityQuery, Actor};

pub struct ActivityRepository {
    database: Arc<Database>,
}

impl ActivityRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub fn record(&self, draft: &ActivityDraft) -> AppResult<i64> {
        let detail = serde_json::to_string(&draft.event).map_err(|error| {
            AppError::internal(format!("could not write an activity entry: {error}"))
        })?;
        let (actor_kind, user_id, email) = match &draft.actor {
            Actor::Account { user_id, email } => ("account", Some(*user_id), Some(email.as_str())),
            Actor::Guest => ("guest", None, None),
            Actor::Host => ("host", None, None),
        };
        self.database.with_connection(|conn| {
            conn.execute(
                "INSERT INTO activity (at_ms, actor_kind, actor_user_id, actor_email, address, user_agent, kind, detail)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    draft.at_ms,
                    actor_kind,
                    user_id,
                    email,
                    draft.address,
                    draft.user_agent,
                    draft.event.kind(),
                    detail,
                ],
            )?;
            Ok(conn.last_insert_rowid())
        })
    }

    /// Entries matching the query, newest first. A row that cannot be read back is skipped with a
    /// warning, so one bad row never hides the rest of the log.
    pub fn list(&self, query: &ActivityQuery) -> AppResult<Vec<ActivityEntry>> {
        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn ToSql>> = Vec::new();
        if let Some(before_id) = query.before_id {
            params.push(Box::new(before_id));
            conditions.push(format!("id < ?{}", params.len()));
        }
        if let Some(email) = &query.email {
            params.push(Box::new(email.trim().to_string()));
            conditions.push(format!("actor_email = ?{} COLLATE NOCASE", params.len()));
        }
        if let Some(group) = query.group {
            let mut placeholders = Vec::new();
            for kind in group.kinds() {
                params.push(Box::new(kind));
                placeholders.push(format!("?{}", params.len()));
            }
            conditions.push(format!("kind IN ({})", placeholders.join(", ")));
        }
        if let Some(from_ms) = query.from_ms {
            params.push(Box::new(from_ms));
            conditions.push(format!("at_ms >= ?{}", params.len()));
        }
        if let Some(to_ms) = query.to_ms {
            params.push(Box::new(to_ms));
            conditions.push(format!("at_ms <= ?{}", params.len()));
        }
        params.push(Box::new(i64::from(query.limit)));
        let limit = params.len();

        let filter = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT id, at_ms, actor_kind, actor_user_id, actor_email, address, user_agent, detail
             FROM activity {filter} ORDER BY id DESC LIMIT ?{limit}"
        );

        self.database.with_connection(|conn| {
            let mut statement = conn.prepare(&sql)?;
            let bound: Vec<&dyn ToSql> = params.iter().map(|param| param.as_ref()).collect();
            let rows = statement.query_map(bound.as_slice(), read_row)?;
            let mut entries = Vec::new();
            for row in rows {
                match row? {
                    Some(entry) => entries.push(entry),
                    None => tracing::warn!("skipped an activity entry that could not be read"),
                }
            }
            Ok(entries)
        })
    }

    /// Remove every entry older than the cutoff, returning how many went.
    pub fn prune_before(&self, cutoff_ms: i64) -> AppResult<usize> {
        self.database.with_connection(|conn| {
            Ok(conn.execute(
                "DELETE FROM activity WHERE at_ms < ?1",
                rusqlite::params![cutoff_ms],
            )?)
        })
    }
}

/// One row, or `None` when its actor or detail cannot be understood.
fn read_row(row: &Row<'_>) -> rusqlite::Result<Option<ActivityEntry>> {
    let actor_kind: String = row.get(2)?;
    let user_id: Option<i64> = row.get(3)?;
    let email: Option<String> = row.get(4)?;
    let detail: String = row.get(7)?;
    let actor = match (actor_kind.as_str(), user_id, email) {
        ("account", Some(user_id), Some(email)) => Actor::Account { user_id, email },
        ("guest", _, _) => Actor::Guest,
        ("host", _, _) => Actor::Host,
        _ => return Ok(None),
    };
    let Ok(event) = serde_json::from_str::<ActivityEvent>(&detail) else {
        return Ok(None);
    };
    Ok(Some(ActivityEntry {
        id: row.get(0)?,
        at_ms: row.get(1)?,
        actor,
        address: row.get(5)?,
        user_agent: row.get(6)?,
        event,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::activity::{ActivityEvent, ActivityGroup, Actor, SignInMethod};

    fn repository() -> ActivityRepository {
        ActivityRepository::new(Arc::new(Database::open_in_memory().expect("database")))
    }

    fn owner() -> Actor {
        Actor::Account {
            user_id: 1,
            email: "owner@example.com".to_string(),
        }
    }

    fn kitchen() -> Actor {
        Actor::Account {
            user_id: 2,
            email: "kitchen@example.com".to_string(),
        }
    }

    fn draft(at_ms: i64, actor: Actor, event: ActivityEvent) -> ActivityDraft {
        ActivityDraft {
            at_ms,
            actor,
            address: Some("192.168.1.20".to_string()),
            user_agent: Some("Firefox".to_string()),
            event,
        }
    }

    fn signed_in() -> ActivityEvent {
        ActivityEvent::SignedIn {
            method: SignInMethod::Password,
        }
    }

    fn all(repository: &ActivityRepository) -> Vec<ActivityEntry> {
        repository
            .list(&ActivityQuery {
                limit: 100,
                ..ActivityQuery::default()
            })
            .expect("list")
    }

    #[test]
    fn an_entry_reads_back_exactly_as_it_was_written() {
        let repository = repository();
        let written = draft(1_000, owner(), signed_in());
        let id = repository.record(&written).expect("record");
        assert!(id > 0);
        assert_eq!(
            all(&repository),
            vec![ActivityEntry {
                id,
                at_ms: 1_000,
                actor: owner(),
                address: Some("192.168.1.20".to_string()),
                user_agent: Some("Firefox".to_string()),
                event: signed_in(),
            }]
        );
    }

    #[test]
    fn every_kind_of_event_and_actor_survives_the_round_trip() {
        let repository = repository();
        let events = crate::models::activity::every_event();
        let actors = [owner(), Actor::Guest, Actor::Host];
        for (index, event) in events.iter().enumerate() {
            let mut entry = draft(index as i64, actors[index % 3].clone(), event.clone());
            entry.address = None;
            entry.user_agent = None;
            repository.record(&entry).expect("record");
        }
        let read: Vec<_> = all(&repository).into_iter().rev().collect();
        assert_eq!(read.len(), events.len());
        for (index, entry) in read.iter().enumerate() {
            assert_eq!(entry.event, events[index]);
            assert_eq!(entry.actor, actors[index % 3]);
            assert_eq!(entry.address, None);
        }
    }

    #[test]
    fn the_newest_entry_comes_first() {
        let repository = repository();
        for at in [1_000, 2_000, 3_000] {
            repository
                .record(&draft(at, owner(), signed_in()))
                .expect("record");
        }
        let times: Vec<i64> = all(&repository).iter().map(|entry| entry.at_ms).collect();
        assert_eq!(times, vec![3_000, 2_000, 1_000]);
    }

    #[test]
    fn pages_follow_on_from_the_last_entry_seen() {
        let repository = repository();
        for at in 1..=5 {
            repository
                .record(&draft(at, owner(), signed_in()))
                .expect("record");
        }
        let first = repository
            .list(&ActivityQuery {
                limit: 2,
                ..ActivityQuery::default()
            })
            .expect("first page");
        assert_eq!(
            first.iter().map(|entry| entry.at_ms).collect::<Vec<_>>(),
            vec![5, 4]
        );
        let second = repository
            .list(&ActivityQuery {
                limit: 2,
                before_id: first.last().map(|entry| entry.id),
                ..ActivityQuery::default()
            })
            .expect("second page");
        assert_eq!(
            second.iter().map(|entry| entry.at_ms).collect::<Vec<_>>(),
            vec![3, 2]
        );
    }

    #[test]
    fn filters_narrow_by_person_group_and_time_and_combine() {
        let repository = repository();
        repository
            .record(&draft(1_000, owner(), signed_in()))
            .expect("record");
        repository
            .record(&draft(2_000, kitchen(), signed_in()))
            .expect("record");
        repository
            .record(&draft(3_000, kitchen(), ActivityEvent::CaptureStarted))
            .expect("record");
        repository
            .record(&draft(4_000, Actor::Guest, ActivityEvent::SignedOut))
            .expect("record");

        let times = |query: ActivityQuery| {
            repository
                .list(&ActivityQuery {
                    limit: 100,
                    ..query
                })
                .expect("list")
                .iter()
                .map(|entry| entry.at_ms)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            times(ActivityQuery {
                email: Some("kitchen@example.com".to_string()),
                ..ActivityQuery::default()
            }),
            vec![3_000, 2_000]
        );
        assert_eq!(
            times(ActivityQuery {
                group: Some(ActivityGroup::Access),
                ..ActivityQuery::default()
            }),
            vec![4_000, 2_000, 1_000]
        );
        assert_eq!(
            times(ActivityQuery {
                from_ms: Some(2_000),
                to_ms: Some(3_000),
                ..ActivityQuery::default()
            }),
            vec![3_000, 2_000],
            "both ends included"
        );
        assert_eq!(
            times(ActivityQuery {
                email: Some("kitchen@example.com".to_string()),
                group: Some(ActivityGroup::Access),
                ..ActivityQuery::default()
            }),
            vec![2_000]
        );
        assert_eq!(
            times(ActivityQuery {
                email: Some("nobody@example.com".to_string()),
                ..ActivityQuery::default()
            }),
            Vec::<i64>::new()
        );
    }

    #[test]
    fn a_person_filter_matches_the_email_however_it_is_cased() {
        let repository = repository();
        repository
            .record(&draft(1, owner(), signed_in()))
            .expect("record");
        let found = repository
            .list(&ActivityQuery {
                limit: 10,
                email: Some("Owner@Example.com".to_string()),
                ..ActivityQuery::default()
            })
            .expect("list");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn pruning_removes_only_what_is_older_than_the_cutoff() {
        let repository = repository();
        for at in [1_000, 2_000, 3_000] {
            repository
                .record(&draft(at, owner(), signed_in()))
                .expect("record");
        }
        assert_eq!(repository.prune_before(2_000).expect("prune"), 1);
        let times: Vec<i64> = all(&repository).iter().map(|entry| entry.at_ms).collect();
        assert_eq!(times, vec![3_000, 2_000], "the cutoff itself is kept");
        assert_eq!(repository.prune_before(0).expect("prune nothing"), 0);
    }

    /// A row whose detail cannot be read, from a hand edit or a future version, is skipped rather than
    /// failing the whole page.
    #[test]
    fn an_unreadable_entry_is_skipped_not_fatal() {
        let repository = repository();
        repository
            .record(&draft(1_000, owner(), signed_in()))
            .expect("record");
        repository
            .database
            .with_connection(|conn| {
                conn.execute(
                    "INSERT INTO activity (at_ms, actor_kind, kind, detail) VALUES (2000, 'guest', 'from_the_future', '{not json')",
                    [],
                )?;
                Ok(())
            })
            .expect("insert");
        let entries = all(&repository);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].at_ms, 1_000);
    }
}
