//! One WebSocket connection, modelled as an actor with a small state machine.
//!
//! Each connection owns a task and nothing else. There is no shared mutable state between sessions, so a
//! listener on a congested Wi-Fi link cannot slow the recorder or another listener down: it simply falls
//! behind on its own broadcast receiver and is resynchronised.
//!
//! The state machine has three states and every transition is explicit:
//!
//! ```text
//!            seek                     end of disk while capturing
//!   Live  ----------->  Playback  ---------------------------------->  Live
//!    ^                    |  ^                                          ^
//!    |  live              |  | resume                                   |
//!    +--------------------+  |                                          |
//!    |                    pause                                         |
//!    +---------------  Paused  ---------------------------------------- +
//! ```
//!
//! DVR code that tracks this with a pair of booleans always ends up with a state nobody meant to allow,
//! such as paused and live at the same time, so the mode is one value and the transitions are functions.

use axum::extract::ws::{Message, WebSocket};
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::Receiver;

use crate::app::AppState;
use crate::models::AudioFrame;
use crate::services::{CursorOutput, PlaybackCursor};
use crate::util::time::now_ms;
use crate::ws::messages::{ClientMessage, ServerMessage, StreamMode};
use crate::ws::protocol::encode_audio_frame;

/// Frames pushed straight after a seek, before real time pacing takes over.
///
/// The browser holds a jitter buffer, and filling it from an empty state at one frame per frame period
/// would mean the buffer's worth of silence after every scrub. Three frames covers the default buffer
/// almost exactly, so a seek is heard immediately.
const PREBUFFER_FRAMES: usize = 3;

/// Playback speeds offered, slowest first.
///
/// A fixed ladder rather than a free number: the client shows these as buttons, and an arbitrary speed
/// would let a typo ask for a thousand times real time and pin a core reading the disk.
pub const PLAYBACK_SPEEDS: [f32; 6] = [0.25, 0.5, 1.0, 1.5, 2.0, 4.0];

/// Never tick faster than this, whatever the speed and frame size work out to.
const MIN_TICK: std::time::Duration = std::time::Duration::from_millis(5);

/// How often an open stream checks that its listener is still allowed to hear it.
///
/// HTTP requests are checked one by one, but a socket is checked only at the handshake, so without this
/// a removed account, a signed out session, or a page that connected before accounts were switched on
/// would keep hearing the microphone for as long as the tab stayed open. Fifteen seconds bounds that
/// without adding a database lookup per frame.
const ACCESS_RECHECK: std::time::Duration = std::time::Duration::from_secs(15);

/// Snap a requested speed onto the nearest offered one.
fn clamp_speed(value: f32) -> f32 {
    if !value.is_finite() {
        return 1.0;
    }

    PLAYBACK_SPEEDS
        .into_iter()
        .min_by(|left, right| {
            (left - value)
                .abs()
                .partial_cmp(&(right - value).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(1.0)
}

/// How often the playback cursor should tick to deliver `frame_ms` of audio at `speed`.
///
/// Speed is expressed as pacing rather than as anything done to the samples: at double speed the server
/// simply hands over frames twice as often, and the client plays each one twice as fast. That keeps the
/// cursor, the segment index and the wire format entirely speed agnostic.
fn tick_period(frame_ms: u32, speed: f32) -> std::time::Duration {
    let millis = (frame_ms as f32 / speed.max(0.05)).round().max(1.0) as u64;
    std::time::Duration::from_millis(millis).max(MIN_TICK)
}

type Sink = SplitSink<WebSocket, Message>;

/// Outcome of driving playback forward, which decides what the loop does next.
enum PlaybackStep {
    /// Frames were produced, stay in playback.
    Continue,
    /// The disk ran out while capture is running, so the listener should rejoin the live feed.
    CaughtUp,
    /// The disk ran out and nothing is being recorded, so there is nothing to wait for.
    Exhausted,
    /// The client is gone.
    SocketClosed,
}

pub struct StreamSession {
    state: Arc<AppState>,
    /// The session cookie the socket was opened with, re-checked every [`ACCESS_RECHECK`].
    token: Option<String>,
}

impl StreamSession {
    pub fn new(state: Arc<AppState>, token: Option<String>) -> Self {
        Self { state, token }
    }

    /// Drive the connection until the client disconnects or loses access.
    pub async fn run(self, socket: WebSocket) {
        let state = self.state;
        let token = self.token;
        let (mut sink, mut source) = socket.split();

        let mut access_check = tokio::time::interval(ACCESS_RECHECK);
        // The first tick of an interval fires at once; the handshake has only just been checked.
        access_check.tick().await;

        let mut mode = StreamMode::Live;
        // Where a resume should return to. A listener who paused during playback wants their position
        // back, not a jump to live.
        let mut resume_mode = StreamMode::Live;
        let mut live_rx: Option<Receiver<AudioFrame>> = Some(state.hub.subscribe());
        let mut cursor: Option<PlaybackCursor> = None;

        let frame_ms = effective_frame_ms(&state);
        let mut speed = 1.0f32;
        let mut ticker = tokio::time::interval(tick_period(frame_ms, speed));
        // A stalled network must not make the cursor sprint to catch up afterwards, which would deliver a
        // burst of audio the client cannot play in order.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        if !send_message(&mut sink, stream_info(&state, mode)).await {
            return;
        }

        tracing::debug!(
            listeners = state.hub.listener_count(),
            "stream session opened"
        );

        loop {
            let event = tokio::select! {
                incoming = source.next() => Event::Incoming(incoming),
                frame = receive_live(&mut live_rx), if mode == StreamMode::Live => Event::Live(frame),
                _ = ticker.tick(), if mode == StreamMode::Playback => Event::Tick,
                _ = access_check.tick() => Event::AccessCheck,
            };

            match event {
                Event::AccessCheck => {
                    if !still_allowed(&state, token.as_deref()) {
                        tracing::debug!("closing a stream whose listener is no longer signed in");
                        let _ = sink.send(Message::Close(None)).await;
                        break;
                    }
                }
                Event::Incoming(None) => break,
                Event::Incoming(Some(Err(error))) => {
                    tracing::debug!(%error, "stream session read failed");
                    break;
                }
                Event::Incoming(Some(Ok(message))) => {
                    match message {
                        Message::Close(_) => break,
                        Message::Text(text) => {
                            let command = match serde_json::from_str::<ClientMessage>(text.as_str())
                            {
                                Ok(command) => command,
                                Err(error) => {
                                    let rejected = ServerMessage::Error {
                                        code: "bad_request".to_string(),
                                        message: format!(
                                            "could not parse the control message: {error}"
                                        ),
                                    };
                                    if !send_message(&mut sink, rejected).await {
                                        break;
                                    }
                                    continue;
                                }
                            };

                            let outcome = handle_command(
                                command,
                                &state,
                                &mut sink,
                                &mut mode,
                                &mut resume_mode,
                                &mut live_rx,
                                &mut cursor,
                                frame_ms,
                                &mut speed,
                                &mut ticker,
                            )
                            .await;

                            if !outcome {
                                break;
                            }
                        }
                        // Ping and Pong are answered by axum itself, and binary input is not part of the
                        // protocol, so both are ignored rather than treated as an error.
                        _ => {}
                    }
                }
                Event::Live(None) => break,
                Event::Live(Some(Err(RecvError::Closed))) => {
                    // Capture stopped. Stay connected: the operator may start it again, and dropping the
                    // socket would make the UI look broken.
                    live_rx = None;
                    if !send_message(&mut sink, stream_info(&state, mode)).await {
                        break;
                    }
                }
                Event::Live(Some(Err(RecvError::Lagged(missed)))) => {
                    // The receiver has already skipped to the newest frames, which for live audio is
                    // exactly the right recovery: be late by nothing rather than by everything.
                    tracing::debug!(missed, "live listener fell behind and was resynchronised");
                }
                Event::Live(Some(Ok(frame))) => {
                    if !send_audio(&mut sink, &state, &frame).await {
                        break;
                    }
                    let level = ServerMessage::Level {
                        rms: frame.rms,
                        peak: frame.peak,
                    };
                    if !send_message(&mut sink, level).await {
                        break;
                    }
                }
                Event::Tick => {
                    let Some(active) = cursor.as_mut() else {
                        mode = StreamMode::Live;
                        continue;
                    };

                    match drive_playback(&mut sink, active, &state, 1).await {
                        PlaybackStep::Continue => {}
                        PlaybackStep::SocketClosed => break,
                        PlaybackStep::CaughtUp => {
                            cursor = None;
                            mode = StreamMode::Live;
                            resume_mode = StreamMode::Live;
                            live_rx = Some(state.hub.subscribe());

                            // The live feed arrives in real time, so any other speed is meaningless.
                            if speed != 1.0 {
                                speed = 1.0;
                                ticker = tokio::time::interval(tick_period(frame_ms, speed));
                                if !send_message(&mut sink, ServerMessage::Speed { value: speed })
                                    .await
                                {
                                    break;
                                }
                            }

                            let edge = state.live_edge_ms().unwrap_or_else(now_ms);
                            if !send_message(
                                &mut sink,
                                ServerMessage::SwitchedToLive { timestamp_ms: edge },
                            )
                            .await
                            {
                                break;
                            }
                            if !send_message(&mut sink, mode_message(mode, edge)).await {
                                break;
                            }
                        }
                        PlaybackStep::Exhausted => {
                            let at_ms = active.position_ms();
                            mode = StreamMode::Paused;
                            resume_mode = StreamMode::Playback;
                            if !send_message(
                                &mut sink,
                                ServerMessage::EndOfRecording {
                                    timestamp_ms: at_ms,
                                },
                            )
                            .await
                            {
                                break;
                            }
                            if !send_message(&mut sink, mode_message(mode, at_ms)).await {
                                break;
                            }
                        }
                    }
                }
            }
        }

        tracing::debug!("stream session closed");
    }
}

enum Event {
    Incoming(Option<Result<Message, axum::Error>>),
    Live(Option<Result<AudioFrame, RecvError>>),
    Tick,
    AccessCheck,
}

/// Whether the listener behind a socket may still hear it: always without accounts, and with accounts
/// only while their session is valid. A database hiccup keeps the stream up rather than cutting off a
/// listener over something that is not their fault.
fn still_allowed(state: &AppState, token: Option<&str>) -> bool {
    let needed = crate::models::Access::Listen;
    match state.auth.mode() {
        Ok(mode) if mode.requires_login() => match state.auth.resolve(token) {
            Ok(user) => crate::models::authorize(mode, user.as_ref(), needed).is_ok(),
            Err(_) => true,
        },
        Ok(_) => true,
        Err(_) => true,
    }
}

/// Apply one client command. Returns false when the socket should be closed.
#[allow(clippy::too_many_arguments)]
async fn handle_command(
    command: ClientMessage,
    state: &Arc<AppState>,
    sink: &mut Sink,
    mode: &mut StreamMode,
    resume_mode: &mut StreamMode,
    live_rx: &mut Option<Receiver<AudioFrame>>,
    cursor: &mut Option<PlaybackCursor>,
    frame_ms: u32,
    speed: &mut f32,
    ticker: &mut tokio::time::Interval,
) -> bool {
    match command {
        ClientMessage::Live => {
            *cursor = None;
            *mode = StreamMode::Live;
            *resume_mode = StreamMode::Live;
            *live_rx = Some(state.hub.subscribe());

            if *speed != 1.0 {
                *speed = 1.0;
                *ticker = tokio::time::interval(tick_period(frame_ms, *speed));
                if !send_message(sink, ServerMessage::Speed { value: *speed }).await {
                    return false;
                }
            }

            let edge = state.live_edge_ms().unwrap_or_else(now_ms);
            send_message(sink, mode_message(*mode, edge)).await
        }

        ClientMessage::Speed { value } => {
            let applied = clamp_speed(value);
            if applied != *speed {
                *speed = applied;
                // Replacing the interval rather than resetting it, because the period itself changed.
                *ticker = tokio::time::interval(tick_period(frame_ms, applied));
            }

            send_message(sink, ServerMessage::Speed { value: applied }).await
        }

        ClientMessage::Seek { timestamp_ms } => {
            let mut active = state.playback.cursor_at(timestamp_ms, frame_ms);
            *mode = StreamMode::Playback;
            *resume_mode = StreamMode::Playback;
            // Drop the live subscription while scrubbing so the connection is not also buffering audio
            // the listener is not hearing.
            *live_rx = None;
            ticker.reset();

            if !send_message(sink, mode_message(*mode, timestamp_ms)).await {
                return false;
            }

            let step = drive_playback(sink, &mut active, state, PREBUFFER_FRAMES).await;
            *cursor = Some(active);

            match step {
                PlaybackStep::SocketClosed => false,
                PlaybackStep::CaughtUp => {
                    *cursor = None;
                    *mode = StreamMode::Live;
                    *resume_mode = StreamMode::Live;
                    *live_rx = Some(state.hub.subscribe());
                    let edge = state.live_edge_ms().unwrap_or_else(now_ms);
                    send_message(sink, ServerMessage::SwitchedToLive { timestamp_ms: edge }).await
                }
                PlaybackStep::Exhausted => {
                    *mode = StreamMode::Paused;
                    send_message(
                        sink,
                        ServerMessage::EndOfRecording {
                            timestamp_ms: timestamp_ms
                                .max(state.live_edge_ms().unwrap_or(timestamp_ms)),
                        },
                    )
                    .await
                }
                PlaybackStep::Continue => true,
            }
        }

        ClientMessage::Pause => {
            if *mode != StreamMode::Paused {
                *resume_mode = *mode;
            }
            *mode = StreamMode::Paused;
            *live_rx = None;

            let position = cursor
                .as_ref()
                .map(|active| active.position_ms())
                .or_else(|| state.live_edge_ms())
                .unwrap_or_else(now_ms);
            send_message(sink, mode_message(*mode, position)).await
        }

        ClientMessage::Resume => {
            *mode = *resume_mode;
            if *mode == StreamMode::Live {
                *cursor = None;
                *live_rx = Some(state.hub.subscribe());
            } else {
                ticker.reset();
            }

            let position = cursor
                .as_ref()
                .map(|active| active.position_ms())
                .or_else(|| state.live_edge_ms())
                .unwrap_or_else(now_ms);
            send_message(sink, mode_message(*mode, position)).await
        }

        ClientMessage::Ping { client_time_ms } => {
            send_message(
                sink,
                ServerMessage::Pong {
                    client_time_ms,
                    server_time_ms: now_ms(),
                },
            )
            .await
        }
    }
}

/// Emit up to `steps` playback frames.
async fn drive_playback(
    sink: &mut Sink,
    cursor: &mut PlaybackCursor,
    state: &Arc<AppState>,
    steps: usize,
) -> PlaybackStep {
    for _ in 0..steps {
        let output = match cursor.advance() {
            Ok(output) => output,
            Err(error) => {
                tracing::error!(%error, "playback cursor failed");
                let message = ServerMessage::Error {
                    code: "internal".to_string(),
                    message: "could not read the recording".to_string(),
                };
                return if send_message(sink, message).await {
                    PlaybackStep::Exhausted
                } else {
                    PlaybackStep::SocketClosed
                };
            }
        };

        match output {
            CursorOutput::Frame(frame) => {
                if !send_audio(sink, state, &frame).await {
                    return PlaybackStep::SocketClosed;
                }
            }
            CursorOutput::Gap { from_ms, to_ms } => {
                if !send_message(sink, ServerMessage::Gap { from_ms, to_ms }).await {
                    return PlaybackStep::SocketClosed;
                }
            }
            CursorOutput::EndOfRecording { .. } => {
                return if state.capture.is_active() {
                    PlaybackStep::CaughtUp
                } else {
                    PlaybackStep::Exhausted
                };
            }
        }
    }

    PlaybackStep::Continue
}

/// Wait for the next live frame, or forever when there is no subscription.
///
/// Returning a never resolving future rather than `None` keeps the `select!` branch honest: without a
/// subscription the branch simply never fires, instead of spinning the loop.
async fn receive_live(
    live_rx: &mut Option<Receiver<AudioFrame>>,
) -> Option<Result<AudioFrame, RecvError>> {
    match live_rx {
        Some(receiver) => Some(receiver.recv().await),
        None => std::future::pending().await,
    }
}

async fn send_audio(sink: &mut Sink, state: &Arc<AppState>, frame: &AudioFrame) -> bool {
    let encoded = encode_audio_frame(frame, state.encoder.as_ref());
    sink.send(Message::Binary(encoded.into())).await.is_ok()
}

async fn send_message(sink: &mut Sink, message: ServerMessage) -> bool {
    match serde_json::to_string(&message) {
        Ok(json) => sink.send(Message::Text(json.into())).await.is_ok(),
        Err(error) => {
            tracing::error!(%error, "could not serialise a control message");
            true
        }
    }
}

fn mode_message(mode: StreamMode, position_ms: i64) -> ServerMessage {
    ServerMessage::Mode { mode, position_ms }
}

fn stream_info(state: &Arc<AppState>, mode: StreamMode) -> ServerMessage {
    let snapshot = state.capture.snapshot();

    ServerMessage::StreamInfo {
        // Before the first capture there is no negotiated rate, so report the format the next session
        // will most likely use rather than zero, which a client cannot build an audio buffer from.
        sample_rate: if snapshot.sample_rate > 0 {
            snapshot.sample_rate
        } else {
            48_000
        },
        channels: snapshot.channels.max(1),
        frame_ms: effective_frame_ms(state),
        mode,
        server_time_ms: now_ms(),
        live_edge_ms: state.live_edge_ms(),
        earliest_ms: state.playback.earliest_ms().ok().flatten(),
        capturing: snapshot.state.is_active(),
    }
}

/// Frame size in force: whatever capture negotiated, or the configured value before it starts.
fn effective_frame_ms(state: &Arc<AppState>) -> u32 {
    let snapshot = state.capture.snapshot();
    if snapshot.frame_ms > 0 {
        snapshot.frame_ms
    } else {
        state.settings.current().frame_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::models::Role;

    /// A full application on a throwaway data directory, removed when the guard drops.
    struct TempState {
        state: Arc<AppState>,
        data_dir: std::path::PathBuf,
    }

    impl Drop for TempState {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.data_dir);
        }
    }

    fn temp_state(name: &str) -> TempState {
        let data_dir =
            std::env::temp_dir().join(format!("oar-session-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data_dir);
        let config = AppConfig {
            data_dir: data_dir.clone(),
            static_dir: data_dir.join("no-ui"),
            ..AppConfig::default()
        };
        TempState {
            state: AppState::bootstrap(config).expect("bootstrap"),
            data_dir,
        }
    }

    #[test]
    fn an_open_or_undecided_stream_is_always_allowed() {
        let temp = temp_state("open");
        assert!(still_allowed(&temp.state, None));

        temp.state.auth.choose_open().expect("open");
        assert!(still_allowed(&temp.state, None));
    }

    #[test]
    fn a_stream_opened_before_accounts_were_switched_on_is_cut_off() {
        let temp = temp_state("switched-on");
        assert!(still_allowed(&temp.state, None));

        temp.state
            .auth
            .set_up("owner@example.com", "a long password")
            .expect("setup");
        assert!(!still_allowed(&temp.state, None));
    }

    #[test]
    fn a_stream_lasts_exactly_as_long_as_its_session() {
        let temp = temp_state("session");
        let auth = &temp.state.auth;
        auth.set_up("owner@example.com", "a long password")
            .expect("setup");
        let listener = auth
            .create_user("kitchen@example.com", "listen only", Role::Listener)
            .expect("listener");
        let client = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

        let signed_in = auth
            .log_in(client, "kitchen@example.com", "listen only")
            .expect("login");
        assert!(still_allowed(&temp.state, Some(&signed_in.token)));

        auth.log_out(&signed_in.token).expect("logout");
        assert!(!still_allowed(&temp.state, Some(&signed_in.token)));

        let again = auth
            .log_in(client, "kitchen@example.com", "listen only")
            .expect("second login");
        assert!(still_allowed(&temp.state, Some(&again.token)));

        auth.delete_user(listener.id).expect("remove");
        assert!(!still_allowed(&temp.state, Some(&again.token)));
    }

    #[test]
    fn speeds_snap_to_the_offered_ladder() {
        assert_eq!(clamp_speed(1.0), 1.0);
        assert_eq!(clamp_speed(2.0), 2.0);
        assert_eq!(clamp_speed(1.9), 2.0);
        assert_eq!(clamp_speed(0.3), 0.25);
        // Absurd requests land on the extremes rather than pinning a core.
        assert_eq!(clamp_speed(1000.0), 4.0);
        assert_eq!(clamp_speed(0.0), 0.25);
        assert_eq!(clamp_speed(-5.0), 0.25);
        assert_eq!(clamp_speed(f32::NAN), 1.0);
    }

    #[test]
    fn the_tick_period_scales_inversely_with_speed() {
        assert_eq!(tick_period(100, 1.0), std::time::Duration::from_millis(100));
        assert_eq!(tick_period(100, 2.0), std::time::Duration::from_millis(50));
        assert_eq!(tick_period(100, 4.0), std::time::Duration::from_millis(25));
        // Slower than real time means waiting longer between frames.
        assert_eq!(tick_period(100, 0.5), std::time::Duration::from_millis(200));
    }

    #[test]
    fn the_tick_period_never_collapses_to_a_busy_loop() {
        // A short frame at the fastest speed still leaves room between ticks.
        assert!(tick_period(20, 4.0) >= MIN_TICK);
        assert!(tick_period(1, 4.0) >= MIN_TICK);
    }
}
