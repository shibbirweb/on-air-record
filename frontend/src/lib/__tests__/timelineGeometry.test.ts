import { describe, expect, it } from 'vitest';

import {
  chooseTickStepMs,
  tickTimestamps,
  timeToX,
  xToTime,
  zoomWindow,
} from '../timelineGeometry';

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

describe('zoomWindow', () => {
  const window = { startMs: 1_000_000, spanMs: 60_000 };

  it('holds the anchor under the same pixel when zooming in', () => {
    // A quarter of the way across the window should still be a quarter of the way across afterwards.
    const anchor = window.startMs + window.spanMs * 0.25;
    const zoomed = zoomWindow(window, 30_000, anchor);

    expect(zoomed.spanMs).toBe(30_000);
    expect((anchor - zoomed.startMs) / zoomed.spanMs).toBeCloseTo(0.25, 6);
  });

  it('holds the anchor when zooming out', () => {
    const anchor = window.startMs + window.spanMs * 0.8;
    const zoomed = zoomWindow(window, 240_000, anchor);

    expect((anchor - zoomed.startMs) / zoomed.spanMs).toBeCloseTo(0.8, 6);
  });

  it('round trips back to the original window', () => {
    const anchor = window.startMs + window.spanMs * 0.3;
    const inThenOut = zoomWindow(zoomWindow(window, 15_000, anchor), 60_000, anchor);

    expect(inThenOut.startMs).toBeCloseTo(window.startMs, 6);
    expect(inThenOut.spanMs).toBe(window.spanMs);
  });

  it('falls back to the centre when there is no anchor', () => {
    const centre = window.startMs + window.spanMs / 2;
    const zoomed = zoomWindow(window, 30_000, null);

    expect(zoomed.startMs + zoomed.spanMs / 2).toBeCloseTo(centre, 6);
    expect(zoomWindow(window, 30_000).startMs).toBeCloseTo(zoomed.startMs, 6);
  });

  it('centres an anchor that is off screen rather than flinging the view sideways', () => {
    const before = zoomWindow(window, 30_000, window.startMs - 500_000);
    expect(before.startMs + before.spanMs / 2).toBeCloseTo(window.startMs - 500_000, 6);

    const after = zoomWindow(window, 30_000, window.startMs + 500_000);
    expect(after.startMs + after.spanMs / 2).toBeCloseTo(window.startMs + 500_000, 6);
  });

  it('holds an anchor sitting exactly on an edge', () => {
    const atStart = zoomWindow(window, 30_000, window.startMs);
    expect(atStart.startMs).toBeCloseTo(window.startMs, 6);

    const end = window.startMs + window.spanMs;
    const atEnd = zoomWindow(window, 30_000, end);
    expect(atEnd.startMs + atEnd.spanMs).toBeCloseTo(end, 6);
  });

  it('degrades safely on a degenerate window', () => {
    expect(zoomWindow({ startMs: 0, spanMs: 0 }, 1_000, 500).spanMs).toBe(1_000);
    expect(zoomWindow(window, 0, 500).spanMs).toBe(0);
  });
});
