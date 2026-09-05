/**
 * Playback transport: what the listener is hearing and where on the timeline they are.
 *
 * The store holds intent and state. The machinery that actually moves audio, the WebSocket and the Web
 * Audio graph, is registered here as a controller by the hook that owns it. That inversion lets any
 * component call `seek` without being handed a socket, while keeping the imperative audio code out of
 * React's render path where it would cause clicks.
 *
 * The playhead is a deliberate exception: it moves at 60 frames a second and is read straight off the
 * audio clock by the components that draw it, never through this store.
 */

import { create } from 'zustand';

import type { StreamMode } from '@/api/types';

export type TransportController = {
  play: () => Promise<void>;
  pause: () => void;
  seek: (timestampMs: number) => void;
  goLive: () => void;
  setVolume: (volume: number) => void;
  setMuted: (muted: boolean) => void;
  setSpeed: (speed: number) => void;
};

type TransportState = {
  /** True once the listener has started playback, which browsers only allow from a gesture. */
  playing: boolean;
  mode: StreamMode;
  /** True while the playhead should chase the live edge. */
  followingLive: boolean;
  /** Where the listener asked to be, in epoch milliseconds. Null means the live edge. */
  requestedPositionMs: number | null;
  volume: number;
  muted: boolean;
  /** Playback rate for history. The live feed is always real time, so this snaps back to 1 there. */
  speed: number;
  /** Set when playback ran off the end of the recording. */
  endOfRecording: boolean;
  controller: TransportController | null;

  attachController: (controller: TransportController | null) => void;
  setPlaying: (playing: boolean) => void;
  /** Record the speed the server actually applied, which may differ from what was asked for. */
  setAppliedSpeed: (speed: number) => void;
  setMode: (mode: StreamMode) => void;
  setEndOfRecording: (reached: boolean) => void;
  markLive: () => void;

  play: () => Promise<void>;
  pause: () => void;
  seek: (timestampMs: number) => void;
  goLive: () => void;
  setVolume: (volume: number) => void;
  toggleMuted: () => void;
  requestSpeed: (speed: number) => void;
};

export const useTransportStore = create<TransportState>((set, get) => ({
  playing: false,
  mode: 'live',
  followingLive: true,
  requestedPositionMs: null,
  volume: 0.8,
  muted: false,
  speed: 1,
  endOfRecording: false,
  controller: null,

  attachController: (controller) => set({ controller }),
  setPlaying: (playing) => set({ playing }),
  setAppliedSpeed: (speed) => set({ speed }),
  setMode: (mode) => set({ mode }),
  setEndOfRecording: (endOfRecording) => set({ endOfRecording }),

  markLive: () =>
    set({
      mode: 'live',
      followingLive: true,
      requestedPositionMs: null,
      endOfRecording: false,
      // The present arrives in real time, so there is no faster to go.
      speed: 1,
    }),

  play: async () => {
    await get().controller?.play();
    set({ playing: true });
  },

  pause: () => {
    get().controller?.pause();
    set({ playing: false });
  },

  seek: (timestampMs) => {
    set({
      followingLive: false,
      requestedPositionMs: timestampMs,
      mode: 'playback',
      endOfRecording: false,
    });
    get().controller?.seek(timestampMs);
  },

  goLive: () => {
    set({
      followingLive: true,
      requestedPositionMs: null,
      mode: 'live',
      endOfRecording: false,
      speed: 1,
    });
    get().controller?.goLive();
  },

  setVolume: (volume) => {
    const clamped = Math.min(Math.max(volume, 0), 1);
    set({ volume: clamped, muted: clamped === 0 ? get().muted : false });
    get().controller?.setVolume(clamped);
  },

  toggleMuted: () => {
    const muted = !get().muted;
    set({ muted });
    get().controller?.setMuted(muted);
  },

  requestSpeed: (speed) => {
    // Applied locally at once so the buttons respond, then confirmed by the server's echo in case it
    // clamped the value to something it supports.
    set({ speed });
    get().controller?.setSpeed(speed);
  },
}));
