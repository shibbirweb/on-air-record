//! Control messages exchanged as JSON text on the audio socket.
//!
//! Text and binary WebSocket messages are distinguishable at the framing layer, so control traffic and
//! audio share one connection without needing an envelope or a discriminator byte on every frame.

use serde::{Deserialize, Serialize};

/// What the session is currently doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamMode {
    Live,
    Playback,
    Paused,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ServerMessage {
    /// Sent on connect and whenever the audio format or the mode changes.
    #[serde(rename_all = "camelCase")]
    StreamInfo {
        sample_rate: u32,
        channels: u16,
        frame_ms: u32,
        mode: StreamMode,
        server_time_ms: i64,
        live_edge_ms: Option<i64>,
        earliest_ms: Option<i64>,
        capturing: bool,
    },

    #[serde(rename_all = "camelCase")]
    Mode { mode: StreamMode, position_ms: i64 },

    #[serde(rename_all = "camelCase")]
    SwitchedToLive { timestamp_ms: i64 },

    /// Playback crossed a stretch with no recording and skipped to the next material.
    #[serde(rename_all = "camelCase")]
    Gap { from_ms: i64, to_ms: i64 },

    #[serde(rename_all = "camelCase")]
    EndOfRecording { timestamp_ms: i64 },

    #[serde(rename_all = "camelCase")]
    Level { rms: f32, peak: f32 },

    /// The applied playback speed, echoed after a request so the client learns what it was clamped to.
    #[serde(rename_all = "camelCase")]
    Speed { value: f32 },

    #[serde(rename_all = "camelCase")]
    Pong {
        client_time_ms: i64,
        server_time_ms: i64,
    },

    /// A request could not be honoured. The socket stays open, because a bad seek is not a reason to
    /// drop a listener who is otherwise happily connected.
    #[serde(rename_all = "camelCase")]
    Error { code: String, message: String },

    /// Everybody connected right now. Sent only to a session that may see it (an admin, or anybody on an
    /// open recorder): once on connect, then whenever somebody arrives, leaves, or moves between live,
    /// history and paused.
    #[serde(rename_all = "camelCase")]
    Listeners { listeners: Vec<ListenerView> },

    /// The session may no longer see the list, because its account stopped being an admin or accounts were
    /// switched on. The client drops the copy it has.
    ListenersHidden,
}

/// One connection, as the admin's listener list shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenerView {
    pub id: u64,
    /// `None` for a guest, on an open recorder.
    pub email: Option<String>,
    pub role: Option<crate::models::Role>,
    pub address: String,
    pub user_agent: Option<String>,
    pub connected_at_ms: i64,
    /// `live`, `playback` or `paused`.
    pub activity: StreamMode,
    /// Where a listener in history started or last jumped to. `None` unless the activity is `playback`.
    pub from_ms: Option<i64>,
    /// `idle`, `playing` or `paused`: whether the person is hearing it, as their browser reports.
    pub player: crate::models::PlayerState,
}

impl From<crate::models::ListenerEntry> for ListenerView {
    fn from(entry: crate::models::ListenerEntry) -> Self {
        use crate::models::ListenerActivity;

        let (activity, from_ms) = match entry.activity {
            ListenerActivity::Live => (StreamMode::Live, None),
            ListenerActivity::Playback { from_ms } => (StreamMode::Playback, Some(from_ms)),
            ListenerActivity::Paused => (StreamMode::Paused, None),
        };
        let (email, role) = match entry.account {
            Some(account) => (Some(account.email), Some(account.role)),
            None => (None, None),
        };
        Self {
            id: entry.id,
            email,
            role,
            address: entry.address.to_string(),
            user_agent: entry.user_agent,
            connected_at_ms: entry.connected_at_ms,
            activity,
            from_ms,
            player: entry.player,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ClientMessage {
    /// Jump to the live edge and follow it.
    Live,

    #[serde(rename_all = "camelCase")]
    Seek {
        timestamp_ms: i64,
    },

    Pause,

    Resume,

    /// Play history faster or slower. Ignored while following the live feed, which is always real time.
    #[serde(rename_all = "camelCase")]
    Speed {
        value: f32,
    },

    #[serde(rename_all = "camelCase")]
    Ping {
        client_time_ms: i64,
    },

    /// Whether the person is hearing the stream: sent by the UI when its socket opens and whenever play or
    /// pause is pressed. Changes nothing about what is streamed; it only feeds the admin listener list.
    Player {
        state: crate::models::PlayerState,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_messages_use_kebab_case_types_and_camel_case_fields() {
        let json = serde_json::to_string(&ServerMessage::SwitchedToLive {
            timestamp_ms: 1_757_034_000_000,
        })
        .expect("serialise");

        assert_eq!(
            json,
            r#"{"type":"switched-to-live","timestampMs":1757034000000}"#
        );
    }

    #[test]
    fn stream_info_carries_the_format_and_the_bounds() {
        let json = serde_json::to_value(ServerMessage::StreamInfo {
            sample_rate: 48_000,
            channels: 1,
            frame_ms: 100,
            mode: StreamMode::Live,
            server_time_ms: 10,
            live_edge_ms: Some(9),
            earliest_ms: None,
            capturing: true,
        })
        .expect("serialise");

        assert_eq!(json["type"], "stream-info");
        assert_eq!(json["sampleRate"], 48_000);
        assert_eq!(json["mode"], "live");
        assert!(json["earliestMs"].is_null());
    }

    #[test]
    fn client_messages_parse_from_the_documented_shapes() {
        let seek: ClientMessage =
            serde_json::from_str(r#"{"type":"seek","timestampMs":1757031000000}"#).expect("parse");
        assert!(matches!(
            seek,
            ClientMessage::Seek {
                timestamp_ms: 1_757_031_000_000
            }
        ));

        let live: ClientMessage = serde_json::from_str(r#"{"type":"live"}"#).expect("parse");
        assert!(matches!(live, ClientMessage::Live));

        let ping: ClientMessage =
            serde_json::from_str(r#"{"type":"ping","clientTimeMs":5}"#).expect("parse");
        assert!(matches!(ping, ClientMessage::Ping { client_time_ms: 5 }));

        let player: ClientMessage =
            serde_json::from_str(r#"{"type":"player","state":"idle"}"#).expect("parse");
        assert!(matches!(
            player,
            ClientMessage::Player {
                state: crate::models::PlayerState::Idle
            }
        ));
        assert!(
            serde_json::from_str::<ClientMessage>(r#"{"type":"player","state":"loud"}"#).is_err()
        );
    }

    #[test]
    fn speed_round_trips_in_both_directions() {
        let request: ClientMessage =
            serde_json::from_str(r#"{"type":"speed","value":2.0}"#).expect("parse");
        assert!(matches!(request, ClientMessage::Speed { value } if value == 2.0));

        let applied =
            serde_json::to_string(&ServerMessage::Speed { value: 1.5 }).expect("serialise");
        assert_eq!(applied, r#"{"type":"speed","value":1.5}"#);
    }

    #[test]
    fn unknown_client_messages_are_rejected_rather_than_ignored() {
        assert!(serde_json::from_str::<ClientMessage>(r#"{"type":"explode"}"#).is_err());
        assert!(serde_json::from_str::<ClientMessage>(r#"{"type":"seek"}"#).is_err());
    }
}
