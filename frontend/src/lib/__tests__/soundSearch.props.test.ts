/**
 * Properties of where the next and previous sound buttons search from.
 *
 * The rule is an order of preference over four sources. The example tests walk the paths somebody would
 * click; these generate every combination of present and absent sources and check the order itself: the
 * answer is always one of the candidates, a source only matters when everything above it is absent, and
 * nothing below the winner can change the answer.
 */

import fc from 'fast-check';
import { describe, expect, it } from 'vitest';

import { soundSearchFrom, type SoundSearchContext } from '../soundSearch';

const moment = fc.integer({ min: 0, max: 4_102_444_800_000 });

const context: fc.Arbitrary<SoundSearchContext> = fc.record({
  playheadMs: fc.option(moment, { nil: null }),
  cuedMs: fc.option(moment, { nil: null }),
  windowStartMs: moment,
  followingLive: fc.boolean(),
  liveEdgeMs: fc.option(moment, { nil: null }),
  nowMs: moment,
});

/** The documented order, written out independently of the implementation. */
function expectedFrom(value: SoundSearchContext): number {
  const ranked: (number | null)[] = [
    value.playheadMs,
    value.cuedMs,
    value.followingLive ? null : value.windowStartMs,
    value.liveEdgeMs,
    value.nowMs,
  ];
  return ranked.find((candidate): candidate is number => candidate !== null) as number;
}

describe('soundSearchFrom', () => {
  it('follows the documented order of preference for every combination', () => {
    fc.assert(
      fc.property(context, (value) => {
        expect(soundSearchFrom(value)).toBe(expectedFrom(value));
      }),
    );
  });

  it('answers with one of the positions it was given, never an invented one', () => {
    fc.assert(
      fc.property(context, (value) => {
        const candidates = [value.playheadMs, value.cuedMs, value.windowStartMs, value.liveEdgeMs, value.nowMs];
        expect(candidates).toContain(soundSearchFrom(value));
      }),
    );
  });

  it('lets nothing below what is heard change the answer', () => {
    fc.assert(
      fc.property(context, context, moment, (value, other, playheadMs) => {
        const heard = { ...value, playheadMs };
        const rest = { ...other, playheadMs };
        expect(soundSearchFrom(heard)).toBe(playheadMs);
        expect(soundSearchFrom(rest)).toBe(playheadMs);
      }),
    );
  });

  it('lets nothing below a cue change the answer when nothing is heard', () => {
    fc.assert(
      fc.property(context, moment, (value, cuedMs) => {
        expect(soundSearchFrom({ ...value, playheadMs: null, cuedMs })).toBe(cuedMs);
      }),
    );
  });

  it('searches from the left edge of a timeline not following live, whatever the live edge and clock say', () => {
    fc.assert(
      fc.property(context, (value) => {
        const idle = { ...value, playheadMs: null, cuedMs: null, followingLive: false };
        expect(soundSearchFrom(idle)).toBe(value.windowStartMs);
      }),
    );
  });
});
