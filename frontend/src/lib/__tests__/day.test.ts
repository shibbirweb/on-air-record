import { describe, expect, it } from 'vitest';

import { dayLabel, localDayId, parseDayId, previousDayId } from '../day';

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
