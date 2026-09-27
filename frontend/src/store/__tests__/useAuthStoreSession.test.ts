// @vitest-environment jsdom
/**
 * A session ending under the page: any request that comes back 401 sends the page back to sign in.
 *
 * This runs the real API client over a stubbed `fetch`, with the handler registered the way `AuthGate`
 * registers it, so what is proven is the whole chain: a store that knows nothing about logins makes a
 * request, the client sees the 401, the auth store refreshes and ends up signed out. Also covers the
 * `useCanAdminister` hook, which follows the store as the account changes.
 */

import '@/test/dom';

import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { setUnauthorizedHandler } from '@/api/client';
import type { AuthState, User } from '@/api/types';
import { useStatusStore } from '@/store/useStatusStore';

import { useAuthStore, useCanAdminister } from '../useAuthStore';

const ADMIN: User = {
  id: 1,
  email: 'owner@example.com',
  role: 'admin',
  createdAtMs: 0,
  twoFactorEnabled: false,
};

let authAnswer: AuthState = { mode: 'accounts', user: null, pendingTwoFactor: false };
let requested: string[] = [];

function reply(body: unknown, status: number): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
}

beforeEach(() => {
  requested = [];
  authAnswer = { mode: 'accounts', user: null, pendingTwoFactor: false };
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => {
      requested.push(url);
      if (url === '/api/auth/state') {
        return reply(authAnswer, 200);
      }
      if (url === '/api/auth/login') {
        return reply({ error: { code: 'unauthorized', message: 'the email or password is not right' } }, 401);
      }
      return reply({ error: { code: 'unauthorized', message: 'sign in first' } }, 401);
    }),
  );
  useAuthStore.setState({ mode: 'accounts', user: ADMIN, pendingTwoFactor: false, loaded: true, error: null });
  // Registered as AuthGate registers it.
  setUnauthorizedHandler(() => void useAuthStore.getState().refresh());
});

afterEach(() => {
  setUnauthorizedHandler(null);
  vi.unstubAllGlobals();
});

describe('a session that ends between requests', () => {
  it('sends the page back to sign in when an unrelated store gets a 401', async () => {
    await useStatusStore.getState().refresh();

    await vi.waitFor(() => {
      expect(useAuthStore.getState().user).toBeNull();
    });
    expect(requested).toEqual(['/api/status', '/api/auth/state']);
    expect(useAuthStore.getState().mode).toBe('accounts');
  });

  it('leaves the signed in account alone when the sign in form is refused', async () => {
    await expect(useAuthStore.getState().logIn('owner@example.com', 'wrong')).rejects.toMatchObject({
      status: 401,
    });
    expect(requested).toEqual(['/api/auth/login']);
    expect(useAuthStore.getState().user).toEqual(ADMIN);
  });
});

describe('useCanAdminister', () => {
  it('follows the account as it changes', () => {
    const { result } = renderHook(() => useCanAdminister());
    expect(result.current).toBe(true);

    act(() => {
      useAuthStore.setState({ user: { ...ADMIN, role: 'listener' } });
    });
    expect(result.current).toBe(false);

    act(() => {
      useAuthStore.setState({ mode: 'open', user: null });
    });
    expect(result.current).toBe(true);
  });
});
