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
import type { AudioFrame, ServerMessage } from '@/api/types';
import { AudioEngine } from '@/lib/audio/audioEngine';
import { useConnectionStore } from '@/store/useConnectionStore';
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

        case 'error':
          useConnectionStore.getState().setError(message.message);
          break;

        case 'pong':
          break;
      }
    };

    const socket = new StreamSocket({
      onFrame,
      onMessage,
      onOpen: () => {
        useConnectionStore.getState().setConnected(true);
        useConnectionStore.getState().setError(null);
      },
      onClose: () => {
        useConnectionStore.getState().setConnected(false);
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
        socket.send({ type: 'seek', timestampMs: Math.round(timestampMs) });
      },
      goLive: () => {
        engine.flush();
        socket.send({ type: 'live' });
      },
      setVolume: (volume) => engine.setVolume(volume),
      setMuted: (muted) => engine.setMuted(muted),
    });

    // Restore whatever the listener had set before this effect ran.
    engine.setVolume(transport.volume);
    engine.setMuted(transport.muted);
    connection.setConnected(false);

    return () => {
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
