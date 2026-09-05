/** Runtime preferences, mirrored from the settings endpoint. */

import { create } from 'zustand';

import { api, ApiError } from '@/api/client';
import type { Settings, SettingsPatch } from '@/api/types';

type SettingsState = {
  settings: Settings | null;
  /** What a reset restores, fetched from the server so the UI never keeps its own copy. */
  defaults: Settings | null;
  loading: boolean;
  saving: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  update: (patch: SettingsPatch) => Promise<void>;
  reset: () => Promise<void>;
};

export const useSettingsStore = create<SettingsState>((set, get) => ({
  settings: null,
  defaults: null,
  loading: false,
  saving: false,
  error: null,

  refresh: async () => {
    set({ loading: true });
    try {
      const [settings, defaults] = await Promise.all([api.settings(), api.settingsDefaults()]);
      set({ settings, defaults, error: null });
    } catch (cause) {
      set({ error: cause instanceof ApiError ? cause.message : 'could not load settings' });
    } finally {
      set({ loading: false });
    }
  },

  reset: async () => {
    set({ saving: true });
    try {
      // No optimistic update here: the defaults live on the server, and guessing them in the UI is how
      // the two drift apart.
      set({ settings: await api.resetSettings(), error: null });
    } catch (cause) {
      set({ error: cause instanceof ApiError ? cause.message : 'could not reset settings' });
    } finally {
      set({ saving: false });
    }
  },

  update: async (patch) => {
    const previous = get().settings;
    // Update optimistically so a slider tracks the pointer instead of snapping back on every frame while
    // the request is in flight. The response is authoritative and replaces it, clamping included.
    if (previous) {
      set({ settings: { ...previous, ...patch } });
    }

    set({ saving: true });
    try {
      set({ settings: await api.updateSettings(patch), error: null });
    } catch (cause) {
      set({
        settings: previous,
        error: cause instanceof ApiError ? cause.message : 'could not save settings',
      });
    } finally {
      set({ saving: false });
    }
  },
}));
