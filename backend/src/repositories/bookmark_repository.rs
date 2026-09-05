//! Persistence for timeline bookmarks.

use std::sync::Arc;

use rusqlite::Row;

use crate::db::Database;
use crate::error::AppResult;
use crate::models::{Bookmark, BookmarkDraft, TimeRange};
use crate::util::time::now_ms;

const SELECT_COLUMNS: &str = "id, timestamp_ms, label, note, created_at_ms";

pub struct BookmarkRepository {
    database: Arc<Database>,
}

impl BookmarkRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    pub fn create(&self, draft: &BookmarkDraft) -> AppResult<Bookmark> {
        let created_at_ms = now_ms();

        self.database.with_connection(|conn| {
            conn.execute(
                "INSERT INTO bookmarks (timestamp_ms, label, note, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![draft.timestamp_ms, draft.label, draft.note, created_at_ms],
            )?;

            Ok(Bookmark {
                id: conn.last_insert_rowid(),
                timestamp_ms: draft.timestamp_ms,
                label: draft.label.clone(),
                note: draft.note.clone(),
                created_at_ms,
            })
        })
    }

    pub fn find(&self, bookmark_id: i64) -> AppResult<Option<Bookmark>> {
        self.database.with_connection(|conn| {
            let sql = format!("SELECT {SELECT_COLUMNS} FROM bookmarks WHERE id = ?1");
            let mut statement = conn.prepare(&sql)?;
            let mut rows = statement.query(rusqlite::params![bookmark_id])?;
            match rows.next()? {
                Some(row) => Ok(Some(map_bookmark(row)?)),
                None => Ok(None),
            }
        })
    }

    /// Every bookmark, oldest first, which is the order the timeline draws them in.
    pub fn list(&self) -> AppResult<Vec<Bookmark>> {
        self.database.with_connection(|conn| {
            let sql = format!("SELECT {SELECT_COLUMNS} FROM bookmarks ORDER BY timestamp_ms ASC");
            let mut statement = conn.prepare(&sql)?;
            let rows = statement.query_map([], map_bookmark)?;

            let mut bookmarks = Vec::new();
            for row in rows {
                bookmarks.push(row?);
            }
            Ok(bookmarks)
        })
    }

    /// Bookmarks inside a window, for drawing only what is on screen.
    pub fn list_in_range(&self, range: TimeRange) -> AppResult<Vec<Bookmark>> {
        self.database.with_connection(|conn| {
            let sql = format!(
                "SELECT {SELECT_COLUMNS} FROM bookmarks
                 WHERE timestamp_ms >= ?1 AND timestamp_ms < ?2
                 ORDER BY timestamp_ms ASC"
            );
            let mut statement = conn.prepare(&sql)?;
            let rows = statement.query_map(
                rusqlite::params![range.start_ms, range.end_ms],
                map_bookmark,
            )?;

            let mut bookmarks = Vec::new();
            for row in rows {
                bookmarks.push(row?);
            }
            Ok(bookmarks)
        })
    }

    pub fn update(&self, bookmark_id: i64, draft: &BookmarkDraft) -> AppResult<()> {
        self.database.with_connection(|conn| {
            conn.execute(
                "UPDATE bookmarks SET timestamp_ms = ?2, label = ?3, note = ?4 WHERE id = ?1",
                rusqlite::params![bookmark_id, draft.timestamp_ms, draft.label, draft.note],
            )?;
            Ok(())
        })
    }

    pub fn delete(&self, bookmark_id: i64) -> AppResult<bool> {
        self.database.with_connection(|conn| {
            let affected = conn.execute(
                "DELETE FROM bookmarks WHERE id = ?1",
                rusqlite::params![bookmark_id],
            )?;
            Ok(affected > 0)
        })
    }

    /// Drop bookmarks pointing at audio that has aged out.
    ///
    /// A bookmark whose recording has been pruned is a link to nothing: the timeline no longer reaches
    /// that far back, so clicking it would land on silence with no way to tell why.
    pub fn delete_before(&self, cutoff_ms: i64) -> AppResult<usize> {
        self.database.with_connection(|conn| {
            let affected = conn.execute(
                "DELETE FROM bookmarks WHERE timestamp_ms < ?1",
                rusqlite::params![cutoff_ms],
            )?;
            Ok(affected)
        })
    }
}

fn map_bookmark(row: &Row<'_>) -> rusqlite::Result<Bookmark> {
    Ok(Bookmark {
        id: row.get(0)?,
        timestamp_ms: row.get(1)?,
        label: row.get(2)?,
        note: row.get(3)?,
        created_at_ms: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> BookmarkRepository {
        BookmarkRepository::new(Arc::new(Database::open_in_memory().expect("database")))
    }

    fn draft(timestamp_ms: i64, label: &str) -> BookmarkDraft {
        BookmarkDraft {
            timestamp_ms,
            label: label.to_string(),
            note: None,
        }
    }

    #[test]
    fn create_assigns_an_id_and_a_creation_time() {
        let repository = repository();
        let created = repository
            .create(&draft(1_000, "Interview"))
            .expect("create");

        assert!(created.id > 0);
        assert!(created.created_at_ms > 0);
        assert_eq!(created.label, "Interview");
    }

    #[test]
    fn list_is_ordered_by_position_on_the_timeline() {
        let repository = repository();
        repository.create(&draft(3_000, "third")).expect("create");
        repository.create(&draft(1_000, "first")).expect("create");
        repository.create(&draft(2_000, "second")).expect("create");

        let labels: Vec<String> = repository
            .list()
            .expect("list")
            .into_iter()
            .map(|bookmark| bookmark.label)
            .collect();
        assert_eq!(labels, vec!["first", "second", "third"]);
    }

    #[test]
    fn list_in_range_uses_a_half_open_window() {
        let repository = repository();
        for timestamp in [1_000, 2_000, 3_000] {
            repository
                .create(&draft(timestamp, &format!("at {timestamp}")))
                .expect("create");
        }

        let found = repository
            .list_in_range(TimeRange::new(1_000, 3_000))
            .expect("range");

        // Start inclusive, end exclusive, matching every other range in the codebase.
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].timestamp_ms, 1_000);
        assert_eq!(found[1].timestamp_ms, 2_000);
    }

    #[test]
    fn update_replaces_every_field() {
        let repository = repository();
        let created = repository.create(&draft(1_000, "Before")).expect("create");

        repository
            .update(
                created.id,
                &BookmarkDraft {
                    timestamp_ms: 5_000,
                    label: "After".to_string(),
                    note: Some("with a note".to_string()),
                },
            )
            .expect("update");

        let stored = repository.find(created.id).expect("find").expect("present");
        assert_eq!(stored.timestamp_ms, 5_000);
        assert_eq!(stored.label, "After");
        assert_eq!(stored.note, Some("with a note".to_string()));
        // The creation time is history and must not move.
        assert_eq!(stored.created_at_ms, created.created_at_ms);
    }

    #[test]
    fn delete_reports_whether_anything_went() {
        let repository = repository();
        let created = repository.create(&draft(1_000, "Doomed")).expect("create");

        assert!(repository.delete(created.id).expect("delete"));
        assert!(!repository.delete(created.id).expect("delete again"));
        assert!(repository.find(created.id).expect("find").is_none());
    }

    #[test]
    fn pruning_removes_only_what_is_past_the_cutoff() {
        let repository = repository();
        repository.create(&draft(1_000, "old")).expect("create");
        repository.create(&draft(9_000, "kept")).expect("create");

        assert_eq!(repository.delete_before(5_000).expect("prune"), 1);

        let remaining = repository.list().expect("list");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].label, "kept");
    }

    #[test]
    fn a_missing_bookmark_is_not_an_error() {
        assert!(repository().find(404).expect("find").is_none());
    }
}
