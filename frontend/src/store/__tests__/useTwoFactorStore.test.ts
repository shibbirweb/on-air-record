/**
 * The two factor store: the signed in account's own second factor, and the steps to turn it on, off, or
 * issue new recovery codes.
 *
 * Switching it on or off also refreshes the auth store, because the account's `twoFactorEnabled` flag
 * lives there, and the settings page reads it from there. The actions throw rather than store an error,
 * so the dialog can show the problem next to its field; both halves are tested.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { TwoFactorStatus } from '@/api/types';

const server = vi.hoisted(() => ({
  calls: [] as string[],
  status: { enabled: false, recoveryCodesLeft: 0 } as TwoFactorStatus,
  failWith: null as Error | null,
  statusFails: false,
}));

vi.mock('@/api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/api/client')>();
  const refuse = () => {
    if (server.failWith) {
      throw server.failWith;
    }
  };
  return {
    ...actual,
    api: {
      twoFactorStatus: async () => {
        server.calls.push('status');
        if (server.statusFails) {
          throw new Error('down');
        }
        return { ...server.status };
      },
      beginTwoFactorSetup: async () => {
        server.calls.push('setup');
        refuse();
        return { secretKey: 'ABCD EFGH', otpauthUri: 'otpauth://totp/x', qrSvg: '<svg/>' };
      },
      enableTwoFactor: async (code: string) => {
        server.calls.push(`enable ${code}`);
        refuse();
        server.status = { enabled: true, recoveryCodesLeft: 2 };
        return ['aaaaa-bbbbb', 'ccccc-ddddd'];
      },
      disableTwoFactor: async (password: string) => {
        server.calls.push(`disable ${password}`);
        refuse();
        server.status = { enabled: false, recoveryCodesLeft: 0 };
      },
      regenerateRecoveryCodes: async (password: string) => {
        server.calls.push(`regenerate ${password}`);
        refuse();
        server.status = { ...server.status, recoveryCodesLeft: 1 };
        return ['eeeee-fffff'];
      },
      authState: async () => {
        server.calls.push('authState');
        return { mode: 'accounts', user: null, pendingTwoFactor: false };
      },
    },
  };
});

const { ApiError } = await import('@/api/client');
const { useTwoFactorStore } = await import('../useTwoFactorStore');
const { useAuthStore } = await import('../useAuthStore');

beforeEach(() => {
  server.calls = [];
  server.status = { enabled: false, recoveryCodesLeft: 0 };
  server.failWith = null;
  server.statusFails = false;
  useTwoFactorStore.setState({ status: null });
  useAuthStore.setState({ mode: null, user: null, pendingTwoFactor: false, loaded: false, error: null });
});

describe('refresh', () => {
  it('reads whether two factor sign in is on', async () => {
    server.status = { enabled: true, recoveryCodesLeft: 8 };
    await useTwoFactorStore.getState().refresh();
    expect(useTwoFactorStore.getState().status).toEqual({ enabled: true, recoveryCodesLeft: 8 });
  });

  it('keeps the last known status when the read fails, without throwing', async () => {
    useTwoFactorStore.setState({ status: { enabled: true, recoveryCodesLeft: 3 } });
    server.statusFails = true;
    await expect(useTwoFactorStore.getState().refresh()).resolves.toBeUndefined();
    expect(useTwoFactorStore.getState().status).toEqual({ enabled: true, recoveryCodesLeft: 3 });
  });
});

describe('beginSetup', () => {
  it('hands back the secret and QR code from the server, storing nothing', async () => {
    const setup = await useTwoFactorStore.getState().beginSetup();
    expect(setup).toEqual({ secretKey: 'ABCD EFGH', otpauthUri: 'otpauth://totp/x', qrSvg: '<svg/>' });
    expect(useTwoFactorStore.getState().status).toBeNull();
  });
});

describe('enable', () => {
  it('returns the recovery codes and refreshes both its own status and the account', async () => {
    const codes = await useTwoFactorStore.getState().enable('123456');

    expect(codes).toEqual(['aaaaa-bbbbb', 'ccccc-ddddd']);
    expect(server.calls[0]).toBe('enable 123456');
    expect([...server.calls.slice(1)].sort()).toEqual(['authState', 'status']);
    expect(useTwoFactorStore.getState().status).toEqual({ enabled: true, recoveryCodesLeft: 2 });
    expect(useAuthStore.getState().loaded).toBe(true);
  });

  it('throws a wrong code for the dialog to show, refreshing nothing', async () => {
    const refused = new ApiError('that code is not right', 'unauthorized', 401);
    server.failWith = refused;
    await expect(useTwoFactorStore.getState().enable('000000')).rejects.toBe(refused);
    expect(server.calls).toEqual(['enable 000000']);
  });
});

describe('disable', () => {
  it('turns it off and refreshes both its own status and the account', async () => {
    server.status = { enabled: true, recoveryCodesLeft: 8 };
    await useTwoFactorStore.getState().disable('a long password');

    expect(server.calls[0]).toBe('disable a long password');
    expect([...server.calls.slice(1)].sort()).toEqual(['authState', 'status']);
    expect(useTwoFactorStore.getState().status).toEqual({ enabled: false, recoveryCodesLeft: 0 });
  });

  it('throws a wrong password for the dialog to show, refreshing nothing', async () => {
    server.failWith = new ApiError('the password is not right', 'unauthorized', 401);
    await expect(useTwoFactorStore.getState().disable('wrong')).rejects.toMatchObject({
      message: 'the password is not right',
    });
    expect(server.calls).toEqual(['disable wrong']);
  });
});

describe('regenerate', () => {
  it('returns the new codes and refreshes its own status only, since the flag is unchanged', async () => {
    server.status = { enabled: true, recoveryCodesLeft: 0 };
    const codes = await useTwoFactorStore.getState().regenerate('a long password');

    expect(codes).toEqual(['eeeee-fffff']);
    expect(server.calls).toEqual(['regenerate a long password', 'status']);
    expect(useTwoFactorStore.getState().status).toEqual({ enabled: true, recoveryCodesLeft: 1 });
  });

  it('throws a wrong password for the dialog to show, refreshing nothing', async () => {
    server.failWith = new ApiError('the password is not right', 'unauthorized', 401);
    await expect(useTwoFactorStore.getState().regenerate('wrong')).rejects.toBeInstanceOf(ApiError);
    expect(server.calls).toEqual(['regenerate wrong']);
  });
});
