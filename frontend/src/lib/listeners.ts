/**
 * Shaping the realtime listener list for the header dropdown.
 *
 * The server sends one entry per open stream, which is one per browser tab. People think in accounts, not
 * tabs, so the list is grouped by who is listening, with each tab underneath saying what it is doing.
 */

import type { ListenerView, PlayerState, Role } from '@/api/types';
import { formatClock, formatDateTime } from '@/lib/format';

export type ListenerGroup = {
  /** Stable across updates, so rows do not remount when somebody else arrives. */
  key: string;
  /** Absent for a guest on an open recorder. */
  email: string | null;
  role: Role | null;
  /** Whether this is the account looking at the list. */
  you: boolean;
  /** Oldest first. */
  connections: ListenerView[];
};

/**
 * One group per account, and one per address for guests, since a guest has nothing else to tell them
 * apart by. The viewer's own group comes first, then everybody else in the order they arrived.
 */
export function groupListeners(listeners: ListenerView[], viewerEmail: string | null): ListenerGroup[] {
  const groups = new Map<string, ListenerGroup>();
  const ordered = [...listeners].sort(
    (left, right) => left.connectedAtMs - right.connectedAtMs || left.id - right.id,
  );

  for (const listener of ordered) {
    const key = listener.email === null ? `guest:${listener.address}` : `account:${listener.email}`;
    let group = groups.get(key);
    if (!group) {
      group = {
        key,
        email: listener.email,
        role: listener.role,
        you: listener.email !== null && listener.email === viewerEmail,
        connections: [],
      };
      groups.set(key, group);
    }
    group.connections.push(listener);
  }

  const all = [...groups.values()];
  return [...all.filter((group) => group.you), ...all.filter((group) => !group.you)];
}

const BROWSERS: [RegExp, string][] = [
  // Order matters: Edge, Opera and Samsung all claim to be Chrome, and Chrome claims to be Safari.
  [/Edg(e|A|iOS)?\//, 'Edge'],
  [/OPR\/|Opera/, 'Opera'],
  [/SamsungBrowser\//, 'Samsung Internet'],
  [/Firefox\/|FxiOS\//, 'Firefox'],
  [/Chrome\/|CriOS\//, 'Chrome'],
  [/Version\/[\d.]+.*Safari\//, 'Safari'],
];

const SYSTEMS: [RegExp, string][] = [
  // Before macOS and Linux: an iPad in desktop mode says Macintosh, and Android says Linux.
  [/iPhone|iPod/, 'iOS'],
  [/iPad/, 'iPadOS'],
  [/Android/, 'Android'],
  [/CrOS/, 'ChromeOS'],
  [/Windows/, 'Windows'],
  [/Mac OS X|Macintosh/, 'macOS'],
  [/Linux/, 'Linux'],
];

function firstMatch(text: string, table: [RegExp, string][]): string | null {
  for (const [pattern, name] of table) {
    if (pattern.test(text)) {
      return name;
    }
  }
  return null;
}

/**
 * `Chrome on macOS` from a user agent string. A rough reading, good enough to tell the kitchen tablet from
 * the office laptop; it is not trying to be a device database.
 */
export function describeUserAgent(userAgent: string | null): string {
  if (!userAgent) {
    return 'Unknown browser';
  }
  const browser = firstMatch(userAgent, BROWSERS);
  const system = firstMatch(userAgent, SYSTEMS);
  if (browser && system) {
    return `${browser} on ${system}`;
  }
  return browser ?? system ?? 'Unknown browser';
}

/**
 * What this browser reports about itself. Browsers only allow audio after a click, so until play is pressed
 * nobody is hearing anything, however live the stream is; after that, pause means paused.
 */
export function playerState(playing: boolean, started: boolean): PlayerState {
  if (playing) {
    return 'playing';
  }
  return started ? 'paused' : 'idle';
}

/** The colour of a tab's dot: what the person hears first, then what the stream is doing. */
export type ActivityTone = 'live' | 'history' | 'paused' | 'idle';

export function activityTone(listener: ListenerView): ActivityTone {
  if (listener.player === 'idle') {
    return 'idle';
  }
  if (listener.player === 'paused' || listener.activity === 'paused') {
    return 'paused';
  }
  return listener.activity === 'playback' ? 'history' : 'live';
}

/**
 * `Not playing`, `Paused`, `Live`, or where in history a tab is listening from. What the person hears
 * comes first: a tab nobody pressed play in is not listening, whatever it is being sent.
 */
export function describeActivity(listener: ListenerView, nowMs: number): string {
  if (listener.player === 'idle') {
    return 'Not playing';
  }
  if (listener.player === 'paused') {
    return 'Paused';
  }
  if (listener.activity === 'live') {
    return 'Live';
  }
  if (listener.activity === 'paused') {
    return 'Paused';
  }
  if (listener.fromMs === null) {
    return 'History';
  }
  const sameDay = new Date(listener.fromMs).toDateString() === new Date(nowMs).toDateString();
  return `History from ${sameDay ? formatClock(listener.fromMs) : formatDateTime(listener.fromMs)}`;
}

/** `just now`, `12 min`, `2 h 5 min`, `3 d 4 h`: how long a tab has been connected. */
export function formatConnectedFor(connectedAtMs: number, nowMs: number): string {
  const minutes = Math.floor(Math.max(nowMs - connectedAtMs, 0) / 60_000);
  if (minutes < 1) {
    return 'just now';
  }
  if (minutes < 60) {
    return `${minutes} min`;
  }
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return minutes % 60 === 0 ? `${hours} h` : `${hours} h ${minutes % 60} min`;
  }
  const days = Math.floor(hours / 24);
  return hours % 24 === 0 ? `${days} d` : `${days} d ${hours % 24} h`;
}
