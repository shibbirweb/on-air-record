/**
 * The list of accounts, for the admin's accounts section on the settings page.
 *
 * Every change reloads the list from the server rather than patching it locally, because the server is
 * the one enforcing rules like "there is always an admin", and its answer is the one to show.
 *
 * Role changes are a draft, like the rest of the settings page: picking a role stages it, and the page's
 * Save applies it. A role menu that took effect on selection was the one control on a page of held edits
 * that did not wait, and a misclick there silently took somebody's powers away. Adding and removing an
 * account, and setting a password, still happen when their dialog or confirm button is pressed, since that
 * press is already the deliberate second step.
 */

import { create } from 'zustand';

import { api, ApiError } from '@/api/client';
import type { Role, User } from '@/api/types';

/** Staged role changes by account id, holding only those that differ from the stored role. */
export type RoleDraft = Record<number, Role>;

type AccountsState = {
  users: User[];
  loading: boolean;
  /** The last thing that went wrong, shown above the list. */
  error: string | null;
  roleDraft: RoleDraft;
  savingRoles: boolean;
  /** Why the last save left some role changes unsaved, shown in the Save bar. */
  roleError: string | null;
  /** Stage a role. Choosing the stored role again takes the account out of the draft. */
  stageRole: (userId: number, role: Role) => void;
  discardRoles: () => void;
  /** Apply the draft. What the server refuses stays staged, so it is still visibly unsaved. */
  saveRoles: () => Promise<void>;
  refresh: () => Promise<void>;
  /** Throws, so the add form can show the problem next to its fields. */
  create: (email: string, password: string, role: Role) => Promise<void>;
  /** Throws, so the password dialog can show the problem next to its field. */
  setPassword: (userId: number, password: string) => Promise<void>;
  remove: (userId: number) => Promise<void>;
  /** Remove somebody's second factor, for a lost phone with no recovery codes left. */
  resetTwoFactor: (userId: number) => Promise<void>;
};

const describe = (cause: unknown) =>
  cause instanceof ApiError ? cause.message : 'something went wrong, try again';

/**
 * The order to apply staged roles in: promotions before demotions. The server refuses to leave the
 * recorder without an admin, so handing the role from one account to another only works if the new admin
 * exists before the old one steps down.
 */
export function orderRoleChanges(draft: RoleDraft): [number, Role][] {
  const changes = Object.entries(draft).map(([id, role]) => [Number(id), role] as [number, Role]);
  return [
    ...changes.filter(([, role]) => role === 'admin'),
    ...changes.filter(([, role]) => role !== 'admin'),
  ];
}

/** Drop staged roles for accounts that are gone, or whose stored role already matches. */
export function pruneRoleDraft(draft: RoleDraft, users: User[]): RoleDraft {
  const pruned: RoleDraft = {};
  for (const [id, role] of Object.entries(draft)) {
    const user = users.find((candidate) => candidate.id === Number(id));
    if (user && user.role !== role) {
      pruned[Number(id)] = role;
    }
  }
  return pruned;
}

export const useAccountsStore = create<AccountsState>((set, get) => ({
  users: [],
  loading: false,
  error: null,
  roleDraft: {},
  savingRoles: false,
  roleError: null,

  refresh: async () => {
    set({ loading: true });
    try {
      const users = await api.users();
      set((state) => ({ users, error: null, roleDraft: pruneRoleDraft(state.roleDraft, users) }));
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

  stageRole: (userId, role) =>
    set((state) => {
      const stored = state.users.find((user) => user.id === userId)?.role;
      const roleDraft = { ...state.roleDraft };
      if (role === stored) {
        delete roleDraft[userId];
      } else {
        roleDraft[userId] = role;
      }
      return { roleDraft, roleError: null };
    }),

  discardRoles: () => set({ roleDraft: {}, roleError: null }),

  saveRoles: async () => {
    const changes = orderRoleChanges(get().roleDraft);
    if (changes.length === 0) {
      return;
    }
    set({ savingRoles: true });
    const refused: RoleDraft = {};
    const problems: string[] = [];
    // One at a time and in order, so a promotion lands before the demotion that depends on it.
    for (const [userId, role] of changes) {
      try {
        await api.updateUserRole(userId, role);
      } catch (cause) {
        refused[userId] = role;
        problems.push(describe(cause));
      }
    }
    set({ roleDraft: refused, roleError: problems[0] ?? null, savingRoles: false });
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
