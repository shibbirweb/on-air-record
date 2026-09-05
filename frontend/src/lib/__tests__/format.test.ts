import { describe, expect, it } from 'vitest';

import {
  formatBytes,
  formatDuration,
  formatOffsetFromLive,
  meterScale,
  pcmBitRateKbps,
  pcmBytesPerHour,
  toDecibels,
} from '../format';

describe('formatDuration', () => {
  it('formats hours, minutes and seconds', () => {
    expect(formatDuration(3_723_000)).toBe('01:02:03');
  });

  it('clamps a negative span to zero', () => {
    expect(formatDuration(-1)).toBe('00:00:00');
  });
});

describe('formatOffsetFromLive', () => {
  it('calls anything inside the jitter buffer live', () => {
    expect(formatOffsetFromLive(0)).toBe('live');
    expect(formatOffsetFromLive(1_200)).toBe('live');
  });

  it('scales the unit with the distance', () => {
    expect(formatOffsetFromLive(45_000)).toBe('45s behind');
    expect(formatOffsetFromLive(150_000)).toBe('2m 30s behind');
    expect(formatOffsetFromLive(7_200_000)).toBe('2h 0m behind');
  });
});

describe('formatBytes', () => {
  it('picks a sensible unit', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1024)).toBe('1.0 KB');
    expect(formatBytes(345 * 1024 * 1024)).toBe('345.0 MB');
  });

  it('does not fall over on rubbish input', () => {
    expect(formatBytes(Number.NaN)).toBe('0 B');
    expect(formatBytes(-10)).toBe('0 B');
  });
});

describe('meterScale', () => {
  it('puts full scale at the top and silence at the bottom', () => {
    expect(meterScale(1)).toBeCloseTo(1, 5);
    expect(meterScale(0)).toBe(0);
  });

  it('spreads normal speech across the visible range', () => {
    // Roughly -26 dBFS, a typical conversational level, should sit near the middle rather than being
    // crushed against zero the way a linear scale would render it.
    const scaled = meterScale(0.05);
    expect(scaled).toBeGreaterThan(0.4);
    expect(scaled).toBeLessThan(0.7);
  });

  it('floors anything below the visible range', () => {
    expect(meterScale(0.0001)).toBe(0);
    expect(toDecibels(0)).toBe(-100);
  });
});

describe('pcm storage arithmetic', () => {
  it('matches the server figure for full quality mono', () => {
    // 48000 * 2 bytes * 3600 seconds, the same expression the backend uses.
    expect(pcmBytesPerHour(48_000)).toBe(345_600_000);
    expect(pcmBitRateKbps(48_000)).toBe(768);
  });

  it('scales linearly, which is the whole premise of the selector', () => {
    expect(pcmBytesPerHour(24_000)).toBe(pcmBytesPerHour(48_000) / 2);
    expect(pcmBytesPerHour(16_000)).toBe(pcmBytesPerHour(48_000) / 3);
    expect(pcmBitRateKbps(8_000)).toBe(128);
  });

  it('reports nothing rather than a negative size for a missing rate', () => {
    expect(pcmBytesPerHour(0)).toBe(0);
    expect(pcmBytesPerHour(Number.NaN)).toBe(0);
    expect(pcmBitRateKbps(0)).toBe(0);
  });
});
