/**
 * The auth store: loading who is signed in, the actions behind the sign in, setup and first run forms,
 * and the rule for who may change things.
 *
 * The form actions deliberately throw rather than store the error, so each form shows its own message;
 * that is tested as much as the happy path, because swallowing the error would leave a form that silently
 * does nothing. Sign out reloads the page instead of resetting state, so `window.location` is stubbed.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { AuthState, User } from '@/api/types';

const server = vi.hoisted(() => ({
  calls: [] as string[],
  answer: null as AuthState | null,
  failWith: null as Error | null,
}));

vi.mock('@/api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/api/client')>();
  const reply = (name: string) => async (...args: unknown[]) => {
    server.calls.push(args.length > 0 ? `${name} ${args.join(' ')}` : name);
    if (server.failWith) {
      throw server.failWith;
    }
    return server.answer;
  };
  return {
    ...actual,
    api: {
      authState: reply('authState'),
      chooseOpen: reply('chooseOpen'),
      setUp: reply('setUp'),
      logIn: reply('logIn'),
      verifyLogin: reply('verifyLogin'),
      logOut: reply('logOut'),
      changePassword: reply('changePassword'),
    },
  };
});

const { ApiError } = await import('@/api/client');
const { canAdminister, useAuthStore } = await import('../useAuthStore');

const ADMIN: User = {
  id: 1,
  email: 'owner@example.com',
  role: 'admin',
  createdAtMs: 0,
  twoFactorEnabled: false,
};

function state(overrides: Partial<AuthState> = {}): AuthState {
  return { mode: 'accounts', user: ADMIN, pendingTwoFactor: false, ...overrides };
}

const INITIAL = {
  mode: null,
  user: null,
  pendingTwoFactor: false,
  loaded: false,
  error: null,
};

beforeEach(() => {
  server.calls = [];
  server.answer = state();
  server.failWith = null;
  useAuthStore.setState(INITIAL);
});

describe('refresh', () => {
  it('starts not loaded, so the page waits instead of flashing the wrong screen', () => {
    expect(useAuthStore.getState().loaded).toBe(false);
    expect(useAuthStore.getState().mode).toBeNull();
  });

  it('takes the mode, the account and the pending code step from the answer', async () => {
    server.answer = state({ pendingTwoFactor: true, user: null });
    await useAuthStore.getState().refresh();

    expect(useAuthStore.getState()).toMatchObject({
      mode: 'accounts',
      user: null,
      pendingTwoFactor: true,
      loaded: true,
      error: null,
    });
  });

  it('says what the server said when it answers with an error, and counts as loaded', async () => {
    server.failWith = new ApiError('the database is locked', 'internal', 500);
    await useAuthStore.getState().refresh();

    expect(useAuthStore.getState().loaded).toBe(true);
    expect(useAuthStore.getState().error).toBe('the database is locked');
  });

  it('calls the service unreachable when the failure is not the server speaking', async () => {
    server.failWith = new Error('boom');
    await useAuthStore.getState().refresh();
    expect(useAuthStore.getState().error).toBe('the service is unreachable');
  });

  it('keeps the last known account when a refresh fails', async () => {
    await useAuthStore.getState().refresh();
    server.failWith = new ApiError('down', 'network', 0);
    await useAuthStore.getState().refresh();

    expect(useAuthStore.getState().user).toEqual(ADMIN);
    expect(useAuthStore.getState().error).toBe('down');
  });

  it('clears an earlier error once the service answers again', async () => {
    server.failWith = new ApiError('down', 'network', 0);
    await useAuthStore.getState().refresh();
    server.failWith = null;
    await useAuthStore.getState().refresh();
    expect(useAuthStore.getState().error).toBeNull();
  });
});

describe('the form actions', () => {
  it('chooses an open recorder and takes the new mode', async () => {
    server.answer = state({ mode: 'open', user: null });
    await useAuthStore.getState().chooseOpen();

    expect(server.calls).toEqual(['chooseOpen']);
    expect(useAuthStore.getState()).toMatchObject({ mode: 'open', user: null });
  });

  it('sets up the first account and signs it in', async () => {
    await useAuthStore.getState().setUp('owner@example.com', 'a long password');

    expect(server.calls).toEqual(['setUp owner@example.com a long password']);
    expect(useAuthStore.getState()).toMatchObject({ mode: 'accounts', user: ADMIN });
  });

  it('signs in with a password', async () => {
    await useAuthStore.getState().logIn('owner@example.com', 'a long password');

    expect(server.calls).toEqual(['logIn owner@example.com a long password']);
    expect(useAuthStore.getState().user).toEqual(ADMIN);
  });

  it('holds at the code step when the account has two factor sign in', async () => {
    server.answer = state({ user: null, pendingTwoFactor: true });
    await useAuthStore.getState().logIn('owner@example.com', 'a long password');

    expect(useAuthStore.getState()).toMatchObject({ user: null, pendingTwoFactor: true });
  });

  it('finishes the sign in with the code', async () => {
    useAuthStore.setState({ pendingTwoFactor: true });
    await useAuthStore.getState().verifyCode('123456');

    expect(server.calls).toEqual(['verifyLogin 123456']);
    expect(useAuthStore.getState()).toMatchObject({ user: ADMIN, pendingTwoFactor: false });
  });

  it('goes back from the code step to the password by signing out, without a reload', async () => {
    useAuthStore.setState({ mode: 'accounts', pendingTwoFactor: true });
    server.answer = state({ user: null, pendingTwoFactor: false });
    await useAuthStore.getState().backToPassword();

    expect(server.calls).toEqual(['logOut']);
    expect(useAuthStore.getState()).toMatchObject({ user: null, pendingTwoFactor: false });
  });

  it('changes the password and leaves the rest of the state alone', async () => {
    useAuthStore.setState({ mode: 'accounts', user: ADMIN, loaded: true });
    await useAuthStore.getState().changePassword('old password', 'new password');

    expect(server.calls).toEqual(['changePassword old password new password']);
    expect(useAuthStore.getState()).toMatchObject({ mode: 'accounts', user: ADMIN, loaded: true });
  });

  it.each([
    ['chooseOpen', () => useAuthStore.getState().chooseOpen()],
    ['setUp', () => useAuthStore.getState().setUp('a@b.c', 'password1')],
    ['logIn', () => useAuthStore.getState().logIn('a@b.c', 'wrong')],
    ['verifyCode', () => useAuthStore.getState().verifyCode('000000')],
    ['backToPassword', () => useAuthStore.getState().backToPassword()],
    ['changePassword', () => useAuthStore.getState().changePassword('wrong', 'new password')],
  ])('%s throws the error for the form to show, and changes nothing', async (_name, action) => {
    useAuthStore.setState({ mode: 'accounts', user: null, pendingTwoFactor: true, loaded: true });
    const refused = new ApiError('the email or password is not right', 'unauthorized', 401);
    server.failWith = refused;

    await expect(action()).rejects.toBe(refused);
    expect(useAuthStore.getState()).toMatchObject({
      mode: 'accounts',
      user: null,
      pendingTwoFactor: true,
      error: null,
    });
  });
});

describe('logOut', () => {
  let assign: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    assign = vi.fn();
    vi.stubGlobal('window', { location: { assign } });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('signs out, then reloads the page from the top', async () => {
    await useAuthStore.getState().logOut();
    expect(server.calls).toEqual(['logOut']);
    expect(assign).toHaveBeenCalledWith('/');
  });

  it('reloads even when the sign out request fails, and still reports the failure', async () => {
    server.failWith = new ApiError('the service is unreachable', 'network', 0);
    await expect(useAuthStore.getState().logOut()).rejects.toBeInstanceOf(ApiError);
    expect(assign).toHaveBeenCalledWith('/');
  });
});

describe('canAdminister', () => {
  it('lets anyone change things before accounts exist and on an open recorder', () => {
    expect(canAdminister(null, null)).toBe(true);
    expect(canAdminister('undecided', null)).toBe(true);
    expect(canAdminister('open', null)).toBe(true);
  });

  it('lets only an admin change things with accounts on', () => {
    expect(canAdminister('accounts', ADMIN)).toBe(true);
    expect(canAdminister('accounts', { ...ADMIN, role: 'listener' })).toBe(false);
    expect(canAdminister('accounts', null)).toBe(false);
  });
});
