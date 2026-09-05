/** Input device list and selection. */

import { create } from 'zustand';

import { api, ApiError } from '@/api/client';
import type { InputDevice } from '@/api/types';

type DeviceState = {
  devices: InputDevice[];
  loading: boolean;
  selecting: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  select: (deviceId: string | null) => Promise<void>;
};

export const useDeviceStore = create<DeviceState>((set, get) => ({
  devices: [],
  loading: false,
  selecting: false,
  error: null,

  refresh: async () => {
    set({ loading: true });
    try {
      set({ devices: await api.devices(), error: null });
    } catch (cause) {
      set({ error: cause instanceof ApiError ? cause.message : 'could not list input devices' });
    } finally {
      set({ loading: false });
    }
  },

  select: async (deviceId) => {
    set({ selecting: true, error: null });
    try {
      await api.selectDevice(deviceId);
      // Selecting restarts capture on the backend, so the list is refetched: availability and the
      // negotiated sample rate can both differ on the new device.
      await get().refresh();
    } catch (cause) {
      set({ error: cause instanceof ApiError ? cause.message : 'could not select that device' });
    } finally {
      set({ selecting: false });
    }
  },
}));
