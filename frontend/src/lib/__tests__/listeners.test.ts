import { describe, expect, it } from 'vitest';

import type { ListenerView } from '@/api/types';

import {
  activityTone,
  describeActivity,
  describeUserAgent,
  formatConnectedFor,
  groupListeners,
  playerState,
} from '../listeners';

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

describe('groupListeners', () => {
  it('gathers the tabs of one account under it, oldest first', () => {
    const groups = groupListeners(
      [
        listener({
          id: 3,
          email: 'kitchen@example.com',
          role: 'listener',
          connectedAtMs: NOW - 1_000,
        }),
        listener({
          id: 1,
          email: 'kitchen@example.com',
          role: 'listener',
          connectedAtMs: NOW - 9_000,
        }),
        listener({
          id: 2,
          email: 'office@example.com',
          role: 'admin',
          connectedAtMs: NOW - 5_000,
        }),
      ],
      null,
    );

    expect(groups.map((group) => group.email)).toEqual(['kitchen@example.com', 'office@example.com']);
    expect(groups[0].connections.map((connection) => connection.id)).toEqual([1, 3]);
    expect(groups[1].role).toBe('admin');
  });

  it('puts the viewer first and marks them', () => {
    const groups = groupListeners(
      [
        listener({
          id: 1,
          email: 'kitchen@example.com',
          connectedAtMs: NOW - 9_000,
        }),
        listener({
          id: 2,
          email: 'owner@example.com',
          connectedAtMs: NOW - 1_000,
        }),
      ],
      'owner@example.com',
    );

    expect(groups[0].email).toBe('owner@example.com');
    expect(groups[0].you).toBe(true);
    expect(groups[1].you).toBe(false);
  });

  it('tells guests apart by address and never calls one of them you', () => {
    const groups = groupListeners(
      [
        listener({ id: 1, address: '192.168.1.24' }),
        listener({ id: 2, address: '192.168.1.30' }),
        listener({ id: 3, address: '192.168.1.24' }),
      ],
      null,
    );

    expect(groups).toHaveLength(2);
    expect(groups[0].connections).toHaveLength(2);
    expect(groups.every((group) => !group.you)).toBe(true);
  });

  it('is empty when nobody is connected', () => {
    expect(groupListeners([], 'owner@example.com')).toEqual([]);
  });
});

describe('describeUserAgent', () => {
  it.each([
    [
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36',
      'Chrome on macOS',
    ],
    [
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36 Edg/128.0.0.0',
      'Edge on Windows',
    ],
    ['Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0', 'Firefox on Linux'],
    [
      'Mozilla/5.0 (iPhone; CPU iPhone OS 17_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.6 Mobile/15E148 Safari/604.1',
      'Safari on iOS',
    ],
    [
      'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Mobile Safari/537.36',
      'Chrome on Android',
    ],
  ])('reads %s', (userAgent, expected) => {
    expect(describeUserAgent(userAgent)).toBe(expected);
  });

  it('says so when there is nothing to read', () => {
    expect(describeUserAgent(null)).toBe('Unknown browser');
    expect(describeUserAgent('curl/8.7.1')).toBe('Unknown browser');
  });
});

describe('describeActivity', () => {
  it('names live and paused plainly', () => {
    expect(describeActivity(listener({ activity: 'live' }), NOW)).toBe('Live');
    expect(describeActivity(listener({ activity: 'paused' }), NOW)).toBe('Paused');
  });

  it('adds the day only when history is from another day', () => {
    const today = describeActivity(listener({ activity: 'playback', fromMs: NOW - 3_600_000 }), NOW);
    const yesterday = describeActivity(listener({ activity: 'playback', fromMs: NOW - 86_400_000 }), NOW);

    expect(today).toMatch(/^History from \d\d:\d\d:\d\d$/);
    expect(yesterday).not.toMatch(/^History from \d\d:\d\d:\d\d$/);
    expect(yesterday.startsWith('History from ')).toBe(true);
  });
});

describe('what the person hears comes first', () => {
  it('calls a tab nobody pressed play in not playing, whatever it is sent', () => {
    expect(describeActivity(listener({ player: 'idle' }), NOW)).toBe('Not playing');
    expect(describeActivity(listener({ player: 'idle', activity: 'playback', fromMs: NOW }), NOW)).toBe(
      'Not playing',
    );
    expect(activityTone(listener({ player: 'idle' }))).toBe('idle');
  });

  it('calls a tab paused in the UI paused, though the server is still streaming live', () => {
    expect(describeActivity(listener({ player: 'paused', activity: 'live' }), NOW)).toBe('Paused');
    expect(activityTone(listener({ player: 'paused', activity: 'live' }))).toBe('paused');
  });

  it('falls through to the stream while playing', () => {
    expect(activityTone(listener({ activity: 'live' }))).toBe('live');
    expect(activityTone(listener({ activity: 'playback', fromMs: NOW }))).toBe('history');
    // A client pausing through the socket itself is paused too.
    expect(activityTone(listener({ activity: 'paused' }))).toBe('paused');
  });
});

describe('playerState', () => {
  it('is idle until play is first pressed, then playing or paused', () => {
    expect(playerState(false, false)).toBe('idle');
    expect(playerState(true, true)).toBe('playing');
    expect(playerState(false, true)).toBe('paused');
  });
});

describe('formatConnectedFor', () => {
  it('scales the unit with the time connected', () => {
    expect(formatConnectedFor(NOW - 20_000, NOW)).toBe('just now');
    expect(formatConnectedFor(NOW - 12 * 60_000, NOW)).toBe('12 min');
    expect(formatConnectedFor(NOW - 125 * 60_000, NOW)).toBe('2 h 5 min');
    expect(formatConnectedFor(NOW - 120 * 60_000, NOW)).toBe('2 h');
    expect(formatConnectedFor(NOW - 76 * 3_600_000, NOW)).toBe('3 d 4 h');
  });

  it('treats a clock slightly ahead of the browser as just now', () => {
    expect(formatConnectedFor(NOW + 5_000, NOW)).toBe('just now');
  });
});
