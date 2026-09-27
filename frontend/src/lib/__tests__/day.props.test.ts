/**
 * Properties of the calendar day helpers over generated instants.
 *
 * Days are local, so these helpers are only right if they agree with each other on every instant in every
 * timezone, including the hour a daylight saving change skips or repeats, and not only on the dates the
 * example tests name. They run in whatever zone the machine has, which is part of the point: the same
 * properties must hold on a developer's laptop and a UTC runner alike.
 */

import fc from 'fast-check';
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

/** 1970 to 2100, the span any recording falls in, with four digit years for the day id format. */
const instant = fc.integer({ min: 0, max: 4_102_444_800_000 });

const HOUR_MS = 3_600_000;

describe('dayBoundsMs and localDayId', () => {
  it('bracket the instant with local midnights a calendar day apart', () => {
    fc.assert(
      fc.property(instant, (timestampMs) => {
        const { startMs, endMs } = dayBoundsMs(timestampMs);

        expect(startMs).toBeLessThanOrEqual(timestampMs);
        expect(timestampMs).toBeLessThan(endMs);
        // 23 or 25 hours on a daylight saving change, 24 otherwise.
        expect([23 * HOUR_MS, 24 * HOUR_MS, 25 * HOUR_MS]).toContain(endMs - startMs);
        expect(localDayId(startMs)).toBe(localDayId(timestampMs));
        expect(localDayId(endMs - 1)).toBe(localDayId(timestampMs));
        expect(localDayId(endMs)).not.toBe(localDayId(timestampMs));
      }),
    );
  });

  it('name a day that parses back to its own midnight', () => {
    fc.assert(
      fc.property(instant, (timestampMs) => {
        const day = localDayId(timestampMs);
        expect(day).toMatch(/^\d{4}-\d{2}-\d{2}$/);
        expect(parseDayId(day)?.getTime()).toBe(dayBoundsMs(timestampMs).startMs);
      }),
    );
  });

  it('step back exactly one calendar day', () => {
    fc.assert(
      fc.property(instant.filter((value) => value >= 2 * 86_400_000), (timestampMs) => {
        expect(previousDayId(timestampMs)).toBe(localDayId(dayBoundsMs(timestampMs).startMs - 1));
      }),
    );
  });
});

describe('parseDayId', () => {
  it('never throws, and returns only the start of a real day in a real month', () => {
    fc.assert(
      fc.property(fc.oneof(fc.string({ maxLength: 16 }), fc.stringMatching(/^-?\d{1,5}-\d{1,3}-\d{1,3}$/)), (day) => {
        const parsed = parseDayId(day);
        if (parsed === null) {
          return;
        }
        // The start of its own day, which is midnight except where a clock change skips midnight itself.
        expect(parsed.getTime()).toBe(dayBoundsMs(parsed.getTime()).startMs);
        const [year, month, date] = day.split('-').map(Number);
        expect(parsed.getFullYear()).toBe(year);
        expect(parsed.getMonth()).toBe(month - 1);
        expect(parsed.getDate()).toBe(date);
      }),
    );
  });

  it('rejects a date past the end of its month', () => {
    fc.assert(
      fc.property(fc.integer({ min: 1971, max: 2099 }), fc.integer({ min: 1, max: 12 }), (year, month) => {
        const lastDay = new Date(year, month, 0).getDate();
        const pad = (value: number) => String(value).padStart(2, '0');
        expect(parseDayId(`${year}-${pad(month)}-${pad(lastDay)}`)).not.toBeNull();
        if (lastDay < 31) {
          expect(parseDayId(`${year}-${pad(month)}-${pad(lastDay + 1)}`)).toBeNull();
        }
      }),
    );
  });
});

describe('dayLabel', () => {
  it('prefers Today over Yesterday, and returns anything unparseable as it came', () => {
    fc.assert(
      fc.property(
        fc.oneof(instant.map(localDayId), fc.string({ maxLength: 12 })),
        fc.option(instant.map(localDayId), { nil: null }),
        fc.option(instant.map(localDayId), { nil: null }),
        (day, todayId, yesterdayId) => {
          const label = dayLabel(day, todayId, yesterdayId);
          if (day === todayId) {
            expect(label).toBe('Today');
          } else if (day === yesterdayId) {
            expect(label).toBe('Yesterday');
          } else if (parseDayId(day) === null) {
            expect(label).toBe(day);
          } else {
            expect(label.length).toBeGreaterThan(0);
          }
        },
      ),
    );
  });
});

describe('toDateTimeLocal and fromDateTimeLocal', () => {
  it('round trip an instant to the second', () => {
    fc.assert(
      fc.property(instant, (timestampMs) => {
        const text = toDateTimeLocal(timestampMs);
        expect(text).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}$/);
        const back = fromDateTimeLocal(text);
        // In the hour a clock change repeats, the wall clock names two instants and either is right;
        // what must survive is the wall clock itself.
        expect(back).not.toBeNull();
        expect(toDateTimeLocal(back as number)).toBe(text);
        expect(Math.abs((back as number) - Math.floor(timestampMs / 1000) * 1000)).toBeLessThanOrEqual(HOUR_MS);
      }),
    );
  });

  it('read blank input as nothing rather than a date', () => {
    fc.assert(
      fc.property(fc.stringMatching(/^[ \t]*$/), (blank) => {
        expect(fromDateTimeLocal(blank)).toBeNull();
      }),
    );
  });
});
