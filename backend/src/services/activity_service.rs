//! The activity log: writing entries as things happen, reading them for the admin's page, and pruning.
//!
//! **Writing never fails the action it records.** A sign in or a saved setting that worked must not be
//! reported as failed because the log could not be written, so [`ActivityService::record`] returns
//! nothing and reports its own trouble to the service log. For a recorder at home that is the right
//! trade; a system that must refuse what it cannot audit would do the opposite.

use std::sync::Arc;

use crate::error::AppResult;
use crate::models::activity::{
    ActivityDraft, ActivityEntry, ActivityEvent, ActivityQuery, Actor, Origin,
};
use crate::repositories::ActivityRepository;
use crate::util::time::now_ms;

/// The most entries one page returns, and what a page holds when the caller does not say.
pub const PAGE_LIMIT_MAX: u32 = 200;
pub const PAGE_LIMIT_DEFAULT: u32 = 50;

pub struct ActivityService {
    repository: Arc<ActivityRepository>,
}

impl ActivityService {
    pub fn new(repository: Arc<ActivityRepository>) -> Self {
        Self { repository }
    }

    /// Write an entry stamped now.
    pub fn record(&self, actor: Actor, origin: &Origin, event: ActivityEvent) {
        let kind = event.kind();
        let draft = ActivityDraft {
            at_ms: now_ms(),
            actor,
            address: origin.address.clone(),
            user_agent: origin.user_agent.clone(),
            event,
        };
        if let Err(error) = self.repository.record(&draft) {
            tracing::warn!(%error, kind, "could not write an activity log entry");
        }
    }

    /// A page of entries, newest first. A limit of zero means the default, and one past the maximum is
    /// brought down to it.
    pub fn list(&self, mut query: ActivityQuery) -> AppResult<Vec<ActivityEntry>> {
        query.limit = match query.limit {
            0 => PAGE_LIMIT_DEFAULT,
            limit => limit.min(PAGE_LIMIT_MAX),
        };
        self.repository.list(&query)
    }

    pub fn prune_before(&self, cutoff_ms: i64) -> AppResult<usize> {
        self.repository.prune_before(cutoff_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::models::activity::SignInMethod;

    fn service() -> (ActivityService, Arc<Database>) {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let service = ActivityService::new(Arc::new(ActivityRepository::new(database.clone())));
        (service, database)
    }

    fn owner() -> Actor {
        Actor::Account {
            user_id: 1,
            email: "owner@example.com".to_string(),
        }
    }

    fn from_the_kitchen() -> Origin {
        Origin {
            address: Some("192.168.1.20".to_string()),
            user_agent: Some("Firefox".to_string()),
        }
    }

    fn page(service: &ActivityService) -> Vec<ActivityEntry> {
        service.list(ActivityQuery::default()).expect("list")
    }

    #[test]
    fn a_recorded_event_is_stamped_now_with_who_and_where() {
        let (service, _) = service();
        let before = now_ms();
        service.record(
            owner(),
            &from_the_kitchen(),
            ActivityEvent::SignedIn {
                method: SignInMethod::Code,
            },
        );
        let entries = page(&service);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert!(entry.at_ms >= before && entry.at_ms <= now_ms());
        assert_eq!(entry.actor, owner());
        assert_eq!(entry.address.as_deref(), Some("192.168.1.20"));
        assert_eq!(entry.user_agent.as_deref(), Some("Firefox"));
        assert_eq!(
            entry.event,
            ActivityEvent::SignedIn {
                method: SignInMethod::Code
            }
        );
    }

    #[test]
    fn a_log_that_cannot_be_written_does_not_fail_the_caller() {
        let (service, database) = service();
        database
            .with_connection(|conn| {
                conn.execute_batch("DROP TABLE activity")?;
                Ok(())
            })
            .expect("drop");
        // Returns nothing to fail with, and must not panic.
        service.record(owner(), &Origin::default(), ActivityEvent::SignedOut);
        assert!(
            service.list(ActivityQuery::default()).is_err(),
            "reading says so"
        );
    }

    #[test]
    fn a_page_holds_the_default_when_no_limit_is_asked_for() {
        let (service, _) = service();
        for _ in 0..(PAGE_LIMIT_DEFAULT + 5) {
            service.record(Actor::Guest, &Origin::default(), ActivityEvent::SignedOut);
        }
        assert_eq!(page(&service).len(), PAGE_LIMIT_DEFAULT as usize);
    }

    #[test]
    fn a_page_never_holds_more_than_the_maximum() {
        let (service, _) = service();
        for _ in 0..(PAGE_LIMIT_MAX + 5) {
            service.record(Actor::Guest, &Origin::default(), ActivityEvent::SignedOut);
        }
        let huge = service
            .list(ActivityQuery {
                limit: 10_000,
                ..ActivityQuery::default()
            })
            .expect("list");
        assert_eq!(huge.len(), PAGE_LIMIT_MAX as usize);
        let small = service
            .list(ActivityQuery {
                limit: 3,
                ..ActivityQuery::default()
            })
            .expect("list");
        assert_eq!(small.len(), 3);
    }

    #[test]
    fn pruning_passes_the_cutoff_through() {
        let (service, database) = service();
        service.record(
            Actor::Host,
            &Origin::default(),
            ActivityEvent::AccountsDisabled,
        );
        database
            .with_connection(|conn| {
                conn.execute("UPDATE activity SET at_ms = 5", [])?;
                Ok(())
            })
            .expect("age it");
        assert_eq!(service.prune_before(6).expect("prune"), 1);
        assert!(page(&service).is_empty());
    }
}
