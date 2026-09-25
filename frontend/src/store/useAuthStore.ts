/**
 * Who is signed in, and whether this install asks for a login at all.
 *
 * Fetched once when the page loads and again whenever any request reports that the session ended. The
 * actions behind forms throw an `ApiError` rather than storing it, so each form can show its own message
 * next to its own fields.
 */

import { create } from 'zustand';

import { api, ApiError } from '@/api/client';
import type { AuthMode, AuthState, User } from '@/api/types';

type AuthStoreState = {
  mode: AuthMode | null;
  user: User | null;
  /** True after a right password on an account with two factor sign in, until the code is accepted. */
  pendingTwoFactor: boolean;
  /** False until the first answer arrives, so the page can wait rather than flash the wrong screen. */
  loaded: boolean;
  /** Set when the service cannot be reached at all, which is different from being signed out. */
  error: string | null;
  refresh: () => Promise<void>;
  chooseOpen: () => Promise<void>;
  setUp: (email: string, password: string) => Promise<void>;
  logIn: (email: string, password: string) => Promise<void>;
  /** The second step of a sign in. Throws, so the code form can show what went wrong. */
  verifyCode: (code: string) => Promise<void>;
  /** Leave the code step and start again from the password. */
  backToPassword: () => Promise<void>;
  logOut: () => Promise<void>;
  changePassword: (currentPassword: string, newPassword: string) => Promise<void>;
};

/** The part of every auth answer the page acts on. */
const fromState = (state: AuthState) => ({
  mode: state.mode,
  user: state.user,
  pendingTwoFactor: state.pendingTwoFactor,
});

export const useAuthStore = create<AuthStoreState>((set) => ({
  mode: null,
  user: null,
  pendingTwoFactor: false,
  loaded: false,
  error: null,

  refresh: async () => {
    try {
      const state = await api.authState();
      set({ ...fromState(state), loaded: true, error: null });
    } catch (cause) {
      set({
        loaded: true,
        error: cause instanceof ApiError ? cause.message : 'the service is unreachable',
      });
    }
  },

  chooseOpen: async () => {
    const state = await api.chooseOpen();
    set(fromState(state));
  },

  setUp: async (email, password) => {
    const state = await api.setUp(email, password);
    set(fromState(state));
  },

  logIn: async (email, password) => {
    const state = await api.logIn(email, password);
    set(fromState(state));
  },

  verifyCode: async (code) => {
    const state = await api.verifyLogin(code);
    set(fromState(state));
  },

  backToPassword: async () => {
    // Logging out also forgets the pending sign in, and needs no reload: nothing was loaded yet.
    const state = await api.logOut();
    set(fromState(state));
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
