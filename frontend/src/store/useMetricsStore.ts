/**
 * The Prometheus scrape token, as the Monitoring card shows it.
 *
 * The server sends a token once, in the answer to making it, and keeps only its hash. So `freshToken` is
 * the one copy that exists anywhere, held just long enough for the admin to copy it; the card drops it
 * when it closes, and after that only the date is known.
 */

import { create } from 'zustand';

import { api, ApiError } from '@/api/client';

type MetricsState = {
  /** When the current token was made, `null` when there is none. */
  createdAtMs: number | null;
  /** Whether the service has answered yet, so "no token" is never shown before it is known. */
  loaded: boolean;
  /** The token just made, shown until the card is left. */
  freshToken: string | null;
  busy: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  /** Make a token, or a new one in place of the old, which stops working at once. */
  create: () => Promise<void>;
  revoke: () => Promise<void>;
  forgetFreshToken: () => void;
};

const describe = (cause: unknown) =>
  cause instanceof ApiError ? cause.message : 'could not reach the recorder';

export const useMetricsStore = create<MetricsState>((set) => ({
  createdAtMs: null,
  loaded: false,
  freshToken: null,
  busy: false,
  error: null,

  refresh: async () => {
    try {
      const { createdAtMs } = await api.metricsToken();
      set({ createdAtMs, loaded: true, error: null });
    } catch (cause) {
      set({ error: describe(cause) });
    }
  },

  create: async () => {
    set({ busy: true });
    try {
      const { token, createdAtMs } = await api.createMetricsToken();
      set({ freshToken: token, createdAtMs, loaded: true, error: null });
    } catch (cause) {
      set({ error: describe(cause) });
    } finally {
      set({ busy: false });
    }
  },

  revoke: async () => {
    set({ busy: true });
    try {
      const { createdAtMs } = await api.revokeMetricsToken();
      set({ createdAtMs, freshToken: null, error: null });
    } catch (cause) {
      set({ error: describe(cause) });
    } finally {
      set({ busy: false });
    }
  },

  forgetFreshToken: () => set({ freshToken: null }),
}));
