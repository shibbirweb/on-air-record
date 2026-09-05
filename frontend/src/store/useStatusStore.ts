/**
 * Service and capture status.
 *
 * Polled rather than pushed. The values here change on a human timescale (someone clicks record, a device
 * is unplugged), so a poll is simpler than a second stream and it doubles as the liveness check that tells
 * the UI the backend is reachable at all.
 */

import { create } from 'zustand';

import { api, ApiError } from '@/api/client';
import type { ServiceStatus } from '@/api/types';

type StatusState = {
  status: ServiceStatus | null;
  reachable: boolean;
  error: string | null;
  busy: boolean;
  refresh: () => Promise<void>;
  startCapture: () => Promise<void>;
  stopCapture: () => Promise<void>;
};

export const useStatusStore = create<StatusState>((set) => ({
  status: null,
  reachable: false,
  error: null,
  busy: false,

  refresh: async () => {
    try {
      const status = await api.status();
      set({ status, reachable: true, error: null });
    } catch (cause) {
      // A failed poll means the service is down or restarting. Keep the last known status on screen so
      // the panel does not blank out, and say plainly that it is stale.
      set({
        reachable: false,
        error: cause instanceof ApiError ? cause.message : 'the service is unreachable',
      });
    }
  },

  startCapture: async () => {
    set({ busy: true });
    try {
      const status = await api.startCapture();
      set({ status, reachable: true, error: null });
    } catch (cause) {
      set({ error: cause instanceof ApiError ? cause.message : 'could not start capture' });
    } finally {
      set({ busy: false });
    }
  },

  stopCapture: async () => {
    set({ busy: true });
    try {
      const status = await api.stopCapture();
      set({ status, reachable: true, error: null });
    } catch (cause) {
      set({ error: cause instanceof ApiError ? cause.message : 'could not stop capture' });
    } finally {
      set({ busy: false });
    }
  },
}));
