/**
 * Properties of the settings draft over generated sequences of edits.
 *
 * The draft promises that it holds only real changes, so the change count and the Save button tell the
 * truth. The example tests walk a few edit sequences; these generate long ones, drawing values that often
 * equal what is stored so edits back to the original are common, and check after every step that the
 * draft holds nothing equal to the stored value, that stored plus draft is what the edits asked for, and
 * that discarding or editing everything back always lands on the stored settings.
 */

import fc from 'fast-check';
import { beforeEach, describe, expect, it } from 'vitest';

import type { Settings, SettingsPatch } from '@/api/types';

import { EDITABLE_FIELDS, useSettingsStore } from '../useSettingsStore';

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

const DEFAULTS: Settings = {
  ...STORED,
  gain: 2,
  retentionHours: 72,
  soundSensitivity: 'high',
};

/** Small domains per field, each including the stored value, so an edit back to it turns up often. */
const patch: fc.Arbitrary<SettingsPatch> = fc.record(
  {
    gain: fc.constantFrom(1, 2, 0.5),
    segmentSeconds: fc.constantFrom(10, 30, 60),
    retentionHours: fc.constantFrom(24, 48, null),
    autoStart: fc.boolean(),
    autoStartDelaySeconds: fc.constantFrom(0, 5, 30),
    recordingSampleRate: fc.constantFrom(null, 16_000, 48_000),
    recordingsDir: fc.constantFrom(null, '/mnt/audio', 'recordings'),
    checkForUpdates: fc.boolean(),
    soundSensitivity: fc.constantFrom('low' as const, 'medium' as const, 'high' as const),
  },
  { requiredKeys: [] },
);

function editable(settings: Settings | SettingsPatch): SettingsPatch {
  const picked: Record<string, unknown> = {};
  for (const key of EDITABLE_FIELDS) {
    picked[key] = (settings as Record<string, unknown>)[key];
  }
  return picked as SettingsPatch;
}

function expectDraftHoldsOnlyChanges(): void {
  const { draft } = useSettingsStore.getState();
  for (const [key, value] of Object.entries(draft)) {
    expect(Object.is(value, STORED[key as keyof Settings])).toBe(false);
  }
}

describe('useSettingsStore draft, over any sequence of edits', () => {
  beforeEach(() => {
    useSettingsStore.setState({ settings: STORED, defaults: DEFAULTS, draft: {}, error: null });
  });

  it('holds only real changes, and stored plus draft is what the edits asked for', () => {
    fc.assert(
      fc.property(fc.array(patch, { maxLength: 12 }), (edits) => {
        useSettingsStore.setState({ draft: {} });
        let wanted: SettingsPatch = {};
        for (const edit of edits) {
          useSettingsStore.getState().edit(edit);
          wanted = { ...wanted, ...edit };
          expectDraftHoldsOnlyChanges();
          const { draft } = useSettingsStore.getState();
          expect(editable({ ...STORED, ...draft })).toEqual(editable({ ...STORED, ...wanted }));
        }
      }),
    );
  });

  it('returns to what is stored on discard, or when every change is edited back', () => {
    fc.assert(
      fc.property(fc.array(patch, { maxLength: 12 }), fc.boolean(), (edits, discard) => {
        useSettingsStore.setState({ draft: {} });
        for (const edit of edits) {
          useSettingsStore.getState().edit(edit);
        }

        if (discard) {
          useSettingsStore.getState().discard();
        } else {
          const { draft } = useSettingsStore.getState();
          const back: Record<string, unknown> = {};
          for (const key of Object.keys(draft)) {
            back[key] = STORED[key as keyof Settings];
          }
          useSettingsStore.getState().edit(back as SettingsPatch);
        }

        expect(useSettingsStore.getState().draft).toEqual({});
      }),
    );
  });

  it('lands on the defaults after staging them, whatever was edited before', () => {
    fc.assert(
      fc.property(fc.array(patch, { maxLength: 8 }), (edits) => {
        useSettingsStore.setState({ draft: {} });
        for (const edit of edits) {
          useSettingsStore.getState().edit(edit);
        }
        useSettingsStore.getState().stageDefaults();

        const { draft } = useSettingsStore.getState();
        expect(editable({ ...STORED, ...draft })).toEqual(editable(DEFAULTS));
        expectDraftHoldsOnlyChanges();
      }),
    );
  });
});
