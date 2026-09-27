/**
 * Properties of the timeline geometry over generated windows, widths and moments.
 *
 * Every pixel the scrubber draws and every click it resolves goes through these functions, at every zoom
 * from ten seconds to a week and on screens from a phone to a wall display. The example tests pin a few
 * windows; these pin the relationships that must hold at all of them: the two mappings undo each other and
 * never reorder, a zoom holds its anchor under the same pixel, ticks land on whole steps inside the window,
 * and shaded sounds stay on the canvas.
 */

import fc from 'fast-check';
import { describe, expect, it } from 'vitest';

import {
  chooseTickStepMs,
  soundBands,
  tickTimestamps,
  timeToX,
  xToTime,
  zoomWindow,
  type TimelineWindow,
} from '../timelineGeometry';

const DAY_MS = 86_400_000;
const TICK_STEPS_MS = [
  1_000, 5_000, 10_000, 15_000, 30_000,
  60_000, 5 * 60_000, 10 * 60_000, 15 * 60_000, 30 * 60_000,
  3_600_000, 3 * 3_600_000, 6 * 3_600_000, 12 * 3_600_000,
  DAY_MS,
];

/** Epoch milliseconds from 1970 to 2100, the span any real recording falls in. */
const instant = fc.integer({ min: 0, max: 4_102_444_800_000 });

/** The store's zoom limits, ten seconds to a week. */
const span = fc.integer({ min: 10_000, max: 7 * DAY_MS });

const timelineWindow: fc.Arbitrary<TimelineWindow> = fc.record({
  startMs: instant,
  spanMs: span,
  width: fc.integer({ min: 1, max: 8_000 }),
});

describe('timeToX and xToTime', () => {
  it('undo each other, within floating point rounding', () => {
    fc.assert(
      fc.property(timelineWindow, fc.double({ min: -2, max: 3, noNaN: true }), (view, fraction) => {
        // A few units in the last place of the largest moment involved, carried across into pixels: far
        // below a pixel for any real window, but honest about epoch milliseconds being large numbers.
        const roundingMs = 8 * Number.EPSILON * (view.startMs + 3 * view.spanMs);
        const pixelsPerMs = view.width / view.spanMs;

        const timestampMs = view.startMs + fraction * view.spanMs;
        expect(Math.abs(xToTime(timeToX(timestampMs, view), view) - timestampMs)).toBeLessThanOrEqual(roundingMs);

        const x = fraction * view.width;
        expect(Math.abs(timeToX(xToTime(x, view), view) - x)).toBeLessThanOrEqual(roundingMs * pixelsPerMs + 1e-9);
      }),
    );
  });

  it('put the window edges on the canvas edges', () => {
    fc.assert(
      fc.property(timelineWindow, (view) => {
        expect(timeToX(view.startMs, view)).toBe(0);
        expect(timeToX(view.startMs + view.spanMs, view)).toBe(view.width);
        expect(xToTime(0, view)).toBe(view.startMs);
        expect(xToTime(view.width, view)).toBe(view.startMs + view.spanMs);
      }),
    );
  });

  it('never reorder two moments or two pixels', () => {
    fc.assert(
      fc.property(timelineWindow, instant, instant, (view, first, second) => {
        const [earlier, later] = first <= second ? [first, second] : [second, first];
        expect(timeToX(earlier, view)).toBeLessThanOrEqual(timeToX(later, view));
        expect(xToTime(timeToX(earlier, view), view)).toBeLessThanOrEqual(
          xToTime(timeToX(later, view), view),
        );
      }),
    );
  });

  it('stay finite on a degenerate window instead of dividing by zero', () => {
    fc.assert(
      fc.property(
        instant,
        fc.integer({ min: -DAY_MS, max: 0 }),
        fc.integer({ min: -100, max: 0 }),
        (at, badSpan, badWidth) => {
          expect(timeToX(at, { startMs: at, spanMs: badSpan, width: 800 })).toBe(0);
          expect(xToTime(123, { startMs: at, spanMs: DAY_MS, width: badWidth })).toBe(at);
        },
      ),
    );
  });
});

describe('zoomWindow', () => {
  const current = fc.record({ startMs: instant, spanMs: span });

  it('always takes the requested span', () => {
    fc.assert(
      fc.property(current, span, fc.option(instant, { nil: undefined }), (window, nextSpanMs, anchorMs) => {
        expect(zoomWindow(window, nextSpanMs, anchorMs).spanMs).toBe(nextSpanMs);
      }),
    );
  });

  it('holds an anchor inside the window at the same fraction of the screen', () => {
    fc.assert(
      fc.property(current, span, fc.double({ min: 0, max: 1, noNaN: true }), (window, nextSpanMs, fraction) => {
        const anchorMs = window.startMs + fraction * window.spanMs;
        const zoomed = zoomWindow(window, nextSpanMs, anchorMs);
        const before = (anchorMs - window.startMs) / window.spanMs;
        const after = (anchorMs - zoomed.startMs) / zoomed.spanMs;
        expect(Math.abs(after - before)).toBeLessThan(1e-6);
      }),
    );
  });

  it('centres an anchor outside the window, and holds the centre when there is none', () => {
    fc.assert(
      fc.property(
        current,
        span,
        fc.double({ min: 0.001, max: 5, noNaN: true }),
        fc.boolean(),
        (window, nextSpanMs, beyond, after) => {
          const anchorMs = after
            ? window.startMs + window.spanMs * (1 + beyond)
            : window.startMs - window.spanMs * beyond;
          const outside = zoomWindow(window, nextSpanMs, anchorMs);
          expect(Math.abs(outside.startMs + outside.spanMs / 2 - anchorMs)).toBeLessThan(0.01);

          const centred = zoomWindow(window, nextSpanMs, null);
          const oldCentre = window.startMs + window.spanMs / 2;
          expect(Math.abs(centred.startMs + centred.spanMs / 2 - oldCentre)).toBeLessThan(0.01);
        },
      ),
    );
  });

  it('returns to the original window when zoomed back about the same anchor', () => {
    fc.assert(
      fc.property(current, span, fc.double({ min: 0, max: 1, noNaN: true }), (window, nextSpanMs, fraction) => {
        const anchorMs = window.startMs + fraction * window.spanMs;
        const back = zoomWindow(zoomWindow(window, nextSpanMs, anchorMs), window.spanMs, anchorMs);
        // Zooming in then out multiplies the rounding of the anchor by the zoom factor, so the allowance
        // grows with it; at a week against ten seconds it is still well under a millisecond.
        const roundingMs = 8 * Number.EPSILON * (anchorMs + window.spanMs) * (1 + window.spanMs / nextSpanMs);
        expect(back.spanMs).toBe(window.spanMs);
        expect(Math.abs(back.startMs - window.startMs)).toBeLessThanOrEqual(roundingMs);
      }),
    );
  });
});

describe('chooseTickStepMs', () => {
  it('picks the smallest familiar step at least as wide as the target spacing', () => {
    fc.assert(
      fc.property(timelineWindow, fc.integer({ min: 20, max: 400 }), (view, spacingPx) => {
        const step = chooseTickStepMs(view, spacingPx);
        const wanted = (spacingPx / view.width) * view.spanMs;
        const index = TICK_STEPS_MS.indexOf(step);

        expect(index).toBeGreaterThanOrEqual(0);
        if (step !== DAY_MS) {
          expect(step).toBeGreaterThanOrEqual(wanted);
        }
        if (index > 0) {
          expect(TICK_STEPS_MS[index - 1]).toBeLessThan(wanted);
        }
      }),
    );
  });

  it('never picks a finer step for a wider span, or a coarser one for a wider canvas', () => {
    fc.assert(
      fc.property(timelineWindow, span, fc.integer({ min: 1, max: 8_000 }), (view, otherSpan, otherWidth) => {
        const wider = { ...view, spanMs: Math.max(view.spanMs, otherSpan) };
        expect(chooseTickStepMs(wider)).toBeGreaterThanOrEqual(chooseTickStepMs(view));

        const broader = { ...view, width: Math.max(view.width, otherWidth) };
        expect(chooseTickStepMs(broader)).toBeLessThanOrEqual(chooseTickStepMs(view));
      }),
    );
  });
});

describe('tickTimestamps', () => {
  it('lists every whole step inside the window, in order, and nothing else', () => {
    fc.assert(
      fc.property(timelineWindow, fc.constantFrom(...TICK_STEPS_MS), (view, stepMs) => {
        const ticks = tickTimestamps(view, stepMs);
        const endMs = view.startMs + view.spanMs;

        expect(ticks.length).toBeLessThanOrEqual(512);
        ticks.forEach((tick, index) => {
          expect(tick % stepMs).toBe(0);
          expect(tick).toBeGreaterThanOrEqual(view.startMs);
          expect(tick).toBeLessThanOrEqual(endMs);
          if (index > 0) {
            expect(tick - ticks[index - 1]).toBe(stepMs);
          }
        });

        // Nothing missing at either end, unless the cap stopped the list early.
        const expected = Math.floor(endMs / stepMs) - Math.ceil(view.startMs / stepMs) + 1;
        expect(ticks.length).toBe(Math.min(Math.max(expected, 0), 512));
      }),
    );
  });

  it('returns nothing for a step that is not positive', () => {
    fc.assert(
      fc.property(timelineWindow, fc.integer({ min: -DAY_MS, max: 0 }), (view, stepMs) => {
        expect(tickTimestamps(view, stepMs)).toEqual([]);
      }),
    );
  });
});

describe('soundBands', () => {
  const sounds = (view: TimelineWindow) =>
    fc.array(
      fc
        .record({
          startMs: fc.integer({ min: view.startMs - view.spanMs, max: view.startMs + 2 * view.spanMs }),
          lengthMs: fc.integer({ min: 0, max: view.spanMs }),
        })
        .map(({ startMs, lengthMs }) => ({ startMs, endMs: startMs + lengthMs })),
      { maxLength: 20 },
    );

  const scenario = timelineWindow.chain((view) =>
    fc.record({ view: fc.constant(view), sounds: sounds(view), minWidthPx: fc.integer({ min: 0, max: 10 }) }),
  );

  it('keeps every band on the canvas and at least the minimum width where there is room', () => {
    fc.assert(
      fc.property(scenario, ({ view, sounds: list, minWidthPx }) => {
        for (const band of soundBands(list, view, minWidthPx)) {
          expect(band.left).toBeGreaterThanOrEqual(0);
          expect(band.width).toBeGreaterThanOrEqual(0);
          expect(band.left + band.width).toBeLessThanOrEqual(view.width + 1e-9);
          expect(band.width).toBeGreaterThanOrEqual(Math.min(minWidthPx, view.width - band.left) - 1e-9);
        }
      }),
    );
  });

  it('draws one band for each sound touching the window, in order, and none for the rest', () => {
    fc.assert(
      fc.property(scenario, ({ view, sounds: list, minWidthPx }) => {
        const bands = soundBands(list, view, minWidthPx);
        const visible = list.filter(
          (sound) => timeToX(sound.endMs, view) >= 0 && timeToX(sound.startMs, view) <= view.width,
        );

        expect(bands).toHaveLength(visible.length);
        visible.forEach((sound, index) => {
          expect(bands[index].left).toBe(Math.max(timeToX(sound.startMs, view), 0));
        });
      }),
    );
  });
});
