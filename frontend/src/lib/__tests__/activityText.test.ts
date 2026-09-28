/**
 * How the activity page words each entry. An admin reads these to answer "who did this, and when", so each
 * kind of event gets a plain sentence with the details that matter, and nothing is left as a raw kind name.
 */

import { describe, expect, it } from 'vitest';

import type { ActivityEvent, ActivityKind, Actor } from '@/api/types';
import { formatDateTime } from '@/lib/format';

import { describeActor, describeEvent, groupOf, spokenDuration } from '../activityText';

const KITCHEN = 'kitchen@example.com';

describe('describeActor', () => {
  it('names an account by its email, a guest as a guest, and the host as the host', () => {
    const account: Actor = { kind: 'account', userId: 2, email: KITCHEN };
    expect(describeActor(account)).toBe(KITCHEN);
    expect(describeActor({ kind: 'guest' })).toBe('A guest');
    expect(describeActor({ kind: 'host' })).toBe('The host command line');
  });
});

describe('spokenDuration', () => {
  it('reads like a person would say it', () => {
    expect(spokenDuration(0)).toBe('0 s');
    expect(spokenDuration(45_000)).toBe('45 s');
    expect(spokenDuration(59_999)).toBe('59 s');
    expect(spokenDuration(60_000)).toBe('1 min');
    expect(spokenDuration(9 * 60_000 + 30_000)).toBe('9 min');
    expect(spokenDuration(3_600_000)).toBe('1 h');
    expect(spokenDuration(3_600_000 + 5 * 60_000)).toBe('1 h 5 min');
    expect(spokenDuration(26 * 3_600_000)).toBe('26 h');
  });

  it('never shows a negative time', () => {
    expect(spokenDuration(-5_000)).toBe('0 s');
  });
});

describe('describeEvent', () => {
  const says = (event: ActivityEvent) => describeEvent(event);

  it('says how somebody signed in', () => {
    expect(says({ kind: 'signed_in', method: 'password' })).toBe('Signed in');
    expect(says({ kind: 'signed_in', method: 'code' })).toBe('Signed in with a code from an authenticator app');
    expect(says({ kind: 'signed_in', method: 'recovery_code' })).toBe('Signed in with a recovery code');
  });

  it('says which email a failed sign in tried, when it looked like one', () => {
    expect(says({ kind: 'sign_in_failed', email: KITCHEN })).toBe(`Failed to sign in as ${KITCHEN}`);
    expect(says({ kind: 'sign_in_failed', email: null })).toBe(
      'Failed to sign in with something that was not an email address',
    );
    expect(says({ kind: 'second_factor_failed' })).toBe('Entered a wrong sign in code');
    expect(says({ kind: 'sign_in_blocked', email: KITCHEN })).toBe(
      `Refused a sign in as ${KITCHEN} after too many failed attempts`,
    );
    expect(says({ kind: 'sign_in_blocked', email: null })).toBe(
      'Refused a sign in after too many failed attempts',
    );
    expect(says({ kind: 'signed_out' })).toBe('Signed out');
  });

  it('words changes to your own account', () => {
    expect(says({ kind: 'password_changed' })).toBe('Changed their password');
    expect(says({ kind: 'two_factor_enabled' })).toBe('Turned on two factor sign in');
    expect(says({ kind: 'two_factor_disabled' })).toBe('Turned off two factor sign in');
    expect(says({ kind: 'recovery_codes_replaced' })).toBe('Made new recovery codes');
  });

  it('names the account an admin changed', () => {
    expect(says({ kind: 'accounts_set_up' })).toBe('Turned sign in on, as the first admin');
    expect(says({ kind: 'stayed_open' })).toBe('Chose to keep the recorder open, with no sign in');
    expect(says({ kind: 'account_created', email: KITCHEN, role: 'listener' })).toBe(
      `Added ${KITCHEN} as a listener`,
    );
    expect(says({ kind: 'account_created', email: KITCHEN, role: 'admin' })).toBe(`Added ${KITCHEN} as an admin`);
    expect(says({ kind: 'account_removed', email: KITCHEN })).toBe(`Removed ${KITCHEN}`);
    expect(says({ kind: 'role_changed', email: KITCHEN, from: 'listener', to: 'admin' })).toBe(
      `Made ${KITCHEN} an admin, from a listener`,
    );
    expect(says({ kind: 'password_set', email: KITCHEN })).toBe(`Set a new password for ${KITCHEN}`);
    expect(says({ kind: 'two_factor_removed', email: KITCHEN })).toBe(
      `Removed two factor sign in for ${KITCHEN}`,
    );
    expect(says({ kind: 'accounts_disabled' })).toBe('Turned sign in off and removed every account');
  });

  it('says how long somebody listened, and how far back they went', () => {
    const base = { kind: 'listened', startedAtMs: 0, connectedMs: 600_000, playedMs: 540_000 } as const;
    expect(says({ ...base, playedBack: false, earliestMs: null })).toBe('Listened live for 9 min');
    expect(says({ ...base, playedBack: true, earliestMs: 1_790_000_000_000 })).toBe(
      `Listened for 9 min, going back as far as ${formatDateTime(1_790_000_000_000)}`,
    );
    expect(says({ ...base, playedMs: 0, playedBack: false, earliestMs: null })).toBe(
      'Had the stream open for 10 min without playing it',
    );
  });

  /** The page says it is idle a moment after it connects, which the server counts as a sliver of play. */
  it('counts under a second of play as not playing', () => {
    const idle = {
      kind: 'listened',
      startedAtMs: 0,
      connectedMs: 30_000,
      playedMs: 40,
      playedBack: false,
      earliestMs: null,
    } as const;
    expect(says(idle)).toBe('Had the stream open for 30 s without playing it');
    expect(says({ ...idle, playedMs: 1_000 })).toBe('Listened live for 1 s');
  });

  it('says what was downloaded', () => {
    expect(says({ kind: 'exported', fromMs: 1_790_000_000_000, toMs: 1_790_003_600_000 })).toBe(
      `Downloaded the audio from ${formatDateTime(1_790_000_000_000)} to ${formatDateTime(1_790_003_600_000)}`,
    );
  });

  it('words the recorder being changed', () => {
    expect(says({ kind: 'capture_started' })).toBe('Started recording');
    expect(says({ kind: 'capture_stopped' })).toBe('Stopped recording');
    expect(says({ kind: 'device_selected', deviceId: 'Scarlett Solo USB' })).toBe(
      'Chose the microphone Scarlett Solo USB',
    );
    expect(says({ kind: 'device_selected', deviceId: null })).toBe("Chose the system's default microphone");
    expect(says({ kind: 'settings_reset' })).toBe('Restored the default settings');
    expect(says({ kind: 'bookmark_added', label: 'Door', timestampMs: 1_790_000_000_000 })).toBe(
      `Added the bookmark "Door" at ${formatDateTime(1_790_000_000_000)}`,
    );
    expect(says({ kind: 'bookmark_removed', label: 'Door' })).toBe('Removed the bookmark "Door"');
    expect(says({ kind: 'metrics_token_created', replaced: false })).toBe('Made a Prometheus scrape token');
    expect(says({ kind: 'metrics_token_created', replaced: true })).toBe(
      'Replaced the Prometheus scrape token, so the old one stopped working',
    );
    expect(says({ kind: 'metrics_token_revoked' })).toBe('Revoked the Prometheus scrape token');
  });

  it('lists each changed setting by the name the settings page gives it', () => {
    expect(
      says({
        kind: 'settings_changed',
        changes: [
          { key: 'retentionHours', from: 24, to: null },
          { key: 'soundSensitivity', from: 'medium', to: 'high' },
        ],
      }),
    ).toBe('Changed how long recordings are kept from 24 hours to forever, and sound sensitivity from medium to high');
  });

  it('words every kind of setting value readably', () => {
    const one = (key: string, from: unknown, to: unknown) =>
      says({ kind: 'settings_changed', changes: [{ key, from, to }] });
    expect(one('autoStart', true, false)).toBe('Changed record on start up from on to off');
    expect(one('gain', 1, 1.5)).toBe('Changed input gain from 1x to 1.5x');
    expect(one('recordingSampleRate', null, 16_000)).toBe(
      "Changed the recording rate from the microphone's own to 16 kHz",
    );
    expect(one('recordingsDir', null, '/mnt/audio')).toBe(
      'Changed where recordings are stored from the default folder to /mnt/audio',
    );
    expect(one('activityRetentionDays', 90, 30)).toBe(
      'Changed how long the activity log is kept from 90 days to 30 days',
    );
    expect(one('somethingNew', 'a', 'b')).toBe('Changed somethingNew from a to b');
  });
});

describe('groupOf', () => {
  it('files every kind under the heading the filter shows it under', () => {
    const expected: Record<ActivityKind, string> = {
      signed_in: 'access',
      sign_in_failed: 'access',
      second_factor_failed: 'access',
      sign_in_blocked: 'access',
      signed_out: 'access',
      accounts_set_up: 'accounts',
      stayed_open: 'accounts',
      password_changed: 'accounts',
      two_factor_enabled: 'accounts',
      two_factor_disabled: 'accounts',
      recovery_codes_replaced: 'accounts',
      account_created: 'accounts',
      account_removed: 'accounts',
      role_changed: 'accounts',
      password_set: 'accounts',
      two_factor_removed: 'accounts',
      accounts_disabled: 'accounts',
      listened: 'listening',
      exported: 'listening',
      capture_started: 'recorder',
      capture_stopped: 'recorder',
      device_selected: 'recorder',
      settings_changed: 'recorder',
      settings_reset: 'recorder',
      bookmark_added: 'recorder',
      bookmark_removed: 'recorder',
      metrics_token_created: 'recorder',
      metrics_token_revoked: 'recorder',
    };
    for (const [kind, group] of Object.entries(expected)) {
      expect(groupOf(kind as ActivityKind), kind).toBe(group);
    }
  });
});
