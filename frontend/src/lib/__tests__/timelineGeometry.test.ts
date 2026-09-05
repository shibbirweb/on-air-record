import { describe, expect, it } from 'vitest';

import { chooseTickStepMs, tickTimestamps, timeToX, xToTime } from '../timelineGeometry';

const view = { startMs: 1_000_000, spanMs: 60_000, width: 600 };

describe('timeToX and xToTime', () => {
  it('maps the window edges onto the canvas edges', () => {
    expect(timeToX(view.startMs, view)).toBe(0);
    expect(timeToX(view.startMs + view.spanMs, view)).toBe(view.width);
  });

  it('round trips a moment through a pixel', () => {
    const timestamp = view.startMs + 17_500;
    expect(xToTime(timeToX(timestamp, view), view)).toBeCloseTo(timestamp, 6);
  });

  it('extrapolates outside the window instead of clamping', () => {
    // The playhead can sit off screen, and the caller decides whether to draw it.
    expect(timeToX(view.startMs - 30_000, view)).toBeLessThan(0);
    expect(timeToX(view.startMs + 120_000, view)).toBeGreaterThan(view.width);
  });

  it('degrades safely when the canvas has no size yet', () => {
    expect(timeToX(1, { startMs: 0, spanMs: 0, width: 0 })).toBe(0);
    expect(xToTime(10, { startMs: 500, spanMs: 1000, width: 0 })).toBe(500);
  });
});

describe('chooseTickStepMs', () => {
  it('picks a readable step for a one minute window', () => {
    expect(chooseTickStepMs(view)).toBe(15_000);
  });

  it('picks a coarser step as the window widens', () => {
    const narrow = chooseTickStepMs({ startMs: 0, spanMs: 60_000, width: 1200 });
    const wide = chooseTickStepMs({ startMs: 0, spanMs: 24 * 3_600_000, width: 1200 });
    expect(wide).toBeGreaterThan(narrow);
  });
});

describe('tickTimestamps', () => {
  it('aligns ticks to whole multiples of the step', () => {
    const ticks = tickTimestamps({ startMs: 1_007, spanMs: 5_000, width: 600 }, 1_000);
    expect(ticks[0]).toBe(2_000);
    expect(ticks.every((tick) => tick % 1_000 === 0)).toBe(true);
  });

  it('covers the window without running past it', () => {
    const ticks = tickTimestamps({ startMs: 0, spanMs: 10_000, width: 600 }, 1_000);
    expect(ticks[0]).toBe(0);
    expect(ticks[ticks.length - 1]).toBe(10_000);
  });

  it('refuses to draw an unbounded number of ticks', () => {
    const ticks = tickTimestamps({ startMs: 0, spanMs: 7 * 24 * 3_600_000, width: 600 }, 1_000);
    expect(ticks.length).toBeLessThanOrEqual(512);
  });

  it('returns nothing for a nonsensical step', () => {
    expect(tickTimestamps(view, 0)).toEqual([]);
  });
});
