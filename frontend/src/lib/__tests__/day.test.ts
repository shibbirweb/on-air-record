import { describe, expect, it } from 'vitest';

import {
  dayBoundsMs,
  dayLabel,
  fromDateTimeLocal,
  localDayId,
  parseDayId,
  previousDayId,
  toDateTimeLocal,
} from '../day';

describe('localDayId', () => {
  it('names the day in local time, not UTC', () => {
    // Built from local parts, so the assertion holds in whatever timezone the suite runs in.
    const local = new Date(2026, 8, 5, 23, 30);
    expect(localDayId(local.getTime())).toBe('2026-09-05');
  });

  it('zero pads month and day so the ids sort chronologically', () => {
    expect(localDayId(new Date(2026, 0, 3, 12).getTime())).toBe('2026-01-03');
    expect(localDayId(new Date(2026, 0, 3).getTime()) < localDayId(new Date(2026, 0, 10).getTime())).toBe(
      true,
    );
  });
});

describe('previousDayId', () => {
  it('steps back one calendar day', () => {
    expect(previousDayId(new Date(2026, 8, 5, 12).getTime())).toBe('2026-09-04');
  });

  it('crosses a month boundary', () => {
    expect(previousDayId(new Date(2026, 8, 1, 12).getTime())).toBe('2026-08-31');
  });

  it('crosses a year boundary', () => {
    expect(previousDayId(new Date(2026, 0, 1, 12).getTime())).toBe('2025-12-31');
  });

  it('handles a leap day', () => {
    expect(previousDayId(new Date(2028, 2, 1, 12).getTime())).toBe('2028-02-29');
  });
});

describe('parseDayId', () => {
  it('returns local midnight rather than a UTC instant', () => {
    const parsed = parseDayId('2026-09-05');
    expect(parsed).not.toBeNull();
    expect(parsed?.getFullYear()).toBe(2026);
    expect(parsed?.getMonth()).toBe(8);
    expect(parsed?.getDate()).toBe(5);
    expect(parsed?.getHours()).toBe(0);
  });

  it('round trips through localDayId', () => {
    const day = '2026-03-09';
    expect(localDayId(parseDayId(day)!.getTime())).toBe(day);
  });

  it('rejects dates that do not exist rather than rolling them over', () => {
    // The Date constructor would happily turn this into 2 March.
    expect(parseDayId('2026-02-30')).toBeNull();
    expect(parseDayId('2026-13-01')).toBeNull();
    expect(parseDayId('2026-00-10')).toBeNull();
  });

  it('rejects anything that is not a day id', () => {
    for (const invalid of ['', 'today', '2026-09', '2026/09/05', 'x-y-z']) {
      expect(parseDayId(invalid)).toBeNull();
    }
  });
});

describe('dayLabel', () => {
  it('names today and yesterday', () => {
    expect(dayLabel('2026-09-05', '2026-09-05', '2026-09-04')).toBe('Today');
    expect(dayLabel('2026-09-04', '2026-09-05', '2026-09-04')).toBe('Yesterday');
  });

  it('formats older days as a date', () => {
    const label = dayLabel('2026-09-03', '2026-09-05', '2026-09-04');
    expect(label).not.toBe('Today');
    expect(label).not.toBe('Yesterday');
    expect(label).toContain('3');
  });

  it('falls back to the raw id when the day cannot be parsed', () => {
    expect(dayLabel('not-a-day', '2026-09-05', '2026-09-04')).toBe('not-a-day');
  });

  it('still labels days when the reference day is unknown', () => {
    // Before the first status poll there is no server clock to compare against.
    expect(dayLabel('2026-09-05', null, null)).toContain('5');
  });
});

describe('dayBoundsMs', () => {
  it('spans local midnight to local midnight', () => {
    const { startMs, endMs } = dayBoundsMs(new Date(2026, 8, 5, 14, 37).getTime());

    expect(new Date(startMs).getHours()).toBe(0);
    expect(new Date(startMs).getDate()).toBe(5);
    expect(new Date(endMs).getHours()).toBe(0);
    expect(new Date(endMs).getDate()).toBe(6);
  });

  it('contains every instant of its own day', () => {
    const midday = new Date(2026, 8, 5, 12).getTime();
    const { startMs, endMs } = dayBoundsMs(midday);

    for (const probe of [startMs, midday, endMs - 1]) {
      expect(localDayId(probe)).toBe('2026-09-05');
    }
    expect(localDayId(endMs)).toBe('2026-09-06');
  });

  it('is idempotent at the boundaries', () => {
    const { startMs, endMs } = dayBoundsMs(new Date(2026, 8, 5, 9).getTime());
    expect(dayBoundsMs(startMs).startMs).toBe(startMs);
    expect(dayBoundsMs(endMs - 1).endMs).toBe(endMs);
  });

  it('runs roughly a day even across a daylight saving change', () => {
    // Stepping by calendar date rather than by 24 hours is what keeps this in range.
    for (const date of [new Date(2026, 2, 8, 12), new Date(2026, 10, 1, 12)]) {
      const { startMs, endMs } = dayBoundsMs(date.getTime());
      const hours = (endMs - startMs) / 3_600_000;
      expect(hours).toBeGreaterThanOrEqual(23);
      expect(hours).toBeLessThanOrEqual(25);
    }
  });

  it('gives consecutive days a shared edge', () => {
    const first = dayBoundsMs(new Date(2026, 8, 5, 3).getTime());
    const second = dayBoundsMs(new Date(2026, 8, 6, 3).getTime());
    expect(first.endMs).toBe(second.startMs);
  });
});

describe('datetime-local conversion', () => {
  it('round trips an instant through the input format', () => {
    const instant = new Date(2026, 8, 5, 18, 12, 17).getTime();
    const text = toDateTimeLocal(instant);

    expect(text).toBe('2026-09-05T18:12:17');
    expect(fromDateTimeLocal(text)).toBe(instant);
  });

  it('formats in local time rather than UTC', () => {
    // toISOString would shift this by the machine's offset and show the wrong wall clock.
    const local = new Date(2026, 0, 1, 0, 30, 0);
    expect(toDateTimeLocal(local.getTime())).toBe('2026-01-01T00:30:00');
  });

  it('zero pads every component', () => {
    const early = new Date(2026, 0, 2, 3, 4, 5).getTime();
    expect(toDateTimeLocal(early)).toBe('2026-01-02T03:04:05');
  });

  it('keeps seconds, which the export range depends on', () => {
    const instant = new Date(2026, 8, 5, 18, 12, 59).getTime();
    expect(toDateTimeLocal(instant).endsWith(':59')).toBe(true);
  });

  it('returns null for an empty or unparseable value', () => {
    expect(fromDateTimeLocal('')).toBeNull();
    expect(fromDateTimeLocal('   ')).toBeNull();
    expect(fromDateTimeLocal('not a date')).toBeNull();
    expect(fromDateTimeLocal('2026-13-45T99:99')).toBeNull();
  });

  it('accepts a value without seconds, which some browsers produce', () => {
    expect(fromDateTimeLocal('2026-09-05T18:12')).toBe(new Date(2026, 8, 5, 18, 12).getTime());
  });

  it('survives a bad instant without throwing', () => {
    expect(toDateTimeLocal(Number.NaN)).toBe('');
  });
});
