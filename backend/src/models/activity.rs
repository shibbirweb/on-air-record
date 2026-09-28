//! The activity log: who did what, when, and from where.
//!
//! An entry is written for every sign in and sign out, every change to an account, every listening
//! session, every download and every change to the recorder, so an admin can answer "who was listening
//! last night" or "when was this password changed". Each event is a variant with exactly the details it
//! needs, and **none of them has a field that could hold a password, a code, a token or a cookie**: the
//! type makes it impossible to log a secret, rather than a filter trying to catch one.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::{PlayerState, Role};

/// Who did it. An account's email is copied into the entry, so the log still names them after the
/// account is removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum Actor {
    Account {
        user_id: i64,
        email: String,
    },
    /// Somebody on a recorder without accounts, or not signed in yet, known only by their address.
    Guest,
    /// A recovery command run on the host, where shell access is what proves who it was.
    Host,
}

/// How a sign in was completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignInMethod {
    Password,
    /// The password, then a code from the authenticator app.
    Code,
    /// The password, then one of the account's recovery codes, which is then used up.
    RecoveryCode,
}

/// One setting that a save changed, as the settings page names it, with the values before and after.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingChange {
    pub key: String,
    pub from: Value,
    pub to: Value,
}

/// What happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ActivityEvent {
    SignedIn {
        method: SignInMethod,
    },
    /// A wrong email or password. `email` is what was typed when it looks like an email, and `None`
    /// otherwise, so a password typed into the wrong box never lands in the log.
    SignInFailed {
        email: Option<String>,
    },
    /// The right password, then a wrong code.
    SecondFactorFailed,
    /// Refused without checking, because this address has failed too often.
    SignInBlocked {
        email: Option<String>,
    },
    SignedOut,
    /// The first admin was created, switching accounts on.
    AccountsSetUp,
    /// The first visitor chose to keep the recorder open.
    StayedOpen,
    PasswordChanged,
    TwoFactorEnabled,
    TwoFactorDisabled,
    RecoveryCodesReplaced,
    AccountCreated {
        email: String,
        role: Role,
    },
    AccountRemoved {
        email: String,
    },
    RoleChanged {
        email: String,
        from: Role,
        to: Role,
    },
    PasswordSet {
        email: String,
    },
    TwoFactorRemoved {
        email: String,
    },
    /// Accounts switched off from the host, which removes every one of them.
    AccountsDisabled,
    /// One listening session, logged once when it ends rather than at every seek.
    Listened {
        started_at_ms: i64,
        /// How long the stream was open.
        connected_ms: i64,
        /// How long the page reported it was actually playing.
        played_ms: i64,
        /// Whether they went back into the recordings rather than only following live.
        played_back: bool,
        /// The earliest moment they went back to, when they did.
        earliest_ms: Option<i64>,
    },
    Exported {
        from_ms: i64,
        to_ms: i64,
    },
    CaptureStarted,
    CaptureStopped,
    DeviceSelected {
        /// `None` for the system default.
        device_id: Option<String>,
    },
    SettingsChanged {
        changes: Vec<SettingChange>,
    },
    SettingsReset,
    BookmarkAdded {
        label: String,
        timestamp_ms: i64,
    },
    BookmarkRemoved {
        label: String,
    },
    MetricsTokenCreated {
        /// Whether it took the place of an older token, which stopped working.
        replaced: bool,
    },
    MetricsTokenRevoked,
}

/// The four kinds of thing the log's filter offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActivityGroup {
    /// Signing in and out, and attempts that failed.
    Access,
    /// Passwords, two factor sign in, and accounts added, changed or removed.
    Accounts,
    Listening,
    /// Recording, the microphone, settings, bookmarks and the scrape token.
    Recorder,
}

/// Every stored kind with its group: the one table the filter and [`ActivityEvent::group`] both read.
const KINDS: &[(&str, ActivityGroup)] = &[
    ("signed_in", ActivityGroup::Access),
    ("sign_in_failed", ActivityGroup::Access),
    ("second_factor_failed", ActivityGroup::Access),
    ("sign_in_blocked", ActivityGroup::Access),
    ("signed_out", ActivityGroup::Access),
    ("accounts_set_up", ActivityGroup::Accounts),
    ("stayed_open", ActivityGroup::Accounts),
    ("password_changed", ActivityGroup::Accounts),
    ("two_factor_enabled", ActivityGroup::Accounts),
    ("two_factor_disabled", ActivityGroup::Accounts),
    ("recovery_codes_replaced", ActivityGroup::Accounts),
    ("account_created", ActivityGroup::Accounts),
    ("account_removed", ActivityGroup::Accounts),
    ("role_changed", ActivityGroup::Accounts),
    ("password_set", ActivityGroup::Accounts),
    ("two_factor_removed", ActivityGroup::Accounts),
    ("accounts_disabled", ActivityGroup::Accounts),
    ("listened", ActivityGroup::Listening),
    ("exported", ActivityGroup::Listening),
    ("capture_started", ActivityGroup::Recorder),
    ("capture_stopped", ActivityGroup::Recorder),
    ("device_selected", ActivityGroup::Recorder),
    ("settings_changed", ActivityGroup::Recorder),
    ("settings_reset", ActivityGroup::Recorder),
    ("bookmark_added", ActivityGroup::Recorder),
    ("bookmark_removed", ActivityGroup::Recorder),
    ("metrics_token_created", ActivityGroup::Recorder),
    ("metrics_token_revoked", ActivityGroup::Recorder),
];

impl ActivityGroup {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "access" => Some(Self::Access),
            "accounts" => Some(Self::Accounts),
            "listening" => Some(Self::Listening),
            "recorder" => Some(Self::Recorder),
            _ => None,
        }
    }

    /// The event kinds in this group, as stored.
    pub fn kinds(self) -> Vec<&'static str> {
        KINDS
            .iter()
            .filter(|(_, group)| *group == self)
            .map(|(kind, _)| *kind)
            .collect()
    }
}

impl ActivityEvent {
    /// The name stored in its own column, so the log can be filtered without reading every detail.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::SignedIn { .. } => "signed_in",
            Self::SignInFailed { .. } => "sign_in_failed",
            Self::SecondFactorFailed => "second_factor_failed",
            Self::SignInBlocked { .. } => "sign_in_blocked",
            Self::SignedOut => "signed_out",
            Self::AccountsSetUp => "accounts_set_up",
            Self::StayedOpen => "stayed_open",
            Self::PasswordChanged => "password_changed",
            Self::TwoFactorEnabled => "two_factor_enabled",
            Self::TwoFactorDisabled => "two_factor_disabled",
            Self::RecoveryCodesReplaced => "recovery_codes_replaced",
            Self::AccountCreated { .. } => "account_created",
            Self::AccountRemoved { .. } => "account_removed",
            Self::RoleChanged { .. } => "role_changed",
            Self::PasswordSet { .. } => "password_set",
            Self::TwoFactorRemoved { .. } => "two_factor_removed",
            Self::AccountsDisabled => "accounts_disabled",
            Self::Listened { .. } => "listened",
            Self::Exported { .. } => "exported",
            Self::CaptureStarted => "capture_started",
            Self::CaptureStopped => "capture_stopped",
            Self::DeviceSelected { .. } => "device_selected",
            Self::SettingsChanged { .. } => "settings_changed",
            Self::SettingsReset => "settings_reset",
            Self::BookmarkAdded { .. } => "bookmark_added",
            Self::BookmarkRemoved { .. } => "bookmark_removed",
            Self::MetricsTokenCreated { .. } => "metrics_token_created",
            Self::MetricsTokenRevoked => "metrics_token_revoked",
        }
    }

    pub fn group(&self) -> ActivityGroup {
        let kind = self.kind();
        KINDS
            .iter()
            .find(|(name, _)| *name == kind)
            .map(|(_, group)| *group)
            .unwrap_or(ActivityGroup::Recorder)
    }
}

/// One line of the log.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityEntry {
    pub id: i64,
    pub at_ms: i64,
    pub actor: Actor,
    /// Where the request came from. Behind a reverse proxy this is the proxy.
    pub address: Option<String>,
    pub user_agent: Option<String>,
    pub event: ActivityEvent,
}

/// Where a request came from, as the log records it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Origin {
    pub address: Option<String>,
    pub user_agent: Option<String>,
}

/// An entry about to be written.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityDraft {
    pub at_ms: i64,
    pub actor: Actor,
    pub address: Option<String>,
    pub user_agent: Option<String>,
    pub event: ActivityEvent,
}

/// Which entries to read, newest first. Every filter is optional and they combine.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActivityQuery {
    /// Only entries older than this one, for the next page.
    pub before_id: Option<i64>,
    pub limit: u32,
    /// Only what this account did.
    pub email: Option<String>,
    pub group: Option<ActivityGroup>,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
}

/// One listening session as it goes, turned into a single [`ActivityEvent::Listened`] when it ends.
///
/// Kept apart from the socket so the arithmetic is tested on its own. Play time comes from what the page
/// reports, since pausing in the browser stops the speakers and not the stream. It starts as playing, like
/// the listener list, so a client that never reports, such as a script, is counted by what it streams.
#[derive(Debug, Clone, PartialEq)]
pub struct ListeningTally {
    started_at_ms: i64,
    playing_since_ms: Option<i64>,
    played_ms: i64,
    earliest_ms: Option<i64>,
}

impl ListeningTally {
    pub fn new(started_at_ms: i64) -> Self {
        Self {
            started_at_ms,
            playing_since_ms: Some(started_at_ms),
            played_ms: 0,
            earliest_ms: None,
        }
    }

    /// The page says whether it is playing.
    pub fn player(&mut self, state: PlayerState, at_ms: i64) {
        match (state, self.playing_since_ms) {
            (PlayerState::Playing, None) => self.playing_since_ms = Some(at_ms),
            (PlayerState::Playing, Some(_)) => {}
            (PlayerState::Idle | PlayerState::Paused, Some(since)) => {
                self.played_ms += (at_ms - since).max(0);
                self.playing_since_ms = None;
            }
            (PlayerState::Idle | PlayerState::Paused, None) => {}
        }
    }

    /// The listener went back to this moment in the recordings.
    pub fn went_back_to(&mut self, moment_ms: i64) {
        self.earliest_ms = Some(match self.earliest_ms {
            Some(earliest) => earliest.min(moment_ms),
            None => moment_ms,
        });
    }

    pub fn finish(&self, at_ms: i64) -> ActivityEvent {
        let still_playing = self
            .playing_since_ms
            .map(|since| (at_ms - since).max(0))
            .unwrap_or(0);
        ActivityEvent::Listened {
            started_at_ms: self.started_at_ms,
            connected_ms: (at_ms - self.started_at_ms).max(0),
            played_ms: self.played_ms + still_playing,
            played_back: self.earliest_ms.is_some(),
            earliest_ms: self.earliest_ms,
        }
    }
}

/// What to keep of an email typed into a failed sign in: the text when it has the shape of an email,
/// trimmed and bounded, and nothing otherwise.
pub fn attempted_email(typed: &str) -> Option<String> {
    let text = typed.trim();
    let (local, domain) = text.split_once('@')?;
    let shaped = !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && text.len() <= 254
        && !text.chars().any(char::is_whitespace);
    shaped.then(|| text.to_string())
}

/// The settings a save changed, from the settings as the page sees them before and after.
///
/// `effectiveRecordingsDir` is left out: it follows from `recordingsDir`, so listing both would report one
/// change twice.
pub fn setting_changes(before: &Value, after: &Value) -> Vec<SettingChange> {
    let keys = |value: &Value| {
        value
            .as_object()
            .map(|object| object.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    };
    let mut all: std::collections::BTreeSet<String> = keys(before).into_iter().collect();
    all.extend(keys(after));
    all.into_iter()
        .filter(|key| key != "effectiveRecordingsDir")
        .filter_map(|key| {
            let from = before.get(&key).cloned().unwrap_or(Value::Null);
            let to = after.get(&key).cloned().unwrap_or(Value::Null);
            (from != to).then_some(SettingChange { key, from, to })
        })
        .collect()
}

/// One of every event, for the tests and the wire contract, so a new variant cannot be left out of
/// either: the match below stops compiling until it is added here too.
#[cfg(test)]
pub(crate) fn every_event() -> Vec<ActivityEvent> {
    use serde_json::json;
    let all = vec![
        ActivityEvent::SignedIn {
            method: SignInMethod::Password,
        },
        ActivityEvent::SignInFailed {
            email: Some("owner@example.com".to_string()),
        },
        ActivityEvent::SecondFactorFailed,
        ActivityEvent::SignInBlocked { email: None },
        ActivityEvent::SignedOut,
        ActivityEvent::AccountsSetUp,
        ActivityEvent::StayedOpen,
        ActivityEvent::PasswordChanged,
        ActivityEvent::TwoFactorEnabled,
        ActivityEvent::TwoFactorDisabled,
        ActivityEvent::RecoveryCodesReplaced,
        ActivityEvent::AccountCreated {
            email: "kitchen@example.com".to_string(),
            role: Role::Listener,
        },
        ActivityEvent::AccountRemoved {
            email: "kitchen@example.com".to_string(),
        },
        ActivityEvent::RoleChanged {
            email: "kitchen@example.com".to_string(),
            from: Role::Listener,
            to: Role::Admin,
        },
        ActivityEvent::PasswordSet {
            email: "kitchen@example.com".to_string(),
        },
        ActivityEvent::TwoFactorRemoved {
            email: "kitchen@example.com".to_string(),
        },
        ActivityEvent::AccountsDisabled,
        ActivityEvent::Listened {
            started_at_ms: 1_790_000_000_000,
            connected_ms: 600_000,
            played_ms: 540_000,
            played_back: true,
            earliest_ms: Some(1_789_990_000_000),
        },
        ActivityEvent::Exported {
            from_ms: 1_789_990_000_000,
            to_ms: 1_789_993_600_000,
        },
        ActivityEvent::CaptureStarted,
        ActivityEvent::CaptureStopped,
        ActivityEvent::DeviceSelected {
            device_id: Some("Scarlett Solo USB".to_string()),
        },
        ActivityEvent::SettingsChanged {
            changes: vec![SettingChange {
                key: "retentionHours".to_string(),
                from: json!(24),
                to: json!(null),
            }],
        },
        ActivityEvent::SettingsReset,
        ActivityEvent::BookmarkAdded {
            label: "Door".to_string(),
            timestamp_ms: 1_789_990_000_000,
        },
        ActivityEvent::BookmarkRemoved {
            label: "Door".to_string(),
        },
        ActivityEvent::MetricsTokenCreated { replaced: false },
        ActivityEvent::MetricsTokenRevoked,
    ];
    for event in &all {
        match event {
            ActivityEvent::SignedIn { .. }
            | ActivityEvent::SignInFailed { .. }
            | ActivityEvent::SecondFactorFailed
            | ActivityEvent::SignInBlocked { .. }
            | ActivityEvent::SignedOut
            | ActivityEvent::AccountsSetUp
            | ActivityEvent::StayedOpen
            | ActivityEvent::PasswordChanged
            | ActivityEvent::TwoFactorEnabled
            | ActivityEvent::TwoFactorDisabled
            | ActivityEvent::RecoveryCodesReplaced
            | ActivityEvent::AccountCreated { .. }
            | ActivityEvent::AccountRemoved { .. }
            | ActivityEvent::RoleChanged { .. }
            | ActivityEvent::PasswordSet { .. }
            | ActivityEvent::TwoFactorRemoved { .. }
            | ActivityEvent::AccountsDisabled
            | ActivityEvent::Listened { .. }
            | ActivityEvent::Exported { .. }
            | ActivityEvent::CaptureStarted
            | ActivityEvent::CaptureStopped
            | ActivityEvent::DeviceSelected { .. }
            | ActivityEvent::SettingsChanged { .. }
            | ActivityEvent::SettingsReset
            | ActivityEvent::BookmarkAdded { .. }
            | ActivityEvent::BookmarkRemoved { .. }
            | ActivityEvent::MetricsTokenCreated { .. }
            | ActivityEvent::MetricsTokenRevoked => {}
        }
    }
    all
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const GROUPS: [ActivityGroup; 4] = [
        ActivityGroup::Access,
        ActivityGroup::Accounts,
        ActivityGroup::Listening,
        ActivityGroup::Recorder,
    ];

    #[test]
    fn every_event_has_a_distinct_kind_matching_its_wire_name() {
        let mut seen = std::collections::BTreeSet::new();
        for event in every_event() {
            let kind = event.kind();
            assert!(!kind.is_empty(), "{event:?}");
            assert!(seen.insert(kind), "{kind} used twice");
            let wire = serde_json::to_value(&event).expect("serialise");
            assert_eq!(wire["kind"], kind, "{event:?}");
        }
        assert_eq!(seen.len(), every_event().len());
    }

    #[test]
    fn every_event_reads_back_as_it_was_written() {
        for event in every_event() {
            let text = serde_json::to_string(&event).expect("serialise");
            let back: ActivityEvent = serde_json::from_str(&text).expect("parse");
            assert_eq!(back, event, "{text}");
        }
    }

    #[test]
    fn details_are_camel_case_on_the_wire() {
        let wire = serde_json::to_value(ActivityEvent::Listened {
            started_at_ms: 1,
            connected_ms: 2,
            played_ms: 3,
            played_back: true,
            earliest_ms: None,
        })
        .expect("serialise");
        assert_eq!(
            wire,
            json!({
                "kind": "listened",
                "startedAtMs": 1,
                "connectedMs": 2,
                "playedMs": 3,
                "playedBack": true,
                "earliestMs": null,
            })
        );
        let method = serde_json::to_value(ActivityEvent::SignedIn {
            method: SignInMethod::RecoveryCode,
        })
        .expect("serialise");
        assert_eq!(
            method,
            json!({ "kind": "signed_in", "method": "recovery_code" })
        );
    }

    #[test]
    fn actors_say_who_they_are() {
        assert_eq!(
            serde_json::to_value(Actor::Account {
                user_id: 3,
                email: "owner@example.com".to_string()
            })
            .expect("serialise"),
            json!({ "kind": "account", "userId": 3, "email": "owner@example.com" })
        );
        assert_eq!(
            serde_json::to_value(Actor::Guest).expect("serialise"),
            json!({ "kind": "guest" })
        );
        assert_eq!(
            serde_json::to_value(Actor::Host).expect("serialise"),
            json!({ "kind": "host" })
        );
    }

    #[test]
    fn every_event_is_in_exactly_one_group_and_every_group_lists_its_kinds() {
        for event in every_event() {
            let holders: Vec<_> = GROUPS
                .iter()
                .filter(|group| group.kinds().contains(&event.kind()))
                .collect();
            assert_eq!(holders, vec![&event.group()], "{}", event.kind());
        }
        let total: usize = GROUPS.iter().map(|group| group.kinds().len()).sum();
        assert_eq!(
            total,
            every_event().len(),
            "no kind listed that does not exist"
        );
    }

    #[test]
    fn events_fall_in_the_groups_a_reader_would_look_under() {
        let group = |kind: &str| {
            every_event()
                .into_iter()
                .find(|event| event.kind() == kind)
                .map(|event| event.group())
        };
        assert_eq!(group("signed_in"), Some(ActivityGroup::Access));
        assert_eq!(group("sign_in_failed"), Some(ActivityGroup::Access));
        assert_eq!(group("signed_out"), Some(ActivityGroup::Access));
        assert_eq!(group("password_changed"), Some(ActivityGroup::Accounts));
        assert_eq!(group("two_factor_enabled"), Some(ActivityGroup::Accounts));
        assert_eq!(group("account_created"), Some(ActivityGroup::Accounts));
        assert_eq!(group("listened"), Some(ActivityGroup::Listening));
        assert_eq!(group("exported"), Some(ActivityGroup::Listening));
        assert_eq!(group("settings_changed"), Some(ActivityGroup::Recorder));
        assert_eq!(
            group("metrics_token_created"),
            Some(ActivityGroup::Recorder)
        );
    }

    #[test]
    fn groups_parse_from_their_wire_names_only() {
        for group in GROUPS {
            let name = serde_json::to_value(group).expect("serialise");
            assert_eq!(
                ActivityGroup::parse(name.as_str().expect("text")),
                Some(group)
            );
        }
        assert_eq!(ActivityGroup::parse("Access"), None);
        assert_eq!(ActivityGroup::parse(""), None);
        assert_eq!(ActivityGroup::parse("everything"), None);
    }

    #[test]
    fn an_attempted_email_is_kept_only_when_it_looks_like_one() {
        assert_eq!(
            attempted_email("  owner@example.com "),
            Some("owner@example.com".to_string())
        );
        for not_an_email in ["", "   ", "correct horse battery", "hunter2", "@", "owner@"] {
            assert_eq!(attempted_email(not_an_email), None, "{not_an_email:?}");
        }
        let long = format!("{}@example.com", "a".repeat(400));
        assert_eq!(attempted_email(&long), None, "longer than any real address");
    }

    fn listened(event: ActivityEvent) -> (i64, i64, bool, Option<i64>) {
        match event {
            ActivityEvent::Listened {
                connected_ms,
                played_ms,
                played_back,
                earliest_ms,
                ..
            } => (connected_ms, played_ms, played_back, earliest_ms),
            other => panic!("not a listening session: {other:?}"),
        }
    }

    #[test]
    fn a_session_that_never_reports_counts_as_playing_throughout() {
        let tally = ListeningTally::new(1_000);
        let event = tally.finish(61_000);
        assert_eq!(listened(event.clone()), (60_000, 60_000, false, None));
        assert!(matches!(
            event,
            ActivityEvent::Listened {
                started_at_ms: 1_000,
                ..
            }
        ));
    }

    #[test]
    fn only_the_time_the_page_was_playing_counts_as_played() {
        let mut tally = ListeningTally::new(0);
        tally.player(PlayerState::Idle, 0);
        tally.player(PlayerState::Playing, 10_000);
        tally.player(PlayerState::Paused, 25_000);
        tally.player(PlayerState::Playing, 40_000);
        assert_eq!(
            listened(tally.finish(50_000)),
            (50_000, 25_000, false, None)
        );
    }

    #[test]
    fn a_page_left_open_without_pressing_play_played_nothing() {
        let mut tally = ListeningTally::new(0);
        tally.player(PlayerState::Idle, 0);
        assert_eq!(listened(tally.finish(3_600_000)).1, 0);
    }

    #[test]
    fn saying_playing_twice_does_not_count_twice() {
        let mut tally = ListeningTally::new(0);
        tally.player(PlayerState::Playing, 0);
        tally.player(PlayerState::Playing, 5_000);
        tally.player(PlayerState::Paused, 10_000);
        tally.player(PlayerState::Paused, 20_000);
        assert_eq!(listened(tally.finish(30_000)).1, 10_000);
    }

    #[test]
    fn going_back_keeps_the_earliest_moment_reached() {
        let mut tally = ListeningTally::new(0);
        tally.went_back_to(5_000_000);
        tally.went_back_to(2_000_000);
        tally.went_back_to(4_000_000);
        let (_, _, played_back, earliest) = listened(tally.finish(10));
        assert!(played_back);
        assert_eq!(earliest, Some(2_000_000));
    }

    /// A clock that stepped back mid session must not produce negative time in the log.
    #[test]
    fn times_never_go_negative() {
        let mut tally = ListeningTally::new(10_000);
        tally.player(PlayerState::Playing, 12_000);
        let (connected, played, _, _) = listened(tally.finish(5_000));
        assert_eq!((connected, played), (0, 0));
    }

    #[test]
    fn a_save_lists_only_the_settings_it_changed() {
        let before = json!({ "gain": 1.0, "retentionHours": 24, "autoStart": true });
        let after = json!({ "gain": 1.5, "retentionHours": null, "autoStart": true });
        assert_eq!(
            setting_changes(&before, &after),
            vec![
                SettingChange {
                    key: "gain".to_string(),
                    from: json!(1.0),
                    to: json!(1.5),
                },
                SettingChange {
                    key: "retentionHours".to_string(),
                    from: json!(24),
                    to: json!(null),
                },
            ]
        );
        assert_eq!(setting_changes(&before, &before), vec![]);
    }

    #[test]
    fn the_derived_recordings_folder_is_not_reported_as_a_second_change() {
        let before = json!({ "recordingsDir": null, "effectiveRecordingsDir": "/data/recordings" });
        let after =
            json!({ "recordingsDir": "/mnt/audio", "effectiveRecordingsDir": "/mnt/audio" });
        let changes = setting_changes(&before, &after);
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert_eq!(changes[0].key, "recordingsDir");
    }

    #[test]
    fn a_setting_only_one_side_has_still_counts() {
        let changes = setting_changes(&json!({}), &json!({ "soundSensitivity": "high" }));
        assert_eq!(
            changes,
            vec![SettingChange {
                key: "soundSensitivity".to_string(),
                from: json!(null),
                to: json!("high"),
            }]
        );
    }
}
