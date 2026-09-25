/**
 * The list of accounts, for the admin's accounts section on the settings page.
 *
 * Every change reloads the list from the server rather than patching it locally, because the server is
 * the one enforcing rules like "there is always an admin", and its answer is the one to show.
 */

import { create } from 'zustand';

import { api, ApiError } from '@/api/client';
import type { Role, User } from '@/api/types';

type AccountsState = {
  users: User[];
  loading: boolean;
  /** The last thing that went wrong, shown above the list. */
  error: string | null;
  refresh: () => Promise<void>;
  /** Throws, so the add form can show the problem next to its fields. */
  create: (email: string, password: string, role: Role) => Promise<void>;
  updateRole: (userId: number, role: Role) => Promise<void>;
  /** Throws, so the password dialog can show the problem next to its field. */
  setPassword: (userId: number, password: string) => Promise<void>;
  remove: (userId: number) => Promise<void>;
  /** Remove somebody's second factor, for a lost phone with no recovery codes left. */
  resetTwoFactor: (userId: number) => Promise<void>;
};

const describe = (cause: unknown) =>
  cause instanceof ApiError ? cause.message : 'something went wrong, try again';

export const useAccountsStore = create<AccountsState>((set, get) => ({
  users: [],
  loading: false,
  error: null,

  refresh: async () => {
    set({ loading: true });
    try {
      set({ users: await api.users(), error: null });
    } catch (cause) {
      set({ error: describe(cause) });
    } finally {
      set({ loading: false });
    }
  },

  create: async (email, password, role) => {
    await api.createUser(email, password, role);
    await get().refresh();
  },

  updateRole: async (userId, role) => {
    try {
      await api.updateUserRole(userId, role);
      set({ error: null });
    } catch (cause) {
      set({ error: describe(cause) });
    }
    await get().refresh();
  },

  setPassword: async (userId, password) => {
    await api.setUserPassword(userId, password);
  },

  remove: async (userId) => {
    try {
      await api.deleteUser(userId);
      set({ error: null });
    } catch (cause) {
      set({ error: describe(cause) });
    }
    await get().refresh();
  },

  resetTwoFactor: async (userId) => {
    try {
      await api.resetUserTwoFactor(userId);
      set({ error: null });
    } catch (cause) {
      set({ error: describe(cause) });
    }
    await get().refresh();
  },
}));
