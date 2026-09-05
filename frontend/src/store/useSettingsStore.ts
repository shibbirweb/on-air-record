/**
 * Runtime preferences, edited as a draft and committed on demand.
 *
 * The settings page is explicit save: edits accumulate in `draft` and nothing reaches the server until
 * `save` is called. The draft holds only the fields that actually differ from what is stored, so the page
 * can say how many changes are pending and a value edited back to its original stops counting as one.
 *
 * The draft lives in the store rather than in the page, so navigating to the control room and back does
 * not silently discard work in progress.
 */

import { create } from 'zustand';

import { api, ApiError } from '@/api/client';
import type { Settings, SettingsPatch } from '@/api/types';

/** The fields the settings page owns. The input device is chosen elsewhere and is not part of a draft. */
export const EDITABLE_FIELDS = [
  'gain',
  'segmentSeconds',
  'retentionHours',
  'autoStart',
  'recordingsDir',
] as const satisfies readonly (keyof SettingsPatch)[];

export type EditableField = (typeof EDITABLE_FIELDS)[number];

type SettingsState = {
  settings: Settings | null;
  /** What a reset restores, fetched from the server so the UI never keeps its own copy. */
  defaults: Settings | null;
  /** Edits not yet sent. Only fields that differ from `settings` are present. */
  draft: SettingsPatch;
  loading: boolean;
  saving: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  /** Stage an edit. Values equal to what is stored are dropped rather than counted as changes. */
  edit: (patch: SettingsPatch) => void;
  /** Stage every default, so a reset is reviewed and saved like any other change. */
  stageDefaults: () => void;
  discard: () => void;
  save: () => Promise<void>;
};

export const useSettingsStore = create<SettingsState>((set, get) => ({
  settings: null,
  defaults: null,
  draft: {},
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

  edit: (patch) => {
    set((state) => {
      const merged: SettingsPatch = { ...state.draft, ...patch };
      const stored = state.settings;

      if (stored) {
        for (const key of Object.keys(merged) as EditableField[]) {
          // Editing a value back to what is stored removes it from the draft, so the change count and
          // the Save button reflect what would actually be written.
          if (Object.is(merged[key], stored[key])) {
            delete merged[key];
          }
        }
      }

      return { draft: merged, error: null };
    });
  },

  stageDefaults: () => {
    const { defaults } = get();
    if (!defaults) {
      return;
    }

    const patch: SettingsPatch = {};
    for (const key of EDITABLE_FIELDS) {
      // A deliberate widening: every field is assigned from the matching field of the same type.
      (patch as Record<string, unknown>)[key] = defaults[key];
    }
    get().edit(patch);
  },

  discard: () => set({ draft: {}, error: null }),

  save: async () => {
    const { draft } = get();
    if (Object.keys(draft).length === 0) {
      return;
    }

    set({ saving: true });
    try {
      // One request for the whole draft. The server applies it atomically, so a rejected recording
      // directory leaves every other pending change unsaved too rather than half applying them.
      set({ settings: await api.updateSettings(draft), draft: {}, error: null });
    } catch (cause) {
      set({ error: cause instanceof ApiError ? cause.message : 'could not save settings' });
    } finally {
      set({ saving: false });
    }
  },
}));
