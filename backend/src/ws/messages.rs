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

    mod props {
        use super::*;
        use crate::models::{ListenerAccount, ListenerActivity, ListenerEntry, PlayerState, Role};
        use proptest::prelude::*;
        use std::net::{IpAddr, Ipv4Addr};

        fn mode() -> impl Strategy<Value = StreamMode> {
            prop_oneof![
                Just(StreamMode::Live),
                Just(StreamMode::Playback),
                Just(StreamMode::Paused),
            ]
        }

        fn player() -> impl Strategy<Value = PlayerState> {
            prop_oneof![
                Just(PlayerState::Idle),
                Just(PlayerState::Playing),
                Just(PlayerState::Paused),
            ]
        }

        fn entry() -> impl Strategy<Value = ListenerEntry> {
            let account = proptest::option::of(("[a-z]{1,8}@[a-z]{1,8}", any::<bool>()).prop_map(
                |(email, admin)| ListenerAccount {
                    email,
                    role: if admin { Role::Admin } else { Role::Listener },
                },
            ));
            let activity = prop_oneof![
                Just(ListenerActivity::Live),
                any::<i64>().prop_map(|from_ms| ListenerActivity::Playback { from_ms }),
                Just(ListenerActivity::Paused),
            ];
            (
                any::<u64>(),
                account,
                any::<u32>(),
                proptest::option::of(".{0,20}"),
                any::<i64>(),
                activity,
                player(),
            )
                .prop_map(
                    |(id, account, address, user_agent, connected_at_ms, activity, player)| {
                        ListenerEntry {
                            id,
                            account,
                            address: IpAddr::V4(Ipv4Addr::from(address)),
                            user_agent,
                            connected_at_ms,
                            activity,
                            player,
                        }
                    },
                )
        }

        /// Every variant, with any values its fields can hold, including non finite levels.
        fn server_message() -> impl Strategy<Value = ServerMessage> {
            let optional_ms = || proptest::option::of(any::<i64>());
            prop_oneof![
                (
                    any::<u32>(),
                    any::<u16>(),
                    any::<u32>(),
                    mode(),
                    any::<i64>(),
                    optional_ms(),
                    optional_ms(),
                    any::<bool>(),
                )
                    .prop_map(
                        |(
                            sample_rate,
                            channels,
                            frame_ms,
                            mode,
                            server_time_ms,
                            live_edge_ms,
                            earliest_ms,
                            capturing,
                        )| ServerMessage::StreamInfo {
                            sample_rate,
                            channels,
                            frame_ms,
                            mode,
                            server_time_ms,
                            live_edge_ms,
                            earliest_ms,
                            capturing,
                        },
                    ),
                (mode(), any::<i64>())
                    .prop_map(|(mode, position_ms)| ServerMessage::Mode { mode, position_ms }),
                any::<i64>()
                    .prop_map(|timestamp_ms| ServerMessage::SwitchedToLive { timestamp_ms }),
                (any::<i64>(), any::<i64>())
                    .prop_map(|(from_ms, to_ms)| ServerMessage::Gap { from_ms, to_ms }),
                any::<i64>()
                    .prop_map(|timestamp_ms| ServerMessage::EndOfRecording { timestamp_ms }),
                (any::<f32>(), any::<f32>())
                    .prop_map(|(rms, peak)| ServerMessage::Level { rms, peak }),
                any::<f32>().prop_map(|value| ServerMessage::Speed { value }),
                (any::<i64>(), any::<i64>()).prop_map(|(client_time_ms, server_time_ms)| {
                    ServerMessage::Pong {
                        client_time_ms,
                        server_time_ms,
                    }
                }),
                (".{0,16}", ".{0,32}")
                    .prop_map(|(code, message)| ServerMessage::Error { code, message }),
                proptest::collection::vec(entry(), 0..4).prop_map(|entries| {
                    ServerMessage::Listeners {
                        listeners: entries.into_iter().map(ListenerView::from).collect(),
                    }
                }),
                Just(ServerMessage::ListenersHidden),
            ]
        }

        /// Every key in a JSON value, however deeply nested.
        fn keys(value: &serde_json::Value, found: &mut Vec<String>) {
            match value {
                serde_json::Value::Object(map) => {
                    for (key, inner) in map {
                        found.push(key.clone());
                        keys(inner, found);
                    }
                }
                serde_json::Value::Array(items) => {
                    for inner in items {
                        keys(inner, found);
                    }
                }
                _ => {}
            }
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

            /// A socket hands the session whatever text a client sent, so parsing must answer for any
            /// string at all. Half the inputs are objects with a real `type` and a field of any JSON kind,
            /// which is where a wrong shape would reach the field parsers rather than stop at the tag.
            #[test]
            fn any_text_parses_or_is_refused_without_panicking(
                text in ".{0,64}",
                kind in prop_oneof![
                    Just("live"), Just("seek"), Just("pause"), Just("resume"),
                    Just("speed"), Just("ping"), Just("player"),
                ],
                field in prop_oneof![
                    Just("timestampMs"), Just("value"), Just("clientTimeMs"), Just("state"),
                ],
                raw in prop_oneof![
                    Just("null".to_string()),
                    Just("true".to_string()),
                    Just("1e400".to_string()),
                    Just("-9223372036854775809".to_string()),
                    Just("[]".to_string()),
                    Just("{}".to_string()),
                    any::<f64>().prop_map(|number| number.to_string()),
                    any::<i64>().prop_map(|number| number.to_string()),
                    ".{0,8}".prop_map(|text| serde_json::Value::String(text).to_string()),
                ],
            ) {
                let _ = serde_json::from_str::<ClientMessage>(&text);
                let shaped = format!(r#"{{"type":"{kind}","{field}":{raw}}}"#);
                let _ = serde_json::from_str::<ClientMessage>(&shaped);
            }

            /// The documented shapes carry any value a field can hold through unchanged: a seek to a
            /// moment far in the past, a ping with a negative clock, any finite speed. A lossy parse here
            /// would put a listener somewhere other than where they asked to be.
            #[test]
            fn documented_client_shapes_carry_any_value(
                timestamp_ms in any::<i64>(),
                client_time_ms in any::<i64>(),
                speed in any::<f32>().prop_filter("JSON has no non finite numbers", |value| value.is_finite()),
                state in player(),
            ) {
                let seek = format!(r#"{{"type":"seek","timestampMs":{timestamp_ms}}}"#);
                let parsed = serde_json::from_str::<ClientMessage>(&seek).expect("seek");
                let recognised = matches!(parsed, ClientMessage::Seek { timestamp_ms: got } if got == timestamp_ms);
                prop_assert!(recognised, "{:?}", parsed);

                let ping = format!(r#"{{"type":"ping","clientTimeMs":{client_time_ms}}}"#);
                let parsed = serde_json::from_str::<ClientMessage>(&ping).expect("ping");
                let recognised = matches!(parsed, ClientMessage::Ping { client_time_ms: got } if got == client_time_ms);
                prop_assert!(recognised, "{:?}", parsed);

                let request = serde_json::json!({ "type": "speed", "value": speed }).to_string();
                let parsed = serde_json::from_str::<ClientMessage>(&request).expect("speed");
                let recognised = matches!(parsed, ClientMessage::Speed { value } if value == speed);
                prop_assert!(recognised, "{:?}", parsed);

                let request = serde_json::json!({ "type": "player", "state": state }).to_string();
                let parsed = serde_json::from_str::<ClientMessage>(&request).expect("player");
                let recognised = matches!(parsed, ClientMessage::Player { state: got } if got == state);
                prop_assert!(recognised, "{:?}", parsed);
            }

            /// Every server message, whatever it holds, serialises to an object tagged with a kebab case
            /// `type` and with camel case keys all the way down. The UI reads fields by their camel case
            /// names, so a variant that forgot its `rename_all` would arrive as a field the browser never
            /// sees, and a non finite level must not make serialisation fail and drop the message.
            #[test]
            fn every_server_message_serialises_to_the_wire_naming(message in server_message()) {
                let value = serde_json::to_value(&message).expect("serialise");
                let tag = value["type"].as_str().expect("a type tag").to_string();
                prop_assert!(!tag.is_empty());
                prop_assert!(tag.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "tag {}", tag);

                let mut found = Vec::new();
                keys(&value, &mut found);
                for key in found {
                    prop_assert!(!key.contains('_') && !key.contains('-'), "key {} in {}", key, tag);
                    prop_assert!(key.chars().next().is_some_and(|c| c.is_ascii_lowercase()), "key {}", key);
                }
            }

            /// The listener list's two derived pairings cannot disagree: a starting point is shown only for
            /// somebody in history, and an email never appears without its role or the other way round.
            #[test]
            fn a_listener_view_keeps_its_pairings(entry in entry()) {
                let in_history = matches!(entry.activity, ListenerActivity::Playback { .. });
                let view = ListenerView::from(entry);
                prop_assert_eq!(view.from_ms.is_some(), in_history);
                prop_assert_eq!(view.activity == StreamMode::Playback, in_history);
                prop_assert_eq!(view.email.is_some(), view.role.is_some());
            }
        }
    }
}
