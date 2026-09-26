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
use std::net::IpAddr;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::Receiver;

use crate::app::AppState;
use crate::models::{AudioFrame, ListenerAccount, ListenerActivity};
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

/// How long a client may go without being heard from before its session is closed.
///
/// A tab that closes says so and leaves at once, but a device that vanishes (a phone losing Wi-Fi, a laptop
/// shut mid stream) says nothing, and TCP can take many minutes to give up on it, leaving a ghost in the
/// listener list and a task streaming into the void. The server pings on every [`ACCESS_RECHECK`] and every
/// browser and WebSocket library answers a ping by itself, below JavaScript, so a live client is heard at
/// least that often even from a throttled background tab. Three missed rounds is gone, not slow.
const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

/// How long one send may wait for the network before the client is treated as gone.
///
/// The idle check alone is not enough: a vanished client stops draining the socket, the send buffer fills
/// within seconds of live audio, and the next send then waits forever inside a branch of the loop, where
/// no timer can reach it. A healthy client drains 100 ms of audio in far less than this, so a send stuck
/// this long is a connection that is not coming back.
const SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Whether a client last heard from at `last_heard` has been silent too long by `now`.
fn is_silent(last_heard: tokio::time::Instant, now: tokio::time::Instant) -> bool {
    now.saturating_duration_since(last_heard) >= IDLE_TIMEOUT
}

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

/// Who is on the other end of a socket, for the listener list.
pub struct ListenerIdentity {
    /// `None` for a guest on an open recorder.
    pub account: Option<ListenerAccount>,
    pub address: IpAddr,
    pub user_agent: Option<String>,
}

pub struct StreamSession {
    state: Arc<AppState>,
    /// The session cookie the socket was opened with, re-checked every [`ACCESS_RECHECK`].
    token: Option<String>,
    identity: ListenerIdentity,
}

impl StreamSession {
    pub fn new(state: Arc<AppState>, token: Option<String>, identity: ListenerIdentity) -> Self {
        Self {
            state,
            token,
            identity,
        }
    }

    /// Drive the connection until the client disconnects or loses access.
    pub async fn run(self, socket: WebSocket) {
        let state = self.state;
        let token = self.token;
        let (mut sink, mut source) = socket.split();

        let mut access_check = tokio::time::interval(ACCESS_RECHECK);
        // The first tick of an interval fires at once; the handshake has only just been checked.
        access_check.tick().await;

        // On the list for exactly as long as this function runs: the handle removes the entry when it is
        // dropped, however the session ends.
        let registered = state.listeners.register(
            self.identity.account,
            self.identity.address,
            self.identity.user_agent,
        );
        let mut reported = ListenerActivity::Live;
        // Set by a seek, so a jump within history is reported even though the mode stayed the same.
        let mut repositioned = false;
        let mut presence = state.listeners.subscribe();
        let mut may_watch = may_watch_listeners(&state, token.as_deref());
        let mut last_heard = tokio::time::Instant::now();

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
        if may_watch && !send_message(&mut sink, listeners_message(&state)).await {
            return;
        }

        tracing::debug!(listeners = state.listeners.count(), "stream session opened");

        loop {
            // Report what the previous turn changed before waiting again. Up here rather than at the end
            // of the loop, because several branches below `continue`, and a change they made must not be
            // missed. Only the kind of activity is compared: a playhead moves every frame.
            let activity = activity_of(mode, cursor.as_ref());
            if std::mem::discriminant(&activity) != std::mem::discriminant(&reported)
                || repositioned
            {
                registered.set_activity(activity);
                reported = activity;
                repositioned = false;
            }

            let event = tokio::select! {
                incoming = source.next() => Event::Incoming(incoming),
                frame = receive_live(&mut live_rx), if mode == StreamMode::Live => Event::Live(frame),
                _ = ticker.tick(), if mode == StreamMode::Playback => Event::Tick,
                _ = access_check.tick() => Event::AccessCheck,
                _ = presence.changed(), if may_watch => Event::Presence,
            };

            match event {
                Event::AccessCheck => {
                    if !still_allowed(&state, token.as_deref()) {
                        tracing::debug!("closing a stream whose listener is no longer signed in");
                        deliver(&mut sink, Message::Close(None)).await;
                        break;
                    }

                    if is_silent(last_heard, tokio::time::Instant::now()) {
                        // No close frame: nobody is reading, and it would only queue behind whatever is
                        // already stuck in the send buffer. Dropping the socket is the goodbye.
                        tracing::debug!("dropping a stream whose client has gone silent");
                        break;
                    }
                    // Answered by the client's WebSocket stack itself; the pong is what keeps it heard.
                    if !deliver(&mut sink, Message::Ping(Default::default())).await {
                        break;
                    }

                    // An admin made a listener, or accounts switched on, takes the list away; the reverse
                    // hands it over. Checked on the same beat as access itself.
                    let now_may_watch = may_watch_listeners(&state, token.as_deref());
                    if now_may_watch != may_watch {
                        may_watch = now_may_watch;
                        let update = if may_watch {
                            presence.borrow_and_update();
                            listeners_message(&state)
                        } else {
                            ServerMessage::ListenersHidden
                        };
                        if !send_message(&mut sink, update).await {
                            break;
                        }
                    }
                }
                Event::Presence => {
                    presence.borrow_and_update();
                    if !send_message(&mut sink, listeners_message(&state)).await {
                        break;
                    }
                }
                Event::Incoming(None) => break,
                Event::Incoming(Some(Err(error))) => {
                    tracing::debug!(%error, "stream session read failed");
                    break;
                }
                Event::Incoming(Some(Ok(message))) => {
                    // Anything at all counts, pongs to the server's pings included.
                    last_heard = tokio::time::Instant::now();
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

                            if matches!(command, ClientMessage::Seek { .. }) {
                                repositioned = true;
                            }
                            // Only the listener list cares, so it never reaches the transport.
                            if let ClientMessage::Player { state: player } = command {
                                registered.set_player(player);
                                continue;
                            }

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
    /// Somebody arrived, left, or changed what they are doing.
    Presence,
}

/// What a session is doing, for the listener list.
fn activity_of(mode: StreamMode, cursor: Option<&PlaybackCursor>) -> ListenerActivity {
    match (mode, cursor) {
        (StreamMode::Playback, Some(active)) => ListenerActivity::Playback {
            from_ms: active.position_ms(),
        },
        (StreamMode::Paused, _) => ListenerActivity::Paused,
        _ => ListenerActivity::Live,
    }
}

/// Whether a socket's user may see who else is listening: anybody on an open recorder, where everybody has
/// admin powers anyway, and only admins once there are accounts. Anything that cannot be worked out, such
/// as a database hiccup, answers no, because the list names people and says where they connect from.
fn may_watch_listeners(state: &AppState, token: Option<&str>) -> bool {
    let needed = crate::models::Access::Administer;
    match state.auth.mode() {
        Ok(mode) if mode.requires_login() => match state.auth.resolve(token) {
            Ok(user) => crate::models::authorize(mode, user.as_ref(), needed).is_ok(),
            Err(_) => false,
        },
        Ok(_) => true,
        Err(_) => false,
    }
}

fn listeners_message(state: &AppState) -> ServerMessage {
    ServerMessage::Listeners {
        listeners: state
            .listeners
            .snapshot()
            .into_iter()
            .map(Into::into)
            .collect(),
    }
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

        // Taken by the session loop before it gets here, since it touches the listener list and not the
        // transport. Kept as a no op rather than unreachable, so a future caller cannot panic the task.
        ClientMessage::Player { .. } => true,
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

/// Every write to the socket goes through here, so none can wait on a dead connection for longer than
/// [`SEND_TIMEOUT`]. False means the socket is finished and the session should end.
async fn deliver(sink: &mut Sink, message: Message) -> bool {
    match tokio::time::timeout(SEND_TIMEOUT, sink.send(message)).await {
        Ok(sent) => sent.is_ok(),
        Err(_) => {
            tracing::debug!("closing a stream whose client stopped reading");
            false
        }
    }
}

async fn send_audio(sink: &mut Sink, state: &Arc<AppState>, frame: &AudioFrame) -> bool {
    let encoded = encode_audio_frame(frame, state.encoder.as_ref());
    deliver(sink, Message::Binary(encoded.into())).await
}

async fn send_message(sink: &mut Sink, message: ServerMessage) -> bool {
    match serde_json::to_string(&message) {
        Ok(json) => deliver(sink, Message::Text(json.into())).await,
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
        /// An `Option` only so `drop` can close the database before deleting its folder, which Windows
        /// insists on: it will not delete a file that is still open.
        state: Option<Arc<AppState>>,
        data_dir: std::path::PathBuf,
    }

    impl TempState {
        fn state(&self) -> &AppState {
            self.state.as_deref().expect("state lives until drop")
        }
    }

    impl Drop for TempState {
        fn drop(&mut self) {
            drop(self.state.take());
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
            state: Some(AppState::bootstrap(config).expect("bootstrap")),
            data_dir,
        }
    }

    #[test]
    fn an_open_or_undecided_stream_is_always_allowed() {
        let temp = temp_state("open");
        assert!(still_allowed(temp.state(), None));

        temp.state().auth.choose_open().expect("open");
        assert!(still_allowed(temp.state(), None));
    }

    #[test]
    fn a_stream_opened_before_accounts_were_switched_on_is_cut_off() {
        let temp = temp_state("switched-on");
        assert!(still_allowed(temp.state(), None));

        temp.state()
            .auth
            .set_up("owner@example.com", "a long password")
            .expect("setup");
        assert!(!still_allowed(temp.state(), None));
    }

    #[test]
    fn a_stream_lasts_exactly_as_long_as_its_session() {
        let temp = temp_state("session");
        let auth = &temp.state().auth;
        auth.set_up("owner@example.com", "a long password")
            .expect("setup");
        let listener = auth
            .create_user("kitchen@example.com", "listen only", Role::Listener)
            .expect("listener");
        let client = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

        let signed_in = auth
            .log_in(client, "kitchen@example.com", "listen only")
            .expect("login")
            .signed_in()
            .expect("no second factor on this account");
        assert!(still_allowed(temp.state(), Some(&signed_in.token)));

        auth.log_out(&signed_in.token).expect("logout");
        assert!(!still_allowed(temp.state(), Some(&signed_in.token)));

        let again = auth
            .log_in(client, "kitchen@example.com", "listen only")
            .expect("second login")
            .signed_in()
            .expect("no second factor on this account");
        assert!(still_allowed(temp.state(), Some(&again.token)));

        auth.delete_user(listener.id).expect("remove");
        assert!(!still_allowed(temp.state(), Some(&again.token)));
    }

    #[test]
    fn everybody_sees_the_listener_list_on_an_open_recorder() {
        let temp = temp_state("watch-open");
        assert!(may_watch_listeners(temp.state(), None));

        temp.state().auth.choose_open().expect("open");
        assert!(may_watch_listeners(temp.state(), None));
    }

    #[test]
    fn with_accounts_only_admins_see_the_listener_list() {
        let temp = temp_state("watch-accounts");
        let auth = &temp.state().auth;
        let owner = auth
            .set_up("owner@example.com", "a long password")
            .expect("setup");
        let listener = auth
            .create_user("kitchen@example.com", "listen only", Role::Listener)
            .expect("listener");
        let client = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
        let kitchen = auth
            .log_in(client, "kitchen@example.com", "listen only")
            .expect("login")
            .signed_in()
            .expect("no second factor on this account");

        assert!(!may_watch_listeners(temp.state(), None));
        assert!(may_watch_listeners(temp.state(), Some(&owner.token)));
        assert!(!may_watch_listeners(temp.state(), Some(&kitchen.token)));

        // Promotion hands the list over on the next access check, and demotion takes it away again.
        auth.update_role(listener.id, Role::Admin).expect("promote");
        assert!(may_watch_listeners(temp.state(), Some(&kitchen.token)));
        auth.update_role(listener.id, Role::Listener)
            .expect("demote");
        assert!(!may_watch_listeners(temp.state(), Some(&kitchen.token)));
    }

    #[test]
    fn activity_reports_where_playback_is_and_nothing_finer() {
        assert_eq!(activity_of(StreamMode::Live, None), ListenerActivity::Live);
        assert_eq!(
            activity_of(StreamMode::Paused, None),
            ListenerActivity::Paused
        );
        // Playback without a cursor cannot happen, and reads as live rather than inventing a position.
        assert_eq!(
            activity_of(StreamMode::Playback, None),
            ListenerActivity::Live
        );
    }

    #[test]
    fn a_client_is_silent_only_after_three_missed_ping_rounds() {
        let heard = tokio::time::Instant::now();
        assert!(!is_silent(heard, heard));
        assert!(!is_silent(heard, heard + ACCESS_RECHECK));
        assert!(!is_silent(heard, heard + ACCESS_RECHECK * 2));
        assert!(is_silent(heard, heard + IDLE_TIMEOUT));
        // A clock that looks backwards is not silence.
        assert!(!is_silent(heard + ACCESS_RECHECK, heard));
    }

    #[test]
    fn a_stuck_send_gives_up_before_the_idle_timeout_would_notice() {
        // The idle check runs between events, so a send stuck inside one must end on its own, and soon
        // enough that a vanished client leaves the list in about as long as a silent one.
        assert!(SEND_TIMEOUT < IDLE_TIMEOUT);
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
