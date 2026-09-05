//! Creating, editing and pruning timeline bookmarks.

use std::sync::Arc;

use crate::error::{AppError, AppResult};
use crate::models::{Bookmark, BookmarkDraft, BookmarkPatch};
use crate::repositories::BookmarkRepository;

pub struct BookmarkService {
    repository: Arc<BookmarkRepository>,
}

impl BookmarkService {
    pub fn new(repository: Arc<BookmarkRepository>) -> Self {
        Self { repository }
    }

    pub fn list(&self) -> AppResult<Vec<Bookmark>> {
        self.repository.list()
    }

    pub fn create(&self, draft: BookmarkDraft) -> AppResult<Bookmark> {
        self.repository.create(&draft.sanitised()?)
    }

    pub fn update(&self, bookmark_id: i64, patch: &BookmarkPatch) -> AppResult<Bookmark> {
        if patch.is_empty() {
            return Err(AppError::bad_request("nothing to change"));
        }

        let existing = self.require(bookmark_id)?;
        let draft = patch.apply_to(&existing)?;
        self.repository.update(bookmark_id, &draft)?;

        Ok(Bookmark {
            id: existing.id,
            timestamp_ms: draft.timestamp_ms,
            label: draft.label,
            note: draft.note,
            created_at_ms: existing.created_at_ms,
        })
    }

    pub fn delete(&self, bookmark_id: i64) -> AppResult<()> {
        if self.repository.delete(bookmark_id)? {
            Ok(())
        } else {
            Err(AppError::not_found(format!(
                "no bookmark with id {bookmark_id}"
            )))
        }
    }

    /// Drop bookmarks whose audio has aged out. Called by the retention janitor.
    pub fn prune_before(&self, cutoff_ms: i64) -> AppResult<usize> {
        self.repository.delete_before(cutoff_ms)
    }

    fn require(&self, bookmark_id: i64) -> AppResult<Bookmark> {
        self.repository
            .find(bookmark_id)?
            .ok_or_else(|| AppError::not_found(format!("no bookmark with id {bookmark_id}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn service() -> BookmarkService {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        BookmarkService::new(Arc::new(BookmarkRepository::new(database)))
    }

    fn draft(label: &str) -> BookmarkDraft {
        BookmarkDraft {
            timestamp_ms: 1_757_030_400_000,
            label: label.to_string(),
            note: None,
        }
    }

    #[test]
    fn creating_validates_before_storing() {
        let service = service();
        assert!(service.create(draft("   ")).is_err());
        // The rejected draft must not have reached the database.
        assert!(service.list().expect("list").is_empty());
    }

    #[test]
    fn creating_trims_the_label() {
        let service = service();
        let created = service.create(draft("  Doorbell  ")).expect("create");
        assert_eq!(created.label, "Doorbell");
    }

    #[test]
    fn updating_a_missing_bookmark_reports_not_found() {
        let outcome = service().update(
            404,
            &BookmarkPatch {
                label: Some("new".to_string()),
                ..BookmarkPatch::default()
            },
        );
        assert!(matches!(outcome, Err(AppError::NotFound(_))));
    }

    #[test]
    fn an_empty_patch_is_refused_rather_than_silently_doing_nothing() {
        let service = service();
        let created = service.create(draft("Marker")).expect("create");

        let outcome = service.update(created.id, &BookmarkPatch::default());
        assert!(matches!(outcome, Err(AppError::BadRequest(_))));
    }

    #[test]
    fn updating_keeps_the_identity_and_creation_time() {
        let service = service();
        let created = service.create(draft("Before")).expect("create");

        let updated = service
            .update(
                created.id,
                &BookmarkPatch {
                    label: Some("After".to_string()),
                    ..BookmarkPatch::default()
                },
            )
            .expect("update");

        assert_eq!(updated.id, created.id);
        assert_eq!(updated.created_at_ms, created.created_at_ms);
        assert_eq!(updated.label, "After");
    }

    #[test]
    fn deleting_twice_reports_not_found_the_second_time() {
        let service = service();
        let created = service.create(draft("Doomed")).expect("create");

        service.delete(created.id).expect("delete");
        assert!(matches!(
            service.delete(created.id),
            Err(AppError::NotFound(_))
        ));
    }
}
