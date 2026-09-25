import { beforeEach, describe, expect, it } from 'vitest';

import type { Settings } from '@/api/types';
import { shortensRetention } from '@/features/settings/SettingsActionBar';
import { useSettingsStore } from '../useSettingsStore';

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
};

const DEFAULTS: Settings = { ...STORED };

describe('useSettingsStore draft', () => {
  beforeEach(() => {
    useSettingsStore.setState({ settings: STORED, defaults: DEFAULTS, draft: {}, error: null });
  });

  it('stages only what differs from what is stored', () => {
    useSettingsStore.getState().edit({ gain: 2 });
    expect(useSettingsStore.getState().draft).toEqual({ gain: 2 });
  });

  it('drops a value edited back to what is stored', () => {
    const { edit } = useSettingsStore.getState();
    edit({ gain: 2 });
    edit({ gain: 1 });

    // Nothing would be written, so nothing should be counted as pending.
    expect(useSettingsStore.getState().draft).toEqual({});
  });

  it('accumulates edits across several fields', () => {
    const { edit } = useSettingsStore.getState();
    edit({ gain: 2 });
    edit({ autoStart: false });
    edit({ segmentSeconds: 30 });

    expect(Object.keys(useSettingsStore.getState().draft).sort()).toEqual([
      'autoStart',
      'gain',
      'segmentSeconds',
    ]);
  });

  it('treats keeping forever as a real change rather than an absent value', () => {
    useSettingsStore.getState().edit({ retentionHours: null });
    const { draft } = useSettingsStore.getState();

    expect('retentionHours' in draft).toBe(true);
    expect(draft.retentionHours).toBeNull();
  });

  it('recognises returning to a finite window from forever', () => {
    useSettingsStore.setState({ settings: { ...STORED, retentionHours: null } });
    useSettingsStore.getState().edit({ retentionHours: 24 });
    expect(useSettingsStore.getState().draft).toEqual({ retentionHours: 24 });
  });

  it('discards everything pending', () => {
    const { edit, discard } = useSettingsStore.getState();
    edit({ gain: 3, autoStart: false });
    discard();

    expect(useSettingsStore.getState().draft).toEqual({});
  });

  it('stages defaults only where they differ from what is stored', () => {
    useSettingsStore.setState({ settings: { ...STORED, gain: 3, retentionHours: 168 } });
    useSettingsStore.getState().stageDefaults();

    // autoStart and the rest already match, so restoring defaults is a two field change.
    expect(useSettingsStore.getState().draft).toEqual({ gain: 1, retentionHours: 24 });
  });

  it('restores an immediate auto start along with the other defaults', () => {
    useSettingsStore.setState({ settings: { ...STORED, autoStartDelaySeconds: 30 } });
    useSettingsStore.getState().stageDefaults();

    expect(useSettingsStore.getState().draft).toEqual({ autoStartDelaySeconds: 0 });
  });

  it('treats matching the device rate as a real change rather than an absent value', () => {
    useSettingsStore.setState({ settings: { ...STORED, recordingSampleRate: 16_000 } });
    useSettingsStore.getState().edit({ recordingSampleRate: null });
    const { draft } = useSettingsStore.getState();

    expect('recordingSampleRate' in draft).toBe(true);
    expect(draft.recordingSampleRate).toBeNull();
  });

  it('stages a lower recording rate', () => {
    useSettingsStore.getState().edit({ recordingSampleRate: 16_000 });
    expect(useSettingsStore.getState().draft).toEqual({ recordingSampleRate: 16_000 });
  });

  it('stages nothing when already at the defaults', () => {
    useSettingsStore.getState().stageDefaults();
    expect(useSettingsStore.getState().draft).toEqual({});
  });
});

describe('shortensRetention', () => {
  it('is false when the window is unchanged or grows', () => {
    expect(shortensRetention(STORED, STORED)).toBe(false);
    expect(shortensRetention(STORED, { ...STORED, retentionHours: 168 })).toBe(false);
  });

  it('is true when a finite window shrinks', () => {
    expect(shortensRetention({ ...STORED, retentionHours: 168 }, STORED)).toBe(true);
  });

  it('is true when coming back from keeping forever, however long the new window', () => {
    const forever = { ...STORED, retentionHours: null };
    expect(shortensRetention(forever, { ...STORED, retentionHours: 8760 })).toBe(true);
  });

  it('is false when switching to keeping forever, which deletes nothing', () => {
    expect(shortensRetention(STORED, { ...STORED, retentionHours: null })).toBe(false);
  });

  it('is false before the settings have loaded', () => {
    expect(shortensRetention(null, STORED)).toBe(false);
    expect(shortensRetention(STORED, null)).toBe(false);
  });
});
