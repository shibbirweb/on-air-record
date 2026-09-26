/**
 * Whether a newer release exists, as the service last found out.
 *
 * The service asks GitHub every few hours by itself; this only reads its answer, so polling it costs
 * nothing and never waits on the internet. "Later" is remembered per browser and per version: dismissing
 * 0.7.0 hides the banner until 0.7.1 appears, and a different admin still sees it.
 */

import { create } from 'zustand';

import { api, ApiError } from '@/api/client';
import type { UpdateStatus } from '@/api/types';

const DISMISSED_KEY = 'oar.updates.dismissed';

function readDismissed(): string | null {
  try {
    return window.localStorage.getItem(DISMISSED_KEY);
  } catch {
    // Storage blocked or unavailable: the banner simply comes back, which is the safe way to fail.
    return null;
  }
}

type UpdateState = {
  status: UpdateStatus | null;
  checking: boolean;
  /** A request to this service that failed. A check that could not reach GitHub is in `status.error`. */
  requestError: string | null;
  /** The version the banner was dismissed for, in this browser. */
  dismissed: string | null;
  refresh: () => Promise<void>;
  checkNow: () => Promise<void>;
  dismiss: (version: string) => void;
};

/** Whether to show the banner: an update exists and it is not the one dismissed here. */
export function shouldShowBanner(status: UpdateStatus | null, dismissed: string | null): boolean {
  return Boolean(status?.available && status.available.version !== dismissed);
}

const describe = (cause: unknown) =>
  cause instanceof ApiError ? cause.message : 'could not ask the service about updates';

export const useUpdateStore = create<UpdateState>((set) => ({
  status: null,
  checking: false,
  requestError: null,
  dismissed: readDismissed(),

  refresh: async () => {
    try {
      set({ status: await api.updates(), requestError: null });
    } catch (cause) {
      set({ requestError: describe(cause) });
    }
  },

  checkNow: async () => {
    set({ checking: true });
    try {
      set({ status: await api.checkForUpdates(), requestError: null });
    } catch (cause) {
      set({ requestError: describe(cause) });
    } finally {
      set({ checking: false });
    }
  },

  dismiss: (version) => {
    try {
      window.localStorage.setItem(DISMISSED_KEY, version);
    } catch {
      // Not remembered past this page load, which is all that is lost.
    }
    set({ dismissed: version });
  },
}));
