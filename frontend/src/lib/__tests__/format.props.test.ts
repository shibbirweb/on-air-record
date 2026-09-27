/**
 * Properties of the formatting helpers over generated numbers.
 *
 * These strings are what somebody reads off the status panel, the settings page and the transport bar, so
 * the failures worth hunting are the ones that print `NaN`, `undefined` or a negative span, show `1024.0 KB`
 * where a unit should have rolled over, or make a bigger number read smaller. Example tests pick values a
 * person thinks of; these sweep the range, including fractions and the far ends of what a `Date` holds.
 */

import fc from 'fast-check';
import { describe, expect, it } from 'vitest';

import {
  formatBytes,
  formatClock,
  formatDateTime,
  formatDuration,
  formatOffsetFromLive,
  meterScale,
  pcmBitRateKbps,
  pcmBytesPerHour,
  toDecibels,
} from '../format';

const nonNegative = fc.double({ min: 0, max: 1e15, noNaN: true });

/** Two values in order, for monotonicity. */
const ordered = (arbitrary: fc.Arbitrary<number>) =>
  fc.tuple(arbitrary, arbitrary).map(([first, second]) => (first <= second ? [first, second] : [second, first]));

describe('formatDuration', () => {
  function seconds(label: string): number {
    const [hours, minutes, secs] = label.split(':').map(Number);
    return hours * 3600 + minutes * 60 + secs;
  }

  it('prints whole hours, minutes and seconds, never NaN or a negative', () => {
    fc.assert(
      fc.property(fc.double({ min: -1e12, max: 1e15, noNaN: true }), (durationMs) => {
        const label = formatDuration(durationMs);
        expect(label).toMatch(/^\d{2,}:[0-5]\d:[0-5]\d$/);
        expect(seconds(label)).toBe(Math.max(Math.floor(durationMs / 1000), 0));
      }),
    );
  });

  it('never reads shorter for a longer span', () => {
    fc.assert(
      fc.property(ordered(nonNegative), ([shorter, longer]) => {
        expect(seconds(formatDuration(shorter))).toBeLessThanOrEqual(seconds(formatDuration(longer)));
      }),
    );
  });
});

describe('formatOffsetFromLive', () => {
  /** The seconds a label stands for: `live` is zero, and the hour form drops its seconds. */
  function seconds(label: string): number {
    if (label === 'live') {
      return 0;
    }
    const units: Record<string, number> = { h: 3600, m: 60, s: 1 };
    return [...label.matchAll(/(\d+)([hms])/g)].reduce(
      (total, [, value, unit]) => total + Number(value) * units[unit],
      0,
    );
  }

  it('reads as live or a well formed offset, with each unit below its rollover', () => {
    fc.assert(
      fc.property(fc.double({ min: -1e9, max: 1e12, noNaN: true }), (offsetMs) => {
        const label = formatOffsetFromLive(offsetMs);
        expect(label).toMatch(/^(live|\d+s behind|\d+m \d+s behind|\d+h \d+m behind)$/);
        // Hours are unbounded; every smaller unit rolls over before it reaches 60.
        for (const [, value] of label.matchAll(/(\d+)[ms]/g)) {
          expect(Number(value)).toBeLessThan(60);
        }
        if (offsetMs < 1500) {
          expect(label).toBe('live');
        }
      }),
    );
  });

  it('never reads closer to live for a moment further behind it, and is out by under a minute', () => {
    fc.assert(
      fc.property(ordered(fc.double({ min: 0, max: 1e12, noNaN: true })), ([nearer, further]) => {
        expect(seconds(formatOffsetFromLive(nearer))).toBeLessThanOrEqual(seconds(formatOffsetFromLive(further)));
        if (further >= 1500) {
          expect(Math.round(further / 1000) - seconds(formatOffsetFromLive(further))).toBeLessThan(60);
        }
      }),
    );
  });
});

describe('formatBytes', () => {
  const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

  function bytesShown(label: string): number {
    const [value, unit] = label.split(' ');
    return Number(value) * 1024 ** UNITS.indexOf(unit);
  }

  it('prints a number and a known unit for any input at all', () => {
    fc.assert(
      fc.property(
        fc.oneof(
          fc.constantFrom(Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY, -0),
          fc.double({ min: -1e15, max: 1e15 }),
          fc.double({ min: 0, max: 4, noNaN: true }),
        ),
        (bytes) => {
          expect(formatBytes(bytes)).toMatch(/^\d+(\.\d)? (B|KB|MB|GB|TB)$/);
        },
      ),
    );
  });

  it('rolls over to the next unit instead of showing 1024 of the one below', () => {
    // Values just under each power of 1024 are where a logarithm and a rounding disagree.
    const justUnderAUnit = fc
      .tuple(fc.integer({ min: 1, max: 4 }), fc.double({ min: 0.0001, max: 0.05, noNaN: true }))
      .map(([power, fraction]) => 1024 ** power - fraction * 1024 ** (power - 1));
    fc.assert(
      fc.property(fc.oneof(nonNegative, justUnderAUnit), (bytes) => {
        const [value, unit] = formatBytes(bytes).split(' ');
        if (unit !== 'TB') {
          expect(Number(value)).toBeLessThan(1024);
        }
      }),
    );
  });

  it('never reads smaller for more bytes', () => {
    fc.assert(
      fc.property(ordered(nonNegative), ([fewer, more]) => {
        expect(bytesShown(formatBytes(fewer))).toBeLessThanOrEqual(bytesShown(formatBytes(more)));
      }),
    );
  });

  it('is within the rounding of the unit it shows', () => {
    fc.assert(
      fc.property(fc.double({ min: 1, max: 1e15, noNaN: true }), (bytes) => {
        const label = formatBytes(bytes);
        const unit = 1024 ** UNITS.indexOf(label.split(' ')[1]);
        const tolerance = unit === 1 ? 0.5 : 0.05 * unit;
        expect(Math.abs(bytesShown(label) - bytes)).toBeLessThanOrEqual(tolerance + 1e-9 * bytes);
      }),
    );
  });
});

describe('formatClock and formatDateTime', () => {
  it('never throw for any finite number, and fall back to the placeholder past what a Date holds', () => {
    fc.assert(
      fc.property(fc.double({ noNaN: true }), (timestampMs) => {
        const representable = Math.abs(timestampMs) <= 8.64e15;
        const clock = formatClock(timestampMs);
        const dated = formatDateTime(timestampMs);
        if (representable) {
          expect(clock).not.toBe('--:--:--');
          expect(dated).not.toBe('unknown');
        } else {
          expect(clock).toBe('--:--:--');
          expect(dated).toBe('unknown');
        }
      }),
    );
  });

  it('agree with each other on the time of day', () => {
    fc.assert(
      fc.property(fc.integer({ min: 0, max: 4_102_444_800_000 }), (timestampMs) => {
        expect(formatDateTime(timestampMs)).toContain(formatClock(timestampMs));
      }),
    );
  });
});

describe('toDecibels and meterScale', () => {
  const amplitude = fc.double({ min: -1, max: 4, noNaN: true });

  it('keep the decibel scale at or above its floor, and the meter inside 0..1', () => {
    fc.assert(
      fc.property(amplitude, fc.integer({ min: -120, max: -1 }), (value, floorDb) => {
        expect(toDecibels(value)).toBeGreaterThanOrEqual(-100);
        const scaled = meterScale(value, floorDb);
        expect(scaled).toBeGreaterThanOrEqual(0);
        expect(scaled).toBeLessThanOrEqual(1);
      }),
    );
  });

  it('never move down for a louder signal', () => {
    fc.assert(
      fc.property(ordered(amplitude), fc.integer({ min: -120, max: -1 }), ([quieter, louder], floorDb) => {
        expect(toDecibels(quieter)).toBeLessThanOrEqual(toDecibels(louder));
        expect(meterScale(quieter, floorDb)).toBeLessThanOrEqual(meterScale(louder, floorDb));
      }),
    );
  });

  it('pin full scale and above to a full meter', () => {
    fc.assert(
      fc.property(
        fc.double({ min: 1, max: 1e6, noNaN: true }),
        fc.integer({ min: -120, max: -1 }),
        (value, floorDb) => {
          expect(meterScale(value, floorDb)).toBe(1);
        },
      ),
    );
  });
});

describe('pcmBytesPerHour and pcmBitRateKbps', () => {
  it('are zero for anything that is not a positive rate, and never negative or NaN', () => {
    fc.assert(
      fc.property(fc.double(), (sampleRate) => {
        const bytes = pcmBytesPerHour(sampleRate);
        const kbps = pcmBitRateKbps(sampleRate);
        if (!Number.isFinite(sampleRate) || sampleRate <= 0) {
          expect(bytes).toBe(0);
          expect(kbps).toBe(0);
        } else {
          expect(bytes).toBe(sampleRate * 7200);
          expect(kbps).toBeGreaterThanOrEqual(0);
        }
      }),
    );
  });

  it('grow with the rate', () => {
    fc.assert(
      fc.property(ordered(fc.integer({ min: 1, max: 384_000 })), ([lower, higher]) => {
        expect(pcmBytesPerHour(lower)).toBeLessThanOrEqual(pcmBytesPerHour(higher));
        expect(pcmBitRateKbps(lower)).toBeLessThanOrEqual(pcmBitRateKbps(higher));
      }),
    );
  });
});
