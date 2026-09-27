/**
 * The update store actions that `useUpdateStore.test.ts` leaves out: reading the service's last answer,
 * asking it to check now, and reading the dismissed version back when the page loads.
 *
 * A request error here is a failure to reach this service, kept apart from `status.error`, which is the
 * service failing to reach GitHub; the two are shown differently, so both paths are asserted.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { UpdateStatus } from '@/api/types';

const server = vi.hoisted(() => ({
  calls: [] as string[],
  failWith: null as Error | null,
  gate: null as Promise<void> | null,
}));

function status(available: string | null, error: string | null = null): UpdateStatus {
  const release = available
    ? { version: available, tag: `v${available}`, prerelease: false, publishedAtMs: 0, notes: '', url: '' }
    : null;
  return {
    currentVersion: '0.6.0',
    channel: 'stable',
    automatic: true,
    checkedAtMs: 0,
    error,
    available: release,
    releases: release ? [release] : [],
    install: { kind: 'manual', dir: null, os: 'linux', target: 'x86_64-unknown-linux-gnu' },
    releasesUrl: '',
  };
}

vi.mock('@/api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/api/client')>();
  const answer = async (name: string, result: UpdateStatus) => {
    server.calls.push(name);
    if (server.gate) {
      await server.gate;
    }
    if (server.failWith) {
      throw server.failWith;
    }
    return result;
  };
  return {
    ...actual,
    api: {
      updates: () => answer('updates', status('0.7.0')),
      checkForUpdates: () => answer('checkForUpdates', status('0.7.1')),
    },
  };
});

const { ApiError } = await import('@/api/client');
const { useUpdateStore } = await import('../useUpdateStore');

beforeEach(() => {
  server.calls = [];
  server.failWith = null;
  server.gate = null;
  useUpdateStore.setState({ status: null, checking: false, requestError: null });
});

describe('refresh', () => {
  it('reads the last answer the service has, and clears an earlier request error', async () => {
    useUpdateStore.setState({ requestError: 'old problem' });
    await useUpdateStore.getState().refresh();

    expect(server.calls).toEqual(['updates']);
    expect(useUpdateStore.getState().status?.available?.version).toBe('0.7.0');
    expect(useUpdateStore.getState().requestError).toBeNull();
  });

  it('keeps the last answer and records the request error when the service cannot be reached', async () => {
    await useUpdateStore.getState().refresh();
    server.failWith = new ApiError('connection refused', 'network', 0);
    await useUpdateStore.getState().refresh();

    expect(useUpdateStore.getState().status?.available?.version).toBe('0.7.0');
    expect(useUpdateStore.getState().requestError).toBe('connection refused');
  });

  it('uses its own words when the failure is not the server speaking', async () => {
    server.failWith = new Error('boom');
    await useUpdateStore.getState().refresh();
    expect(useUpdateStore.getState().requestError).toBe('could not ask the service about updates');
  });
});

describe('checkNow', () => {
  it('asks the service to check and takes its fresh answer', async () => {
    await useUpdateStore.getState().checkNow();
    expect(server.calls).toEqual(['checkForUpdates']);
    expect(useUpdateStore.getState().status?.available?.version).toBe('0.7.1');
    expect(useUpdateStore.getState().requestError).toBeNull();
  });

  it('is checking while the request is in flight, and not after', async () => {
    let release = () => undefined as void;
    server.gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const pending = useUpdateStore.getState().checkNow();
    expect(useUpdateStore.getState().checking).toBe(true);

    release();
    await pending;
    expect(useUpdateStore.getState().checking).toBe(false);
  });

  it('records a request error and stops checking when the service cannot be reached', async () => {
    server.failWith = new ApiError('sign in as an admin', 'forbidden', 403);
    await useUpdateStore.getState().checkNow();

    expect(useUpdateStore.getState().requestError).toBe('sign in as an admin');
    expect(useUpdateStore.getState().checking).toBe(false);
    expect(useUpdateStore.getState().status).toBeNull();
  });
});

describe('the dismissed version at page load', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  it('is read back from this browser', async () => {
    vi.stubGlobal('window', {
      localStorage: { getItem: (key: string) => (key === 'oar.updates.dismissed' ? '0.7.0' : null) },
    });
    vi.resetModules();
    const fresh = await import('../useUpdateStore');
    expect(fresh.useUpdateStore.getState().dismissed).toBe('0.7.0');
  });

  it('is nothing when storage is blocked, so the banner comes back', async () => {
    vi.stubGlobal('window', {
      localStorage: {
        getItem: () => {
          throw new Error('blocked');
        },
      },
    });
    vi.resetModules();
    const fresh = await import('../useUpdateStore');
    expect(fresh.useUpdateStore.getState().dismissed).toBeNull();
  });
});
