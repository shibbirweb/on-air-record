/**
 * State of the audio WebSocket.
 *
 * Deliberately separate from the transport: whether the socket is up is a property of the network, while
 * whether the listener is playing is a property of the person. Keeping them apart means a reconnect does
 * not silently change what the transport controls claim to be doing.
 */

import { create } from 'zustand';

import type { StreamInfoMessage } from '@/api/types';

type ConnectionState = {
  connected: boolean;
  /** Format and bounds the server reported on connect. */
  streamInfo: StreamInfoMessage | null;
  /** Input meter from the live feed, updated about ten times a second. */
  levels: { rms: number; peak: number };
  lastError: string | null;
  setConnected: (connected: boolean) => void;
  setStreamInfo: (info: StreamInfoMessage) => void;
  setLevels: (rms: number, peak: number) => void;
  setError: (message: string | null) => void;
};

export const useConnectionStore = create<ConnectionState>((set) => ({
  connected: false,
  streamInfo: null,
  levels: { rms: 0, peak: 0 },
  lastError: null,

  setConnected: (connected) =>
    set((state) => ({
      connected,
      // Levels come from the live feed, so a dropped socket should show silence rather than freeze the
      // meter at whatever it read when the connection died.
      levels: connected ? state.levels : { rms: 0, peak: 0 },
    })),

  setStreamInfo: (streamInfo) => set({ streamInfo }),
  setLevels: (rms, peak) => set({ levels: { rms, peak } }),
  setError: (lastError) => set({ lastError }),
}));
