/**
 * The activity log in words: who did it, and a plain sentence for what they did, with the details that
 * matter. Framework free, so every kind of event is tested here and the page only lays the words out.
 */

import type { ActivityEvent, ActivityGroup, ActivityKind, Actor, Role, SettingChange } from '@/api/types';
import { formatDateTime } from '@/lib/format';

export function describeActor(actor: Actor): string {
  switch (actor.kind) {
    case 'account':
      return actor.email;
    case 'guest':
      return 'A guest';
    case 'host':
      return 'The host command line';
  }
}

/** `9 min`, `45 s`, `1 h 5 min`: rounded down, which is how long somebody listened at least. */
export function spokenDuration(durationMs: number): string {
  const seconds = Math.max(Math.floor(durationMs / 1000), 0);
  if (seconds < 60) {
    return `${seconds} s`;
  }
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes} min`;
  }
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `${hours} h` : `${hours} h ${rest} min`;
}

const article = (role: Role) => (role === 'admin' ? 'an admin' : 'a listener');

/** What each setting is called, as the settings page words it, and how its values read. */
const SETTINGS: Record<string, { name: string; value: (value: unknown) => string }> = {
  retentionHours: {
    name: 'how long recordings are kept',
    value: (value) => (value === null ? 'forever' : `${String(value)} hours`),
  },
  activityRetentionDays: {
    name: 'how long the activity log is kept',
    value: (value) => `${String(value)} days`,
  },
  soundSensitivity: { name: 'sound sensitivity', value: String },
  autoStart: { name: 'record on start up', value: (value) => (value ? 'on' : 'off') },
  autoStartDelaySeconds: { name: 'the start up delay', value: (value) => `${String(value)} s` },
  checkForUpdates: { name: 'checking for updates', value: (value) => (value ? 'on' : 'off') },
  gain: { name: 'input gain', value: (value) => `${String(value)}x` },
  segmentSeconds: { name: 'segment length', value: (value) => `${String(value)} s` },
  frameMs: { name: 'frame length', value: (value) => `${String(value)} ms` },
  recordingSampleRate: {
    name: 'the recording rate',
    value: (value) => (value === null ? "the microphone's own" : `${Number(value) / 1000} kHz`),
  },
  recordingsDir: {
    name: 'where recordings are stored',
    value: (value) => (value === null ? 'the default folder' : String(value)),
  },
  inputDeviceId: {
    name: 'the microphone',
    value: (value) => (value === null ? 'the default' : String(value)),
  },
};

function describeChange(change: SettingChange): string {
  const known = SETTINGS[change.key];
  const name = known?.name ?? change.key;
  const value = known?.value ?? ((raw: unknown) => (raw === null ? 'none' : String(raw)));
  return `${name} from ${value(change.from)} to ${value(change.to)}`;
}

export function describeEvent(event: ActivityEvent): string {
  switch (event.kind) {
    case 'signed_in':
      switch (event.method) {
        case 'password':
          return 'Signed in';
        case 'code':
          return 'Signed in with a code from an authenticator app';
        case 'recovery_code':
          return 'Signed in with a recovery code';
      }
      break;
    case 'sign_in_failed':
      return event.email === null
        ? 'Failed to sign in with something that was not an email address'
        : `Failed to sign in as ${event.email}`;
    case 'second_factor_failed':
      return 'Entered a wrong sign in code';
    case 'sign_in_blocked':
      return event.email === null
        ? 'Refused a sign in after too many failed attempts'
        : `Refused a sign in as ${event.email} after too many failed attempts`;
    case 'signed_out':
      return 'Signed out';
    case 'accounts_set_up':
      return 'Turned sign in on, as the first admin';
    case 'stayed_open':
      return 'Chose to keep the recorder open, with no sign in';
    case 'password_changed':
      return 'Changed their password';
    case 'two_factor_enabled':
      return 'Turned on two factor sign in';
    case 'two_factor_disabled':
      return 'Turned off two factor sign in';
    case 'recovery_codes_replaced':
      return 'Made new recovery codes';
    case 'account_created':
      return `Added ${event.email} as ${article(event.role)}`;
    case 'account_removed':
      return `Removed ${event.email}`;
    case 'role_changed':
      return `Made ${event.email} ${article(event.to)}, from ${article(event.from)}`;
    case 'password_set':
      return `Set a new password for ${event.email}`;
    case 'two_factor_removed':
      return `Removed two factor sign in for ${event.email}`;
    case 'accounts_disabled':
      return 'Turned sign in off and removed every account';
    case 'listened':
      // The page reports it is idle a moment after connecting, which the server counts as a sliver of
      // play; anything under a second is somebody who never pressed play.
      if (event.playedMs < 1000) {
        return `Had the stream open for ${spokenDuration(event.connectedMs)} without playing it`;
      }
      return event.playedBack && event.earliestMs !== null
        ? `Listened for ${spokenDuration(event.playedMs)}, going back as far as ${formatDateTime(event.earliestMs)}`
        : `Listened live for ${spokenDuration(event.playedMs)}`;
    case 'exported':
      return `Downloaded the audio from ${formatDateTime(event.fromMs)} to ${formatDateTime(event.toMs)}`;
    case 'capture_started':
      return 'Started recording';
    case 'capture_stopped':
      return 'Stopped recording';
    case 'device_selected':
      return event.deviceId === null
        ? "Chose the system's default microphone"
        : `Chose the microphone ${event.deviceId}`;
    case 'settings_changed': {
      const parts = event.changes.map(describeChange);
      const listed =
        parts.length <= 1 ? parts.join('') : `${parts.slice(0, -1).join(', ')}, and ${parts[parts.length - 1]}`;
      return `Changed ${listed}`;
    }
    case 'settings_reset':
      return 'Restored the default settings';
    case 'bookmark_added':
      return `Added the bookmark "${event.label}" at ${formatDateTime(event.timestampMs)}`;
    case 'bookmark_removed':
      return `Removed the bookmark "${event.label}"`;
    case 'metrics_token_created':
      return event.replaced
        ? 'Replaced the Prometheus scrape token, so the old one stopped working'
        : 'Made a Prometheus scrape token';
    case 'metrics_token_revoked':
      return 'Revoked the Prometheus scrape token';
  }
  return 'Did something this page does not know how to describe';
}

/** The heading each kind is filed under, the same as the server's filter. */
const GROUPS: Record<ActivityKind, ActivityGroup> = {
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

export function groupOf(kind: ActivityKind): ActivityGroup {
  return GROUPS[kind];
}
