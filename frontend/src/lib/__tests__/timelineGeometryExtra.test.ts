/**
 * The timeline geometry branches `timelineGeometry.test.ts` leaves out: the tick step for a canvas with
 * no width or a span beyond the largest step, a custom label spacing, ticks landing exactly on the window
 * edges, and sound bands at the canvas edges.
 */

import { describe, expect, it } from 'vitest';

import { chooseTickStepMs, soundBands, tickTimestamps, timeToX, xToTime, zoomWindow } from '../timelineGeometry';

const DAY = 24 * 3_600_000;

describe('chooseTickStepMs edges', () => {
  it('falls back to a day for a canvas with no width yet', () => {
    expect(chooseTickStepMs({ startMs: 0, spanMs: 60_000, width: 0 })).toBe(DAY);
  });

  it('never picks more than a day, however wide the window', () => {
    expect(chooseTickStepMs({ startMs: 0, spanMs: 30 * DAY, width: 1200 })).toBe(DAY);
  });

  it('picks a finer step when labels may sit closer together', () => {
    const view = { startMs: 0, spanMs: 3_600_000, width: 1200 };
    expect(chooseTickStepMs(view, 40)).toBeLessThan(chooseTickStepMs(view));
  });

  it('never picks less than a second', () => {
    expect(chooseTickStepMs({ startMs: 0, spanMs: 1_000, width: 5000 })).toBe(1_000);
  });
});

describe('tickTimestamps edges', () => {
  it('includes ticks that land exactly on both edges', () => {
    expect(tickTimestamps({ startMs: 10_000, spanMs: 20_000, width: 100 }, 10_000)).toEqual([
      10_000,
      20_000,
      30_000,
    ]);
  });

  it('aligns to the step even when the window starts before the epoch', () => {
    expect(tickTimestamps({ startMs: -15_000, spanMs: 20_000, width: 100 }, 10_000)).toEqual([-10_000, 0]);
  });
});

describe('soundBands edges', () => {
  const view = { startMs: 0, spanMs: 1_000, width: 100 };

  it('keeps a sound that touches the left or right edge exactly', () => {
    expect(soundBands([{ startMs: -500, endMs: 0 }], view)).toEqual([{ left: 0, width: 3 }]);
    expect(soundBands([{ startMs: 1_000, endMs: 1_200 }], view)).toEqual([{ left: 100, width: 0 }]);
  });

  it('honours a custom minimum width', () => {
    expect(soundBands([{ startMs: 500, endMs: 500 }], view, 8)).toEqual([{ left: 50, width: 8 }]);
  });

  it('is empty for no sounds', () => {
    expect(soundBands([], view)).toEqual([]);
  });
});

describe('mapping edges', () => {
  it('maps a negative width canvas to the window start', () => {
    expect(xToTime(50, { startMs: 700, spanMs: 1_000, width: -1 })).toBe(700);
  });

  it('maps a negative span window to the left edge', () => {
    expect(timeToX(50, { startMs: 0, spanMs: -1, width: 100 })).toBe(0);
  });

  it('keeps the start when zooming a window with no span', () => {
    expect(zoomWindow({ startMs: 500, spanMs: 0 }, 60_000, 800)).toEqual({ startMs: 500, spanMs: 60_000 });
  });
});
