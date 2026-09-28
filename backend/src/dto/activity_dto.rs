//! The activity log on the wire.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::models::activity::{ActivityEntry, ActivityEvent, ActivityGroup, ActivityQuery, Actor};

/// `GET /api/activity` query: every filter optional.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivityListQuery {
    pub before_id: Option<i64>,
    pub limit: Option<u32>,
    pub email: Option<String>,
    pub group: Option<String>,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
}

impl ActivityListQuery {
    /// Into the repository's query. An empty email means everybody; a group the log does not have is
    /// refused rather than quietly returning everything.
    pub fn parsed(self) -> AppResult<ActivityQuery> {
        let group = match self.group.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(text) => Some(ActivityGroup::parse(text).ok_or_else(|| {
                AppError::bad_request(format!(
                    "there is no activity group called {text}; use access, accounts, listening or recorder"
                ))
            })?),
        };
        Ok(ActivityQuery {
            before_id: self.before_id,
            limit: self.limit.unwrap_or(0),
            email: self
                .email
                .map(|email| email.trim().to_string())
                .filter(|email| !email.is_empty()),
            group,
            from_ms: self.from_ms,
            to_ms: self.to_ms,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntryDto {
    pub id: i64,
    pub at_ms: i64,
    pub actor: Actor,
    pub address: Option<String>,
    pub user_agent: Option<String>,
    pub event: ActivityEvent,
}

impl From<ActivityEntry> for ActivityEntryDto {
    fn from(entry: ActivityEntry) -> Self {
        Self {
            id: entry.id,
            at_ms: entry.at_ms,
            actor: entry.actor,
            address: entry.address,
            user_agent: entry.user_agent,
            event: entry.event,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ActivityListResponse {
    pub entries: Vec<ActivityEntryDto>,
}
