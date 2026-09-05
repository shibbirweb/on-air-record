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
}

impl StreamSession {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    /// Drive the connection until the client disconnects.
    pub async fn run(self, socket: WebSocket) {
        let state = self.state;
        let (mut sink, mut source) = socket.split();

        let mut mode = StreamMode::Live;
        // Where a resume should return to. A listener who paused during playback wants their position
        // back, not a jump to live.
        let mut resume_mode = StreamMode::Live;
        let mut live_rx: Option<Receiver<AudioFrame>> = Some(state.hub.subscribe());
        let mut cursor: Option<PlaybackCursor> = None;

        let frame_ms = effective_frame_ms(&state);
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(frame_ms as u64));
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
            };

            match event {
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
    ticker: &mut tokio::time::Interval,
) -> bool {
    match command {
        ClientMessage::Live => {
            *cursor = None;
            *mode = StreamMode::Live;
            *resume_mode = StreamMode::Live;
            *live_rx = Some(state.hub.subscribe());

            let edge = state.live_edge_ms().unwrap_or_else(now_ms);
            send_message(sink, mode_message(*mode, edge)).await
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
