/**
 * The status store: the polled service status, the build version, and starting and stopping capture.
 *
 * A failed poll is the normal sign that the service is restarting, so the rule worth protecting is that
 * it marks the service unreachable while keeping the last status on screen, rather than blanking it.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { ServiceStatus } from '@/api/types';

const server = vi.hoisted(() => ({
  calls: [] as string[],
  failWith: null as Error | null,
  healthFails: false,
  capturing: false,
  gate: null as Promise<void> | null,
}));

vi.mock('@/api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/api/client')>();
  const answer = async (name: string) => {
    server.calls.push(name);
    if (server.gate) {
      await server.gate;
    }
    if (server.failWith) {
      throw server.failWith;
    }
    if (name === 'startCapture') {
      server.capturing = true;
    }
    if (name === 'stopCapture') {
      server.capturing = false;
    }
    return { capture: { state: server.capturing ? 'recording' : 'idle' }, listeners: 0 };
  };
  return {
    ...actual,
    api: {
      status: () => answer('status'),
      startCapture: () => answer('startCapture'),
      stopCapture: () => answer('stopCapture'),
      health: async () => {
        server.calls.push('health');
        if (server.healthFails) {
          throw new Error('down');
        }
        return { status: 'ok', version: '0.8.1', uptimeMs: 1 };
      },
    },
  };
});

const { ApiError } = await import('@/api/client');
const { useStatusStore } = await import('../useStatusStore');

function captureState(): string | undefined {
  return (useStatusStore.getState().status as ServiceStatus | null)?.capture.state;
}

function holdRequests() {
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
  server.calls = [];
  server.failWith = null;
  server.healthFails = false;
  server.capturing = false;
  server.gate = null;
  useStatusStore.setState({ status: null, version: null, reachable: false, error: null, busy: false });
});

describe('refresh', () => {
  it('starts unreachable with nothing known', () => {
    expect(useStatusStore.getState().reachable).toBe(false);
    expect(useStatusStore.getState().status).toBeNull();
  });

  it('stores the status and marks the service reachable', async () => {
    await useStatusStore.getState().refresh();
    expect(captureState()).toBe('idle');
    expect(useStatusStore.getState().reachable).toBe(true);
    expect(useStatusStore.getState().error).toBeNull();
  });

  it('keeps the last status on screen, marked unreachable, when a poll fails', async () => {
    await useStatusStore.getState().refresh();
    server.failWith = new ApiError('connection refused', 'network', 0);
    await useStatusStore.getState().refresh();

    expect(captureState()).toBe('idle');
    expect(useStatusStore.getState().reachable).toBe(false);
    expect(useStatusStore.getState().error).toBe('connection refused');
  });

  it('calls the service unreachable when the failure is not the server speaking', async () => {
    server.failWith = new Error('boom');
    await useStatusStore.getState().refresh();
    expect(useStatusStore.getState().error).toBe('the service is unreachable');
  });

  it('recovers once the service answers again', async () => {
    server.failWith = new ApiError('connection refused', 'network', 0);
    await useStatusStore.getState().refresh();
    server.failWith = null;
    await useStatusStore.getState().refresh();

    expect(useStatusStore.getState().reachable).toBe(true);
    expect(useStatusStore.getState().error).toBeNull();
  });
});

describe('loadVersion', () => {
  it('reads the build version from the health endpoint', async () => {
    await useStatusStore.getState().loadVersion();
    expect(server.calls).toEqual(['health']);
    expect(useStatusStore.getState().version).toBe('0.8.1');
  });

  it('says nothing when the version cannot be read', async () => {
    server.healthFails = true;
    await expect(useStatusStore.getState().loadVersion()).resolves.toBeUndefined();
    expect(useStatusStore.getState().version).toBeNull();
    expect(useStatusStore.getState().error).toBeNull();
  });
});

describe.each([
  ['startCapture', 'recording', 'could not start capture'],
  ['stopCapture', 'idle', 'could not stop capture'],
] as const)('%s', (action, resulting, ownWords) => {
  beforeEach(() => {
    server.capturing = action === 'stopCapture';
  });

  it('takes the status the server answers with and marks the service reachable', async () => {
    await useStatusStore.getState()[action]();
    expect(server.calls).toEqual([action]);
    expect(captureState()).toBe(resulting);
    expect(useStatusStore.getState().reachable).toBe(true);
    expect(useStatusStore.getState().error).toBeNull();
  });

  it('is busy while the request is in flight, and not after', async () => {
    const release = holdRequests();
    const pending = useStatusStore.getState()[action]();
    expect(useStatusStore.getState().busy).toBe(true);
    release();
    await pending;
    expect(useStatusStore.getState().busy).toBe(false);
  });

  it('says what the server said when refused, and stops being busy', async () => {
    server.failWith = new ApiError('no input device', 'conflict', 409);
    await useStatusStore.getState()[action]();

    expect(useStatusStore.getState().error).toBe('no input device');
    expect(useStatusStore.getState().busy).toBe(false);
  });

  it('uses its own words when the failure is not the server speaking', async () => {
    server.failWith = new Error('boom');
    await useStatusStore.getState()[action]();
    expect(useStatusStore.getState().error).toBe(ownWords);
  });
});
