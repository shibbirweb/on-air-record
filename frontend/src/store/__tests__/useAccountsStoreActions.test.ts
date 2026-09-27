/**
 * The accounts store actions that `useAccountsStore.test.ts` leaves out: loading the list, adding an
 * account, setting a password, removing an account, resetting a second factor, and the edges of the role
 * draft (an empty save, the saving flag, a fresh pick clearing the last refusal).
 *
 * Every change reloads the list rather than patching it, because the server enforces the rules and its
 * answer is the one to show, so each test checks the reload happened as well as the request.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Role, User } from '@/api/types';

const server = vi.hoisted(() => ({
  users: [] as User[],
  calls: [] as string[],
  failWith: null as Error | null,
  listFails: null as Error | null,
  gate: null as Promise<void> | null,
}));

vi.mock('@/api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/api/client')>();
  const step = async (call: string) => {
    server.calls.push(call);
    if (server.gate) {
      await server.gate;
    }
    if (server.failWith) {
      throw server.failWith;
    }
  };
  return {
    ...actual,
    api: {
      users: async () => {
        server.calls.push('users');
        if (server.listFails) {
          throw server.listFails;
        }
        return server.users.map((user) => ({ ...user }));
      },
      createUser: async (email: string, password: string, role: Role) => {
        await step(`create ${email} ${password} ${role}`);
        server.users.push({ id: 9, email, role, createdAtMs: 0, twoFactorEnabled: false });
      },
      setUserPassword: (userId: number, password: string) => step(`password ${userId} ${password}`),
      deleteUser: async (userId: number) => {
        await step(`delete ${userId}`);
        server.users = server.users.filter((user) => user.id !== userId);
      },
      resetUserTwoFactor: async (userId: number) => {
        await step(`reset2fa ${userId}`);
        server.users = server.users.map((user) =>
          user.id === userId ? { ...user, twoFactorEnabled: false } : user,
        );
      },
      updateUserRole: async (userId: number, role: Role) => {
        await step(`role ${userId} ${role}`);
        server.users = server.users.map((user) => (user.id === userId ? { ...user, role } : user));
      },
    },
  };
});

const { ApiError } = await import('@/api/client');
const { useAccountsStore } = await import('../useAccountsStore');

function user(id: number, role: Role, twoFactorEnabled = false): User {
  return { id, email: `person${id}@example.com`, role, createdAtMs: 0, twoFactorEnabled };
}

function hold() {
  let release = () => undefined as void;
  server.gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  return () => {
    server.gate = null;
    release();
  };
}

beforeEach(() => {
  server.users = [user(1, 'admin'), user(2, 'listener', true)];
  server.calls = [];
  server.failWith = null;
  server.listFails = null;
  server.gate = null;
  useAccountsStore.setState({
    users: [],
    loading: false,
    error: null,
    roleDraft: {},
    savingRoles: false,
    roleError: null,
  });
});

describe('refresh', () => {
  it('loads the accounts and clears an earlier error', async () => {
    useAccountsStore.setState({ error: 'old problem' });
    await useAccountsStore.getState().refresh();

    expect(useAccountsStore.getState().users.map((entry) => entry.id)).toEqual([1, 2]);
    expect(useAccountsStore.getState().error).toBeNull();
  });

  it('is loading while the request is in flight, and not after', async () => {
    const pending = useAccountsStore.getState().refresh();
    expect(useAccountsStore.getState().loading).toBe(true);
    await pending;
    expect(useAccountsStore.getState().loading).toBe(false);
  });

  it('keeps the list and says why when loading fails', async () => {
    await useAccountsStore.getState().refresh();
    server.listFails = new ApiError('sign in as an admin', 'forbidden', 403);
    await useAccountsStore.getState().refresh();

    expect(useAccountsStore.getState().users).toHaveLength(2);
    expect(useAccountsStore.getState().error).toBe('sign in as an admin');
    expect(useAccountsStore.getState().loading).toBe(false);
  });

  it('uses a general message when the failure is not the server speaking', async () => {
    server.listFails = new Error('boom');
    await useAccountsStore.getState().refresh();
    expect(useAccountsStore.getState().error).toBe('something went wrong, try again');
  });

  it('drops staged roles for accounts somebody else removed meanwhile', async () => {
    await useAccountsStore.getState().refresh();
    useAccountsStore.getState().stageRole(2, 'admin');
    server.users = [user(1, 'admin')];
    await useAccountsStore.getState().refresh();
    expect(useAccountsStore.getState().roleDraft).toEqual({});
  });
});

describe('create', () => {
  it('adds the account, then reloads the list', async () => {
    await useAccountsStore.getState().create('new@example.com', 'a long password', 'listener');

    expect(server.calls).toEqual(['create new@example.com a long password listener', 'users']);
    expect(useAccountsStore.getState().users.map((entry) => entry.email)).toContain('new@example.com');
  });

  it('throws for the add form to show, without reloading', async () => {
    const refused = new ApiError('that email is taken', 'conflict', 409);
    server.failWith = refused;

    await expect(
      useAccountsStore.getState().create('person1@example.com', 'a long password', 'admin'),
    ).rejects.toBe(refused);
    expect(server.calls).toEqual(['create person1@example.com a long password admin']);
    expect(useAccountsStore.getState().error).toBeNull();
  });
});

describe('setPassword', () => {
  it('sets the password without reloading, since nothing on the list changes', async () => {
    await useAccountsStore.getState().setPassword(2, 'a new password');
    expect(server.calls).toEqual(['password 2 a new password']);
  });

  it('throws for the password dialog to show', async () => {
    server.failWith = new ApiError('use at least 8 characters', 'bad_request', 400);
    await expect(useAccountsStore.getState().setPassword(2, 'short')).rejects.toMatchObject({
      message: 'use at least 8 characters',
    });
  });
});

describe.each([
  ['remove', 'delete'],
  ['resetTwoFactor', 'reset2fa'],
] as const)('%s', (action, call) => {
  it('makes the change, clears an earlier error and reloads', async () => {
    useAccountsStore.setState({ error: 'old problem' });
    await useAccountsStore.getState()[action](2);

    expect(server.calls).toEqual([`${call} 2`, 'users']);
    expect(useAccountsStore.getState().error).toBeNull();
  });

  it('still reloads when the server refuses, so the list shows what it kept', async () => {
    server.failWith = new ApiError('there must always be at least one admin', 'conflict', 409);
    await useAccountsStore.getState()[action](1);

    expect(server.calls).toEqual([`${call} 1`, 'users']);
    expect(useAccountsStore.getState().users).toHaveLength(2);
  });

  it('keeps the refusal on screen after the reload that follows it', async () => {
    server.failWith = new ApiError('there must always be at least one admin', 'conflict', 409);
    await useAccountsStore.getState()[action](1);
    expect(useAccountsStore.getState().error).toBe('there must always be at least one admin');
  });

  it('does not throw when the change fails, since the list is where the problem shows', async () => {
    server.failWith = new Error('boom');
    await expect(useAccountsStore.getState()[action](2)).resolves.toBeUndefined();
  });
});

describe('the result of each change on the list', () => {
  it('removes the account from the list', async () => {
    await useAccountsStore.getState().remove(2);
    expect(useAccountsStore.getState().users.map((entry) => entry.id)).toEqual([1]);
  });

  it('shows the second factor as off after a reset', async () => {
    await useAccountsStore.getState().resetTwoFactor(2);
    expect(useAccountsStore.getState().users.find((entry) => entry.id === 2)?.twoFactorEnabled).toBe(false);
  });
});

describe('role draft edges', () => {
  beforeEach(async () => {
    await useAccountsStore.getState().refresh();
    server.calls = [];
  });

  it('does nothing at all when saving an empty draft', async () => {
    await useAccountsStore.getState().saveRoles();
    expect(server.calls).toEqual([]);
    expect(useAccountsStore.getState().savingRoles).toBe(false);
  });

  it('is saving while the roles are being applied, and not after', async () => {
    useAccountsStore.getState().stageRole(2, 'admin');
    const release = hold();
    const pending = useAccountsStore.getState().saveRoles();
    expect(useAccountsStore.getState().savingRoles).toBe(true);

    release();
    await pending;
    expect(useAccountsStore.getState().savingRoles).toBe(false);
    expect(server.calls).toEqual(['role 2 admin', 'users']);
  });

  it('uses a general message for a role refusal that is not the server speaking', async () => {
    useAccountsStore.getState().stageRole(2, 'admin');
    server.failWith = new Error('boom');
    await useAccountsStore.getState().saveRoles();
    expect(useAccountsStore.getState().roleError).toBe('something went wrong, try again');
  });

  it('clears the last refusal as soon as another role is picked, and on discard', () => {
    useAccountsStore.setState({ roleError: 'refused' });
    useAccountsStore.getState().stageRole(2, 'admin');
    expect(useAccountsStore.getState().roleError).toBeNull();

    useAccountsStore.setState({ roleError: 'refused' });
    useAccountsStore.getState().discardRoles();
    expect(useAccountsStore.getState().roleError).toBeNull();
  });
});
