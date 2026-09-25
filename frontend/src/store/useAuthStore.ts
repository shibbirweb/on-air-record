/**
 * Who is signed in, and whether this install asks for a login at all.
 *
 * Fetched once when the page loads and again whenever any request reports that the session ended. The
 * actions behind forms throw an `ApiError` rather than storing it, so each form can show its own message
 * next to its own fields.
 */

import { create } from 'zustand';

import { api, ApiError } from '@/api/client';
import type { AuthMode, User } from '@/api/types';

type AuthStoreState = {
  mode: AuthMode | null;
  user: User | null;
  /** False until the first answer arrives, so the page can wait rather than flash the wrong screen. */
  loaded: boolean;
  /** Set when the service cannot be reached at all, which is different from being signed out. */
  error: string | null;
  refresh: () => Promise<void>;
  chooseOpen: () => Promise<void>;
  setUp: (email: string, password: string) => Promise<void>;
  logIn: (email: string, password: string) => Promise<void>;
  logOut: () => Promise<void>;
  changePassword: (currentPassword: string, newPassword: string) => Promise<void>;
};

export const useAuthStore = create<AuthStoreState>((set) => ({
  mode: null,
  user: null,
  loaded: false,
  error: null,

  refresh: async () => {
    try {
      const state = await api.authState();
      set({ mode: state.mode, user: state.user, loaded: true, error: null });
    } catch (cause) {
      set({
        loaded: true,
        error: cause instanceof ApiError ? cause.message : 'the service is unreachable',
      });
    }
  },

  chooseOpen: async () => {
    const state = await api.chooseOpen();
    set({ mode: state.mode, user: state.user });
  },

  setUp: async (email, password) => {
    const state = await api.setUp(email, password);
    set({ mode: state.mode, user: state.user });
  },

  logIn: async (email, password) => {
    const state = await api.logIn(email, password);
    set({ mode: state.mode, user: state.user });
  },

  logOut: async () => {
    try {
      await api.logOut();
    } finally {
      // A full reload rather than a state change: it drops the audio engine, the socket and every store
      // holding the previous account's view, which is simpler to trust than resetting each one.
      window.location.assign('/');
    }
  },

  changePassword: async (currentPassword, newPassword) => {
    await api.changePassword(currentPassword, newPassword);
  },
}));

/** True when the current visitor may change things: always without accounts, and for admins with them. */
export function canAdminister(mode: AuthMode | null, user: User | null): boolean {
  return mode !== 'accounts' || user?.role === 'admin';
}

/** Hook form of `canAdminister`, for hiding controls a listener cannot use. */
export function useCanAdminister(): boolean {
  return useAuthStore((state) => canAdminister(state.mode, state.user));
}
