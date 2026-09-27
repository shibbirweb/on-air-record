/**
 * The storage store: disk usage and the recording sessions, fetched together.
 *
 * The two requests go out side by side and land in one update, so the panel never shows usage from one
 * moment next to sessions from another; a failure of either keeps both as they were.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { RecordingSession, Storage } from '@/api/types';

const server = vi.hoisted(() => ({
  calls: [] as string[],
  storageFails: null as Error | null,
  sessionsFails: null as Error | null,
  bytes: 1_000,
}));

vi.mock('@/api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/api/client')>();
  return {
    ...actual,
    api: {
      storage: async () => {
        server.calls.push('storage');
        if (server.storageFails) {
          throw server.storageFails;
        }
        return { bytes: server.bytes, segmentCount: 3 };
      },
      sessions: async () => {
        server.calls.push('sessions');
        if (server.sessionsFails) {
          throw server.sessionsFails;
        }
        return [{ id: 1 }, { id: 2 }];
      },
    },
  };
});

const { ApiError } = await import('@/api/client');
const { useStorageStore } = await import('../useStorageStore');

beforeEach(() => {
  server.calls = [];
  server.storageFails = null;
  server.sessionsFails = null;
  server.bytes = 1_000;
  useStorageStore.setState({ storage: null, sessions: [], error: null });
});

describe('useStorageStore', () => {
  it('fetches usage and sessions side by side and stores both', async () => {
    await useStorageStore.getState().refresh();

    expect([...server.calls].sort()).toEqual(['sessions', 'storage']);
    expect(useStorageStore.getState().storage).toMatchObject({ bytes: 1_000, segmentCount: 3 });
    expect(useStorageStore.getState().sessions.map((session: RecordingSession) => session.id)).toEqual([
      1,
      2,
    ]);
    expect(useStorageStore.getState().error).toBeNull();
  });

  it.each([
    ['usage', () => (server.storageFails = new ApiError('disk not mounted', 'internal', 500))],
    ['sessions', () => (server.sessionsFails = new ApiError('disk not mounted', 'internal', 500))],
  ])('keeps both as they were and says why when the %s request fails', async (_which, fail) => {
    await useStorageStore.getState().refresh();
    server.bytes = 2_000;
    fail();
    await useStorageStore.getState().refresh();

    expect((useStorageStore.getState().storage as Storage).bytes).toBe(1_000);
    expect(useStorageStore.getState().sessions).toHaveLength(2);
    expect(useStorageStore.getState().error).toBe('disk not mounted');
  });

  it('uses its own words when the failure is not the server speaking', async () => {
    server.storageFails = new Error('boom');
    await useStorageStore.getState().refresh();
    expect(useStorageStore.getState().error).toBe('could not load storage usage');
  });

  it('clears an earlier error on success', async () => {
    useStorageStore.setState({ error: 'old problem' });
    await useStorageStore.getState().refresh();
    expect(useStorageStore.getState().error).toBeNull();
  });
});
