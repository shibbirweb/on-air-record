//! Who is connected to the audio stream right now, for the admin's listener list.

use std::net::IpAddr;

use crate::models::Role;

/// The account behind a connection. `None` on the connection itself means a guest: an open recorder,
/// where nobody signs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerAccount {
    pub email: String,
    pub role: Role,
}

/// What a connection is doing, as far as anybody watching the list cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerActivity {
    /// Following the live feed.
    Live,
    /// Listening back through history. `from_ms` is where they started or last jumped to, not where the
    /// playhead is now: the playhead moves every frame, and the list is not worth updating that often.
    Playback {
        from_ms: i64,
    },
    Paused,
}

/// Whether the person at a browser is actually hearing anything, as the browser reports it. The stream
/// keeps flowing either way: browsers only allow audio after a click, and pausing in the UI stops the
/// speakers, not the socket. Without this a tab nobody pressed play in would be listed as live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerState {
    /// Play has not been pressed since the page opened.
    Idle,
    Playing,
    /// Play was pressed, then pause.
    Paused,
}

/// One open audio stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerEntry {
    /// Stable for the life of the connection, so a list that changes does not reshuffle its rows.
    pub id: u64,
    pub account: Option<ListenerAccount>,
    /// Where the connection comes from. Behind a reverse proxy this is the proxy.
    pub address: IpAddr,
    /// The browser's own description of itself, as sent. Shown to admins only.
    pub user_agent: Option<String>,
    pub connected_at_ms: i64,
    pub activity: ListenerActivity,
    /// Starts as `Playing`, so a client that never reports, such as a script reading the socket, is
    /// listed by what it streams. The UI reports as soon as its socket opens.
    pub player: PlayerState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_states_travel_as_lowercase_words_both_ways() {
        for (state, word) in [
            (PlayerState::Idle, "idle"),
            (PlayerState::Playing, "playing"),
            (PlayerState::Paused, "paused"),
        ] {
            assert_eq!(serde_json::to_value(state).expect("serialise"), word);
            let parsed: PlayerState =
                serde_json::from_value(serde_json::json!(word)).expect("parse");
            assert_eq!(parsed, state);
        }
    }

    #[test]
    fn a_player_state_the_ui_never_sends_is_refused() {
        for word in ["Playing", "stopped", ""] {
            assert!(
                serde_json::from_value::<PlayerState>(serde_json::json!(word)).is_err(),
                "{word:?}"
            );
        }
    }

    #[test]
    fn a_connection_without_an_account_is_a_guest() {
        let entry = ListenerEntry {
            id: 1,
            account: None,
            address: "192.168.1.20".parse().expect("address"),
            user_agent: None,
            connected_at_ms: 0,
            activity: ListenerActivity::Live,
            player: PlayerState::Playing,
        };
        assert!(entry.account.is_none());
        let signed_in = ListenerEntry {
            account: Some(ListenerAccount {
                email: "kitchen@example.com".to_string(),
                role: Role::Listener,
            }),
            ..entry.clone()
        };
        assert_ne!(signed_in, entry);
        assert_eq!(
            ListenerActivity::Playback { from_ms: 5 },
            ListenerActivity::Playback { from_ms: 5 }
        );
    }
}
