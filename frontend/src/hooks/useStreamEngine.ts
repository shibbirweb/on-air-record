/**
 * Owns the audio pipeline for the page: one WebSocket, one Web Audio graph.
 *
 * Mounted exactly once, at the top of the control room. Everything else reaches the transport through the
 * store, so no component below needs to know a socket exists. The engine and the socket live in refs
 * because they must survive every render: recreating an `AudioContext` on a re render produces a click and
 * loses the scheduling clock, which is the single most common way streaming audio in React goes wrong.
 */

import { useEffect, useRef, useState } from 'react';

import { StreamSocket } from '@/api/streamSocket';
import type { AudioFrame, PlayerState, ServerMessage } from '@/api/types';
import { AudioEngine } from '@/lib/audio/audioEngine';
import { bindSessionActions, showNowPlaying } from '@/lib/audio/mediaSession';
import { playerState } from '@/lib/listeners';
import { useConnectionStore } from '@/store/useConnectionStore';
import { useListenersStore } from '@/store/useListenersStore';
import { useTimelineStore } from '@/store/useTimelineStore';
import { useTransportStore } from '@/store/useTransportStore';

export type StreamEngine = {
  engine: AudioEngine;
  /** Latest media timestamp handed to the audio graph, for the playhead. */
  playheadMs: () => number | null;
};

export function useStreamEngine(): StreamEngine {
  const socketRef = useRef<StreamSocket | null>(null);
  // Built once for the lifetime of the page. An `AudioContext` rebuilt on a render clicks audibly and
  // loses the scheduling clock, so it must never be tied to the render cycle.
  const [engine] = useState(() => new AudioEngine());

  useEffect(() => {
    const connection = useConnectionStore.getState();
    const transport = useTransportStore.getState();

    const onFrame = (frame: AudioFrame) => {
      // Frames arrive whether or not the listener pressed play. Dropping them here rather than not
      // subscribing keeps the meter and the connection alive while muted, and the engine ignores anything
      // queued before its context is running.
      engine.enqueue(frame);
    };

    const onMessage = (message: ServerMessage) => {
      switch (message.type) {
        case 'stream-info':
          useConnectionStore.getState().setStreamInfo(message);
          useTransportStore.getState().setMode(message.mode);
          break;

        case 'mode':
          useTransportStore.getState().setMode(message.mode);
          break;

        case 'switched-to-live':
          // The server ran out of recorded material and rejoined the live feed. Flush first: what is
          // still scheduled is the tail of the old position.
          engine.flush();
          engine.setSpeed(1);
          useTransportStore.getState().markLive();
          useTimelineStore.getState().setFollowingLive(true);
          break;

        case 'gap':
          // Nothing was recorded across this stretch, so drop what is queued and let the clock restart
          // rather than playing the two sides of the hole back to back.
          engine.flush();
          break;

        case 'end-of-recording':
          useTransportStore.getState().setEndOfRecording(true);
          break;

        case 'level':
          useConnectionStore.getState().setLevels(message.rms, message.peak);
          break;

        case 'speed':
          // The server's word is final: it may have clamped the request onto a speed it supports, and
          // it resets to real time on its own whenever the session rejoins the live feed.
          engine.setSpeed(message.value);
          useTransportStore.getState().setAppliedSpeed(message.value);
          break;

        case 'error':
          useConnectionStore.getState().setError(message.message);
          break;

        case 'listeners':
          useListenersStore.getState().setListeners(message.listeners);
          break;

        case 'listeners-hidden':
          useListenersStore.getState().clear();
          break;

        case 'pong':
          break;
      }
    };

    // Tell the server whether anybody is hearing this tab, for the admin listener list. Only changes are
    // sent, and the state is sent again on every connect, because a reconnect is a new session that knows
    // nothing about the old one.
    let started = transport.playing;
    let reported: PlayerState | null = null;
    const reportPlayer = () => {
      const current = playerState(useTransportStore.getState().playing, started);
      if (current !== reported && socketRef.current?.connected) {
        reported = current;
        socketRef.current.send({ type: 'player', state: current });
      }
    };
    const stopWatchingPlayer = useTransportStore.subscribe((state) => {
      if (state.playing) {
        started = true;
      }
      reportPlayer();
    });

    // The lock screen: what is playing, and play and pause buttons that drive the same transport as the
    // page. Kept in step with the store, so pausing from either place shows on both.
    const session = typeof navigator !== 'undefined' ? navigator.mediaSession : undefined;
    const unbindSession = bindSessionActions(session, {
      play: () => void useTransportStore.getState().play(),
      pause: () => useTransportStore.getState().pause(),
    });
    const stopShowingNowPlaying = useTransportStore.subscribe((state, previous) => {
      if (state.playing !== previous.playing || state.mode !== previous.mode) {
        showNowPlaying(session, state.playing, state.mode, window.location.host);
      }
    });
    // The phone took the audio away (another app, a call). Pause the transport so the page and the lock
    // screen stop claiming to play, and the next tap on play starts cleanly.
    engine.onInterrupted(() => useTransportStore.getState().pause());

    const socket = new StreamSocket({
      onFrame,
      onMessage,
      onOpen: () => {
        useConnectionStore.getState().setConnected(true);
        useConnectionStore.getState().setError(null);
        reported = null;
        reportPlayer();
      },
      onClose: () => {
        useConnectionStore.getState().setConnected(false);
        // Nobody is keeping the list current any more. The count falls back to the polled status until the
        // reconnected socket sends a fresh list.
        useListenersStore.getState().clear();
        // The scheduled buffers belong to a connection that no longer exists, and playing them out after
        // a reconnect would put stale audio in front of the new stream.
        engine.flush();
      },
    });

    socketRef.current = socket;
    socket.connect();

    transport.attachController({
      play: async () => {
        await engine.start();
        engine.setVolume(useTransportStore.getState().volume);
        engine.setMuted(useTransportStore.getState().muted);
      },
      pause: () => {
        void engine.suspend();
      },
      seek: (timestampMs) => {
        engine.flush();
        // Jumping to a moment means the timeline should stop chasing the present. Without this the next
        // range poll drags the window back to the live edge a couple of seconds later, undoing both the
        // scroll and any zoom the listener anchored on that moment.
        useTimelineStore.getState().setFollowingLive(false);
        socket.send({ type: 'seek', timestampMs: Math.round(timestampMs) });
      },
      goLive: () => {
        engine.flush();
        useTimelineStore.getState().setFollowingLive(true);
        socket.send({ type: 'live' });
      },
      setVolume: (volume) => engine.setVolume(volume),
      setMuted: (muted) => engine.setMuted(muted),
      setSpeed: (speed) => {
        // Both halves are needed: the server paces frames faster, and the engine plays each one faster.
        // Either alone would just change how much audio is buffered rather than how fast it plays.
        engine.setSpeed(speed);
        socket.send({ type: 'speed', value: speed });
      },
    });

    // Restore whatever the listener had set before this effect ran.
    engine.setVolume(transport.volume);
    engine.setMuted(transport.muted);
    connection.setConnected(false);

    return () => {
      stopWatchingPlayer();
      stopShowingNowPlaying();
      unbindSession();
      engine.onInterrupted(null);
      useTransportStore.getState().attachController(null);
      socket.close();
      socketRef.current = null;
      void engine.close();
    };
  }, [engine]);

  return {
    engine,
    playheadMs: () => engine.currentPlayheadMs(),
  };
}
