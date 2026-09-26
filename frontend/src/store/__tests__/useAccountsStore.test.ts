import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Role, User } from '@/api/types';

const server = vi.hoisted(() => ({
  users: [] as User[],
  calls: [] as [number, Role][],
  /** Mimics the server's rule: it never leaves the recorder without an admin. */
  updateUserRole: async (userId: number, role: Role) => {
    const { ApiError } = await import('@/api/client');
    server.calls.push([userId, role]);
    const next = server.users.map((user) => (user.id === userId ? { ...user, role } : user));
    if (!next.some((user) => user.role === 'admin')) {
      throw new ApiError('there must always be at least one admin', 'conflict', 409);
    }
    server.users = next;
    return next.find((user) => user.id === userId);
  },
}));

vi.mock('@/api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/api/client')>();
  return {
    ...actual,
    api: {
      users: async () => server.users.map((user) => ({ ...user })),
      updateUserRole: server.updateUserRole,
    },
  };
});

const { orderRoleChanges, pruneRoleDraft, useAccountsStore } = await import('../useAccountsStore');

function user(id: number, role: Role): User {
  return { id, email: `person${id}@example.com`, role, createdAtMs: 0, twoFactorEnabled: false };
}

describe('staging roles', () => {
  beforeEach(async () => {
    server.users = [user(1, 'admin'), user(2, 'listener'), user(3, 'listener')];
    server.calls = [];
    useAccountsStore.setState({ users: [], roleDraft: {}, roleError: null, savingRoles: false });
    await useAccountsStore.getState().refresh();
  });

  it('sends nothing to the server until saved', () => {
    useAccountsStore.getState().stageRole(2, 'admin');
    expect(useAccountsStore.getState().roleDraft).toEqual({ 2: 'admin' });
    expect(server.calls).toEqual([]);
    expect(useAccountsStore.getState().users.find((entry) => entry.id === 2)?.role).toBe('listener');
  });

  it('forgets a change picked back to the stored role', () => {
    useAccountsStore.getState().stageRole(2, 'admin');
    useAccountsStore.getState().stageRole(2, 'listener');
    expect(useAccountsStore.getState().roleDraft).toEqual({});
  });

  it('discards every staged change', () => {
    useAccountsStore.getState().stageRole(2, 'admin');
    useAccountsStore.getState().stageRole(3, 'admin');
    useAccountsStore.getState().discardRoles();
    expect(useAccountsStore.getState().roleDraft).toEqual({});
    expect(server.calls).toEqual([]);
  });

  it('applies the draft on save and clears it', async () => {
    useAccountsStore.getState().stageRole(2, 'admin');
    await useAccountsStore.getState().saveRoles();
    expect(server.calls).toEqual([[2, 'admin']]);
    expect(useAccountsStore.getState().roleDraft).toEqual({});
    expect(useAccountsStore.getState().users.find((entry) => entry.id === 2)?.role).toBe('admin');
    expect(useAccountsStore.getState().roleError).toBeNull();
  });

  it('hands the admin role over in one save by promoting before demoting', async () => {
    // Staged demotion first, the order a person would most likely click them in.
    useAccountsStore.getState().stageRole(1, 'listener');
    useAccountsStore.getState().stageRole(2, 'admin');
    await useAccountsStore.getState().saveRoles();
    expect(server.calls).toEqual([
      [2, 'admin'],
      [1, 'listener'],
    ]);
    expect(useAccountsStore.getState().roleDraft).toEqual({});
    expect(useAccountsStore.getState().roleError).toBeNull();
  });

  it('keeps what the server refused staged, with the reason', async () => {
    useAccountsStore.getState().stageRole(1, 'listener');
    await useAccountsStore.getState().saveRoles();
    expect(useAccountsStore.getState().roleDraft).toEqual({ 1: 'listener' });
    expect(useAccountsStore.getState().roleError).toMatch(/at least one admin/);
    expect(useAccountsStore.getState().savingRoles).toBe(false);
  });

  it('saves the rest when one change is refused', async () => {
    server.users = [user(1, 'admin'), user(2, 'listener')];
    await useAccountsStore.getState().refresh();
    useAccountsStore.getState().stageRole(1, 'listener');
    useAccountsStore.getState().stageRole(2, 'admin');
    // Account 2 is deleted by somebody else before the save, so its promotion fails and the demotion
    // that depended on it is refused too.
    server.users = [user(1, 'admin')];
    await useAccountsStore.getState().saveRoles();
    expect(useAccountsStore.getState().roleDraft).toEqual({ 1: 'listener' });
    expect(useAccountsStore.getState().users.map((entry) => entry.id)).toEqual([1]);
  });
});

describe('orderRoleChanges', () => {
  it('puts promotions first', () => {
    expect(orderRoleChanges({ 1: 'listener', 2: 'admin', 3: 'listener', 4: 'admin' })).toEqual([
      [2, 'admin'],
      [4, 'admin'],
      [1, 'listener'],
      [3, 'listener'],
    ]);
  });
});

describe('pruneRoleDraft', () => {
  it('drops accounts that are gone or already match', () => {
    const users = [user(1, 'admin'), user(2, 'admin')];
    expect(pruneRoleDraft({ 1: 'listener', 2: 'admin', 9: 'admin' }, users)).toEqual({ 1: 'listener' });
  });
});
