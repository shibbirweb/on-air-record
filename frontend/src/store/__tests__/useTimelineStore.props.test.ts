/**
 * Properties of the timeline window's zoom and pan actions, over generated windows and requests.
 *
 * The store owns the zoom limits and hands the geometry to `zoomWindow`, so the property worth checking
 * here is the combination: whatever span is asked for, from a wheel spinning past either end or a stray
 * negative, the result stays inside ten seconds to a week, and a moment under the pointer stays under it.
 * Panning is checked for being exactly reversible, since a drag back to where it started must not drift.
 */

import fc from 'fast-check';
import { beforeEach, describe, expect, it } from 'vitest';

import { useTimelineStore } from '../useTimelineStore';

const MIN_SPAN_MS = 10_000;
const MAX_SPAN_MS = 7 * 24 * 3_600_000;

const timelineWindow = fc.record({
  windowStartMs: fc.integer({ min: 0, max: 4_102_444_800_000 }),
  spanMs: fc.integer({ min: MIN_SPAN_MS, max: MAX_SPAN_MS }),
});

describe('useTimelineStore zoom and pan', () => {
  beforeEach(() => {
    useTimelineStore.setState({ followingLive: false, range: null, days: [] });
  });

  it('keeps the span inside the zoom limits whatever is asked for', () => {
    fc.assert(
      fc.property(
        timelineWindow,
        fc.oneof(
          fc.integer({ min: -MAX_SPAN_MS, max: 2 * MAX_SPAN_MS }),
          fc.constantFrom(0, 1, MIN_SPAN_MS - 1, MAX_SPAN_MS + 1),
        ),
        (start, requested) => {
          useTimelineStore.setState(start);
          useTimelineStore.getState().zoomTo(requested);
          const { spanMs } = useTimelineStore.getState();
          expect(spanMs).toBe(Math.min(Math.max(requested, MIN_SPAN_MS), MAX_SPAN_MS));
        },
      ),
    );
  });

  it('holds a moment inside the window at the same place on screen', () => {
    fc.assert(
      fc.property(
        timelineWindow,
        fc.integer({ min: 0, max: 2 * MAX_SPAN_MS }),
        fc.double({ min: 0, max: 1, noNaN: true }),
        (start, requested, fraction) => {
          useTimelineStore.setState(start);
          const anchorMs = start.windowStartMs + fraction * start.spanMs;
          useTimelineStore.getState().zoomTo(requested, anchorMs);

          const { windowStartMs, spanMs } = useTimelineStore.getState();
          expect(Math.abs((anchorMs - windowStartMs) / spanMs - fraction)).toBeLessThan(1e-6);
        },
      ),
    );
  });

  it('returns exactly to where it started after panning there and back', () => {
    fc.assert(
      fc.property(
        timelineWindow,
        fc.array(fc.integer({ min: -MAX_SPAN_MS, max: MAX_SPAN_MS }), { maxLength: 10 }),
        (start, deltas) => {
          useTimelineStore.setState({ ...start, followingLive: true });
          for (const delta of deltas) {
            useTimelineStore.getState().panBy(delta);
          }
          for (const delta of deltas) {
            useTimelineStore.getState().panBy(-delta);
          }

          const state = useTimelineStore.getState();
          expect(state.windowStartMs).toBe(start.windowStartMs);
          expect(state.spanMs).toBe(start.spanMs);
          // Any pan at all means somebody is looking at something, so the view stops following live.
          expect(state.followingLive).toBe(deltas.length === 0);
        },
      ),
    );
  });
});
