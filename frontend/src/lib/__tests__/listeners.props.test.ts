/**
 * Properties of the listener list shaping, over generated lists of connections.
 *
 * The server pushes the whole list on every change, in no promised order, so grouping has to be a pure
 * function of the set of connections: shuffling them must change nothing, every tab must land in exactly
 * one group, and the viewer's own group must lead. The per tab descriptions are checked for agreement
 * with each other across every combination of player and stream state, and the connected-for label for
 * never reading shorter as time passes.
 */

import fc from 'fast-check';
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

const emails = ['ana@example.com', 'bo@example.com', 'cy@example.com'];

const listenerFields = fc.record({
  email: fc.option(fc.constantFrom(...emails), { nil: null }),
  role: fc.option(fc.constantFrom('admin' as const, 'listener' as const), { nil: null }),
  address: fc.constantFrom('192.168.1.10', '192.168.1.11', '10.0.0.5'),
  userAgent: fc.option(fc.string({ maxLength: 40 }), { nil: null }),
  connectedAtMs: fc.integer({ min: 1_700_000_000_000, max: 1_700_000_600_000 }),
  activity: fc.constantFrom('live' as const, 'playback' as const, 'paused' as const),
  fromMs: fc.option(fc.integer({ min: 0, max: 4_102_444_800_000 }), { nil: null }),
  player: fc.constantFrom('idle' as const, 'playing' as const, 'paused' as const),
});

/** Connections with the unique ids the server guarantees. */
const listenerList: fc.Arbitrary<ListenerView[]> = fc
  .array(listenerFields, { maxLength: 15 })
  .map((list) => list.map((fields, index) => ({ ...fields, id: index + 1 })));

const viewer = fc.option(fc.constantFrom(...emails, 'nobody@example.com'), { nil: null });

function keyOf(listener: ListenerView): string {
  return listener.email === null ? `guest:${listener.address}` : `account:${listener.email}`;
}

function earlier(left: ListenerView, right: ListenerView): boolean {
  if (left.connectedAtMs !== right.connectedAtMs) {
    return left.connectedAtMs < right.connectedAtMs;
  }
  return left.id < right.id;
}

describe('groupListeners', () => {
  it('puts every connection in exactly one group, with the others that share its key', () => {
    fc.assert(
      fc.property(listenerList, viewer, (list, viewerEmail) => {
        const groups = groupListeners(list, viewerEmail);
        const placed = groups.flatMap((group) => group.connections.map((connection) => connection.id));

        expect([...placed].sort((a, b) => a - b)).toEqual(list.map((listener) => listener.id));
        expect(new Set(groups.map((group) => group.key)).size).toBe(groups.length);
        expect(groups.length).toBe(new Set(list.map(keyOf)).size);
        for (const group of groups) {
          expect(group.connections.length).toBeGreaterThan(0);
          for (const connection of group.connections) {
            expect(keyOf(connection)).toBe(group.key);
          }
        }
      }),
    );
  });

  it('lists each group oldest first, the viewer first, then the others by who arrived first', () => {
    fc.assert(
      fc.property(listenerList, viewer, (list, viewerEmail) => {
        const groups = groupListeners(list, viewerEmail);

        for (const group of groups) {
          for (let index = 1; index < group.connections.length; index += 1) {
            expect(earlier(group.connections[index - 1], group.connections[index])).toBe(true);
          }
          expect(group.you).toBe(group.email !== null && group.email === viewerEmail);
        }

        const yours = groups.filter((group) => group.you);
        expect(yours.length).toBeLessThanOrEqual(1);
        if (yours.length === 1) {
          expect(groups[0].you).toBe(true);
        }

        const others = groups.filter((group) => !group.you);
        for (let index = 1; index < others.length; index += 1) {
          expect(earlier(others[index - 1].connections[0], others[index].connections[0])).toBe(true);
        }
      }),
    );
  });

  it('gives the same answer whatever order the server sent, and leaves the input alone', () => {
    fc.assert(
      fc.property(
        listenerList.chain((list) =>
          fc.tuple(fc.constant(list), fc.shuffledSubarray(list, { minLength: list.length })),
        ),
        viewer,
        ([list, shuffled], viewerEmail) => {
          const snapshot = shuffled.map((listener) => listener.id);
          expect(groupListeners(shuffled, viewerEmail)).toEqual(groupListeners(list, viewerEmail));
          expect(shuffled.map((listener) => listener.id)).toEqual(snapshot);
        },
      ),
    );
  });
});

describe('describeUserAgent', () => {
  it('always says something, and names both halves in the documented form when it knows them', () => {
    fc.assert(
      fc.property(fc.option(fc.string({ unit: 'binary', maxLength: 80 }), { nil: null }), (userAgent) => {
        const label = describeUserAgent(userAgent);
        expect(label.length).toBeGreaterThan(0);
        expect(label).toMatch(/^([A-Za-z ]+ on [A-Za-z]+|[A-Za-z ]+)$/);
      }),
    );
  });
});

describe('activityTone and describeActivity', () => {
  it('agree for every combination of player and stream state', () => {
    fc.assert(
      fc.property(listenerFields, fc.integer({ min: 0, max: 4_102_444_800_000 }), (fields, nowMs) => {
        const listener: ListenerView = { ...fields, id: 1 };
        const tone = activityTone(listener);
        const description = describeActivity(listener, nowMs);

        expect(tone === 'idle').toBe(description === 'Not playing');
        expect(tone === 'paused').toBe(description === 'Paused');
        expect(tone === 'live').toBe(description === 'Live');
        expect(tone === 'history').toBe(description.startsWith('History'));
      }),
    );
  });

  it('match what playerState reports for any pair of flags', () => {
    fc.assert(
      fc.property(fc.boolean(), fc.boolean(), (playing, started) => {
        const state = playerState(playing, started);
        expect(state === 'playing').toBe(playing);
        expect(state === 'idle').toBe(!playing && !started);
      }),
    );
  });
});

describe('formatConnectedFor', () => {
  /** The minutes a label stands for; the day form drops its minutes. */
  function minutes(label: string): number {
    if (label === 'just now') {
      return 0;
    }
    const units: Record<string, number> = { d: 1440, h: 60, min: 1 };
    return [...label.matchAll(/(\d+) (d|h|min)/g)].reduce(
      (total, [, value, unit]) => total + Number(value) * units[unit],
      0,
    );
  }

  const start = fc.integer({ min: 0, max: 4_000_000_000_000 });
  const elapsed = fc.integer({ min: -3_600_000, max: 400 * 86_400_000 });

  it('reads as a well formed label that is out by under an hour', () => {
    fc.assert(
      fc.property(start, elapsed, (connectedAtMs, elapsedMs) => {
        const label = formatConnectedFor(connectedAtMs, connectedAtMs + elapsedMs);
        expect(label).toMatch(/^(just now|\d+ min|\d+ h( [1-5]?\d min)?|\d+ d( \d+ h)?)$/);
        const truth = Math.floor(Math.max(elapsedMs, 0) / 60_000);
        expect(truth - minutes(label)).toBeGreaterThanOrEqual(0);
        expect(truth - minutes(label)).toBeLessThan(60);
      }),
    );
  });

  it('never reads shorter as time passes', () => {
    fc.assert(
      fc.property(start, elapsed, elapsed, (connectedAtMs, first, second) => {
        const [sooner, later] = first <= second ? [first, second] : [second, first];
        expect(minutes(formatConnectedFor(connectedAtMs, connectedAtMs + sooner))).toBeLessThanOrEqual(
          minutes(formatConnectedFor(connectedAtMs, connectedAtMs + later)),
        );
      }),
    );
  });
});
