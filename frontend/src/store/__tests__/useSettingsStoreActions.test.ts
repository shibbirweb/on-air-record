/**
 * The settings store actions that `useSettingsStore.test.ts` leaves out: loading the settings with their
 * defaults, saving the draft, and the edges of editing (no settings loaded yet, no defaults yet, errors
 * cleared by the next edit or a discard).
 *
 * Save sends the whole draft in one request because the server applies it atomically, so a refusal must
 * leave every pending change staged, not just the one that failed. That is the case most worth guarding.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Settings, SettingsPatch } from '@/api/types';

const STORED: Settings = {
  inputDeviceId: null,
  gain: 1,
  segmentSeconds: 10,
  retentionHours: 24,
  autoStart: true,
  autoStartDelaySeconds: 0,
  frameMs: 100,
  recordingSampleRate: null,
  recordingsDir: null,
  effectiveRecordingsDir: '/srv/oar/recordings',
  checkForUpdates: true,
  soundSensitivity: 'medium',
};

const server = vi.hoisted(() => ({
  calls: [] as unknown[][],
  settingsFails: null as Error | null,
  defaultsFails: null as Error | null,
  saveFails: null as Error | null,
  gate: null as Promise<void> | null,
  stored: null as unknown,
}));

vi.mock('@/api/client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/api/client')>();
  return {
    ...actual,
    api: {
      settings: async () => {
        server.calls.push(['settings']);
        if (server.settingsFails) {
          throw server.settingsFails;
        }
        return { ...(server.stored as object) };
      },
      settingsDefaults: async () => {
        server.calls.push(['settingsDefaults']);
        if (server.defaultsFails) {
          throw server.defaultsFails;
        }
        return { ...(server.stored as object), gain: 1.5 };
      },
      updateSettings: async (patch: object) => {
        server.calls.push(['updateSettings', patch]);
        if (server.gate) {
          await server.gate;
        }
        if (server.saveFails) {
          throw server.saveFails;
        }
        server.stored = { ...(server.stored as object), ...patch };
        return { ...(server.stored as object) };
      },
    },
  };
});

const { ApiError } = await import('@/api/client');
const { useSettingsStore } = await import('../useSettingsStore');

beforeEach(() => {
  server.calls = [];
  server.settingsFails = null;
  server.defaultsFails = null;
  server.saveFails = null;
  server.gate = null;
  server.stored = { ...STORED };
  useSettingsStore.setState({
    settings: null,
    defaults: null,
    draft: {},
    loading: false,
    saving: false,
    error: null,
  });
});

describe('refresh', () => {
  it('loads the settings and the defaults together', async () => {
    await useSettingsStore.getState().refresh();

    expect(server.calls.map(([name]) => name).sort()).toEqual(['settings', 'settingsDefaults']);
    expect(useSettingsStore.getState().settings).toEqual(STORED);
    expect(useSettingsStore.getState().defaults?.gain).toBe(1.5);
    expect(useSettingsStore.getState().error).toBeNull();
  });

  it('is loading while the requests are in flight, and not after', async () => {
    const pending = useSettingsStore.getState().refresh();
    expect(useSettingsStore.getState().loading).toBe(true);
    await pending;
    expect(useSettingsStore.getState().loading).toBe(false);
  });

  it.each([
    ['settings', () => (server.settingsFails = new ApiError('sign in first', 'unauthorized', 401))],
    ['defaults', () => (server.defaultsFails = new ApiError('sign in first', 'unauthorized', 401))],
  ])('stores neither and says why when the %s request fails', async (_which, fail) => {
    fail();
    await useSettingsStore.getState().refresh();

    expect(useSettingsStore.getState().settings).toBeNull();
    expect(useSettingsStore.getState().defaults).toBeNull();
    expect(useSettingsStore.getState().error).toBe('sign in first');
    expect(useSettingsStore.getState().loading).toBe(false);
  });

  it('uses its own words when the failure is not the server speaking', async () => {
    server.settingsFails = new Error('boom');
    await useSettingsStore.getState().refresh();
    expect(useSettingsStore.getState().error).toBe('could not load settings');
  });

  it('keeps a draft in progress across a reload', async () => {
    await useSettingsStore.getState().refresh();
    useSettingsStore.getState().edit({ gain: 2 });
    await useSettingsStore.getState().refresh();
    expect(useSettingsStore.getState().draft).toEqual({ gain: 2 });
  });
});

describe('save', () => {
  beforeEach(async () => {
    await useSettingsStore.getState().refresh();
    server.calls = [];
  });

  it('sends nothing when there is nothing to save', async () => {
    await useSettingsStore.getState().save();
    expect(server.calls).toEqual([]);
    expect(useSettingsStore.getState().saving).toBe(false);
  });

  it('sends the whole draft in one request and takes the stored result', async () => {
    const patch: SettingsPatch = { gain: 2, retentionHours: null, autoStart: false };
    useSettingsStore.getState().edit(patch);
    await useSettingsStore.getState().save();

    expect(server.calls).toEqual([['updateSettings', patch]]);
    expect(useSettingsStore.getState().settings).toMatchObject(patch);
    expect(useSettingsStore.getState().draft).toEqual({});
    expect(useSettingsStore.getState().error).toBeNull();
  });

  it('is saving while the request is in flight, and not after', async () => {
    useSettingsStore.getState().edit({ gain: 2 });
    let release = () => undefined as void;
    server.gate = new Promise<void>((resolve) => {
      release = resolve;
    });

    const pending = useSettingsStore.getState().save();
    expect(useSettingsStore.getState().saving).toBe(true);
    release();
    await pending;
    expect(useSettingsStore.getState().saving).toBe(false);
  });

  it('keeps every pending change staged, and says why, when the server refuses', async () => {
    useSettingsStore.getState().edit({ gain: 2, recordingsDir: '/nowhere' });
    server.saveFails = new ApiError('the recordings directory is not writable', 'bad_request', 400);
    await useSettingsStore.getState().save();

    expect(useSettingsStore.getState().draft).toEqual({ gain: 2, recordingsDir: '/nowhere' });
    expect(useSettingsStore.getState().settings).toEqual(STORED);
    expect(useSettingsStore.getState().error).toBe('the recordings directory is not writable');
    expect(useSettingsStore.getState().saving).toBe(false);
  });

  it('uses its own words when the failure is not the server speaking', async () => {
    useSettingsStore.getState().edit({ gain: 2 });
    server.saveFails = new Error('boom');
    await useSettingsStore.getState().save();
    expect(useSettingsStore.getState().error).toBe('could not save settings');
  });
});

describe('editing edges', () => {
  it('stages everything as a change before the settings have loaded', () => {
    useSettingsStore.getState().edit({ gain: 1, autoStart: true });
    expect(useSettingsStore.getState().draft).toEqual({ gain: 1, autoStart: true });
  });

  it('clears an error as soon as something is edited', () => {
    useSettingsStore.setState({ settings: STORED, error: 'could not save settings' });
    useSettingsStore.getState().edit({ gain: 2 });
    expect(useSettingsStore.getState().error).toBeNull();
  });

  it('clears an error on discard', () => {
    useSettingsStore.setState({ settings: STORED, draft: { gain: 2 }, error: 'could not save settings' });
    useSettingsStore.getState().discard();
    expect(useSettingsStore.getState().error).toBeNull();
  });

  it('stages nothing for a reset before the defaults have loaded', () => {
    useSettingsStore.setState({ settings: { ...STORED, gain: 3 }, defaults: null });
    useSettingsStore.getState().stageDefaults();
    expect(useSettingsStore.getState().draft).toEqual({});
  });

  it('leaves the device choice out of a reset, since it is chosen elsewhere', () => {
    useSettingsStore.setState({
      settings: { ...STORED, inputDeviceId: 'usb-mic', soundSensitivity: 'high' },
      defaults: { ...STORED, inputDeviceId: null },
    });
    useSettingsStore.getState().stageDefaults();
    expect('inputDeviceId' in useSettingsStore.getState().draft).toBe(false);
  });

  it('puts the recording quality and the sound sensitivity back with everything else', () => {
    useSettingsStore.setState({
      settings: { ...STORED, recordingSampleRate: 16_000, soundSensitivity: 'high' },
      defaults: STORED,
      draft: {},
    });
    useSettingsStore.getState().stageDefaults();
    expect(useSettingsStore.getState().draft).toEqual({
      recordingSampleRate: null,
      soundSensitivity: 'medium',
    });
  });
});
