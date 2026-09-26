/**
 * The signed in account's own two factor sign in: whether it is on, and the steps to change that.
 *
 * The setup and change actions throw an `ApiError` rather than storing it, so the dialog can show the
 * problem next to the field that caused it. After anything that switches it on or off, the auth store is
 * refreshed too, because the account's `twoFactorEnabled` flag lives there.
 */

import { create } from 'zustand';

import { api } from '@/api/client';
import type { TwoFactorSetup, TwoFactorStatus } from '@/api/types';
import { useAuthStore } from '@/store/useAuthStore';

type TwoFactorState = {
  status: TwoFactorStatus | null;
  refresh: () => Promise<void>;
  beginSetup: () => Promise<TwoFactorSetup>;
  /** Returns the recovery codes, which the server sends exactly once. */
  enable: (code: string) => Promise<string[]>;
  disable: (password: string) => Promise<void>;
  /** Returns the new recovery codes; the old ones stop working. */
  regenerate: (password: string) => Promise<string[]>;
};

export const useTwoFactorStore = create<TwoFactorState>((set, get) => ({
  status: null,

  refresh: async () => {
    try {
      set({ status: await api.twoFactorStatus() });
    } catch {
      // The dialog shows its own loading state; a failed status read leaves the last known one.
    }
  },

  beginSetup: () => api.beginTwoFactorSetup(),

  enable: async (code) => {
    const codes = await api.enableTwoFactor(code);
    await Promise.all([get().refresh(), useAuthStore.getState().refresh()]);
    return codes;
  },

  disable: async (password) => {
    await api.disableTwoFactor(password);
    await Promise.all([get().refresh(), useAuthStore.getState().refresh()]);
  },

  regenerate: async (password) => {
    const codes = await api.regenerateRecoveryCodes(password);
    await get().refresh();
    return codes;
  },
}));
