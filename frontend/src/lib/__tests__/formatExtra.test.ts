/**
 * The formatting helpers `format.test.ts` leaves out, `formatClock` and `formatDateTime`, plus the edges
 * of the tested ones: the exact boundary where an offset stops being live, rounding across a unit, the
 * largest byte unit, and a meter with its own floor.
 *
 * The clock labels use the browser locale, so their assertions check the shape and the numbers from a
 * local `Date`, never a literal string that would only hold in one locale.
 */

import { describe, expect, it } from 'vitest';

import {
  formatBytes,
  formatClock,
  formatDateTime,
  formatDuration,
  formatOffsetFromLive,
  meterScale,
  toDecibels,
} from '../format';

const LOCAL = new Date(2026, 8, 5, 14, 3, 7).getTime();

describe('formatClock', () => {
  it('shows a moment as a 24 hour clock with seconds', () => {
    const label = formatClock(LOCAL);
    expect(label).toMatch(/14\D03\D07/);
    expect(label).not.toMatch(/AM|PM/i);
  });

  it('zero pads the hour', () => {
    expect(formatClock(new Date(2026, 8, 5, 4, 5, 6).getTime())).toMatch(/04\D05\D06/);
  });

  it('shows dashes for a missing or impossible moment', () => {
    for (const missing of [null, undefined, Number.NaN, Number.POSITIVE_INFINITY]) {
      expect(formatClock(missing)).toBe('--:--:--');
    }
  });

  it('treats zero as a moment, not as missing', () => {
    expect(formatClock(0)).not.toBe('--:--:--');
  });
});

describe('formatDateTime', () => {
  it('includes the day as well as the time', () => {
    const label = formatDateTime(LOCAL);
    expect(label).toMatch(/14\D03\D07/);
    expect(label).toMatch(/\b5\b/);
    expect(label).not.toBe(formatClock(LOCAL));
  });

  it('says unknown for a missing or impossible moment', () => {
    for (const missing of [null, undefined, Number.NaN, Number.NEGATIVE_INFINITY]) {
      expect(formatDateTime(missing)).toBe('unknown');
    }
  });
});

describe('formatDuration edges', () => {
  it('rounds a part second down rather than up', () => {
    expect(formatDuration(59_999)).toBe('00:00:59');
  });

  it('keeps counting hours past a day rather than wrapping', () => {
    expect(formatDuration(26 * 3_600_000)).toBe('26:00:00');
  });
});

describe('formatOffsetFromLive edges', () => {
  it('stops calling it live at exactly one and a half seconds', () => {
    expect(formatOffsetFromLive(1_499)).toBe('live');
    expect(formatOffsetFromLive(1_500)).toBe('2s behind');
  });

  it('moves to minutes once the rounded seconds reach sixty', () => {
    expect(formatOffsetFromLive(59_400)).toBe('59s behind');
    expect(formatOffsetFromLive(59_600)).toBe('1m 0s behind');
  });

  it('shows hours and minutes past an hour', () => {
    expect(formatOffsetFromLive(3_600_000 + 25 * 60_000)).toBe('1h 25m behind');
  });

  it('calls a negative offset live, as when the clock runs slightly ahead', () => {
    expect(formatOffsetFromLive(-500)).toBe('live');
  });
});

describe('formatBytes edges', () => {
  it('stops at terabytes rather than running off the unit list', () => {
    expect(formatBytes(1024 ** 4)).toBe('1.0 TB');
    expect(formatBytes(1024 ** 5)).toBe('1024.0 TB');
  });

  it('shows a fraction of a unit to one place', () => {
    expect(formatBytes(1536)).toBe('1.5 KB');
    expect(formatBytes(1024 ** 3 * 2.25)).toBe('2.3 GB');
  });

  it('treats infinity as rubbish input', () => {
    expect(formatBytes(Number.POSITIVE_INFINITY)).toBe('0 B');
  });
});

describe('meter edges', () => {
  it('reads full scale as zero decibels', () => {
    expect(toDecibels(1)).toBe(0);
    expect(toDecibels(0.1)).toBeCloseTo(-20, 10);
  });

  it('treats negative amplitude as silence', () => {
    expect(toDecibels(-0.5)).toBe(-100);
  });

  it('honours a custom floor', () => {
    expect(meterScale(0.1, -40)).toBeCloseTo(0.5, 10);
    expect(meterScale(0.001, -40)).toBe(0);
  });

  it('never goes above full for a clipping signal', () => {
    expect(meterScale(4)).toBe(1);
  });
});
