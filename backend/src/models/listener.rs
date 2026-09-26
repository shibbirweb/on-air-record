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
