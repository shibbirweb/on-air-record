/**
 * The listener list branches `listeners.test.ts` leaves out: the rest of the browser and system table
 * (the order of which is what keeps Edge from reading as Chrome and an iPad from reading as a Mac), a
 * user agent that names only one of the two, history with no start moment, a tab paused through the
 * socket while its player reports playing, whole days connected, and grouping ties.
 */

import { describe, expect, it } from 'vitest';

import type { ListenerView } from '@/api/types';

import { activityTone, describeActivity, describeUserAgent, formatConnectedFor, groupListeners } from '../listeners';

const NOW = new Date(2026, 8, 26, 15, 0, 0).getTime();

function listener(overrides: Partial<ListenerView>): ListenerView {
  return {
    id: 1,
    email: null,
    role: null,
    address: '192.168.1.24',
    userAgent: null,
    connectedAtMs: NOW,
    activity: 'live',
    fromMs: null,
    player: 'playing',
    ...overrides,
  };
}

describe('describeUserAgent, the rest of the table', () => {
  it.each([
    [
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36 OPR/113.0.0.0',
      'Opera on Windows',
    ],
    [
      'Mozilla/5.0 (Linux; Android 14; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/25.0 Chrome/121.0.0.0 Mobile Safari/537.36',
      'Samsung Internet on Android',
    ],
    [
      'Mozilla/5.0 (iPhone; CPU iPhone OS 17_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) FxiOS/130.0 Mobile/15E148 Safari/605.1.15',
      'Firefox on iOS',
    ],
    [
      'Mozilla/5.0 (iPhone; CPU iPhone OS 17_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/128.0.6613.98 Mobile/15E148 Safari/604.1',
      'Chrome on iOS',
    ],
    [
      'Mozilla/5.0 (iPhone; CPU iPhone OS 17_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) EdgiOS/128.0 Mobile/15E148 Safari/605.1.15',
      'Edge on iOS',
    ],
    [
      'Mozilla/5.0 (iPad; CPU OS 17_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.6 Mobile/15E148 Safari/604.1',
      'Safari on iPadOS',
    ],
    [
      'Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36',
      'Chrome on ChromeOS',
    ],
    [
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.6 Safari/605.1.15',
      'Safari on macOS',
    ],
  ])('reads %s', (userAgent, expected) => {
    expect(describeUserAgent(userAgent)).toBe(expected);
  });

  it('names only the browser or only the system when that is all it can read', () => {
    expect(describeUserAgent('Firefox/130.0')).toBe('Firefox');
    expect(describeUserAgent('SomeBot (Windows NT 10.0)')).toBe('Windows');
  });

  it('treats an empty string like no user agent at all', () => {
    expect(describeUserAgent('')).toBe('Unknown browser');
  });
});

describe('describeActivity and activityTone, remaining branches', () => {
  it('says plain history when the start moment is not known', () => {
    expect(describeActivity(listener({ activity: 'playback', fromMs: null }), NOW)).toBe('History');
  });

  it('calls a tab paused through the socket paused, though its player says playing', () => {
    expect(describeActivity(listener({ player: 'playing', activity: 'paused' }), NOW)).toBe('Paused');
    expect(activityTone(listener({ player: 'playing', activity: 'paused' }))).toBe('paused');
  });

  it('calls a tab paused in the UI paused while it listens to history', () => {
    expect(describeActivity(listener({ player: 'paused', activity: 'playback', fromMs: NOW }), NOW)).toBe(
      'Paused',
    );
    expect(activityTone(listener({ player: 'paused', activity: 'playback', fromMs: NOW }))).toBe('paused');
  });
});

describe('formatConnectedFor, whole units', () => {
  it('drops the smaller unit when it is zero', () => {
    expect(formatConnectedFor(NOW - 48 * 3_600_000, NOW)).toBe('2 d');
    expect(formatConnectedFor(NOW - 60 * 60_000, NOW)).toBe('1 h');
  });

  it('switches unit exactly at the hour and the day', () => {
    expect(formatConnectedFor(NOW - 59 * 60_000, NOW)).toBe('59 min');
    expect(formatConnectedFor(NOW - 60_000, NOW)).toBe('1 min');
    expect(formatConnectedFor(NOW - (24 * 60 - 1) * 60_000, NOW)).toBe('23 h 59 min');
    expect(formatConnectedFor(NOW - 24 * 3_600_000, NOW)).toBe('1 d');
  });
});

describe('groupListeners ties', () => {
  it('orders tabs that connected at the same moment by id', () => {
    const groups = groupListeners(
      [
        listener({ id: 3, email: 'a@example.com' }),
        listener({ id: 1, email: 'a@example.com' }),
        listener({ id: 2, email: 'a@example.com' }),
      ],
      null,
    );
    expect(groups[0].connections.map((entry) => entry.id)).toEqual([1, 2, 3]);
  });

  it('marks nobody as you when the viewer has no account', () => {
    const groups = groupListeners([listener({ email: 'a@example.com' }), listener({ id: 2 })], null);
    expect(groups.every((group) => !group.you)).toBe(true);
  });

  it('keeps an account and a guest at the same address apart', () => {
    const groups = groupListeners(
      [listener({ id: 1, email: 'a@example.com' }), listener({ id: 2, email: null })],
      null,
    );
    expect(groups.map((group) => group.key)).toEqual(['account:a@example.com', 'guest:192.168.1.24']);
  });
});
