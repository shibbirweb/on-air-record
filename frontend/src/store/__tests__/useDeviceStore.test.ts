/**
 * The input device store: listing devices and choosing one.
 *
 * Choosing restarts capture on the server, so a successful choice is followed by a fresh list; that
 * ordering, and the two busy flags the panel shows while each step runs, are what is tested here along
 * with the error each failure leaves.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { InputDevice } from '@/api/types';

const server = vi.hoisted(() => ({
  devices: [] as InputDevice[],
  calls: [] as string[],
  listFails: null as Error | null,
  selectFails: null as Error | null,
  gate: null as Promise<void> | null,
}));

vi.mock('@/api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/api/client')>();
  return {
    ...actual,
    api: {
      devices: async () => {
        server.calls.push('devices');
        if (server.listFails) {
          throw server.listFails;
        }
        return server.devices.map((device) => ({ ...device }));
      },
      selectDevice: async (deviceId: string | null) => {
        server.calls.push(`select ${deviceId}`);
        if (server.gate) {
          await server.gate;
        }
        if (server.selectFails) {
          throw server.selectFails;
        }
        server.devices = server.devices.map((device) => ({
          ...device,
          isSelected: device.id === deviceId,
        }));
        return {};
      },
    },
  };
});

const { ApiError } = await import('@/api/client');
const { useDeviceStore } = await import('../useDeviceStore');

function device(id: string, isSelected = false): InputDevice {
  return { id, name: id, isDefault: false, isSelected, available: true, channels: 1, sampleRate: 48_000 };
}

beforeEach(() => {
  server.devices = [device('built-in', true), device('usb-mic')];
  server.calls = [];
  server.listFails = null;
  server.selectFails = null;
  server.gate = null;
  useDeviceStore.setState({ devices: [], loading: false, selecting: false, error: null });
});

describe('refresh', () => {
  it('lists the devices', async () => {
    await useDeviceStore.getState().refresh();
    expect(useDeviceStore.getState().devices.map((entry) => entry.id)).toEqual(['built-in', 'usb-mic']);
    expect(useDeviceStore.getState().error).toBeNull();
  });

  it('is loading while the request is in flight, and not after', async () => {
    const pending = useDeviceStore.getState().refresh();
    expect(useDeviceStore.getState().loading).toBe(true);
    await pending;
    expect(useDeviceStore.getState().loading).toBe(false);
  });

  it('keeps the last list and says why when listing fails', async () => {
    await useDeviceStore.getState().refresh();
    server.listFails = new ApiError('audio host unavailable', 'internal', 500);
    await useDeviceStore.getState().refresh();

    expect(useDeviceStore.getState().devices).toHaveLength(2);
    expect(useDeviceStore.getState().error).toBe('audio host unavailable');
    expect(useDeviceStore.getState().loading).toBe(false);
  });

  it('uses its own words when the failure is not the server speaking', async () => {
    server.listFails = new Error('boom');
    await useDeviceStore.getState().refresh();
    expect(useDeviceStore.getState().error).toBe('could not list input devices');
  });
});

describe('select', () => {
  it('chooses the device, then lists again to show what the server did', async () => {
    await useDeviceStore.getState().select('usb-mic');

    expect(server.calls).toEqual(['select usb-mic', 'devices']);
    expect(useDeviceStore.getState().devices.find((entry) => entry.isSelected)?.id).toBe('usb-mic');
    expect(useDeviceStore.getState().error).toBeNull();
  });

  it('passes null through to go back to the system default', async () => {
    await useDeviceStore.getState().select(null);
    expect(server.calls[0]).toBe('select null');
  });

  it('is selecting while the choice is in flight, and clears an old error at once', async () => {
    useDeviceStore.setState({ error: 'old problem' });
    let release = () => undefined as void;
    server.gate = new Promise<void>((resolve) => {
      release = resolve;
    });

    const pending = useDeviceStore.getState().select('usb-mic');
    expect(useDeviceStore.getState().selecting).toBe(true);
    expect(useDeviceStore.getState().error).toBeNull();

    release();
    await pending;
    expect(useDeviceStore.getState().selecting).toBe(false);
  });

  it('does not list again when the choice is refused, and says why', async () => {
    server.selectFails = new ApiError('that device is gone', 'not_found', 404);
    await useDeviceStore.getState().select('usb-mic');

    expect(server.calls).toEqual(['select usb-mic']);
    expect(useDeviceStore.getState().error).toBe('that device is gone');
    expect(useDeviceStore.getState().selecting).toBe(false);
  });

  it('uses its own words when choosing fails for another reason', async () => {
    server.selectFails = new Error('boom');
    await useDeviceStore.getState().select('usb-mic');
    expect(useDeviceStore.getState().error).toBe('could not select that device');
  });

  it('reports a failed relisting after a successful choice', async () => {
    server.listFails = new ApiError('audio host unavailable', 'internal', 500);
    await useDeviceStore.getState().select('usb-mic');

    expect(useDeviceStore.getState().error).toBe('audio host unavailable');
    expect(useDeviceStore.getState().selecting).toBe(false);
  });
});
